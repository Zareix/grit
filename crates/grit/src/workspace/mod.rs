//! The workspace: application state, Git actions, and top-level layout.
//!
//! The layout pieces live in [`components`]; this module owns the state.

mod actions;
mod components;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme as _, ResizableState,
    diff::{DiffMode, DiffState},
    h_resizable,
    input::{InputEvent, InputState},
    resizable_panel,
    tree::{TreeEvent, TreeState},
    v_resizable,
};
use gpui_kit::*;
use grit_core::{
    Oid, Repo,
    branch::Branch,
    commit::Commit,
    status::{FileEntry, Status},
    tags::Tag,
};

use self::components::{
    branches, commit_details, diff_view, history, staging, status_bar, toolbar,
};

/// A ref (branch or tag) pointing at a commit, shown as a chip in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRef {
    pub name: SharedString,
    pub kind: CommitRefKind,
    /// The checked-out branch (rendered with extra emphasis).
    pub is_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitRefKind {
    Local,
    Remote,
    Tag,
}

/// Which part of a file path the "Copy file path" submenu puts on the
/// clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyPath {
    /// `<repo root>/<relative path>`
    Absolute,
    /// Relative to the repository root, like git prints it.
    Relative,
    /// Only the file name.
    Name,
}

/// What the "create here" dialog (shared name input) will build, once the
/// user confirms a name for the commit it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateTarget {
    /// A new local branch pointing at the commit.
    Branch(Oid),
    /// A new tag pointing at the commit.
    Tag(Oid),
}

/// A stash in the branches column's stash panel.
pub type Stash = grit_core::stash::Stash;

/// What the auto-updater has last learned; rendered by the status bar and
/// driven by the menu / button / dialogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing known (or the check found nothing to do) — no UI.
    Idle,
    /// A network check is running.
    Checking,
    /// A newer release is published; the status bar shows the "Update
    /// available" button that opens the confirm dialog.
    Available(AvailableUpdate),
    /// Download / verify / install in progress (phase label for the bar).
    Busy(&'static str),
    /// Installed and validated; the bar shows "Restart to update".
    ReadyToRestart,
}

/// A newer published release, resolved against this build's platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    /// Clean version ("0.6.0", mux without the tag's `v` prefix).
    pub version: String,
    /// Release notes (`body`), shown in the confirm dialog.
    pub notes: Option<String>,
    /// DMG asset name ("grit-v0.6.0-aarch64-macos.dmg").
    pub asset: String,
    /// Direct download URL of the DMG.
    pub url: String,
    /// Direct download URL of the release's SHA256SUMS digest file.
    pub checksum_url: String,
    /// DMG size in bytes (asset metadata), for the dialog.
    pub size: u64,
}

/// Branch-to-branch operations. Targets are full git refs (`refs/heads/x`,
/// `refs/remotes/origin/x`) so remote tips can serve as sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOp {
    /// Move local branch `from` to the tip of `to_ref`, only fast-forward.
    FastForward { from: String, to_ref: String },
    /// Advance the remote branch `remote_branch` (like `origin/main`) to
    /// the tip of local branch `from` — a fast-forward push. git refuses
    /// (safely, without force) when the remote tip has moved on.
    FastForwardPush { from: String, remote_branch: String },
    /// Check out `branch` with its tip reset to `to_ref` (its upstream
    /// tip). A non-checked-out branch force-moves its ref then switches
    /// the worktree; the checked-out branch hard-resets in place — both
    /// discard commits not on `to_ref`, so a diverged branch needs the
    /// confirm dialog. When the branch is merely *behind* `to_ref` the
    /// moves are fast-forward-only and effectively non-destructive.
    ForceCheckout { branch: String, to_ref: String },
    /// Merge branch `what_ref` into the checked-out branch.
    Merge { what_ref: String },
    /// Rebase local branch `branch` onto `onto_ref`.
    Rebase { branch: String, onto_ref: String },
}

impl BranchOp {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            BranchOp::FastForward { .. }
            | BranchOp::FastForwardPush { .. }
            | BranchOp::ForceCheckout { .. } => "fast-forward",
            BranchOp::Merge { .. } => "merge",
            BranchOp::Rebase { .. } => "rebase",
        }
    }

    /// Confirm-dialog button text (capitalized).
    pub(crate) fn confirm_label(&self) -> &'static str {
        match self {
            BranchOp::FastForward { .. } | BranchOp::FastForwardPush { .. } => "Fast-forward",
            BranchOp::ForceCheckout { .. } => "Force checkout",
            BranchOp::Merge { .. } => "Merge",
            BranchOp::Rebase { .. } => "Rebase",
        }
    }
}

pub struct Workspace {
    repo: Option<Repo>,
    /// Source of truth for counts / commit button state.
    status: Status,
    message: Entity<InputState>,
    /// Inline "new branch" input, toggled by the + button.
    branch_input: Entity<InputState>,
    /// Whether the next commit amends HEAD (`--amend`): message prefilled
    /// with the last commit's, button becomes "Amend".
    amend: bool,
    creating_branch: bool,
    /// Name input of the shared "create here" dialog (branch / tag at a
    /// commit); its Enter press confirms through `confirm_create_here`.
    name_input: Entity<InputState>,
    /// What `confirm_create_here` builds, set when the dialog opens.
    pending_create: Option<CreateTarget>,
    /// Currently selected file (shown in the diff view).
    selected_path: Option<PathBuf>,
    /// Whether the selected file's diff is the staged one (HEAD -> index).
    selected_staged: bool,
    /// Files highlighted in one staging side (staged or unstaged); the diff
    /// pane follows `selected_path`, the file clicked last.
    selected_paths: Vec<PathBuf>,
    /// First file of the current selection — shift-click extends the
    /// selection from it to the clicked file.
    selection_anchor: Option<PathBuf>,
    /// Diff viewer state: one `DiffFile` for the selected file, kept across
    /// selections so expansion / scroll position behave sanely.
    diff: Entity<DiffState>,
    log: Vec<Commit>,
    branches: Vec<Branch>,
    current_branch: Option<Branch>,
    tags: Vec<Tag>,
    /// Tag names published on the default remote (origin-first), read by
    /// a background `ls-remote`; `None` = not known yet. Drives the
    /// "already on the remote" marker and the disabled `Push tag`.
    remote_tags: Option<HashSet<String>>,
    /// When the last remote-tags lookup was attempted; a failed attempt
    /// (offline) retries at most once a minute, silently.
    remote_tags_attempt: Option<std::time::Instant>,
    /// Stash entries (`git stash list`), shown in the branches column's
    /// bottom panel.
    stashes: Vec<Stash>,
    /// Whether the stash panel is collapsed (single toggle, no records).
    stashes_collapsed: bool,
    /// Remote branches rendered as a tree (`origin` → folders → leaves);
    /// rebuilt from `branches` on every refresh.
    remote_tree: Entity<TreeState>,
    /// Remote-tree folder ids the user collapsed (everything is expanded by
    /// default; fresh `TreeItem`s are collapsed, so reapply this on refresh).
    remote_collapsed: HashSet<String>,
    /// Refs (branches + tags) grouped by the commit they point to, for the
    /// log's ref chips.
    refs_by_commit: HashMap<Oid, Vec<CommitRef>>,
    /// Ahead/behind counts between every pair of branch tips, keyed
    /// `relations[a][b]` (commits unique to `a`, commits unique to `b`).
    /// Filled on refresh; drives the coherence of the branch context-menu
    /// entries (fast-forward / merge / rebase).
    branch_relations: HashMap<Oid, HashMap<Oid, (usize, usize)>>,
    /// Horizontal panel layout: branches | center | staging.
    layout: Entity<ResizableState>,
    /// Vertical split of the left column's bottom: remotes | stashes.
    bottom_layout: Entity<ResizableState>,
    /// Vertical split of the center column: history | diff.
    center_layout: Entity<ResizableState>,
    /// Label of the remote operation currently running, if any.
    remote_busy: Option<&'static str>,
    /// Transient success/info message for the status bar.
    info: Option<SharedString>,
    /// Scroll position of the commit log; branch clicks scroll the list to
    /// the branch tip through this.
    log_scroll: UniformListScrollHandle,
    /// Commit selected in the history (its details replace the staging panel).
    selected_commit: Option<Commit>,
    /// Generation counter bumped on every `refresh`; a background snapshot
    /// from a superseded refresh is dropped instead of overwriting fresher
    /// state.
    refresh_seq: u64,
    /// Files changed by the selected commit.
    commit_files: Vec<FileEntry>,
    /// Whether `selected_path`'s diff comes from the selected commit (rather
    /// than the worktree).
    diff_from_commit: bool,
    /// Whether the diff panel is currently open; used to detect the
    /// open transition (frame-sized panel layout resets).
    diff_open: bool,
    error: Option<SharedString>,
    /// Auto-updater state, surfaced in the status bar (see `UpdateState`).
    update_state: UpdateState,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(path: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx
            .new(|cx| InputState::new(window, cx).placeholder("Commit message…").clean_on_escape());

        let branch_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("New branch name…").clean_on_escape()
        });
        let enter_subscription =
            cx.subscribe_in(&branch_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.create_branch(window, cx);
                }
            });
        let name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Name…").clean_on_escape());
        let name_enter_subscription =
            cx.subscribe_in(&name_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.confirm_create_here(window, cx);
                }
            });

        let layout = cx.new(|_| ResizableState::default());
        let center_layout = cx.new(|_| ResizableState::default());
        let bottom_layout = cx.new(|_| ResizableState::default());
        let diff = cx.new(|cx| DiffState::new([], cx).with_mode(DiffMode::Unified));
        let remote_tree = cx.new(|cx| TreeState::new(cx).items(Vec::new()));
        // Track collapse/expand so rebuilds (every refresh) keep the user's
        // expansion state; items render expanded unless recorded here.
        let tree_subscription = cx.subscribe(&remote_tree, |this, _, event, _cx| match event {
            TreeEvent::Collapsed(id) => {
                this.remote_collapsed.insert(id.to_string());
            }
            TreeEvent::Expanded(id) => {
                this.remote_collapsed.remove(id.as_ref());
            }
        });
        let mut this = Self {
            repo: None,
            status: Status::default(),
            message,
            branch_input,
            creating_branch: false,
            amend: false,
            name_input,
            pending_create: None,
            selected_path: None,
            selected_staged: false,
            selected_paths: Vec::new(),
            selection_anchor: None,
            diff,
            log: Vec::new(),
            branches: Vec::new(),
            current_branch: None,
            tags: Vec::new(),
            remote_tags: None,
            remote_tags_attempt: None,
            stashes: Vec::new(),
            stashes_collapsed: false,
            remote_tree,
            remote_collapsed: HashSet::new(),
            refs_by_commit: HashMap::new(),
            branch_relations: HashMap::new(),
            layout,
            center_layout,
            bottom_layout,
            remote_busy: None,
            info: None,
            log_scroll: UniformListScrollHandle::new(),
            selected_commit: None,
            refresh_seq: 0,
            commit_files: Vec::new(),
            diff_from_commit: false,
            diff_open: false,
            error: None,
            update_state: UpdateState::Idle,
            _subscriptions: vec![enter_subscription, name_enter_subscription, tree_subscription],
        };
        this.open(path, cx);
        // Launch auto-check (throttled to once >> 24h; silent failures).
        this.maybe_check_for_updates(cx);
        this
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let foreground = cx.theme().foreground;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .text_color(foreground)
            // ---- toolbar ----
            .child(toolbar::render(self, cx))
            // ---- main split: branches | history + diff | staging ----
            // The resizable group always renders size_full — it must live in a
            // constrained wrapper, otherwise it overflows past the toolbar and
            // the status bar.
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("main-layout")
                        .with_state(&self.layout)
                        .child(
                            resizable_panel()
                                .size(px(180.))
                                .size_range(px(120.)..px(320.))
                                .child(branches::render(self, cx)),
                        )
                        // center column: history on top, diff below (when selected)
                        .child(
                            v_resizable("center-layout")
                                .with_state(&self.center_layout)
                                .child(resizable_panel().child(history::render(self, cx)))
                                .child(
                                    resizable_panel()
                                        .size(px(520.))
                                        .flex_none()
                                        .size_range(px(120.)..gpui::Pixels::MAX)
                                        .visible(self.selected_path.is_some())
                                        .child(diff_view::render(self, cx)),
                                ),
                        )
                        .child(
                            resizable_panel().size(px(340.)).size_range(px(240.)..px(520.)).child(
                                match self.selected_commit.clone() {
                                    Some(_) => commit_details::render(self, cx),
                                    None => staging::render(self, cx),
                                },
                            ),
                        ),
                ),
            )
            // ---- status bar ----
            .child(status_bar::render(self, cx))
    }
}
