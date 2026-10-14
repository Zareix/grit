//! Center column: commit history, virtualized (`uniform_list`).
//!
//! Layout follows GitKraken's log: ref chips (branches / tags) on the left,
//! then the graph lane, then the commit title, then author + date. The list
//! shows the log across all refs; branch clicks scroll to a branch tip
//! through the tracked `UniformListScrollHandle`. Branch chips and remotes
//! carry the sidebar's context menus on right-click; tags have no actions
//! yet. The WIP row — shown when
//! the current branch has pending changes — is item 0 of the same list so
//! it scrolls with the commits; every row is 28px tall, which is the
//! uniform height `uniform_list` requires.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod refs;

use crate::workspace::Workspace;
use grit_core::Commit;
use grit_core::Oid;

/// Column widths, shared by the header labels and every row so the columns
/// stay aligned across the whole list.
const REFS_W: f32 = 176.;
const GRAPH_W: f32 = 96.;
const AUTHOR_W: f32 = 96.;
const DATE_W: f32 = 112.;

/// The staging summary row shown above the current branch's commits.
fn wip_row(ws: &Workspace, workspace: &WeakEntity<Workspace>, cx: &App) -> Stateful<Div> {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    // X/Y: staged files / all changed files (staged + unstaged).
    let changes = ws.status.staged.len() + ws.status.unstaged.len();

    h_flex()
        .id("wip-row")
        .w_full()
        .h_7()
        .flex_none()
        .px_3()
        .gap_3()
        .items_center()
        .border_b_1()
        .border_color(border.opacity(0.5))
        .text_size(px(12.))
        .when(ws.selected_commit.is_none(), |this| this.bg(cx.theme().accent))
        .when(ws.selected_commit.is_some(), |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.close_commit(cx)).ok();
            }
        })
        // same column skeleton as commit rows
        .child(div().flex_none().w(px(REFS_W)).min_w_0())
        .child(div().flex_none().w(px(GRAPH_W)).min_w_0())
        .child(div().flex_1().min_w_0().truncate().text_color(cx.theme().info).child("// WIP"))
        .child(
            div()
                .flex_none()
                .w(px(AUTHOR_W + DATE_W))
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(muted)
                .child(format!("{}/{} staged", ws.status.staged.len(), changes)),
        )
}

/// One commit row; click selects the commit (details on the right), and
/// right-click offers checkout-at-commit / create branch / create tag.
fn commit_row(
    ws: &Workspace,
    workspace: &WeakEntity<Workspace>,
    commit: &Commit,
    cx: &App,
) -> gpui_kit::AnyElement {
    let border = cx.theme().border;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;
    let oid = commit.id;
    let is_selected = ws.selected_commit.as_ref().is_some_and(|sel| sel.id == oid);
    let refs = ws.refs_by_commit.get(&oid).map(Vec::as_slice).unwrap_or(&[]);

    h_flex()
        .id(SharedString::from(format!("commit-{oid}")))
        .w_full()
        .h_7()
        .flex_none()
        .px_3()
        .gap_3()
        .items_center()
        .border_b_1()
        .border_color(border.opacity(0.5))
        .text_size(px(12.))
        .when(is_selected, |this| this.bg(cx.theme().accent))
        .when(!is_selected, |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.select_commit(oid, cx)).ok();
            }
        })
        // refs | graph | title | author | date
        .child(refs::ref_chips(refs, oid, workspace, cx))
        // Graph lane placeholder: a plain node dot; the real graph (edges,
        // lanes) comes later.
        .child(
            div()
                .flex_none()
                .w(px(GRAPH_W))
                .min_w_0()
                .flex()
                .justify_center()
                .child(div().size_2().rounded_full().bg(muted.opacity(0.6))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(foreground)
                .child(commit.summary.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(AUTHOR_W))
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(muted)
                .child(commit.author.name.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(DATE_W))
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(muted)
                .child(commit.author.date_time()),
        )
        .context_menu({
            let workspace = workspace.clone();
            let summary = commit.summary.clone();
            move |menu, _, _| commit_menu(menu, oid, &summary, &workspace)
        })
        .into_any_element()
}

/// The commit row's menu: detach at this commit, or attach a branch / tag
/// here via the shared name dialog.
fn commit_menu(
    mut menu: PopupMenu,
    oid: Oid,
    summary: &str,
    workspace: &WeakEntity<Workspace>,
) -> PopupMenu {
    // Checkout this commit (detached HEAD)
    menu = menu.item(
        PopupMenuItem::new("Checkout this commit").icon(IconName::GitCommitHorizontal).on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.checkout_commit(oid, cx)).ok();
            }
        }),
    );

    menu = menu.separator();

    // Create branch / tag here (name dialogs)
    menu =
        menu.item(PopupMenuItem::new("Create branch here").icon(IconName::GitBranch).on_click({
            let workspace = workspace.clone();
            let summary = summary.to_owned();
            move |_, window, cx| {
                workspace
                    .update(cx, |ws, cx| ws.open_create_branch_dialog(oid, &summary, window, cx))
                    .ok();
            }
        }));
    menu = menu.item(PopupMenuItem::new("Create tag here").icon(IconName::Tag).on_click({
        let workspace = workspace.clone();
        let summary = summary.to_owned();
        move |_, window, cx| {
            workspace
                .update(cx, |ws, cx| ws.open_create_tag_dialog(oid, &summary, window, cx))
                .ok();
        }
    }));

    menu
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;

    let workspace = cx.entity().downgrade();

    // The WIP row shows when the checked-out branch has pending changes:
    // nothing staged nor unstaged means no row at all.
    let changes = ws.status.staged.len() + ws.status.unstaged.len();
    let has_wip = changes > 0;
    let item_count = ws.log.len() + usize::from(has_wip);

    // Column labels, aligned with the row columns.
    let label = |title: &'static str, w: f32| {
        div()
            .flex_none()
            .w(px(w))
            .min_w_0()
            .truncate()
            .text_size(px(10.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(muted)
            .child(title)
    };

    v_flex()
        .size_full()
        .min_w_0()
        // header: title + count
        .child(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .items_center()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child("History"),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(muted)
                        .child(format!("{} commits", ws.log.len())),
                ),
        )
        // column labels
        .child(
            h_flex()
                .px_3()
                .py_1()
                .gap_3()
                .items_center()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(label("BRANCH / TAG", REFS_W))
                .child(label("GRAPH", GRAPH_W))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child("COMMIT MESSAGE"),
                )
                .child(label("AUTHOR", AUTHOR_W))
                .child(label("DATE", DATE_W)),
        )
        // virtualized body: WIP (when present) is item 0, commits follow.
        // Branch clicks drive `ws.log_scroll` via `scroll_to_item`.
        .child(
            uniform_list("commit-log", item_count, move |range, _window, cx| {
                let Some(entity) = workspace.upgrade() else { return Vec::new() };
                let ws = entity.read(cx);

                range
                    .map(|ix| {
                        // Must match the `has_wip` used for item_count above.
                        let has_wip = !ws.status.is_empty();
                        if has_wip && ix == 0 {
                            return wip_row(ws, &workspace, cx).into_any_element();
                        }
                        let index = ix - usize::from(has_wip);
                        match ws.log.get(index) {
                            Some(commit) => {
                                commit_row(ws, &workspace, commit, cx).into_any_element()
                            }
                            None => div()
                                .id(SharedString::from(format!("commit-empty-{ix}")))
                                .into_any_element(),
                        }
                    })
                    .collect()
            })
            .track_scroll(&ws.log_scroll)
            .flex_1()
            .min_h_0()
            .w_full(),
        )
}
