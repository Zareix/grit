//! Repository watching: report worktree / `.git` changes so the app can
//! re-check status (staged files, branches) without waiting for the
//! periodic timer.
//!
//! The `notify` recommended watcher (FSEvents on macOS, inotify on Linux)
//! covers the whole repository recursively, `.git` included — index, HEAD
//! and refs updates are what make branch state change on disk. The caller
//! must keep the `RepoWatcher` alive: dropping it disconnects the events
//! channel, which is how the waiter detects a closed workspace.

use std::path::Path;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};

use crate::error::{Error, Result};

/// Events forwarded by the watcher. The sender lives inside the watcher;
/// the waiter drains this receiver on the background thread. (`Arc` so the
/// waiter job can hold a cheap clone while the workspace owns the channel.)
pub type WatchEvents = Arc<Mutex<std::sync::mpsc::Receiver<notify::Result<Event>>>>;

/// After the first event, silence for this long ends the batch: an editor
/// save (or a git command) flushes a burst of writes and one refresh for
/// the whole burst is enough.
const QUIET: Duration = Duration::from_millis(300);

pub struct RepoWatcher {
    _watcher: notify::RecommendedWatcher,
    pub events: WatchEvents,
}

impl RepoWatcher {
    /// Watch `path` recursively (worktree + `.git`).
    pub fn new(path: &Path) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            // A closed receiver is the waiter's shutdown signal; drops here
            // are expected during teardown.
            let _ = tx.send(res);
        })
        .map_err(watch_error)?;
        watcher.watch(path, RecursiveMode::Recursive).map_err(watch_error)?;

        Ok(Self { _watcher: watcher, events: Arc::new(Mutex::new(rx)) })
    }
}

fn watch_error(err: notify::Error) -> Error {
    Error::Io(std::io::Error::other(err.to_string()))
}

/// Block until the next on-disk change (run on a background thread).
/// Returns `true` when a change batch arrived, `false` when the channel
/// disconnected (the watcher was dropped — stop waiting).
pub fn wait_batch(events: &WatchEvents) -> bool {
    let rx = events.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // First change (blocks until it happens, or the watcher is dropped).
    if rx.recv().is_err() {
        return false;
    }
    // Swallow the burst that follows the first event.
    loop {
        match rx.recv_timeout(QUIET) {
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_repo() -> (git2::Repository, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        (repo, dir)
    }

    #[test]
    fn watch_detects_worktree_change() {
        let (_repo, dir) = temp_repo();
        let watcher = RepoWatcher::new(dir.path()).unwrap();

        // Allow the watch to settle, then touch a file.
        std::thread::sleep(std::time::Duration::from_millis(300));
        fs::write(dir.path().join("hello.txt"), "hi").unwrap();

        // FSEvent / inotify latency varies; allow up to a few distinct
        // wait cycles (a batch typically lands within 1–2 s on macOS).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut detected = false;
        while std::time::Instant::now() < deadline {
            if wait_batch(&watcher.events) {
                detected = true;
                break;
            }
        }
        assert!(detected, "repo change was not reported by the watcher");
    }

    #[test]
    fn wait_batch_ends_when_watcher_dropped() {
        let (_repo, dir) = temp_repo();
        let watcher = RepoWatcher::new(dir.path()).unwrap();
        let events = watcher.events.clone();
        drop(watcher);
        assert!(!wait_batch(&events), "dropped watcher must report closure");
    }
}
