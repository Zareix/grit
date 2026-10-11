//! Center column: commit history, virtualized (`uniform_list`).
//!
//! The WIP row — shown when the current branch is being viewed *and* has
//! pending changes — is item 0 of the same list so it scrolls with the
//! commits; every row is 28px tall, which is the uniform height
//! `uniform_list` requires.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::Workspace;
use grit_core::Commit;

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
        .when(ws.selected_commit.is_none(), |this| this.bg(cx.theme().accent.opacity(0.3)))
        .when(ws.selected_commit.is_some(), |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.close_commit(cx)).ok();
            }
        })
        .child(div().flex_1().min_w_0().truncate().text_color(cx.theme().info).child("// WIP"))
        .child(div().flex_none().text_size(px(11.)).text_color(muted).child(format!(
            "{}/{} staged",
            ws.status.staged.len(),
            changes
        )))
}

/// One commit row; click selects the commit (details on the right).
fn commit_row(
    ws: &Workspace,
    workspace: &WeakEntity<Workspace>,
    commit: &Commit,
    cx: &App,
) -> Stateful<Div> {
    let border = cx.theme().border;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;
    let short_id = commit.id.to_string().chars().take(7).collect::<String>();
    let oid = commit.id;
    let is_selected = ws.selected_commit.as_ref().is_some_and(|sel| sel.id == oid);

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
        .when(is_selected, |this| this.bg(cx.theme().accent.opacity(0.3)))
        .when(!is_selected, |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.select_commit(oid, cx)).ok();
            }
        })
        .child(div().flex_none().w_16().text_size(px(11.)).text_color(muted).child(short_id))
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
                .w_20()
                .truncate()
                .text_size(px(11.))
                .text_color(muted)
                .child(commit.author.name.clone()),
        )
        .child(
            div().flex_none().text_size(px(11.)).text_color(muted).child(commit.author.date_time()),
        )
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;

    let workspace = cx.entity().downgrade();
    let viewing_other = ws.viewed_branch.is_some();

    // WIP applies only to the checked-out branch's history, and only with
    // pending changes: nothing staged nor unstaged means no row at all.
    let changes = ws.status.staged.len() + ws.status.unstaged.len();
    let has_wip = !viewing_other && changes > 0;
    let log = if viewing_other { &ws.branch_log } else { &ws.log };
    let item_count = log.len() + usize::from(has_wip);

    v_flex()
        .size_full()
        .min_w_0()
        // header
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
                        .child("History")
                        // branch chip when viewing a specific branch
                        .when_some(ws.viewed_branch.clone(), |this, name| {
                            this.child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().info)
                                    .child(name),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(muted)
                        .child(format!("{} commits", log.len())),
                ),
        )
        // virtualized body: WIP (when present) is item 0, commits follow
        .child(
            uniform_list("commit-log", item_count, move |range, _window, cx| {
                let Some(entity) = workspace.upgrade() else { return Vec::new() };
                let ws = entity.read(cx);

                range
                    .map(|ix| {
                        let viewing_other = ws.viewed_branch.is_some();
                        // Must match the `has_wip` used for item_count above.
                        let has_wip = !viewing_other && !ws.status.is_empty();
                        if has_wip && ix == 0 {
                            return wip_row(ws, &workspace, cx).into_any_element();
                        }
                        let log = if viewing_other { &ws.branch_log } else { &ws.log };
                        let index = ix - usize::from(has_wip);
                        match log.get(index) {
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
            .flex_1()
            .min_h_0()
            .w_full(),
        )
}
