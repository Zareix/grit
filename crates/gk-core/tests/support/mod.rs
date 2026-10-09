//! Test helpers: build small throwaway repos to exercise the plumbing.

use gk_core::Repo;

/// A temp dir initialized as a git repo with one commit on `main`.
pub struct TestRepo {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    pub repo: Repo,
}

impl TestRepo {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("init repo");

        // Deterministic identity so commits are reproducible in tests.
        let mut config = git_repo.config().expect("config");
        config.set_str("user.name", "Test User").expect("set name");
        config.set_str("user.email", "test@example.com").expect("set email");

        let repo = Repo::open(dir.path()).expect("open repo");
        Self { dir, repo }
    }

    /// Write a file in the repo and stage it.
    pub fn write_and_stage(&self, rel: &str, content: &str) {
        self.write(rel, content);
        self.repo.stage_file(rel).expect("stage file");
    }

    /// Write a file in the repo (unstaged / untracked).
    pub fn write(&self, rel: &str, content: &str) {
        let path = self.repo.path.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, content).expect("write file");
    }

    #[expect(dead_code)]
    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.repo.path.join(rel)).expect("read file")
    }

    /// Commit whatever is staged with an unique-ish message and return the new HEAD oid.
    pub fn commit(&self, message: &str) -> git2::Oid {
        self.repo.commit(message).expect("commit")
    }
}
