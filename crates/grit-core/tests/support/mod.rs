//! Test helpers: build small throwaway repos to exercise the plumbing.

use grit_core::Repo;

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

        // Point the (unborn) HEAD at `main` explicitly: libgit2 defaults to
        // `master` and ignores the machine's init.defaultBranch, which made
        // these tests depend on the local git config.
        git_repo
            .reference_symbolic("HEAD", "refs/heads/main", true, "test: default to main")
            .expect("set HEAD to main");

        let repo = Repo::open(dir.path()).expect("open repo");
        Self { dir, repo }
    }

    /// Write a file in the repo and stage it.
    #[allow(dead_code)]
    pub fn write_and_stage(&self, rel: &str, content: &str) {
        self.write(rel, content);
        self.repo.stage_file(rel).expect("stage file");
    }

    /// Write a file in the repo (unstaged / untracked).
    #[allow(dead_code)]
    pub fn write(&self, rel: &str, content: &str) {
        let path = self.repo.path.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, content).expect("write file");
    }

    #[allow(dead_code)]
    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.repo.path.join(rel)).expect("read file")
    }

    /// Commit whatever is staged with an unique-ish message and return the new HEAD oid.
    #[allow(dead_code)]
    pub fn commit(&self, message: &str) -> git2::Oid {
        self.repo.commit(message).expect("commit")
    }

    /// Create a lightweight tag at the given commit.
    #[allow(dead_code)]
    pub fn tag(&self, name: &str, target: git2::Oid) {
        let repo = git2::Repository::open(self.repo.path.clone()).expect("open git2 repo");
        let sig = git2::Signature::now("Test User", "test@example.com").expect("signature");
        repo.tag(name, &repo.find_object(target, None).expect("object"), &sig, "test: tag", true)
            .expect("create tag");
    }

    /// Run the git binary in the repo (for flows grit-core itself shells
    /// out for: branch/checkout/merge before git2-backed asserts).
    #[allow(dead_code)]
    pub fn cli(&self, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&self.repo.path)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
