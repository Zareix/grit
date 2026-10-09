//! Bottom status bar: current branch + errors.

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;

    h_flex()
        .px_3()
        .py_1()
        .gap_3()
        .items_center()
        .border_t_1()
        .border_color(border)
        .text_size(px(11.))
        .text_color(muted)
        .child(
            ws.current_branch
                .as_ref()
                .map(|b| b.name.clone())
                .unwrap_or_else(|| "no branch".into()),
        )
        .child(div().flex_1())
        .when_some(ws.error.clone(), |this, error| {
            this.child(div().text_color(cx.theme().danger).child(error))
        })
}
