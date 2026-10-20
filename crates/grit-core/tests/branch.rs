mod support;

use grit_core::branch::BranchType;
use support::TestRepo;

#[test]
fn create_and_current_branch() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");

    let current = t.repo.current_branch().unwrap().expect("a branch");
    assert_eq!(current.name, "main");
    assert_eq!(current.kind, BranchType::Local);

    t.repo.create_branch("feature").unwrap();
    let names: Vec<_> = t
        .repo
        .branches()
        .unwrap()
        .into_iter()
        .filter(|b| b.kind == BranchType::Local)
        .map(|b| b.name)
        .collect();
    assert_eq!(names, vec!["feature".to_string(), "main".to_string()]);
}

#[test]
fn checkout_switches_head() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");
    t.repo.create_branch("feature").unwrap();

    t.repo.checkout_branch("feature").unwrap();
    assert_eq!(t.repo.current_branch().unwrap().unwrap().name, "feature");

    // Changes committed on feature are not on main.
    t.write_and_stage("feature-only.txt", "feats");
    t.commit("on feature");
    t.repo.checkout_branch("main").unwrap();
    assert!(!t.repo.path.join("feature-only.txt").exists());
}

#[test]
fn commit_log_ref_walks_any_branch() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");
    t.repo.create_branch("feature").unwrap();
    t.repo.checkout_branch("feature").unwrap();
    t.write_and_stage("feature-only.txt", "feats");
    let feature_tip = t.commit("on feature");
    t.repo.checkout_branch("main").unwrap();
    t.write_and_stage("main-only.txt", "main");
    let main_tip = t.commit("on main");

    let feature_log = t.repo.commit_log_ref("feature", 10).unwrap();
    assert_eq!(feature_log[0].id, feature_tip, "feature log goes to its tip");
    assert!(feature_log.iter().any(|c| c.summary == "c1"));

    let main_log = t.repo.commit_log_ref("main", 10).unwrap();
    assert_eq!(main_log[0].id, main_tip);
    assert!(
        !main_log.iter().any(|c| c.summary == "on feature"),
        "main must not include feature commits"
    );
}

#[test]
fn delete_branch_guards_current() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");
    t.repo.create_branch("feature").unwrap();

    // Deleting a non-checked-out branch works.
    t.repo.delete_branch("feature").unwrap();
    assert!(t.repo.branches().unwrap().iter().all(|b| b.name != "feature"));

    // Deleting the checked-out branch is refused.
    assert!(t.repo.delete_branch("main").is_err());
}

#[test]
fn checkout_remote_branch_creates_tracking_local() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");
    t.write_and_stage("b.txt", "2");
    let tip = t.commit("second");

    // Fabricate a remote-tracking ref (no network needed).
    let git_repo = git2::Repository::open(&t.repo.path).unwrap();
    git_repo.remote("origin", "https://example.com/repo.git").unwrap();
    git_repo.reference("refs/remotes/origin/feature", tip, true, "test: remote branch").unwrap();

    t.repo.checkout_remote_branch("origin/feature").unwrap();
    let current = t.repo.current_branch().unwrap().expect("a branch");
    assert_eq!(current.name, "feature");
    assert_eq!(current.head, tip);
    assert_eq!(current.upstream.as_deref(), Some("origin/feature"));
}

#[test]
fn checkout_remote_branch_reuses_existing_local() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");
    t.repo.create_branch("feature").unwrap();

    let git_repo = git2::Repository::open(&t.repo.path).unwrap();
    git_repo.remote("origin", "https://example.com/repo.git").unwrap();
    let tip = t.repo.current_branch().unwrap().unwrap().head;
    git_repo.reference("refs/remotes/origin/feature", tip, true, "test: remote branch").unwrap();

    // Local `feature` exists: it is checked out as-is (no upstream rewrite).
    t.repo.checkout_remote_branch("origin/feature").unwrap();
    let current = t.repo.current_branch().unwrap().expect("a branch");
    assert_eq!(current.name, "feature");
    assert_eq!(current.upstream, None);
}

#[test]
fn checkout_remote_branch_handles_nested_names() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let tip = t.commit("init");

    let git_repo = git2::Repository::open(&t.repo.path).unwrap();
    git_repo.remote("origin", "https://example.com/repo.git").unwrap();
    git_repo.reference("refs/remotes/origin/feature/x", tip, true, "test: remote branch").unwrap();

    t.repo.checkout_remote_branch("origin/feature/x").unwrap();
    let current = t.repo.current_branch().unwrap().expect("a branch");
    assert_eq!(current.name, "feature/x");
    assert_eq!(current.head, tip);
    assert_eq!(current.upstream.as_deref(), Some("origin/feature/x"));
}

#[test]
fn create_branch_at_specific_commit() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let first = t.commit("first");
    t.write_and_stage("b.txt", "2");
    t.commit("second");

    // The branch points at the first commit without touching HEAD, so the
    // worktree still reflects the second commit.
    t.repo.create_branch_at("at-first", first).unwrap();
    t.repo.checkout_branch("at-first").unwrap();

    let current = t.repo.current_branch().unwrap().expect("a branch");
    assert_eq!(current.name, "at-first");
    assert_eq!(current.head, first);
    assert!(!t.repo.path.join("b.txt").exists(), "worktree back at first");
}

#[test]
fn checkout_commit_detaches_head() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let first = t.commit("first");
    t.write_and_stage("b.txt", "2");
    t.commit("second");

    t.repo.checkout_commit(first).unwrap();

    assert!(t.repo.current_branch().unwrap().is_none(), "detached HEAD");
    assert!(!t.repo.path.join("b.txt").exists(), "worktree back at first");
}

/// Diverged branches: each side sees the other's unique-commit count, and
/// the relation is symmetric (rows mirror each other).
#[test]
fn ahead_behind_all_covers_pairs_and_symmetry() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");
    t.repo.create_branch("feature").unwrap();

    // main: c1 -> c2 / feature: c1 -> c3
    t.write_and_stage("main-only.txt", "m");
    t.commit("c2");
    t.repo.checkout_branch("feature").unwrap();
    t.write_and_stage("feature-only.txt", "f");
    t.commit("c3");
    t.repo.checkout_branch("main").unwrap();

    let mut heads = t.repo.branches().unwrap();
    heads.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(heads[0].head != heads[1].head, "tips diverged");
    let tips: Vec<_> = heads.iter().map(|b| b.head).collect();

    let relations = t.repo.ahead_behind_all(&tips).unwrap();
    for a in &heads {
        for b in &heads {
            if a.name == b.name {
                continue;
            }
            let (ahead, behind) = relations[&a.head][&b.head];
            // One commit on each side, whichever way the row is read.
            assert_eq!((ahead, behind), (1, 1), "{} vs {}", a.name, b.name);
        }
    }
}

/// Branches pointing at the same commit report (0, 0) both ways.
#[test]
fn ahead_behind_all_shared_tip_is_neutral() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let tip = t.commit("c1");
    t.repo.create_branch("feature").unwrap();

    let heads = t.repo.branches().unwrap();
    let tips: Vec<_> = heads.iter().map(|b| b.head).collect();
    let relations = t.repo.ahead_behind_all(&tips).unwrap();

    for a in &heads {
        for b in &heads {
            assert_eq!(relations[&a.head][&b.head], (0, 0), "{} vs {}", a.name, b.name);
        }
    }
    assert_eq!(heads[0].head, tip);
}

/// A branch strictly behind another reports zero ahead; the branch ahead
/// of it sees the same pair mirrored.
#[test]
fn ahead_behind_all_tracks_ancestry() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");
    let tip_before = t.repo.current_branch().unwrap().unwrap().head;
    t.repo.create_branch("behind").unwrap();
    t.write_and_stage("b.txt", "2");
    t.commit("c2");

    let heads = t.repo.branches().unwrap();
    let tips: Vec<_> = heads.iter().map(|b| b.head).collect();
    let relations = t.repo.ahead_behind_all(&tips).unwrap();

    let behind = heads.iter().find(|b| b.name == "behind").unwrap();
    let main = heads.iter().find(|b| b.name == "main").unwrap();
    let (ahead, total_behind) = relations[&behind.head][&main.head];
    assert_eq!(ahead, 0, "behind has no unique commits");
    assert_eq!(total_behind, 1, "main is one ahead");
    assert_eq!(behind.head, tip_before);
}

/// Reference check: the single-pass paint must agree with git2's own
/// pairwise `graph_ahead_behind` on every pair — including merge commits
/// (a merge makes the merged branch's commits shared with the target).
#[test]
fn ahead_behind_all_matches_pairwise_reference_with_merges() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");
    t.repo.create_branch("feature").unwrap();

    // main: c1 -> c2 -> (merge c3) -> c5; feature: c1 -> c3; side: at c2.
    t.write_and_stage("m.txt", "m");
    t.commit("c2");
    t.repo.create_branch("side").unwrap();
    t.repo.checkout_branch("feature").unwrap();
    t.write_and_stage("f.txt", "f");
    t.commit("c3");
    t.repo.checkout_branch("main").unwrap();
    t.cli(&["merge", "--no-ff", "-m", "merge feature", "feature"]);
    t.write_and_stage("m2.txt", "m2");
    t.commit("c5");

    let heads = t.repo.branches().unwrap();
    let tips: Vec<_> = heads.iter().map(|b| b.head).collect();
    let relations = t.repo.ahead_behind_all(&tips).unwrap();

    let git = git2::Repository::open(&t.repo.path).unwrap();
    for (i, &a) in tips.iter().enumerate() {
        for &b in tips.iter().skip(i + 1) {
            let (ahead, behind) = git.graph_ahead_behind(a, b).unwrap();
            assert_eq!(relations[&a][&b], (ahead, behind), "{} vs {}", a, b);
            assert_eq!(relations[&b][&a], (behind, ahead), "mirrored {} {}", b, a);
        }
    }

    // The merge really shares the feature commits: feature sees main as
    // ahead by c2, the merge commit and c5 — c3 is shared, not re-counted.
    let feature = heads.iter().find(|b| b.name == "feature").unwrap();
    let main = heads.iter().find(|b| b.name == "main").unwrap();
    assert_eq!(relations[&feature.head][&main.head], (0, 3));
}

/// Same reference check with more tips than one u64 paint mask (70
/// branches force the chunked path: 64 + 6).
#[test]
fn ahead_behind_all_matches_pairwise_reference_beyond_64_tips() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");

    // Each branch is one unique commit on top of c1.
    for i in 0..70 {
        t.repo.create_branch(&format!("b{i}")).unwrap();
        t.repo.checkout_branch(&format!("b{i}")).unwrap();
        t.write_and_stage(&format!("f{i}.txt"), "x");
        t.commit(&format!("c b{i}"));
    }
    t.repo.checkout_branch("main").unwrap();

    let heads = t.repo.branches().unwrap();
    assert!(heads.len() > 64);
    let tips: Vec<_> = heads.iter().map(|b| b.head).collect();
    let relations = t.repo.ahead_behind_all(&tips).unwrap();

    let git = git2::Repository::open(&t.repo.path).unwrap();
    for (i, &a) in tips.iter().enumerate() {
        for &b in tips.iter().skip(i + 1) {
            let (ahead, behind) = git.graph_ahead_behind(a, b).unwrap();
            assert_eq!(relations[&a][&b], (ahead, behind), "{} vs {}", a, b);
            assert_eq!(relations[&b][&a], (behind, ahead), "mirrored {} {}", b, a);
        }
    }
}
