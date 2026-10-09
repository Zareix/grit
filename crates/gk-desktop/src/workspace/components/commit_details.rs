//! Commit details view: replaces the staging panel while a commit is
//! selected — metadata on top, the commit's changed files below.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::Workspace;
use gk_core::status::FileEntry;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    let foreground = cx.theme().foreground;

    let Some(commit) = &ws.selected_commit else {
        return div();
    };

    let short_id = commit.id.to_string().chars().take(7).collect::<String>();
    let rows: Vec<gpui_kit::AnyElement> = ws
        .commit_files
        .iter()
        .map(|entry| changed_file_row(ws, entry, cx).into_any_element())
        .collect();

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
        .child(
            h_flex()
                .flex_none()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child("Changes")
                .child(format!("{} files", ws.commit_files.len())),
        )
        .child(
            div()
                .id("commit-files")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_1()
                .gap_0p5()
                .children(rows),
        )
}

/// One changed file of the selected commit; click opens its diff.
fn changed_file_row(
    ws: &Workspace,
    entry: &FileEntry,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
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
    let theme = cx.theme();
    let (label, color) = super::staging::kind_colors(
        entry.kind,
        theme.success,
        theme.info,
        theme.danger,
        theme.muted_foreground,
    );

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
        .when(is_selected, |this| this.bg(cx.theme().accent.opacity(0.3)))
        .when(!is_selected, |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click(cx.listener({
            let path = path.clone();
            move |this, _, _, cx| this.select_commit_file(path.clone(), cx)
        }))
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
                        .text_color(cx.theme().foreground)
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
