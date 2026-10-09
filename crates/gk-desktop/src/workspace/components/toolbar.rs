//! Top toolbar: app title + refresh.

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;

    h_flex()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("git-desktop"))
        .child(div().flex_1())
        .child(
            Button::new("refresh")
                .ghost()
                .small()
                .label("Refresh")
                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
        )
}
