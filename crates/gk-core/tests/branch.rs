mod support;

use gk_core::branch::BranchType;
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
