//! Right column: staging — two fixed sections (unstaged top, staged bottom),
//! each with a header that never scrolls away, and two-line file rows in a
//! virtualized list.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use grit_core::status::FileEntry;

use crate::workspace::{CopyPath, Workspace};

use super::commit_box;
use super::primitives::KindBadge;

/// Theme colors needed by the row closures (they only get `&mut App`).
/// Shared with the commit-details rows, which run under the same
/// constraint.
#[derive(Clone, Copy)]
pub(crate) struct Colors {
    pub(crate) border: gpui_kit::Hsla,
    pub(crate) muted: gpui_kit::Hsla,
    pub(crate) foreground: gpui_kit::Hsla,
    pub(crate) accent: gpui_kit::Hsla,
    pub(crate) success: gpui_kit::Hsla,
    pub(crate) info: gpui_kit::Hsla,
    pub(crate) danger: gpui_kit::Hsla,
}

pub(crate) fn colors(cx: &App) -> Colors {
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
                None => div().id(format!("{id}-empty-{ix}")).into_any_element(),
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
/// centered across the whole row. Returns `AnyElement` because the
/// context-menu wrapper layers an extra element on top of the row.
fn file_row(
    ws: &Workspace,
    entry: &FileEntry,
    staged: bool,
    colors: Colors,
    workspace: WeakEntity<Workspace>,
) -> gpui_kit::AnyElement {
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
    let is_selected = ws.selected_paths.iter().any(|p| p == &entry.path);
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
        .when(is_selected, |this| this.bg(accent))
        .when(!is_selected, |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click({
            let workspace = workspace.clone();
            let path = path.clone();
            move |ev, _, cx| {
                let modifiers = ev.modifiers();
                let add = modifiers.platform || modifiers.control;
                let extend = modifiers.shift;
                workspace
                    .update(cx, |ws, cx| {
                        if extend {
                            // Shift-click: extend from the anchor file.
                            ws.extend_file_selection(path.clone(), staged, cx)
                        } else if add {
                            // Cmd/Ctrl-click: toggle into the selection.
                            ws.toggle_file_selection(path.clone(), staged, cx)
                        } else {
                            ws.select_file(path.clone(), staged, cx)
                        }
                    })
                    .ok();
            }
        })
        // badge — left slot, centered by the row's items_center
        .child(KindBadge::new(entry.kind, success, info, danger, muted))
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
        // right-click: same command + destructive actions (confirm dialogs)
        // + file-system conveniences
        .context_menu({
            let path = path.clone();
            move |menu, window, cx| file_menu(menu, &path, staged, &workspace, window, cx)
        })
        .into_any_element()
}

/// Right-click menu on a staging file row: the state command, destructive
/// actions (each behind a confirm dialog), then file-system conveniences.
fn file_menu(
    mut menu: PopupMenu,
    path: &std::path::Path,
    staged: bool,
    workspace: &WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    // "Copy file path" opens a submenu with the three path spellings.
    let copy_paths = PopupMenu::build(window, cx, |mut menu, _, _| {
        for (part, label) in [
            (CopyPath::Absolute, "Absolute path"),
            (CopyPath::Relative, "Relative"),
            (CopyPath::Name, "File name"),
        ] {
            menu = menu.item(PopupMenuItem::new(label).on_click({
                let workspace = workspace.clone();
                let path = path.to_owned();
                move |_, _, cx| {
                    workspace.update(cx, |ws, cx| ws.copy_file_path(part, path.clone(), cx)).ok();
                }
            }));
        }
        menu
    });

    // The state command follows the selection: right-clicking a row inside
    // a multi-selection acts on all of its files.
    let count = workspace
        .upgrade()
        .map(|entity| entity.read(cx))
        .filter(|ws| ws.selected_staged == staged && ws.selected_paths.iter().any(|p| p == path))
        .map(|ws| ws.selected_paths.len())
        .unwrap_or(1);
    let state_label = if count > 1 {
        format!("{} {} files", if staged { "Unstage" } else { "Stage" }, count)
    } else if staged {
        "Unstage".to_owned()
    } else {
        "Stage".to_owned()
    };
    let stash_label =
        if count > 1 { format!("Stash {count} files") } else { "Stash changes".to_owned() };
    let discard_label =
        if count > 1 { format!("Discard {count} files") } else { "Discard changes".to_owned() };

    menu = menu
        .item(
            PopupMenuItem::new(state_label)
                .icon(if staged { IconName::Minus } else { IconName::Plus })
                .on_click({
                    let workspace = workspace.clone();
                    let path = path.to_owned();
                    move |_, _, cx| {
                        workspace
                            .update(cx, |ws, cx| ws.toggle_file(path.clone(), staged, cx))
                            .ok();
                    }
                }),
        )
        .item(PopupMenuItem::new(stash_label).icon(IconName::Archive).on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.stash_selection(cx)).ok();
            }
        }))
        .item(PopupMenuItem::new(discard_label).icon(IconName::RotateCcw).on_click({
            let workspace = workspace.clone();
            let path = path.to_owned();
            move |_, window, cx| {
                workspace
                    .update(cx, |ws, cx| ws.open_discard_dialog(path.clone(), window, cx))
                    .ok();
            }
        }))
        .separator()
        .item(PopupMenuItem::new("Show in Finder").icon(IconName::ExternalLink).on_click({
            let workspace = workspace.clone();
            let path = path.to_owned();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.show_in_finder(path.clone(), cx)).ok();
            }
        }))
        .item(PopupMenuItem::submenu("Copy file path", copy_paths).icon(IconName::Copy))
        .separator()
        .item(PopupMenuItem::new("Delete File").icon(IconName::Trash).on_click({
            let workspace = workspace.clone();
            let path = path.to_owned();
            move |_, window, cx| {
                workspace.update(cx, |ws, cx| ws.open_delete_dialog(path.clone(), window, cx)).ok();
            }
        }));
    menu
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
