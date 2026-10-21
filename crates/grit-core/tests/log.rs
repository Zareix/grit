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

/// The author date is precomputed at load (`Signature::local_date_time`):
/// the log rows render it per row per frame, so it must be populated —
/// an empty string here means the memoization regressed.
#[test]
fn author_date_time_is_precomputed() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("init");

    let commit = t.repo.commit_log(1).unwrap().remove(0);
    let date = commit.author.date_time();
    assert_eq!(date.len(), 16, "dd/mm/yyyy HH:MM");
    let (day, rest) = date.split_at(2);
    let (sep1, rest) = rest.split_at(1);
    let (month, rest) = rest.split_at(2);
    let (sep2, rest) = rest.split_at(1);
    let (year, rest) = rest.split_at(4);
    let (sep3, rest) = rest.split_at(1);
    let (hour, colon, minute) = (rest.get(..2), rest.get(2..3), rest.get(3..5));
    assert!(day.parse::<u32>().map(|d| (1..=31).contains(&d)).unwrap_or(false));
    assert_eq!(sep1, "/");
    assert!(month.parse::<u32>().map(|m| (1..=12).contains(&m)).unwrap_or(false));
    assert_eq!(sep2, "/");
    assert!((2020..=2100).contains(&year.parse::<i64>().unwrap_or(0)));
    assert_eq!(sep3, " ");
    assert!(hour.map(|h| h.parse::<u32>().map(|h| h < 24).unwrap_or(false)).unwrap_or(false));
    assert_eq!(colon, Some(":"));
    assert!(minute.map(|m| m.parse::<u32>().map(|m| m < 60).unwrap_or(false)).unwrap_or(false));
    assert_eq!(date, commit.author.local_date_time, "method serves the cache");
}

#[test]
fn commit_log_all_empty_repo_is_empty() {
    let t = TestRepo::new();
    assert!(t.repo.commit_log_all(100).unwrap().is_empty());
}
