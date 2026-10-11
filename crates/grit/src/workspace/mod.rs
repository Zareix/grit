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

pub struct Workspace {
    repo: Option<Repo>,
    /// Source of truth for counts / commit button state.
    status: Status,
    message: Entity<InputState>,
    /// Inline "new branch" input, toggled by the + button.
    branch_input: Entity<InputState>,
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
    /// Diff viewer state: one `DiffFile` for the selected file, kept across
    /// selections so expansion / scroll position behave sanely.
    diff: Entity<DiffState>,
    log: Vec<Commit>,
    branches: Vec<Branch>,
    current_branch: Option<Branch>,
    tags: Vec<Tag>,
    /// Remote branches rendered as a tree (`origin` → folders → leaves);
    /// rebuilt from `branches` on every refresh.
    remote_tree: Entity<TreeState>,
    /// Remote-tree folder ids the user collapsed (everything is expanded by
    /// default; fresh `TreeItem`s are collapsed, so reapply this on refresh).
    remote_collapsed: HashSet<String>,
    /// Refs (branches + tags) grouped by the commit they point to, for the
    /// log's ref chips.
    refs_by_commit: HashMap<Oid, Vec<CommitRef>>,
    /// Horizontal panel layout: branches | center | staging.
    layout: Entity<ResizableState>,
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
    /// Files changed by the selected commit.
    commit_files: Vec<FileEntry>,
    /// Whether `selected_path`'s diff comes from the selected commit (rather
    /// than the worktree).
    diff_from_commit: bool,
    /// Whether the diff panel is currently open; used to detect the
    /// open transition (frame-sized panel layout resets).
    diff_open: bool,
    error: Option<SharedString>,
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
            name_input,
            pending_create: None,
            selected_path: None,
            selected_staged: false,
            diff,
            log: Vec::new(),
            branches: Vec::new(),
            current_branch: None,
            tags: Vec::new(),
            remote_tree,
            remote_collapsed: HashSet::new(),
            refs_by_commit: HashMap::new(),
            layout,
            center_layout,
            remote_busy: None,
            info: None,
            log_scroll: UniformListScrollHandle::new(),
            selected_commit: None,
            commit_files: Vec::new(),
            diff_from_commit: false,
            diff_open: false,
            error: None,
            _subscriptions: vec![enter_subscription, name_enter_subscription, tree_subscription],
        };
        this.open(path, cx);
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
