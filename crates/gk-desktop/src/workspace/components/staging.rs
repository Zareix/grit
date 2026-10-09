//! Right column: staging — two fixed sections (unstaged top, staged bottom),
//! each with a header that never scrolls away, and two-line file rows in a
//! virtualized list.

use gk_core::status::{ChangeKind, FileEntry};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::Workspace;

use super::commit_box;

/// Theme colors needed by the row closures (they only get `&mut App`).
#[derive(Clone, Copy)]
struct Colors {
    border: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
    foreground: gpui_kit::Hsla,
    accent: gpui_kit::Hsla,
    success: gpui_kit::Hsla,
    info: gpui_kit::Hsla,
    danger: gpui_kit::Hsla,
}

fn colors(cx: &App) -> Colors {
    let theme = cx.theme();
    Colors {
        border: theme.border,
        muted: theme.muted_foreground,
        foreground: theme.foreground,
        accent: theme.accent,
        success: theme.success,
        info: theme.info,
        danger: theme.danger,
    }
}

/// The whole right column: staging sections + commit box.
pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let workspace = cx.entity().downgrade();

    let unstaged_len = ws.status.unstaged.len();
    let staged_len = ws.status.staged.len();

    let unstaged_list = file_list("unstaged", &workspace, false, unstaged_len, cx);
    let staged_list = file_list("staged", &workspace, true, staged_len, cx);

    v_flex()
        .w_full()
        .h_full()
        .flex_none()
        // --- unstaged section ---
        .when(unstaged_len > 0, |this| {
            this.child(section_header("Changes", unstaged_len, false, cx))
        })
        .child(unstaged_list)
        // --- staged section (sized by content, capped) ---
        .when(staged_len > 0, |this| {
            this.child(section_header("Staged Changes", staged_len, true, cx))
        })
        .when(staged_len > 0, |this| this.child(staged_list))
        .child(commit_box::render(ws, cx))
}

/// A virtualized, scrollable list of file rows.
///
/// `Auto` sizing only fills a definite flex slot: the unstaged list is
/// `flex_1`, the staged list gets an explicit height (row count × 36px,
/// capped) — a `flex_none` uniform_list collapses to zero height.
fn file_list(
    id: &'static str,
    workspace: &WeakEntity<Workspace>,
    staged: bool,
    count: usize,
    _cx: &mut Context<Workspace>,
) -> UniformList {
    let container_flex_none = staged; // staged list is capped, unstaged fills the rest
    let workspace = workspace.clone();

    uniform_list(id, count, move |range, _window, cx| {
        let Some(entity) = workspace.upgrade() else {
            return Vec::new();
        };
        let colors = colors(cx);
        let ws = entity.read(cx);
        let entries: &Vec<FileEntry> = if staged { &ws.status.staged } else { &ws.status.unstaged };

        range
            .map(|ix| match entries.get(ix) {
                Some(entry) => file_row(ws, entry, staged, colors, workspace.clone()),
                None => div().id(format!("{id}-empty-{ix}")),
            })
            .collect()
    })
    .when(container_flex_none, |this| {
        let height = ((count as f32) * 36. + 8.).min(224.);
        this.flex_none().h(px(height))
    })
    .when(!container_flex_none, |this| this.flex_1().min_h_0())
    .w_full()
    .px_1()
}

/// A file on two lines: file name on top, parent path below (when not at the
/// repo root). Fixed height; the badge and stage button are vertically
/// centered across the whole row.
fn file_row(
    ws: &Workspace,
    entry: &FileEntry,
    staged: bool,
    colors: Colors,
    workspace: WeakEntity<Workspace>,
) -> Stateful<Div> {
    let Colors { border, muted, foreground, accent, success, info, danger } = colors;

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
    let (toggle_icon, toggle_id) =
        if staged { (IconName::Minus, "unstage") } else { (IconName::Plus, "stage") };

    div()
        .id(SharedString::from(format!(
            "file-{}-{}",
            if staged { "s" } else { "u" },
            path.to_string_lossy()
        )))
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
                workspace.update(cx, |ws, cx| ws.select_file(path.clone(), staged, cx)).ok();
            }
        })
        // badge — left slot, centered by the row's items_center
        .child(kind_badge(entry.kind, success, info, danger, muted))
        // middle: two stacked lines
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
        // stage/unstage command — right slot, centered. stop_propagation is
        // explicit: without it the row's on_click also fires and re-selects
        // the file in the *wrong* section (diff pane shows "No changes").
        .child(Button::new(toggle_id).ghost().xsmall().icon(toggle_icon).on_click({
            let workspace = workspace.clone();
            let path = path.clone();
            move |_, _, cx| {
                cx.stop_propagation();
                workspace.update(cx, |ws, cx| ws.toggle_file(path.clone(), staged, cx)).ok();
            }
        }))
}

/// Fixed section header: title + count + stage/unstage-all command. Always
/// visible — the lists scroll under it.
fn section_header(
    title: &'static str,
    count: usize,
    staged: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let (button_id, button_label, disabled, action) = if staged {
        ("unstage-all", "Unstage all", count == 0, "unstage")
    } else {
        ("stage-all", "Stage all", count == 0, "stage")
    };

    h_flex()
        .flex_none()
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .justify_between()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted)
        .child(h_flex().gap_2().child(title).child(format!("{count}")))
        .child(
            Button::new(button_id)
                .ghost()
                .xsmall()
                .label(button_label)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if action == "unstage" { this.unstage_all(cx) } else { this.stage_all(cx) }
                })),
        )
}

/// A/M/D/U badge. The letter carries the meaning; color is only reinforcement.
fn kind_badge(
    kind: ChangeKind,
    success: gpui_kit::Hsla,
    info: gpui_kit::Hsla,
    danger: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
) -> Div {
    let (label, color) = match kind {
        ChangeKind::Added => ("A", success),
        ChangeKind::Modified => ("M", info),
        ChangeKind::Deleted => ("D", danger),
        ChangeKind::Renamed | ChangeKind::TypeChange => ("R/T", muted),
        ChangeKind::Conflicted => ("U", danger),
    };
    div().w_5().flex_none().text_center().text_size(px(11.)).text_color(color).child(label)
}
