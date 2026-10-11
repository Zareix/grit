mod support;

use support::TestRepo;

#[test]
fn commit_log_all_includes_every_ref() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    let init = t.commit("init");
    t.write_and_stage("b.txt", "2");
    t.commit("base");

    // Diverged side branch: a commit that never reaches main.
    t.repo.create_branch("side").unwrap();
    t.repo.checkout_branch("side").unwrap();
    t.write_and_stage("s.txt", "side");
    let side_tip = t.commit("side work");
    t.repo.checkout_branch("main").unwrap();

    // A tag on an old commit, covered by the full walk.
    t.tag("v1", init);

    // HEAD's own history skips the side branch ...
    let head_log: Vec<_> =
        t.repo.commit_log(100).unwrap().iter().map(|c| c.summary.clone()).collect();
    assert!(head_log.contains(&"base".to_string()));
    assert!(!head_log.contains(&"side work".to_string()));

    // ... while the full walk does not: side commits (tagged or not) all
    // show up, and every branch head is reachable from it.
    let all: Vec<_> = t.repo.commit_log_all(100).unwrap();
    let summaries: Vec<_> = all.iter().map(|c| c.summary.clone()).collect();
    assert!(summaries.contains(&"side work".to_string()));
    assert!(summaries.contains(&"base".to_string()));
    assert!(summaries.contains(&"init".to_string()));

    let ids: Vec<_> = all.iter().map(|c| c.id).collect();
    assert!(ids.contains(&side_tip));
    assert!(ids.contains(&init));
}

#[test]
fn commit_log_all_empty_repo_is_empty() {
    let t = TestRepo::new();
    assert!(t.repo.commit_log_all(100).unwrap().is_empty());
}
