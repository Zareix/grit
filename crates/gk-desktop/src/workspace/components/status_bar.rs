//! Bottom status bar: current branch + transient info / error messages.
//!
//! Messages are clipped to one line and reveal their full text on hover
//! through a HoverCard (the status bar sits at the bottom of the window, so
//! the card opens above the text via `Anchor::BottomLeft`).

use std::time::Duration;

use gpui_kit::component::{ActiveTheme as _, h_flex, hover_card::HoverCard};
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
        // transient info message (remote operation result)
        .when_some(ws.info.clone(), |this, info| {
            this.child(clipped_message("status-info", info, cx.theme().success))
        })
        .child(div().flex_1())
        .when_some(ws.error.clone(), |this, error| {
            this.child(clipped_message("status-error", error, cx.theme().danger))
        })
}

/// One-line clipped status message; hovering reveals the full text in a
/// HoverCard.
fn clipped_message(id: &'static str, text: SharedString, color: Hsla) -> HoverCard {
    HoverCard::new(id)
        // Open above the trigger: the bar sits at the bottom of the window.
        .anchor(Anchor::BottomLeft)
        .open_delay(Duration::from_millis(300))
        .close_delay(Duration::from_millis(150))
        .trigger(
            div().min_w_0().whitespace_nowrap().truncate().text_color(color).child(text.clone()),
        )
        .child(div().max_w(px(560.)).whitespace_normal().text_color(color).child(text))
}
