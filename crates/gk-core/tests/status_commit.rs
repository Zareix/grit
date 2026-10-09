mod support;

use gk_core::status::ChangeKind;
use support::TestRepo;

#[test]
fn status_reports_untracked_modified_staged() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "hello");
    t.commit("init");

    // New file, unmodified yet -> just a workdir change later.
    t.write("b.txt", "world");
    t.write("a.txt", "hello v2");

    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 0);
    let unstaged_paths: Vec<_> =
        status.unstaged.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
    assert!(unstaged_paths.contains(&"a.txt".to_string()));
    assert!(unstaged_paths.contains(&"b.txt".to_string()));
}

#[test]
fn stage_unstage_roundtrip() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "hello");
    t.commit("init");

    t.write("a.txt", "hello v2");
    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 0, "edit not staged yet");

    t.repo.stage_file("a.txt").unwrap();
    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 1);
    assert_eq!(status.staged[0].path.to_string_lossy(), "a.txt");
    assert_eq!(status.staged[0].kind, ChangeKind::Modified);

    t.repo.unstage_file("a.txt").unwrap();
    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 0);
    assert_eq!(status.unstaged.len(), 1);
}

#[test]
fn unstage_all_empties_staged() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");

    t.write("a.txt", "2");
    t.repo.stage_file("a.txt").unwrap();
    t.write("b.txt", "3");
    t.repo.stage_file("b.txt").unwrap();

    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 2);

    t.repo.unstage_all().unwrap();

    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 0);
    assert_eq!(status.unstaged.len(), 2);
}

#[test]
fn stage_all_stages_everything() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");

    t.write("a.txt", "2"); // modified
    t.write("b.txt", "3"); // untracked
    std::fs::remove_file(t.repo.path.join("a.txt")).ok();
    t.write("c.txt", "4");

    t.repo.stage_all().unwrap();
    let status = t.repo.status().unwrap();
    assert_eq!(status.unstaged.len(), 0, "everything should be staged");
    assert!(!status.staged.is_empty());
}

#[test]
fn unstage_all_on_unborn_head() {
    let t = TestRepo::new(); // no commit yet
    t.write("a.txt", "1");
    t.repo.stage_file("a.txt").unwrap();
    assert_eq!(t.repo.status().unwrap().staged.len(), 1);

    t.repo.unstage_all().unwrap();
    let status = t.repo.status().unwrap();
    assert_eq!(status.staged.len(), 0);
    assert_eq!(status.unstaged.len(), 1); // still untracked on disk
}

#[test]
fn commit_sets_head() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "first");
    let oid = t.commit("first commit");

    // HEAD points at our new commit with the right message.
    let head = t.repo.head_commit().unwrap().expect("head");
    assert_eq!(head.id, oid);
    assert_eq!(head.summary, "first commit");
    assert_eq!(head.author.email, "test@example.com");
}

#[test]
fn commit_log_newest_first() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let c1 = t.commit("c1");
    t.write_and_stage("a.txt", "2");
    let c2 = t.commit("c2");

    let log = t.repo.commit_log(10).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].id, c2);
    assert_eq!(log[1].id, c1);
}
