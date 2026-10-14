mod support;

use support::TestRepo;

#[test]
fn linear_history_is_one_line_per_commit() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("first");
    t.write_and_stage("b.txt", "2");
    t.commit("second");
    t.write_and_stage("c.txt", "3");
    t.commit("third");

    let rows = t.repo.graph_log_all(10).unwrap();
    assert_eq!(rows.len(), 3);

    // Linear history: every cell is a single art line whose node is the
    // leftmost column; nothing is drawn under the commit.
    for row in &rows {
        assert_eq!(row.art.len(), 1);
        assert!(row.art[0].starts_with('*'));
        assert_eq!(row.node_col(), Some(0));
    }
    assert_eq!(rows[0].commit.summary, "third");
    assert_eq!(rows[2].commit.summary, "first");
}

#[test]
fn branches_and_merge_bring_diagonal_art() {
    let t = TestRepo::new();
    t.write_and_stage("a.txt", "1");
    t.commit("base");

    // Branch off through the real git binary.
    t.cli(&["branch", "feature"]);
    t.cli(&["checkout", "feature"]);
    t.write_and_stage("feature.txt", "x");
    t.commit("feature work");

    t.cli(&["checkout", "main"]);
    t.write_and_stage("main.txt", "y");
    t.commit("main work");
    t.cli(&["merge", "feature", "--no-edit"]);

    let rows = t.repo.graph_log_all(10).unwrap();
    assert_eq!(rows.len(), 4);
    let merge = rows.first().unwrap();
    // A merge carries two parents: git draws the second line under it.
    assert_eq!(merge.commit.parents.len(), 2);
    assert_eq!(merge.art.len(), 2);
    // The main-track rows keep a single `*` first-line.
    assert!(rows.iter().any(|r| r.art.len() == 1));

    // Every row hydrates its metadata, not just the node ids.
    assert!(rows.iter().all(|r| !r.commit.author.name.is_empty()));
}
