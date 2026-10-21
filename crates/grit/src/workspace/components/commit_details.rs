//! Commit details view: replaces the staging panel while a commit is
//! selected — metadata on top, the commit's changed files below (in a
//! virtualized list: a merge commit can carry hundreds of them).

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::primitives::PanelHeader;
use super::staging::{Colors, colors as theme_colors};
use crate::workspace::Workspace;
use grit_core::status::FileEntry;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    let foreground = cx.theme().foreground;

    let Some(commit) = &ws.selected_commit else {
        return div();
    };

    let short_id = commit.id.to_string().chars().take(7).collect::<String>();
    let files_len = ws.commit_files.len();

    v_flex()
        .w_full()
        .h_full()
        // ---- details header ----
        .child(
            v_flex()
                .flex_none()
                .p_2()
                .gap_1()
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .min_w_0()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    Icon::new(IconName::GitCommitHorizontal)
                                        .size_3()
                                        .text_color(cx.theme().info),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(foreground)
                                        .child(commit.summary.clone()),
                                ),
                        )
                        .child(
                            Button::new("close-commit")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|this, _, _, cx| this.close_commit(cx))),
                        ),
                )
                // meta: author, date, id
                .child(
                    h_flex()
                        .flex_none()
                        .gap_2()
                        .text_size(px(10.))
                        .text_color(muted)
                        .child(div().flex_none().child(format!(
                            "{} — {}",
                            commit.author.name,
                            commit.author.date_time()
                        )))
                        .child(
                            div()
                                .flex_none()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(short_id.clone()),
                        ),
                )
                // full commit message, if the summary is not the whole story
                .when(
                    commit.message.as_deref().is_some_and(|m| {
                        !m.trim().is_empty() && !m.trim_start().starts_with(commit.summary.trim())
                    }),
                    |this| {
                        this.child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .line_height(relative(1.4))
                                .whitespace_nowrap()
                                .child(commit.message.clone().unwrap_or_default()),
                        )
                    },
                ),
        )
        // ---- changed files ----
        .child(h_flex().flex_none().px_2().py_1().child(PanelHeader::new("Changes", files_len)))
        .child(changed_files_list("commit-files", files_len, cx))
}

/// The commit's changed files, virtualized like the staging lists: the row
/// closure gets `&mut App` only, state is read through a captured
/// `WeakEntity` and actions run through it.
fn changed_files_list(id: &'static str, count: usize, cx: &mut Context<Workspace>) -> UniformList {
    let workspace = cx.entity().downgrade();

    uniform_list(id, count, move |range, _window, cx| {
        let Some(entity) = workspace.upgrade() else {
            return Vec::new();
        };
        let colors = theme_colors(cx);
        let ws = entity.read(cx);

        range
            .map(|ix| match ws.commit_files.get(ix) {
                Some(entry) => changed_file_row(ws, entry, &workspace, colors).into_any_element(),
                None => div().id(format!("{id}-empty-{ix}")).into_any_element(),
            })
            .collect()
    })
    .flex_1()
    .min_h_0()
    .w_full()
    .p_1()
}

/// One changed file of the selected commit; click opens its diff. Two
/// truncated lines (name on top, parent below when not at the repo root)
/// in a fixed `h_9` row — the uniform height is what the virtualized list
/// requires.
fn changed_file_row(
    ws: &Workspace,
    entry: &FileEntry,
    workspace: &WeakEntity<Workspace>,
    colors: Colors,
) -> Stateful<Div> {
    let Colors { border, muted, foreground, accent, success, info, danger } = colors;
    let (label, color) = super::primitives::kind_colors(entry.kind, success, info, danger, muted);
    let name = entry
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.to_string_lossy().into_owned());
    let parent = entry
        .path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().into_owned());
    let path = entry.path.clone();
    let is_selected = ws.selected_path.as_ref() == Some(&entry.path);

    div()
        .id(SharedString::from(format!("cfile-{}", entry.path.to_string_lossy())))
        .w_full()
        .h_9()
        .flex_none()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .rounded_sm()
        .when(is_selected, |this| this.bg(accent.opacity(0.3)))
        .when(!is_selected, |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click({
            let workspace = workspace.clone();
            let path = path.clone();
            move |_, _, cx| {
                workspace.update(cx, |this, cx| this.select_commit_file(path.clone(), cx)).ok();
            }
        })
        .child(
            div().w_5().flex_none().text_center().text_size(px(11.)).text_color(color).child(label),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .overflow_hidden()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(foreground)
                        .child(name),
                )
                .when_some(parent, |this, parent| {
                    this.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(10.))
                            .text_color(muted)
                            .child(parent),
                    )
                }),
        )
}
