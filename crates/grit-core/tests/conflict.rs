//! In-progress operations (merge) and conflict resolution plumbing.

mod support;

use std::process::Command;

use grit_core::{
    Operation,
    status::{ChangeKind, ConflictSide},
};
use support::TestRepo;

/// Two branches edit the same line; `git merge` stops on the conflict and
/// leaves the repo in the merging state until the file is resolved + staged.
#[test]
fn merge_conflict_is_detected_then_resolved_by_writing_and_staging() {
    let t = TestRepo::new();
    t.write_and_stage("file.txt", "base\n");
    t.commit("base");

    t.cli(&["checkout", "-b", "other"]);
    t.write_and_stage("file.txt", "theirs\n");
    t.commit("theirs");

    t.cli(&["checkout", "main"]);
    t.write_and_stage("file.txt", "ours\n");
    t.commit("ours");

    let merge = Command::new("git")
        .args(["merge", "other"])
        .current_dir(&t.repo.path)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success(), "merge should conflict");

    assert_eq!(t.repo.operation(), Some(Operation::Merge));
    let status = t.repo.status().expect("status");
    assert_eq!(status.count(ChangeKind::Conflicted), 1);

    let text = t.repo.working_file_text("file.txt").expect("working text");
    assert!(text.contains("<<<<<<<"), "working file keeps its markers: {text}");

    t.repo.resolve_file("file.txt", "resolved\n").expect("resolve");

    let status = t.repo.status().expect("status");
    assert_eq!(status.count(ChangeKind::Conflicted), 0);
    assert!(status.staged.iter().any(|f| f.path == std::path::Path::new("file.txt")));
    assert_eq!(t.read("file.txt"), "resolved\n");
    // Staging resolves the file; the operation itself stays open until
    // `git merge --continue` (the banner's Continue).
    assert_eq!(t.repo.operation(), Some(Operation::Merge));
    assert!(t.repo.path.join(".git/MERGE_HEAD").is_file());
}

#[test]
fn operation_args_pick_continue_and_abort() {
    assert_eq!(Operation::Merge.continue_args(), ["merge", "--continue"]);
    assert_eq!(Operation::Rebase.abort_args(), ["rebase", "--abort"]);
    assert_eq!(Operation::CherryPick.label(), "Cherry-pick");
    // A clean repository is in no operation.
    let t = TestRepo::new();
    assert_eq!(t.repo.operation(), None);
}

/// `feature` deletes `notes.txt`, `main` edits it: git keeps main's version
/// without markers, and the resolution is `git add` (keep) or `git rm`.
#[test]
fn modify_delete_conflict_reports_the_deleted_side() {
    let t = TestRepo::new();
    t.write_and_stage("notes.txt", "base\n");
    t.commit("base");

    t.cli(&["checkout", "-b", "feature"]);
    t.cli(&["rm", "-q", "notes.txt"]);
    t.commit("feature: drop notes");

    t.cli(&["checkout", "main"]);
    t.write_and_stage("notes.txt", "base\nmain note\n");
    t.commit("main: edit notes");

    let merge = Command::new("git")
        .args(["merge", "feature"])
        .current_dir(&t.repo.path)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success(), "modify/delete should conflict");

    // The worktree keeps the modified side, with no conflict markers.
    let text = t.repo.working_file_text("notes.txt").expect("working text");
    assert!(!text.contains("<<<<<<<"), "no markers in a modify/delete: {text}");
    assert_eq!(t.repo.deleted_side("notes.txt").unwrap(), Some(ConflictSide::Theirs));

    t.repo.stage_file("notes.txt").expect("keep the file");
    let status = t.repo.status().expect("status");
    assert_eq!(status.count(ChangeKind::Conflicted), 0);
    // Kept at HEAD's content: resolved, so nothing left to stage.
    assert!(status.staged.iter().all(|f| f.path != std::path::Path::new("notes.txt")));
    assert_eq!(t.read("notes.txt"), "base\nmain note\n");
}

#[test]
fn modify_delete_conflict_can_take_the_deletion() {
    let t = TestRepo::new();
    t.write_and_stage("notes.txt", "base\n");
    t.commit("base");

    t.cli(&["checkout", "-b", "feature"]);
    t.write_and_stage("notes.txt", "theirs\n");
    t.commit("theirs");

    t.cli(&["checkout", "main"]);
    t.cli(&["rm", "-q", "notes.txt"]);
    t.commit("ours: drop notes");

    let merge = Command::new("git")
        .args(["merge", "feature"])
        .current_dir(&t.repo.path)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success(), "delete/modify should conflict");

    assert_eq!(t.repo.deleted_side("notes.txt").unwrap(), Some(ConflictSide::Ours));
    // `git rm`: gone from the index and the worktree, conflict resolved.
    t.repo.remove_conflicted("notes.txt").expect("take the deletion");
    let status = t.repo.status().expect("status");
    assert_eq!(status.count(ChangeKind::Conflicted), 0);
    assert!(!t.repo.path.join("notes.txt").exists());
}

#[test]
fn content_conflict_has_no_deleted_side() {
    let t = TestRepo::new();
    t.write_and_stage("file.txt", "base\n");
    t.commit("base");
    t.cli(&["checkout", "-b", "other"]);
    t.write_and_stage("file.txt", "theirs\n");
    t.commit("theirs");
    t.cli(&["checkout", "main"]);
    t.write_and_stage("file.txt", "ours\n");
    t.commit("ours");

    let merge = Command::new("git")
        .args(["merge", "other"])
        .current_dir(&t.repo.path)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success());

    assert_eq!(t.repo.deleted_side("file.txt").unwrap(), None);
}

/// The banners name the two branches: `Merging feature into main`,
/// `Deleted in feature, modified in main`.
#[test]
fn merge_sides_names_the_branches() {
    let t = TestRepo::new();
    t.write_and_stage("file.txt", "base\n");
    t.commit("base");
    t.cli(&["checkout", "-b", "feature"]);
    t.write_and_stage("file.txt", "theirs\n");
    t.commit("theirs");
    t.cli(&["checkout", "main"]);
    t.write_and_stage("file.txt", "ours\n");
    t.commit("ours");

    assert_eq!(t.repo.merge_sides(), (Some("main".to_string()), None));

    let merge = Command::new("git")
        .args(["merge", "feature"])
        .current_dir(&t.repo.path)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success());

    assert_eq!(t.repo.merge_sides(), (Some("main".to_string()), Some("feature".to_string())));
}
