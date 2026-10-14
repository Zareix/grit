//! Stashes push/pop, listing entries.

mod support;

use support::TestRepo;

#[test]
fn stash_selected_files_roundtrip() {
    let mut t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");

    // Modify a tracked file and create an untracked one.
    t.write("a.txt", "1 changed");
    t.write("b.txt", "2");
    assert!(t.repo.path.join("b.txt").exists(), "untracked file present");

    t.repo.stash_paths(&["a.txt".into(), "b.txt".into()]).unwrap();

    // The stash list holds the change and the worktree is back at HEAD.
    assert_eq!(t.repo.stashes().unwrap().len(), 1);
    assert_eq!(t.read("a.txt"), "1");
    assert!(!t.repo.path.join("b.txt").exists(), "untracked file stashed");

    // A pop brings everything back and empties the stash.
    t.repo.stash_pop(0).unwrap();
    assert!(t.repo.stashes().unwrap().is_empty());
    assert_eq!(t.read("a.txt"), "1 changed");
    assert!(t.repo.path.join("b.txt").exists());
}

#[test]
fn stash_drop_removes_last_entry() {
    let mut t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("c1");
    t.write("a.txt", "2");
    t.repo.stash_paths(&["a.txt".into()]).unwrap();

    t.repo.stash_drop(0).unwrap();
    assert!(t.repo.stashes().unwrap().is_empty());
}
