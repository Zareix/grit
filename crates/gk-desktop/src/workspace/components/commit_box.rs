//! Commit box: message input + commit button.

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    v_flex,
};
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let staged_count = ws.status.staged.len();

    v_flex()
        .flex_none()
        .p_2()
        .gap_2()
        .border_t_1()
        .border_color(border)
        .child(Input::new(&ws.message))
        .child(
            Button::new("commit")
                .primary()
                .small()
                .label("Commit")
                .disabled(staged_count == 0)
                .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
        )
}
