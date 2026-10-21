//! GitHub account session: the app-wide OAuth Device Flow, plus the
//! sign-in panel used by the Settings window's GitHub tab.
//!
//! The session is a Global holding one `Entity<GitHubSession>` created at
//! startup — one GitHub account for the whole app (repos just read the
//! phase later, e.g. for PR features). The Device Flow needs no window:
//! the request and the token poll run on background executors; whatever
//! view is showing the phase (Settings tab) re-renders via its own
//! observation. The token lives in the login Keychain through
//! `grit_core::github`.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

/// Device-flow phase; surfaced by [`SignInPanel`] where shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPhase {
    /// No token (idle; also the state right after a canceled sign-in).
    SignedOut,
    /// The device-code request is in flight.
    Connecting,
    /// The code is live: enter it at the verification URI before the
    /// expiry; the token poll runs in the background.
    AwaitingDevice {
        /// One-time code shown to the user.
        user_code: SharedString,
        /// Where to enter it (`github.com/login/device`).
        verification_uri: SharedString,
    },
    /// The last attempt failed; the panel offers a retry.
    Failed(SharedString),
    /// Connected; the login name is filled after sign-in (not resolved for
    /// restored Keychain sessions).
    SignedIn { login: Option<SharedString> },
}

/// The app-wide GitHub session: phase, cached token, and the generation /
/// cancel machinery that makes a stray poll landing after a cancel or a
/// restart harmless.
pub struct GitHubSession {
    phase: GitHubPhase,
    token: Option<String>,
    /// Generation counter invalidating superseded background jobs.
    seq: u64,
    /// Cancellation flag for the active token-poll loop.
    cancel: Arc<AtomicBool>,
}

/// Global handle to the session entity (registered by [`init`]).
#[derive(Clone)]
struct SessionRef(Entity<GitHubSession>);

impl Global for SessionRef {}

/// The session entity; panics before [`init`] — startup order guarantees
/// it runs before any window opens.
pub(crate) fn session(cx: &App) -> Entity<GitHubSession> {
    match cx.try_global::<SessionRef>() {
        Some(SessionRef(entity)) => entity.clone(),
        None => panic!("GitHub session not initialized (github::init before windows)"),
    }
}

/// Create the session and restore a previous Keychain token. Called once
/// at startup, before any window.
pub(crate) fn init(cx: &mut App) {
    let entity = cx.new(|cx| {
        let mut this = GitHubSession {
            phase: GitHubPhase::SignedOut,
            token: None,
            seq: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        this.restore(cx);
        this
    });
    cx.set_global(SessionRef(entity));
}

// ---- App-scoped entry points (Settings fields, toolbar) -------------------

pub(crate) fn start_sign_in(cx: &mut App) {
    session(cx).update(cx, |this, cx| this.start_sign_in(cx));
}

pub(crate) fn cancel_sign_in(cx: &mut App) {
    session(cx).update(cx, |this, cx| this.cancel_sign_in(cx));
}

pub(crate) fn sign_out(cx: &mut App) {
    session(cx).update(cx, |this, cx| this.sign_out(cx));
}

// ---- session --------------------------------------------------------------

impl GitHubSession {
    /// Restore a previous session at startup: one background Keychain
    /// read, then the SignedIn phase (login unresolved at this point).
    fn restore(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        let job = cx.background_spawn(async move { grit_core::github::load_token() });
        cx.spawn(async move |this, cx| {
            let token = job.await;
            this.update(cx, |this, cx| {
                if seq == this.seq
                    && let Some(token) = token
                {
                    this.token = Some(token);
                    this.phase = GitHubPhase::SignedIn { login: None };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Request the device + user code pair, then wait for the grant.
    /// Short-circuits while a flow is live (request in flight, code on
    /// screen, already signed in); `Failed` restarts.
    pub(crate) fn start_sign_in(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.phase,
            GitHubPhase::Connecting
                | GitHubPhase::AwaitingDevice { .. }
                | GitHubPhase::SignedIn { .. }
        ) {
            cx.notify();
            return;
        }
        let Some(client_id) = grit_core::github::client_id() else {
            self.phase = GitHubPhase::Failed(
                String::from(
                    "GitHub sign-in is not configured in this build: no OAuth App client id.",
                )
                .into(),
            );
            cx.notify();
            return;
        };

        // A stray previous flow (a dead poll loop) stops here.
        self.seq += 1;
        let seq = self.seq;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        self.phase = GitHubPhase::Connecting;
        cx.notify();

        let poll_client_id = client_id.clone();
        let job =
            cx.background_spawn(async move { grit_core::github::request_device_code(&client_id) });
        cx.spawn(async move |this, cx| {
            let opened = job.await;
            this.update(cx, |this, cx| {
                if seq != this.seq {
                    return; // superseded (cancel, restart)
                }
                let code = match opened {
                    Ok(code) => code,
                    Err(e) => {
                        this.phase = GitHubPhase::Failed(e.to_string().into());
                        cx.notify();
                        return;
                    }
                };
                this.phase = GitHubPhase::AwaitingDevice {
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
    /// screen wherever the panel shows); persists the token on success.
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
                if seq != this.seq {
                    return; // superseded
                }
                match granted {
                    Ok(token) => {
                        this.token = Some(token);
                        this.phase = GitHubPhase::SignedIn { login: None };
                    }
                    Err(e) => {
                        this.phase = GitHubPhase::Failed(e.to_string().into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Explicit cancel (the panel's button): stop the poll loop and
    /// return to signed-out.
    pub(crate) fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        if matches!(self.phase, GitHubPhase::Connecting | GitHubPhase::AwaitingDevice { .. }) {
            self.seq += 1;
            self.cancel.store(true, Ordering::Relaxed);
            self.phase = GitHubPhase::SignedOut;
            cx.notify();
        }
    }

    /// Drop the account: token leaves the Keychain (background job), any
    /// surviving flow is invalidated first.
    pub(crate) fn sign_out(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.phase, GitHubPhase::SignedIn { .. }) {
            return;
        }
        self.seq += 1;
        self.phase = GitHubPhase::SignedOut;
        self.token = None;
        cx.notify();
        let clear = cx.background_spawn(async move { grit_core::github::clear_token() });
        // Detached best-effort: the UI already shows signed-out; a failed
        // Keychain delete surfaces on the next sign-in's save (upsert).
        cx.spawn(async move |_, _| clear.await).detach();
    }
}

/// The device-flow parameters the pending token poll needs (kept together
/// to not strand the poll with an argument list).
struct DeviceFlowGrant {
    client_id: String,
    device_code: String,
    interval: u64,
    expires_in: u64,
}

// ---- sign-in panel ---------------------------------------------------------

/// The Device Flow UI block: status line + the code / retry / sign-out
/// controls matching [`GitHubPhase`]. Observed against the session, so the
/// phase changes (code arriving, grant landing) render in place wherever
/// this panel lives (Settings tab).
pub(crate) struct SignInPanel {
    session: Entity<GitHubSession>,
    /// Keeps the session observation alive for the view's lifetime.
    _observe: Subscription,
}

impl SignInPanel {
    pub(crate) fn new(session: &Entity<GitHubSession>, cx: &mut Context<Self>) -> Self {
        // Re-render whenever the phase advances; the session's other
        // updates never matter and a per-frame re-read is trivial.
        let _observe = cx.observe(session, |_, _, cx| cx.notify());
        Self { session: session.clone(), _observe }
    }
}

impl Render for SignInPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let danger = cx.theme().danger;

        let phase = self.session.read(cx).phase.clone();
        match phase {
            GitHubPhase::SignedOut => v_flex()
                .gap_2()
                .child(div().text_color(muted).child("Not connected to GitHub."))
                .child(
                    Button::new("sign-in")
                        .primary()
                        .label("Sign in to GitHub…")
                        .on_click(|_, _, cx| start_sign_in(cx)),
                )
                .into_any_element(),

            GitHubPhase::Connecting => v_flex()
                .gap_2()
                .child(div().text_color(muted).child("Connecting to GitHub…"))
                .into_any_element(),

            GitHubPhase::Failed(message) => v_flex()
                .gap_2()
                .child(div().text_color(danger).child(message))
                .child(
                    Button::new("retry")
                        .primary()
                        .label("Retry")
                        .on_click(|_, _, cx| start_sign_in(cx)),
                )
                .into_any_element(),

            GitHubPhase::AwaitingDevice { user_code, verification_uri } => {
                let code = user_code.to_string();
                let uri = verification_uri.to_string();
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(foreground)
                            .child("Enter this one-time code on GitHub:"),
                    )
                    .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(user_code))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("open-browser")
                                    .primary()
                                    .label("Open github.com/login/device")
                                    .on_click(move |_, _, cx| {
                                        let uri = uri.clone();
                                        cx.background_spawn(async move {
                                            let _ = Command::new("open").arg(uri).status();
                                        })
                                        .detach();
                                    }),
                            )
                            .child(Button::new("copy-code").label("Copy code").on_click(
                                move |_, _, cx| {
                                    let code = code.clone();
                                    cx.write_to_clipboard(ClipboardItem::new_string(code));
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("Waiting for authorization… the code expires when left unused."),
                    )
                    .child(
                        Button::new("cancel-sign-in")
                            .label("Cancel sign-in")
                            .on_click(|_, _, cx| cancel_sign_in(cx)),
                    )
                    .into_any_element()
            }

            GitHubPhase::SignedIn { login } => {
                let title = match login {
                    Some(login) => format!("Signed in to GitHub as {login}."),
                    None => "Signed in to GitHub.".to_owned(),
                };
                v_flex()
                    .gap_2()
                    .child(title)
                    .child(Button::new("sign-out").label("Sign out").on_click(|_, _, cx| {
                        sign_out(cx);
                    }))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("The token (repo scope) is stored in your login Keychain."),
                    )
                    .into_any_element()
            }
        }
    }
}
