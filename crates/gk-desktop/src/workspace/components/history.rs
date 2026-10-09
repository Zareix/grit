//! Center column: commit history list.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;

    // Viewing a branch's history shows its own log; otherwise HEAD's log.
    // Only the current branch's history carries the WIP row (staging is what
    // HEAD points at).
    let viewing_other = ws.viewed_branch.is_some();
    let log: &[gk_core::commit::Commit] = if viewing_other { &ws.branch_log } else { &ws.log };

    let wip_row = h_flex()
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
        .font_family(cx.theme().mono_font_family.clone())
        .when(ws.selected_commit.is_none(), |this| this.bg(cx.theme().accent.opacity(0.3)))
        .when(ws.selected_commit.is_some(), |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4)))
        })
        .on_click(cx.listener(|this, _, _, cx| this.close_commit(cx)))
        .child(div().flex_1().min_w_0().truncate().text_color(cx.theme().info).child("// WIP"))
        .child(
            div()
                .flex_none()
                .text_size(px(11.))
                .text_color(muted)
                .child(format!("{} staged", ws.status.staged.len())),
        );

    let log_rows = log.iter().map(|c| {
        let short_id = c.id.to_string().chars().take(7).collect::<String>();
        let oid = c.id;
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
            .on_click(cx.listener(move |this, _, _, cx| this.select_commit(oid, cx)))
            .child(div().flex_none().w_16().text_size(px(11.)).text_color(muted).child(short_id))
            .child(
                div().flex_1().min_w_0().truncate().text_color(foreground).child(c.summary.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .w_20()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(c.author.name.clone()),
            )
            .child(
                div().flex_none().text_size(px(11.)).text_color(muted).child(c.author.date_time()),
            )
    });

    v_flex()
        .size_full()
        .min_w_0()
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
                                    .font_weight(FontWeight::NORMAL)
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
        .child(div().id("commit-log").flex_1().min_h_0().overflow_y_scroll().children(
            if log.is_empty() {
                vec![
                    div()
                        .flex()
                        .justify_center()
                        .py_8()
                        .text_sm()
                        .text_color(muted)
                        .child("No commits yet")
                        .into_any_element(),
                ]
            } else if viewing_other {
                // Viewing another branch: no WIP (staging belongs to the
                // checked-out branch).
                log_rows.map(|r| r.into_any_element()).collect()
            } else {
                let mut children = vec![wip_row.into_any_element()];
                children.extend(log_rows.map(|r| r.into_any_element()));
                children
            },
        ))
}
