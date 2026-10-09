mod support;

use support::TestRepo;

#[test]
fn diff_unstaged_shows_changes() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "hello");
    t.commit("init");

    t.write("a.txt", "hello world");
    let patch = t.repo.diff_unstaged("a.txt").unwrap();
    assert!(patch.contains("-hello"), "should contain the removed line:\n{patch}");
    assert!(patch.contains("+hello world"));
    assert!(patch.contains("@@"));
}

#[test]
fn diff_staged_shows_staged_changes() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "hello");
    t.commit("init");

    t.write("a.txt", "hello world");
    t.repo.stage_file("a.txt").unwrap();

    let staged = t.repo.diff_staged("a.txt").unwrap();
    assert!(staged.contains("+hello world"), "staged patch:\n{staged}");
    assert!(staged.contains("-hello"));

    // Nothing left unstaged for that file.
    let unstaged = t.repo.diff_unstaged("a.txt").unwrap();
    assert!(unstaged.is_empty(), "unstaged patch should be empty:\n{unstaged}");
}

#[test]
fn diff_of_untracked_file() {
    let t = TestRepo::new();
    t.write("new.txt", "brand new");
    let patch = t.repo.diff_unstaged("new.txt").unwrap();
    assert!(patch.contains("+brand new"), "patch:\n{patch}");
}

#[test]
fn diff_staged_on_unborn_head() {
    let t = TestRepo::new(); // no commit yet
    t.write_and_stage("a.txt", "first");
    let patch = t.repo.diff_staged("a.txt").unwrap();
    assert!(patch.contains("+first"), "patch:\n{patch}");
}

#[test]
fn diff_ignores_other_files() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "hello");
    t.commit("init");
    t.write("other.txt", "changed");
    t.write("a.txt", "changed too");

    let patch = t.repo.diff_unstaged("a.txt").unwrap();
    assert!(!patch.contains("other.txt"), "patch must be scoped to a.txt:\n{patch}");
}
