//! Headless perf bench for the RepoSnapshot read path (`workspace::actions`).
//!
//! Times every step of the auto-refresh snapshot against a real repository,
//! plus the ahead/behind pair count scaling. Run:
//!
//! ```sh
//! cargo run --release -p grit-core --example bench_snapshot -- /path/to/repo [runs]
//! ```

use grit_core::{Repo, graph::GraphData};
use std::time::{Duration, Instant};

fn bench(label: &str, runs: usize, f: impl Fn() -> usize) -> Duration {
    let mut total = Duration::ZERO;
    let mut best = Duration::MAX;
    let mut worst = Duration::ZERO;
    for _ in 0..runs {
        let start = Instant::now();
        let n = f();
        let elapsed = start.elapsed();
        total += elapsed;
        best = best.min(elapsed);
        worst = worst.max(elapsed);
        if n == usize::MAX {
            unreachable!()
        }
    }
    let avg = total / runs as u32;
    println!("{label:<46} avg {avg:>10.2?}  best {best:>10.2?}  worst {worst:>10.2?}");
    avg
}

fn main() {
    let repo_path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let runs: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(5);

    let repo = Repo::open(&repo_path).expect("open repo");
    println!("repo: {repo_path}  runs: {runs}\n");

    bench("status (full worktree walk)", runs, || {
        repo.status().expect("status");
        usize::MAX - 1
    });
    bench("branches()", runs, || repo.branches().expect("branches").len());
    bench("current_branch()", runs, || repo.current_branch().expect("current").is_some() as usize);
    bench("tags()", runs, || repo.tags().expect("tags").len());

    let log = repo.commit_log_all(500).expect("log");
    println!("commits loaded (max 500): {}\n", log.len());

    bench("commit_log_all(500)", runs, || repo.commit_log_all(500).expect("log").len());
    bench("GraphData::from_commits(loaded log)", runs, || {
        let g = GraphData::from_commits(&log);
        g.lanes.len()
    });
    bench("stashes + find_commit per stash", runs, || {
        let stashes = repo.stashes().expect("stashes");
        stashes.iter().filter(|s| repo.find_commit(s.oid).is_ok()).count()
    });

    // Ahead/behind: one topological paint pass + per-pair derivation
    // (git2's `graph_ahead_behind` is one bounded walk per pair).
    let branches = repo.branches().expect("branches");
    let mut tips: Vec<_> = branches.iter().map(|b| b.head).collect();
    tips.sort_unstable();
    tips.dedup();
    let pairs = tips.len() * (tips.len() - 1) / 2;
    println!("distinct branch tips: {}  pairs: {pairs}", tips.len());

    bench("ahead_behind_all(branch tips)", runs, || {
        repo.ahead_behind_all(&tips).expect("relations");
        usize::MAX - 1
    });

    // Whole snapshot as the app does it, including the clone of the previous
    // relations table (`refresh` does exactly this on every interval).
    let previous = repo.ahead_behind_all(&tips).expect("relations");
    bench("RepoSnapshot::read (full pass)", runs, || {
        let status = repo.status().expect("status");
        let branches = repo.branches().expect("branches");
        let _current = repo.current_branch().expect("current");
        let _tags = repo.tags().expect("tags");
        let log = repo.commit_log_all(500).expect("log");
        let _graph = GraphData::from_commits(&log);
        let stashes = repo.stashes().expect("stashes");
        let stash_commits: Vec<_> = stashes.iter().map(|s| repo.find_commit(s.oid).ok()).collect();
        let tips: Vec<_> = branches.iter().map(|b| b.head).collect();
        let relations = repo.ahead_behind_all(&tips).unwrap_or_else(|_| previous.clone());
        let _previous_clone = previous.clone();
        status.len() + stash_commits.len() + relations.len()
    });

    // select_commit path (runs synchronously on the UI thread on click):
    // find_commit + the full tree-to-tree changed-files diff, then the
    // per-file patch for the first changed file (the widest of the set).
    let Some(latest) = log.first().map(|c| c.id) else { return };
    bench("select_commit: find_commit", runs, || {
        repo.find_commit(latest).expect("commit");
        usize::MAX - 1
    });
    let files = repo.commit_changed_files(latest).expect("changed files");
    println!("changed files in latest commit: {}", files.len());
    bench("select_commit: commit_changed_files", runs, || {
        repo.commit_changed_files(latest).expect("changed").len()
    });
    if let Some(widest) = files.first() {
        let path = &widest.path;
        bench("click file: commit_file_diff(path 1)", runs, || {
            repo.commit_file_diff(latest, path).expect("diff").len()
        });
    }
}
