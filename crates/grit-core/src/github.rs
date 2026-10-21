//! GitHub authentication: the OAuth Device Flow plus token storage.
//!
//! Everything here is headless (no UI) and runs synchronously — calls are
//! expected on background threads like the git remote operations. HTTP goes
//! through the system `curl`, the same choice the app auto-updater makes:
//! TLS and proxy handling come from the OS toolchain and add no dependency.
//!
//! The Device Flow (RFC 8628) suits a local desktop app: no callback web
//! server, no redirect URL. Grit posts its `client_id` to
//! `github.com/login/device/code`, shows the returned short user code, and
//! polls the access-token endpoint while the user enters the code in their
//! browser. The issued token is stored in the user's login Keychain through
//! the `security` CLI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{process::Command, thread};

use serde_json::Value;

use crate::error::{Error, Result};

/// OAuth Device Flow: ask for a device + user code pair.
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
/// OAuth Device Flow: exchange a granted user code for an access token.
const ACCESS_TOKEN_URL: &str = "https://login/oauth/access_token";
/// Where the user enters the one-time `user_code`.
pub const DEVICE_VERIFICATION_URL: &str = "https://github.com/login/device";

const USER_AGENT: &str = concat!("Grit/", env!("CARGO_PKG_VERSION"));
/// Default polling cadence when the device-code response omits `interval`.
const DEFAULT_POLL_INTERVAL: u64 = 5;
/// Device codes expire after 15 minutes when the response omits `expires_in`.
const DEFAULT_EXPIRES_IN: u64 = 15 * 60;

/// OAuth App client identifier. The Device Flow needs only this value (no
/// client secret), but it must be the client id of a GitHub OAuth App
/// registered with the Device Flow grant type enabled — Grit's live
/// registration (github.com/settings/developers). An environment override
/// (`GRIT_GITHUB_CLIENT_ID`) covers dev builds pointing elsewhere.
const CLIENT_ID: &str = "Ov23liRfaiuAUP3YDNf1";

/// The client id to use for the Device Flow, `None` when sign-in is not
/// configured. The environment override lets a dev-build test with a
/// personal OAuth App without touching the constant.
pub fn client_id() -> Option<String> {
    match std::env::var("GRIT_GITHUB_CLIENT_ID") {
        Ok(id) if !id.trim().is_empty() => Some(id.trim().to_owned()),
        _ if !CLIENT_ID.trim().is_empty() => Some(CLIENT_ID.trim().to_owned()),
        _ => None,
    }
}

/// The device + user code pair issued by `github.com/login/device/code`.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    /// Opaque code presented on every token poll (never shown to the user).
    pub device_code: String,
    /// Short code the user enters on [`DEVICE_VERIFICATION_URL`].
    pub user_code: String,
    /// Where to send the user (always [`DEVICE_VERIFICATION_URL`] today,
    /// mirrored from the response for spec compliance).
    pub verification_uri: String,
    /// Minimum seconds between token polls.
    pub interval: u64,
    /// Seconds the pair stays valid.
    pub expires_in: u64,
}

// ---- device flow ---------------------------------------------------------

/// Ask GitHub for a device + user code pair. Blocking HTTP; background
/// threads only.
pub fn request_device_code(client_id: &str) -> Result<DeviceCode> {
    let body = format!("client_id={}&scope={}", form_encode(client_id), form_encode("repo"));
    let json = curl_post_json(DEVICE_CODE_URL, &body)?;

    // An invalid configuration comes back as a JSON error object (the HTTP
    // status itself may still be 200): surface the human-readable parts.
    if let Some(error) = json["error"].as_str() {
        let detail = json["error_description"].as_str().unwrap_or("no detail");
        // `slow_down` is a poll pacing signal, never a first response; any
        // other error cannot recover and reports where to fix it.
        return Err(Error::GitHub(if error == "slow_down" {
            "GitHub rate-limited sign-in (slow_down) — retry in a moment".to_owned()
        } else {
            format!("GitHub refused the sign-in request: {error} ({detail})")
        }));
    }

    Ok(DeviceCode {
        device_code: required(&json, "device_code")?,
        user_code: required(&json, "user_code")?,
        verification_uri: json["verification_uri"]
            .as_str()
            .unwrap_or(DEVICE_VERIFICATION_URL)
            .to_owned(),
        interval: json["interval"].as_u64().unwrap_or(DEFAULT_POLL_INTERVAL),
        expires_in: json["expires_in"].as_u64().unwrap_or(DEFAULT_EXPIRES_IN),
    })
}

/// Poll once; the caller owns pacing and cancellation.
#[derive(Debug, Clone, PartialEq)]
enum TokenPoll {
    /// The user granted the request during the previous interval.
    Granted { access_token: String },
    /// Not yet granted: keep polling.
    Pending,
    /// GitHub asked for a slower cadence (new minimum interval).
    SlowDown(u64),
}

/// One token poll. Blocked-off errors (`invalid_client`, …) come back as
/// `Err` and end the flow; only pacing states are retryable.
fn poll_token_once(client_id: &str, device_code: &str, interval: u64) -> Result<TokenPoll> {
    let body = format!(
        "client_id={}&device_code={}&grant_type={}",
        form_encode(client_id),
        form_encode(device_code),
        form_encode("urn:ietf:params:oauth:grant-type:device_code"),
    );
    let json = curl_post_json(ACCESS_TOKEN_URL, &body)?;

    let Some(error) = json["error"].as_str().map(str::to_owned) else {
        let token = required(&json, "access_token")?;
        if token.is_empty() {
            return Err(Error::GitHub("GitHub returned an empty access token".to_owned()));
        }
        return Ok(TokenPoll::Granted { access_token: token });
    };

    match error.as_str() {
        "authorization_pending" => Ok(TokenPoll::Pending),
        // GitHub mandates +5 seconds; the returned interval is authoritative.
        "slow_down" => {
            Ok(TokenPoll::SlowDown(interval.max(json["interval"].as_u64().unwrap_or(0))))
        }
        "expired_token" => {
            Err(Error::GitHub("the sign-in code expired — open the dialog and start again".into()))
        }
        other => {
            let detail = json["error_description"].as_str().unwrap_or("no detail");
            Err(Error::GitHub(format!("GitHub sign-in failed: {other} ({detail})")))
        }
    }
}

/// Wait for the user to complete authorization. Polls every `interval`
/// seconds (honoring `slow_down` bumps), until granted, expired, a
/// terminal error occurs, or `cancel` turns true. Blocked runs on a
/// background thread.
pub fn await_access_token(
    client_id: &str,
    device_code: &str,
    mut interval: u64,
    expires_in: u64,
    cancel: &AtomicBool,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(expires_in);
    loop {
        match poll_token_once(client_id, device_code, interval)? {
            TokenPoll::Granted { access_token } => return Ok(access_token),
            TokenPoll::Pending => {}
            TokenPoll::SlowDown(new) => interval = new,
        }
        // Sleep in quarter-second slices so cancellation is responsive (a
        // single interval-sized sleep would keep the thread blocked for
        // seconds after the dialog closed).
        let slices = interval * 4;
        for _ in 0..slices {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::GitHub("sign-in canceled".to_owned()));
            }
            thread::sleep(Duration::from_millis(250));
        }
        if Instant::now() >= deadline {
            return Err(Error::GitHub("the sign-in code expired — start again".to_owned()));
        }
    }
}

// ---- token storage (login Keychain through `security`) -------------------

/// Keychain service / account names this app owns.
const KEYCHAIN_SERVICE: &str = "grit-github";
const KEYCHAIN_ACCOUNT: &str = "oauth-token";

/// The stored GitHub token, `None` when absent (fresh install, deleted
/// entry). Not from a test: the login Keychain is UI-gated.
pub fn load_token() -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", KEYCHAIN_ACCOUNT, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None; // missing entry (exit 44) and same-account failure both mean "no token"
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!token.is_empty()).then_some(token)
}

/// Store the token (upsert: `-U` updates the existing entry). The token
/// rounds through the command line (`security` offers no stdin input for
/// `-w`), visible briefly in `ps` on a single-user machine — accepted for
/// a user-scoped token.
pub fn save_token(token: &str) -> Result<()> {
    let out = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            KEYCHAIN_ACCOUNT,
            "-w",
            token,
        ])
        .output()
        .map_err(Error::Io)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::GitHub(format!(
            "saving the GitHub token in the Keychain failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Remove the stored token (best effort; used on sign-out).
pub fn clear_token() {
    let _ = Command::new("security")
        .args(["delete-generic-password", "-s", KEYCHAIN_SERVICE, "-a", KEYCHAIN_ACCOUNT])
        .output();
}

// ---- HTTP plumbing --------------------------------------------------------

/// Run `curl` synchronously and parse the UTF-8 JSON body. Takes the
/// borrowed command because `Command`'s builder methods chain on
/// `&mut self` (`Command::args` returns `&mut Command`).
fn curl_json(cmd: &mut Command) -> Result<Value> {
    let out = cmd.output().map_err(Error::Io)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(Error::GitHub(if stderr.is_empty() {
            format!("curl exited with {}", out.status)
        } else {
            stderr
        }));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| Error::GitHub(format!("GitHub response unreadable: {e}")))
}

fn curl_post_json(url: &str, body: &str) -> Result<Value> {
    let mut cmd = Command::new("curl");
    cmd.args(["--fail", "--silent", "--show-error", "--location"])
        .arg("--user-agent")
        .arg(USER_AGENT)
        .arg("--header")
        .arg("Accept: application/json")
        .arg("--header")
        .arg("Content-Type: application/x-www-form-urlencoded")
        .arg("--data")
        .arg(body)
        .arg(url);
    curl_json(&mut cmd)
}

// ---- helpers --------------------------------------------------------------

fn required(json: &Value, key: &str) -> Result<String> {
    json[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::GitHub(format!("GitHub response missing {key}")))
}

/// Percent-encode a form value (RFC 3986 unreserved set kept verbatim;
/// everything else goes hex — space becomes `%20`, not `+`).
pub(crate) fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encode_keeps_unreserved() {
        assert_eq!(form_encode("abc-._~Z9"), "abc-._~Z9");
    }

    #[test]
    fn form_encode_escapes_the_rest() {
        assert_eq!(form_encode("a b"), "a%20b");
        assert_eq!(form_encode("urn:ietf:params"), "urn%3Aietf%3Aparams");
        assert_eq!(form_encode("+"), "%2B");
        assert_eq!(form_encode("café"), "caf%C3%A9");
    }

    #[test]
    fn form_encode_empty() {
        assert_eq!(form_encode(""), "");
    }
}
