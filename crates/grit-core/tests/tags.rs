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
