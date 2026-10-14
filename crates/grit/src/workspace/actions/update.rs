//! macOS auto-updater: check the GitHub release, download + verify the
//! DMG, and swap the installed bundle.
//!
//! The app ships as a **signed & notarized `grit.app` in a DMG**, so the
//! updater cannot swap a single binary: that would invalidate the bundle
//! signature. The flow stages the whole replacement `.app` outside the
//! live installation, validates its signature **before** touching the
//! installed one, then swaps the bundles while keeping the
//! LaunchServices-known `.app` path. After a verified swap the running
//! process still executes the old code — the status bar's "Restart to
//! update" button applies it.
//!
//! Only macOS system tools are used (`curl`, `shasum`, `hdiutil`,
//! `codesign`, `spctl`, `open`), all shipped with the OS. Supported
//! target: Apple Silicon macOS — the single `aarch64-macos` artifact the
//! release builds; an Intel Mac gets no self-update offer.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use gpui_kit::*;

use super::Workspace;
use crate::workspace::{AvailableUpdate, UpdateState};

const API_LATEST: &str = "https://api.github.com/repos/Zareix/grit/releases/latest";
const USER_AGENT: &str = concat!("Grit/", env!("CARGO_PKG_VERSION"), " auto-update");
/// Auto-checks are throttled to this cooldown (marker file in
/// Application Support). Manual menu checks bypass it.
const CHECK_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// One entry of the release's `assets[]` array.
struct Asset {
    name: String,
    url: String,
    size: u64,
}

impl Workspace {
    /// Menu-driven check: always runs (ignores the cooldown), errors
    /// surface on the status bar like every other background operation.
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        self.launch_check(cx);
    }

    /// Launch-time check: skipped on unsupported platforms, throttled by
    /// the cooldown marker, silent on failure — network errors must keep
    /// the app usable.
    pub(crate) fn maybe_check_for_updates(&mut self, cx: &mut Context<Self>) {
        if !update_supported() {
            return;
        }
        if let Some(last) = read_last_check()
            && last.elapsed().unwrap_or(CHECK_COOLDOWN) < CHECK_COOLDOWN
        {
            return;
        }
        self.launch_check(cx);
    }

    fn launch_check(&mut self, cx: &mut Context<Self>) {
        if !update_supported() {
            self.info = Some("Self-update ships in the Apple Silicon build".into());
            cx.notify();
            return;
        }
        if matches!(self.update_state, UpdateState::Checking | UpdateState::Busy(_)) {
            return;
        }
        write_last_check();
        // Manual checks run even from a dev build (availability info is
        // still useful); only the *install* step refuses non-bundle runs.
        self.update_state = UpdateState::Checking;
        cx.notify();

        let job = cx.background_spawn(async move { fetch_latest_release() });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| match result {
                Ok(Some(update)) => {
                    this.update_state = UpdateState::Available(update);
                    cx.notify();
                }
                Ok(None) => {
                    this.update_state = UpdateState::Idle;
                    this.info =
                        Some(format!("Up to date (grit {})", env!("CARGO_PKG_VERSION")).into());
                    cx.notify();
                }
                Err(e) => {
                    this.update_state = UpdateState::Idle;
                    this.error = Some(format!("Update check failed: {e}").into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Confirm-dialog action: download the release DMG, verify its
    /// SHA-256 against the release's `SHA256SUMS` asset, mount it,
    /// validate the replacement bundle's signature, and swap bundles.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(update) = self.update_state.clone() else { return };
        self.update_state = UpdateState::Busy("Downloading update");
        cx.notify();

        let job = cx.background_spawn(async move { install(update) });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| match result {
                Ok(version) => {
                    this.update_state = UpdateState::ReadyToRestart;
                    this.info =
                        Some(format!("Grit v{version} installed — restart to apply").into());
                    cx.notify();
                }
                Err(e) => {
                    this.update_state = UpdateState::Idle;
                    this.error = Some(format!("Update install failed: {e}").into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// "Restart to update" action: launch the replaced bundle as a new
    /// process, then quit this one (the old process holds no locks).
    pub(crate) fn restart_after_update(&mut self, cx: &mut Context<Self>) {
        let Some(bundle) = running_bundle() else { return };
        match std::process::Command::new("open").arg("-n").arg(&bundle).spawn() {
            Ok(_) => cx.quit(),
            Err(e) => {
                self.error = Some(format!("Restart failed: {e}").into());
                cx.notify();
            }
        }
    }
}

// ---- release metadata --------------------------------------------------

/// Is this build an updater target? Apple Silicon macOS only: the release
/// ships a single `aarch64-macos` artifact, and an Intel Mac must not be
/// offered it just because both run macOS.
fn update_supported() -> bool {
    cfg!(target_os = "macos") && cfg!(target_arch = "aarch64")
}

/// Latest release check + target resolution. `Ok(None)` means nothing to
/// install: up to date, or this dev build is newer than the latest
/// release. No token in the binary — the repo is public and the check is
/// throttled.
fn fetch_latest_release() -> Result<Option<AvailableUpdate>, String> {
    let out = std::process::Command::new("curl")
        .arg("--fail")
        .arg("--silent")
        .arg("--show-error")
        .arg("--user-agent")
        .arg(USER_AGENT)
        .arg(API_LATEST)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(curl_error(&out, "release lookup"));
    }
    let release: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("GitHub release metadata unreadable: {e}"))?;

    let tag = release["tag_name"].as_str().ok_or("GitHub metadata missing tag_name")?.to_owned();
    let latest = tag.trim_start_matches('v');

    // Only offer strictly-newer versions, compared by numeric segments —
    // `0.10.0` must beat `0.9.4`, which a lexical comparison gets wrong.
    if version_sections(latest) <= version_sections(env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }

    let asset_name = format!("grit-{tag}-aarch64-macos.dmg");
    let assets = find_assets(&release)?;
    let dmg = assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| format!("release {tag} has no {asset_name} artifact for this platform"))?;
    let sums = assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .ok_or_else(|| format!("release {tag} has no SHA256SUMS asset"))?;
    let notes = release["body"].as_str().map(str::to_owned);

    Ok(Some(AvailableUpdate {
        version: latest.to_owned(),
        notes,
        asset: asset_name,
        url: dmg.url.clone(),
        checksum_url: sums.url.clone(),
        size: dmg.size,
    }))
}

/// `assets[]` entries as name / download URL / byte size. Malformed
/// entries (missing name or URL) are skipped, not fatal: only the assets
/// the flow actually needs may fail the check.
fn find_assets(release: &Value) -> Result<Vec<Asset>, String> {
    let entries = release["assets"].as_array().ok_or("GitHub metadata missing assets")?;
    Ok(entries
        .iter()
        .filter_map(|a| {
            Some(Asset {
                name: a["name"].as_str()?.to_owned(),
                url: a["browser_download_url"].as_str()?.to_owned(),
                size: a["size"].as_u64().unwrap_or(0),
            })
        })
        .collect())
}

/// `(0, 10, 0)` beats `(0, 9, 4)` where a lexical comparison does not.
fn version_sections(version: &str) -> (u64, u64, u64) {
    let mut next = version.split('.').map(|s| {
        s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u64>().unwrap_or(0)
    });
    let major = next.next().unwrap_or(0);
    let minor = next.next().unwrap_or(0);
    let patch = next.next().unwrap_or(0);
    (major, minor, patch)
}

// ---- install -----------------------------------------------------------

/// Download + verify + mount + swap, all blocking (background worker).
/// The installed bundle is only touched after the payload and its
/// signature verify; every failure leaves it functional.
fn install(update: AvailableUpdate) -> Result<String, String> {
    let version = update.version.clone();
    // Refuse before downloading anything: a dev run (`cargo run`) has no
    // .app bundle to swap into.
    let Some(bundle) = running_bundle() else {
        return Err("no installed .app bundle — updates install the packaged app, not dev \
                    builds"
            .into());
    };

    let staged = create_stage_dir()?;
    let result = install_staged(&bundle, &update, &staged);
    // Staging files are disposable whatever the outcome.
    let _ = std::fs::remove_dir_all(&staged);
    result?;

    Ok(version)
}

fn install_staged(bundle: &Path, update: &AvailableUpdate, staged: &Path) -> Result<(), String> {
    let dmg = staged.join(&update.asset);
    let sums = staged.join("SHA256SUMS");

    // 1. Stage outside the live installation: download + full-payload
    //    verification before anything else moves.
    download_to(&update.url, &dmg)?;
    download_to(&update.checksum_url, &sums)?;
    verify_checksum(&dmg, &sums, &update.asset)?;

    // 2. Mount, locate the replacement and validate its signature before
    //    the installed bundle is touched.
    let mount = hdiutil_attach(&dmg)?;
    let outcome = match find_app(&mount) {
        Some(new_app) => {
            codesign_verify(&new_app).and_then(|_| install_bundle_pair(&new_app, bundle))
        }
        None => Err(format!("no .app bundle in the mounted image at {mount:?}")),
    };
    let _ = hdiutil_detach(&mount);
    outcome
}

/// Move the running bundle aside, copy the staged one in, verify the
/// signature at its final location, then clean up. On any failure the
/// original bundle is restored.
fn install_bundle_pair(source: &Path, bundle: &Path) -> Result<(), String> {
    let backup = backup_path(bundle);
    std::fs::rename(bundle, &backup).map_err(|e| {
        format!("could not move {bundle:?} aside (read-only location or admin rights needed): {e}")
    })?;
    if let Err(e) = copy_bundle(source, bundle) {
        let _ = std::fs::rename(&backup, bundle);
        return Err(e);
    }
    if let Err(e) = codesign_verify(bundle) {
        let _ = std::fs::remove_dir_all(bundle);
        let _ = std::fs::rename(&backup, bundle);
        return Err(e);
    }
    // Verified copy in place; the GitHub release stays the recovery path
    // for anything beyond signature validation.
    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
}

/// `grit.app` → `grit.app.old` beside it (recovery until the new bundle
/// verifies).
fn backup_path(bundle: &Path) -> PathBuf {
    match bundle.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => bundle.with_file_name(format!("{stem}.app.old")),
        None => bundle.with_extension("app.old"),
    }
}

fn copy_bundle(source: &Path, bundle: &Path) -> Result<(), String> {
    let out = std::process::Command::new("cp")
        .arg("-R")
        .arg(source)
        .arg(bundle)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(format!("copying the new bundle failed: {stderr}"))
    }
}

// ---- checksum + image plumbing -----------------------------------------

/// Download to a destination file (`--location` follows GitHub's CDN
/// redirect to its asset host).
fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    let out = std::process::Command::new("curl")
        .arg("--fail")
        .arg("--silent")
        .arg("--show-error")
        .arg("--location")
        .arg("--user-agent")
        .arg(USER_AGENT)
        .arg("--output")
        .arg(dest)
        .arg(url)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(curl_error(&out, url)) }
}

fn curl_error(out: &std::process::Output, url: &str) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if stderr.is_empty() { format!("curl {url} failed with {}", out.status) } else { stderr }
}

/// The release's `SHA256SUMS` is plain text; the line naming `asset`
/// carries the expected digest (`<hex>  <path>`).
fn expected_digest(manifest: &str, asset: &str) -> Option<String> {
    for line in manifest.lines() {
        let mut parts = line.split_whitespace();
        let (digest, name) = (parts.next()?, parts.next()?);
        // Checksums list bare filenames (the collect step writes them
        // from the download directory).
        if name.rsplit('/').next() == Some(asset) {
            return Some(digest.to_lowercase());
        }
    }
    None
}

/// The staged DMG's SHA-256 via the system `shasum`.
fn sha256_digest(file: &Path) -> Result<String, String> {
    let out = std::process::Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(file)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("shasum failed with {}", out.status));
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or("shasum produced no digest".into())
}

fn verify_checksum(dmg: &Path, sums: &Path, asset: &str) -> Result<(), String> {
    let manifest =
        std::fs::read_to_string(sums).map_err(|e| format!("staged SHA256SUMS unreadable: {e}"))?;
    let expected = expected_digest(&manifest, asset)
        .ok_or_else(|| format!("{asset} missing from SHA256SUMS"))?;
    let actual = sha256_digest(dmg)?;
    if actual != expected {
        Err(format!("SHA-256 mismatch for {asset}: expected {expected}, got {actual}"))
    } else {
        Ok(())
    }
}

/// Attach the DMG and return its mount point, parsed from the `-plist`
/// output (`<key>mount-point</key><string>/Volumes/…</string>`). Names
/// are unescaped XML-free paths, so no entities need decoding.
fn hdiutil_attach(dmg: &Path) -> Result<PathBuf, String> {
    let out = std::process::Command::new("hdiutil")
        .arg("attach")
        .arg("-nobrowse")
        .arg("-plist")
        .arg(dmg)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(format!("hdiutil attach: {stderr}"));
    }
    let plist = String::from_utf8_lossy(&out.stdout);
    plist
        .split("<key>mount-point</key><string>")
        .nth(1)
        .and_then(|rest| rest.split("</string>").next())
        .map(|mount| PathBuf::from(mount.trim()))
        .ok_or_else(|| "hdiutil attach reported no mount point".into())
}

fn hdiutil_detach(mount: &Path) -> Result<(), String> {
    let out = std::process::Command::new("hdiutil")
        .arg("detach")
        .arg("-force")
        .arg(mount)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("hdiutil detach {mount:?} failed with {}", out.status))
    }
}

/// The single `.app` bundle in the mounted image.
fn find_app(mount: &Path) -> Option<PathBuf> {
    std::fs::read_dir(mount)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension() == Some("app".as_ref()) && p.is_dir())
}

/// Signature gate: strict code verification plus notarization
/// assessment, run on the staged bundle before anything is replaced.
fn codesign_verify(app: &Path) -> Result<(), String> {
    let verify = std::process::Command::new("codesign")
        .arg("--verify")
        .arg("--deep")
        .arg("--strict")
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !verify.status.success() {
        let stderr = String::from_utf8_lossy(&verify.stderr).trim().to_owned();
        return Err(format!("replacement bundle signature invalid: {stderr}"));
    }
    let assess = std::process::Command::new("spctl")
        .arg("--assess")
        .arg("--type")
        .arg("execute")
        .arg("-v")
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !assess.status.success() {
        let stderr = String::from_utf8_lossy(&assess.stderr).trim().to_owned();
        return Err(format!("replacement bundle failed notarization: {stderr}"));
    }
    Ok(())
}

// ---- local paths -------------------------------------------------------

/// Throttle marker: `~/Library/Application Support/grit/last-check`,
/// epoch seconds of the last check attempt.
fn marker_path() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("grit");
    if !path.is_dir() {
        std::fs::create_dir_all(&path).ok()?;
    }
    path.push("last-check");
    Some(path)
}

fn read_last_check() -> Option<SystemTime> {
    marker_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.trim().parse::<u64>().ok())
        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
}

fn write_last_check() {
    if let Some(path) = marker_path() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        std::fs::write(path, now.as_secs().to_string()).ok();
    }
}

/// The `.app` bundle this process runs from, if packaged (dev/cargo run
/// lives in target/ and has no .app ancestor).
fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).find(|p| p.extension() == Some("app".as_ref())).map(Path::to_owned)
}

/// Staging directory outside the live installation:
/// `~/Library/Application Support/grit/update`.
fn create_stage_dir() -> Result<PathBuf, String> {
    let mut dir = dirs::data_dir().ok_or("no Application Support directory on this system")?;
    dir.push("grit");
    dir.push("update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("staging directory: {e}"))?;
    Ok(dir)
}
