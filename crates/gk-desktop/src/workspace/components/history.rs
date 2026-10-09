//! Center column: commit history list.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;

    let log_rows = ws.log.iter().map(|c| {
        let short_id = c.id.to_string().chars().take(7).collect::<String>();
        h_flex()
            .w_full()
            .px_3()
            .py_1()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(border.opacity(0.5))
            .text_size(px(12.))
            .child(div().flex_none().w_16().text_size(px(11.)).text_color(muted).child(short_id))
            .child(
                div().flex_1().min_w_0().truncate().text_color(foreground).child(c.summary.clone()),
            )
            .child(
                div().flex_none().text_size(px(11.)).text_color(muted).child(c.author.name.clone()),
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
        .child(div().id("commit-log").flex_1().min_h_0().overflow_y_scroll().children(
            if ws.log.is_empty() {
                vec![
                    div()
                        .flex()
                        .justify_center()
                        .py_8()
                        .text_sm()
                        .text_color(muted)
                        .child("No commits yet"),
                ]
            } else {
                log_rows.collect()
            },
        ))
}
