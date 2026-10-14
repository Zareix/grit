//! Commit box: message input + amend toggle + commit/amend button.

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    v_flex,
};
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let staged_count = ws.status.staged.len();
    // Amending needs a checked-out branch (thus a HEAD commit); a plain
    // commit needs something staged. An amend without staging is allowed
    // (message-only rewrite).
    let can_amend = ws.current_branch.is_some();
    let can_commit = if ws.amend { can_amend } else { staged_count > 0 };

    v_flex()
        .flex_none()
        .p_2()
        .gap_2()
        .border_t_1()
        .border_color(border)
        .child(Input::new(&ws.message))
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Checkbox::new("amend")
                        .label("Amend")
                        .text_sm()
                        .checked(ws.amend)
                        .disabled(!can_amend)
                        .on_change(
                            cx.listener(|this, _, window, cx| this.toggle_amend(window, cx)),
                        ),
                )
                .child(
                    Button::new("commit")
                        .primary()
                        .small()
                        .label(if ws.amend { "Amend" } else { "Commit" })
                        .disabled(!can_commit)
                        .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                ),
        )
}
