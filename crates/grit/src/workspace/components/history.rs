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
    ActiveTheme as _, Icon, h_flex,
    hover_card::HoverCard,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::branches::{branch_menu, remote_branch_menu};
use crate::workspace::{CommitRef, CommitRefKind, Workspace};
use grit_core::branch::BranchType;
use grit_core::{Commit, Oid};

use std::time::Duration;

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

/// The refs (branches / tags) pointing at a commit: a single chip, then a
/// muted "+N" whose HoverCard lists the remaining refs on hover.
fn ref_chips(refs: &[CommitRef], oid: Oid, workspace: &WeakEntity<Workspace>, cx: &App) -> Div {
    let mut cell = h_flex().flex_none().w(px(REFS_W)).min_w_0().gap_1().overflow_hidden();
    let Some(first) = refs.first() else { return cell };
    cell = cell.child(ref_chip(first, workspace, 140., cx));

    let extra = refs.len() - 1;
    if extra > 0 {
        let more: Vec<CommitRef> = refs[1..].to_vec();
        let muted = cx.theme().muted_foreground;
        cell = cell.child(
            HoverCard::new(SharedString::from(format!("refs-more-{oid}")))
                // Below the trigger, left-aligned with it.
                .anchor(Anchor::TopLeft)
                .open_delay(Duration::from_millis(300))
                .close_delay(Duration::from_millis(150))
                .flex_none()
                .trigger(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(muted)
                        .hover(|style| style.text_color(cx.theme().foreground))
                        .child(format!("+{extra}")),
                )
                .child({
                    let chips: Vec<_> =
                        more.iter().map(|r| ref_chip(r, workspace, 220., cx)).collect();
                    v_flex().gap_0p5().children(chips)
                }),
        );
    }
    cell
}

/// One ref as a small colored chip (in the log row and in the "+N" hover
/// card): local branch = primary, tag = success, remote = info. `max_w`
/// bounds the name before an ellipsis kicks in. Branch chips open the same
/// context menus as the sidebar on right-click.
fn ref_chip(
    r: &CommitRef,
    workspace: &WeakEntity<Workspace>,
    max_w: f32,
    cx: &App,
) -> gpui_kit::AnyElement {
    let color = match r.kind {
        CommitRefKind::Local => cx.theme().primary,
        CommitRefKind::Remote => cx.theme().info,
        CommitRefKind::Tag => cx.theme().success,
    };
    // Only tags carry their own icon; branch chips stay bare.
    let icon = match r.kind {
        CommitRefKind::Tag => Some(IconName::Tag),
        _ => None,
    };
    div()
        .min_w_0()
        .px_1()
        .rounded_sm()
        .bg(color.opacity(0.15))
        .text_size(px(11.))
        .text_color(color)
        .when(r.is_current, |this| this.font_weight(FontWeight::SEMIBOLD))
        .child(
            h_flex()
                .min_w_0()
                .gap_1()
                .when_some(icon, |this, icon| {
                    this.child(Icon::new(icon).size_3().text_color(color))
                })
                .child(div().max_w(px(max_w)).truncate().child(r.name.clone())),
        )
        // Same menus as the sidebar, read live when the menu opens.
        .context_menu({
            let workspace = workspace.clone();
            let r = r.clone();
            move |menu, _, cx| ref_menu(menu, &r, &workspace, cx)
        })
        .into_any_element()
}

/// The branches sidebar's menus on the log's ref chips. Only branches act —
/// tags have no actions yet, and an empty menu never opens.
fn ref_menu(
    menu: PopupMenu,
    r: &CommitRef,
    workspace: &WeakEntity<Workspace>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(entity) = workspace.upgrade() else { return menu };
    match r.kind {
        CommitRefKind::Local => {
            // Clone the branch up front: `branch_menu` mutably re-uses `cx`.
            let branch = entity
                .read(cx)
                .branches
                .iter()
                .find(|b| b.kind == BranchType::Local && b.name == r.name)
                .cloned();
            if let Some(branch) = branch {
                branch_menu(menu, &branch, r.is_current, workspace, cx)
            } else {
                menu
            }
        }
        CommitRefKind::Remote => remote_branch_menu(menu, &r.name, workspace, cx),
        CommitRefKind::Tag => menu,
    }
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
        .child(ref_chips(refs, oid, workspace, cx))
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
