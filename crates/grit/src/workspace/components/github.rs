//! The sign-in dialog's body: a live view over the Workspace's
//! [`GitHubPhase`]. The dialog builder renders once; this view keeps the
//! body current as the device-code request lands and the token poll advances.

use std::process::Command;

use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;

use crate::workspace::{GitHubPhase, Workspace};

/// Device-flow sign-in dialog content.
pub(crate) struct SignInPanel {
    workspace: WeakEntity<Workspace>,
    /// Keeps the phase observation alive for the view's lifetime.
    _observe: Subscription,
}

impl SignInPanel {
    pub(crate) fn new(workspace: &Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        // Re-render whenever the sign-in phase advances (code arriving,
        // grant landing, failure). The Workspace's redraws never matter —
        // only its `github` fields — and a per-frame re-read is trivial.
        let _observe = cx.observe(workspace, |_, _, cx| cx.notify());
        Self { workspace: workspace.downgrade(), _observe }
    }
}

impl Render for SignInPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let danger = cx.theme().danger;

        let Some(entity) = self.workspace.upgrade() else {
            return v_flex().gap_2().child("Workspace closed.").into_any_element();
        };
        let ws = entity.read(cx);
        match &ws.github.phase {
            GitHubPhase::SignedOut | GitHubPhase::Connecting => v_flex()
                .gap_2()
                .child(div().text_color(muted).child("Connecting to GitHub…"))
                .into_any_element(),

            GitHubPhase::Failed(message) => v_flex()
                .gap_2()
                .child(div().text_color(danger).child(message.clone()))
                .child(Button::new("retry").primary().label("Retry").on_click({
                    let workspace = self.workspace.clone();
                    move |_, _, cx| {
                        workspace.update(cx, |ws, cx| ws.start_sign_in(cx)).ok();
                    }
                }))
                .into_any_element(),

            GitHubPhase::AwaitingDevice { user_code, verification_uri } => {
                let code = user_code.clone();
                let uri = verification_uri.clone();
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(foreground)
                            .child("Enter this one-time code on GitHub:"),
                    )
                    .child(
                        div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(code.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("open-browser")
                                    .primary()
                                    .label("Open github.com/login/device")
                                    .on_click({
                                        let uri = uri.clone();
                                        move |_, _, cx| {
                                            let uri = uri.clone().to_string();
                                            cx.background_spawn(async move {
                                                let _ = Command::new("open").arg(uri).status();
                                            })
                                            .detach();
                                        }
                                    }),
                            )
                            .child(Button::new("copy-code").label("Copy code").on_click({
                                let code = code.clone();
                                move |_, _, cx| {
                                    let code = code.clone().to_string();
                                    cx.write_to_clipboard(ClipboardItem::new_string(code));
                                }
                            })),
                    )
                    .child(heading_for_waiting(muted))
                    .into_any_element()
            }

            GitHubPhase::SignedIn { login: Some(login) } => v_flex()
                .gap_2()
                .child(format!("Signed in to GitHub as {login}."))
                .into_any_element(),

            GitHubPhase::SignedIn { login: None } => {
                v_flex().gap_2().child("Signed in to GitHub.").into_any_element()
            }
        }
    }
}

fn heading_for_waiting(muted: gpui_kit::Hsla) -> gpui_kit::Div {
    div()
        .text_size(px(12.))
        .text_color(muted)
        .child("Waiting for authorization… the code expires when left unused.")
}
