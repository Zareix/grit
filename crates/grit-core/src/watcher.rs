//! Repository watching: report worktree / `.git` changes so the app can
//! re-check status (staged files, branches) without waiting for the
//! periodic timer.
//!
//! The `notify` recommended watcher (FSEvents on macOS, inotify on Linux)
//! covers the whole repository recursively, `.git` included — index, HEAD
//! and refs updates are what make branch state change on disk. A raw watch
//! is far too noisy for a refresh trigger (build output, `.git/objects`
//! churn), so batches are filtered through git's own notion of "changes":
//! gitignored files and the object database are dropped. The caller must
//! keep the `RepoWatcher` alive: dropping it disconnects the events
//! channel, which is how the waiter detects a closed workspace.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::repo::Repo;

/// Events forwarded by the watcher. The sender lives inside the watcher;
/// the waiter drains this receiver on the background thread. (`Arc` so the
/// waiter job can hold a cheap clone while the workspace owns the channel.)
pub type WatchEvents = Arc<Mutex<std::sync::mpsc::Receiver<notify::Result<Event>>>>;

/// After the first event, silence for this long ends the batch: an editor
/// save (or a git command) flushes a burst of writes and one refresh for
/// the whole burst is enough.
const QUIET: Duration = Duration::from_millis(300);

/// The object database: bulk churn on every fetch / gc / commit, never a
/// status change by itself. Branch and HEAD / index updates stay watched.
const OBJECTS_DIR: &str = ".git/objects";

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

/// What one wait cycle learned: a change git considers a status change
/// (`Changed`), a batch of unrelated writes (`Idle` — gitignored files,
/// build output, object-database churn), or the channel closed (`Closed` —
/// the watcher was dropped, the workspace is gone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchBatch {
    Changed,
    Idle,
    Closed,
}

/// Block until the next on-disk change worth a refresh (run on a background
/// thread). Unrelated batches are swallowed and the wait continues: build
/// output must not spin the UI.
pub fn wait_batch(events: &WatchEvents, repo: &Repo) -> WatchBatch {
    let rx = events.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        // First change (blocks until it happens, or the watcher is dropped).
        if rx.recv().is_err() {
            return WatchBatch::Closed;
        }
        // Collect the burst that follows the first event.
        let mut paths: HashSet<PathBuf> = HashSet::new();
        loop {
            match rx.recv_timeout(QUIET) {
                Ok(Ok(event)) => paths.extend(event.paths.iter().cloned()),
                // Watcher internal error (e.g. a watched dir was removed and
                // re-added): harmless, nothing to do.
                Ok(Err(_)) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return WatchBatch::Closed,
            }
        }
        if batch_interesting(&paths, repo) {
            return WatchBatch::Changed;
        }
        // Unrelated batch: keep waiting on the same parked thread.
    }
}

/// Whether any path of the batch is a change git tracks (or could track —
/// untracked files count). `true` also when the answer cannot be
/// determined: a missed refresh is worse than a spurious one.
fn batch_interesting(paths: &HashSet<PathBuf>, repo: &Repo) -> bool {
    let root = &repo.path;
    let Ok(git2_repo) = repo.git2() else { return true };
    for path in paths {
        let Ok(rel) = path.strip_prefix(root) else { continue }; // outside the repo
        if rel.starts_with(OBJECTS_DIR) {
            continue;
        }
        match git2_repo.is_path_ignored(rel) {
            Ok(false) => return true,
            Ok(true) => {}
            // Unresolvable (encoding, missing parents…): refresh to be safe.
            Err(_) => return true,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_repo() -> (Repo, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        (Repo::open(dir.path()).unwrap(), dir)
    }

    /// Run `wait_batch` on a thread and report its outcome through a
    /// channel, so a test can assert that uninteresting batches do NOT
    /// return.
    fn wait_on_thread(events: WatchEvents, repo: Repo) -> std::sync::mpsc::Receiver<WatchBatch> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(wait_batch(&events, &repo)).ok());
        rx
    }

    #[test]
    fn watch_detects_worktree_change() {
        let (repo, dir) = temp_repo();
        let watcher = RepoWatcher::new(dir.path()).unwrap();
        let rx = wait_on_thread(watcher.events.clone(), repo);

        // Allow the watch to settle, then touch a file.
        std::thread::sleep(std::time::Duration::from_millis(300));
        fs::write(dir.path().join("hello.txt"), "hi").unwrap();

        // FSEvent / inotify latency varies; allow up to 20 s.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut detected = None;
        while std::time::Instant::now() < deadline {
            if let Ok(outcome) = rx.recv_timeout(deadline - std::time::Instant::now()) {
                detected = Some(outcome);
                break;
            }
        }
        assert_eq!(detected, Some(WatchBatch::Changed), "tracked file change not reported");
    }

    #[test]
    fn watch_skips_gitignored_changes() {
        let (repo, dir) = temp_repo();
        // Written before the watcher exists: its own creation must not feed
        // the test's outcome.
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        let watcher = RepoWatcher::new(dir.path()).unwrap();
        let rx = wait_on_thread(watcher.events.clone(), repo);

        std::thread::sleep(std::time::Duration::from_millis(300));
        fs::create_dir_all(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/build.log"), "noise").unwrap();

        // Two seconds of build-output churn must not end a wait cycle.
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(
            rx.try_recv().is_err(),
            "gitignored file changes must not be reported as interesting"
        );

        // A tracked change still gets through, in the same wait cycle.
        fs::write(dir.path().join("hello.txt"), "hi").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if let Ok(outcome) = rx.recv_timeout(deadline - std::time::Instant::now()) {
                assert_eq!(outcome, WatchBatch::Changed, "tracked change must be reported");
                return;
            }
        }
        panic!("tracked change after gitignored churn was not reported");
    }

    #[test]
    fn wait_batch_ends_when_watcher_dropped() {
        let (repo, dir) = temp_repo();
        let watcher = RepoWatcher::new(dir.path()).unwrap();
        let events = watcher.events.clone();
        drop(watcher);
        assert_eq!(
            wait_batch(&events, &repo),
            WatchBatch::Closed,
            "dropped watcher must report closure"
        );
    }
}
