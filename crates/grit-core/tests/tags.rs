mod support;

use support::TestRepo;

#[test]
fn tags_list_targets_sorted() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let first = t.commit("init");
    t.write_and_stage("b.txt", "2");
    let second = t.commit("second");

    t.tag("v1.0", second);
    t.tag("v0.1", first);

    let tags = t.repo.tags().unwrap();
    // Sorted by name, each peeled to the commit it points at.
    let names: Vec<_> = tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["v0.1", "v1.0"]);
    assert_eq!(tags[0].target, first);
    assert_eq!(tags[1].target, second);
}

#[test]
fn empty_repo_has_no_tags() {
    let t = TestRepo::new();
    assert!(t.repo.tags().unwrap().is_empty());
}

#[test]
fn create_tag_at_commit() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let first = t.commit("first");
    t.write_and_stage("b.txt", "2");
    let second = t.commit("second");

    t.repo.create_tag("v0.9", first).unwrap();
    let tags = t.repo.tags().unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v0.9");
    assert_eq!(tags[0].target, first);

    // Creating the same tag again is refused, like `git tag` without -f.
    assert!(t.repo.create_tag("v0.9", second).is_err());

    t.repo.create_tag("v1.0", second).unwrap();
    assert_eq!(t.repo.tags().unwrap().len(), 2);
}

#[test]
fn delete_tag_removes_the_local_ref() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let first = t.commit("first");

    t.repo.create_tag("v0.9", first).unwrap();
    t.repo.delete_tag("v0.9").unwrap();
    assert!(t.repo.tags().unwrap().is_empty());

    // Deleting again is refused, like `git tag -d` on a missing tag.
    assert!(t.repo.delete_tag("v0.9").is_err());
}
