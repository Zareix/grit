//! GitHub OAuth Device Flow sign-in: dialog, background polling, cancel.
//!
//! All entry points hold a window (dialog opens synchronously); every
//! network step runs on the background executor like the remote git
//! operations — only state flips land on the UI thread. The generation
//! counter (`github.seq`) plus the `sign_in_cancel` flag make strays
//! harmless: a token poll landing after a cancel or restart is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::*;

use crate::workspace::components::github::SignInPanel;
use crate::workspace::{GitHubPhase, Workspace};

/// The device-flow parameters the pending token poll needs (kept together
/// to not strand the poll with an argument list).
struct DeviceFlowGrant {
    client_id: String,
    device_code: String,
    interval: u64,
    expires_in: u64,
}

impl Workspace {
    /// Restore a previous session on repo open: one background Keychain
    /// read, then the SignedIn phase (login unresolved at this point).
    pub(crate) fn init_github_auth(&mut self, cx: &mut Context<Self>) {
        self.github.seq += 1;
        let seq = self.github.seq;
        let job = cx.background_spawn(async move { grit_core::github::load_token() });
        cx.spawn(async move |this, cx| {
            let token = job.await;
            this.update(cx, |this, cx| {
                if seq == this.github.seq
                    && let Some(token) = token
                {
                    this.github.token = Some(token);
                    this.github.phase = GitHubPhase::SignedIn { login: None };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Open the sign-in dialog; (re)starts the device flow from the panel's
    /// own state. The body is a live view following [`GitHubPhase`], so the
    /// code appears in place when the device-code request lands.
    pub(crate) fn open_sign_in_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let panel = cx.new(|cx| SignInPanel::new(&entity, cx));
        self.start_sign_in(cx);

        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Sign in to GitHub").child(panel.clone()).footer(
                DialogFooter::new().child(Button::new("cancel").label("Cancel").on_click({
                    let weak = weak.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        weak.update(cx, |ws, cx| ws.cancel_sign_in(cx)).ok();
                    }
                })),
            )
        });
    }

    /// Request the device + user code pair, then wait for the grant.
    /// Short-circuits while a flow is live (request in flight, code on
    /// screen, already signed in); `Failed` restarts.
    pub(crate) fn start_sign_in(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.github.phase,
            GitHubPhase::Connecting
                | GitHubPhase::AwaitingDevice { .. }
                | GitHubPhase::SignedIn { .. }
        ) {
            cx.notify();
            return;
        }
        let Some(client_id) = grit_core::github::client_id() else {
            self.github.phase = GitHubPhase::Failed(
                "GitHub sign-in is not configured in this build: no OAuth App client id. \
                 Set the GRIT_GITHUB_CLIENT_ID environment variable, or embed a \
                 registered OAuth App client id."
                    .to_owned()
                    .into(),
            );
            cx.notify();
            return;
        };

        // A stray previous flow (dialog closed without Cancel) stops here.
        self.github.seq += 1;
        let seq = self.github.seq;
        let cancel = Arc::new(AtomicBool::new(false));
        self.github.sign_in_cancel = cancel.clone();
        self.github.phase = GitHubPhase::Connecting;
        cx.notify();

        let poll_client_id = client_id.clone();
        let job =
            cx.background_spawn(async move { grit_core::github::request_device_code(&client_id) });
        cx.spawn(async move |this, cx| {
            let opened = job.await;
            this.update(cx, |this, cx| {
                if seq != this.github.seq {
                    return; // superseded (cancel, restart, repo switch)
                }
                let code = match opened {
                    Ok(code) => code,
                    Err(e) => {
                        this.github.phase = GitHubPhase::Failed(e.to_string().into());
                        cx.notify();
                        return;
                    }
                };
                this.github.phase = GitHubPhase::AwaitingDevice {
                    user_code: code.user_code.into(),
                    verification_uri: code.verification_uri.into(),
                };
                cx.notify();
                this.await_grant(
                    DeviceFlowGrant {
                        client_id: poll_client_id,
                        device_code: code.device_code,
                        interval: code.interval,
                        expires_in: code.expires_in,
                    },
                    cancel,
                    seq,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Background wait for the user's authorization (the code is on
    /// screen); persists the token in the Keychain on success.
    fn await_grant(
        &mut self,
        flow: DeviceFlowGrant,
        cancel: Arc<AtomicBool>,
        seq: u64,
        cx: &mut Context<Self>,
    ) {
        let DeviceFlowGrant { client_id, device_code, interval, expires_in } = flow;
        let job = cx.background_spawn(async move {
            let token = grit_core::github::await_access_token(
                &client_id,
                &device_code,
                interval,
                expires_in,
                &cancel,
            )?;
            grit_core::github::save_token(&token)?;
            Ok::<String, grit_core::error::Error>(token)
        });
        cx.spawn(async move |this, cx| {
            let granted = job.await;
            this.update(cx, |this, cx| {
                if seq != this.github.seq {
                    return; // superseded
                }
                match granted {
                    Ok(token) => {
                        this.github.token = Some(token);
                        this.github.phase = GitHubPhase::SignedIn { login: None };
                        this.info = Some("Signed in to GitHub".into());
                    }
                    Err(e) => {
                        this.github.phase = GitHubPhase::Failed(e.to_string().into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Dialog Cancel: stop the poll loop and return to signed-out. (The
    /// dialog itself is closed by the button before this runs.)
    pub(crate) fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        if matches!(self.github.phase, GitHubPhase::Connecting | GitHubPhase::AwaitingDevice { .. })
        {
            self.github.seq += 1;
            self.github.sign_in_cancel.store(true, Ordering::Relaxed);
            self.github.phase = GitHubPhase::SignedOut;
            cx.notify();
        }
    }
}
