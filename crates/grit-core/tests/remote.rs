mod support;

use support::TestRepo;

#[test]
fn remotes_lists_configured_names() {
    let t = TestRepo::new();
    assert!(t.repo.remotes().unwrap().is_empty());

    let git_repo = git2::Repository::open(&t.repo.path).unwrap();
    git_repo.remote("origin", "https://example.com/repo.git").unwrap();
    git_repo.remote("upstream", "https://example.com/up.git").unwrap();

    let names = t.repo.remotes().unwrap();
    assert_eq!(names.first(), Some(&"origin".to_string()), "origin first");
    assert!(names.contains(&"upstream".to_string()));
}

#[test]
fn run_git_caps_git_output() {
    let t = TestRepo::new();
    // `git rev-parse HEAD` in an empty repo fails; the message flows through.
    let err = t.repo.run_git(&["rev-parse".to_string(), "HEAD".to_string()]).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("head"), "{err}");
}

/// A fetch prunes remote-tracking refs of branches deleted upstream: the
/// remotes tree must not keep ghosts of Renovate-style auto-deleted
/// branches. The remote is a local bare repo, so this needs no network.
#[test]
fn fetch_prunes_deleted_remote_branches() {
    let t = TestRepo::new();
    let origin_dir = tempfile::TempDir::new().unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "--bare", "-q", "-b", "main"])
        .arg(origin_dir.path())
        .output()
        .expect("init bare");

    // First commit + publish branch `feature` to the bare remote.
    t.write_and_stage("a.txt", "1");
    let tip = t.commit("init");
    let origin = origin_dir.path().to_string_lossy().to_string();
    t.cli(&["remote", "add", "origin", &origin]);
    t.cli(&["push", "-q", "origin", &format!("{tip}:refs/heads/feature")]);
    assert!(
        t.repo.branches().unwrap().iter().any(|b| b.name == "origin/feature"),
        "tracking ref exists before the delete"
    );

    // The branch is deleted upstream (GitHub auto-delete after merge).
    let _ = std::process::Command::new("git")
        .args(["-C", origin_dir.path().to_str().unwrap(), "branch", "-D", "feature"])
        .output()
        .expect("delete remote branch");

    t.repo.run_remote_op(grit_core::RemoteOp::Fetch).unwrap();
    assert!(
        !t.repo.branches().unwrap().iter().any(|b| b.name == "origin/feature"),
        "the fetch pruned the deleted branch's tracking ref"
    );
}
