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
