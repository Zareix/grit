//! Bottom status bar: current branch + transient info / error messages.
//!
//! Messages are clipped to one line and reveal their full text on hover
//! through a HoverCard (the status bar sits at the bottom of the window, so
//! the card opens above the text via `Anchor::BottomLeft`).

use std::time::Duration;

use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    hover_card::HoverCard,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::{UpdateState, Workspace};

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
        // Auto-updater surface: an indeterminate phase reads as text, a
        // pending action reads as a button (click opens its dialog).
        .when_some(update_widget(ws, cx), |this, widget| this.child(widget))
        .when_some(ws.error.clone(), |this, error| {
            this.child(clipped_message("status-error", error, cx.theme().danger))
        })
}

/// The updater's status-bar surface, if any:
/// - `Available` → button opening the install-confirmation dialog
/// - `ReadyToRestart` → button opening the restart-confirmation dialog
/// - `Checking`/`Busy` → indeterminate phase, read-only text.
fn update_widget(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let weak = cx.entity().downgrade();
    let element: AnyElement = match &ws.update_state {
        UpdateState::Available(_) => Button::new("update-available")
            .label("Update available")
            .primary()
            .on_click(move |_, window, cx| {
                weak.update(cx, |ws, cx| ws.open_update_dialog(window, cx)).ok();
            })
            .into_any_element(),
        UpdateState::ReadyToRestart => Button::new("restart-to-update")
            .label("Restart to update")
            .primary()
            .on_click(move |_, window, cx| {
                weak.update(cx, |ws, cx| ws.open_restart_dialog(window, cx)).ok();
            })
            .into_any_element(),
        UpdateState::Checking => {
            div().whitespace_nowrap().child("Checking for updates…").into_any_element()
        }
        UpdateState::Busy(phase) => {
            div().whitespace_nowrap().child(format!("{phase}…")).into_any_element()
        }
        UpdateState::Idle => return None,
    };
    Some(element)
}

/// One-line clipped status message; hovering reveals the full text in a
/// HoverCard.
fn clipped_message(id: &'static str, text: SharedString, color: Hsla) -> HoverCard {
    // git output carries explicit line breaks; whitespace_nowrap / truncate
    // never cut a '\n', so the trigger gets a whitespace-collapsed copy while
    // the hover card renders the original text.
    let one_line: SharedString = text.split_whitespace().collect::<Vec<_>>().join(" ").into();

    HoverCard::new(id)
        // Open above the trigger: the bar sits at the bottom of the window.
        .anchor(Anchor::BottomLeft)
        .open_delay(Duration::from_millis(300))
        .close_delay(Duration::from_millis(150))
        .trigger(div().min_w_0().whitespace_nowrap().truncate().text_color(color).child(one_line))
        .child(
            div()
                .max_w(px(560.))
                .whitespace_normal()
                .line_height(relative(1.4))
                .text_color(color)
                .child(text),
        )
}
