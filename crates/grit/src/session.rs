//! Multi-repo session: the root entity owning the tab strip (drawn in the
//! title bar, aligned with the native window buttons), one [`Workspace`] per
//! open repository, the repo-picker "new tab" page, and persistence of
//! known / currently open repositories.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use grit_core::Repo;

use crate::workspace::Workspace;

/// Maximum number of known repositories kept in the persisted session file.
const MAX_KNOWN: usize = 20;

/// Maximum number of tabs restored from the persisted session at startup.
const MAX_RESTORED: usize = 10;

/// One item of the session tab strip.
enum TabEntry {
    /// An open repository with its workspace UI state alive.
    Repo { path: PathBuf, workspace: Entity<Workspace> },
    /// A placeholder tab showing the repo picker page.
    NewTab,
}

/// The root entity: tabs, repo registry and the new-tab picker page.
pub struct Session {
    tabs: Vec<TabEntry>,
    /// Index of the active tab in `tabs`.
    active: usize,
    /// Root paths of known repositories, most recently used first.
    known: Vec<String>,
    /// Session-level error, shown on the picker page.
    error: Option<SharedString>,
}

impl Session {
    pub fn new(initial: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let data = persist::load();

        let mut this =
            Self { tabs: Vec::new(), active: 0, known: normalize_known(data.known), error: None };

        // The CLI path, if given, opens as the first tab; otherwise the
        // persisted open tabs are restored (invalid paths are skipped and
        // reported as an error on the picker page).
        let startup: Vec<PathBuf> = match &initial {
            Some(path) => vec![PathBuf::from(path)],
            None => data
                .open
                .iter()
                .take(MAX_RESTORED)
                .map(|p| normalize_root(PathBuf::from(p)))
                .collect(),
        };
        for path in startup {
            this.add_repo_tab(&path, window, cx);
        }

        if this.tabs.is_empty() {
            this.tabs.push(TabEntry::NewTab);
        }
        this.active = data.active.min(this.tabs.len() - 1);
        // Persist the freshly resumed session so first launches register
        // their repo without requiring a click.
        persist::save(&this.build_data());
        this
    }

    // ---- tab management -------------------------------------------------

    fn select_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.active = ix;
        self.persist(cx);
    }

    /// "+" in the tab strip: activate the existing picker tab or append one.
    pub(crate) fn open_new_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| matches!(t, TabEntry::NewTab)) {
            self.active = ix;
        } else {
            self.tabs.push(TabEntry::NewTab);
            self.active = self.tabs.len() - 1;
        }
        self.persist(cx);
    }

    /// ⌘W handler wrapper: close whichever tab is currently active.
    pub(crate) fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        let active = self.active.min(self.tabs.len().saturating_sub(1));
        self.close_tab(active, cx);
    }

    /// Close button on a tab; the strip never runs empty (picker fallback).
    pub(crate) fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);
        self.adjust_active_after_remove(ix);
        if self.tabs.is_empty() {
            self.tabs.push(TabEntry::NewTab);
            self.active = 0;
        }
        self.persist(cx);
    }

    // ---- repo opening ---------------------------------------------------

    /// Native folder picker → open the chosen repo as a tab.
    pub(crate) fn pick_folder(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a Git repository".into()),
        });
        cx.spawn_in(window, async move |session, cx| {
            let picked = receiver
                .await
                .ok()
                .and_then(|res| res.ok())
                .flatten()
                .and_then(|mut v| (!v.is_empty()).then(|| v.remove(0)));
            let Some(path) = picked else { return };
            let _ = cx.update(|window, app| match Repo::open(&path) {
                Ok(repo) => {
                    let root = normalize_root(repo.path);
                    let workspace = app
                        .new(|cx| Workspace::new(root.to_string_lossy().into_owned(), window, cx));
                    session
                        .update(app, |this, cx| this.show_repo_tab(root, at, workspace, cx))
                        .ok();
                }
                Err(_) => {
                    session
                        .update(app, |this, cx| {
                            this.error = Some(
                                format!("Not a Git repository: \"{}\"", path.display()).into(),
                            );
                            cx.notify();
                        })
                        .ok();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Open (or focus, if already open) the repository at `path` as a tab.
    pub(crate) fn open_repo(
        &mut self,
        path: &Path,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(repo) = Repo::open(path) else {
            self.error = Some(format!("Not a Git repository: \"{}\"", path.display()).into());
            cx.notify();
            return;
        };
        let root = normalize_root(repo.path);
        let workspace =
            cx.new(|cx| Workspace::new(root.to_string_lossy().into_owned(), window, cx));
        self.show_repo_tab(root, at, workspace, cx);
    }

    /// Insert a validated repo tab: focus its existing tab if open, replace
    /// the picker tab on `at`, else append as the last tab.
    fn show_repo_tab(
        &mut self,
        root: PathBuf,
        at: usize,
        workspace: Entity<Workspace>,
        cx: &mut Context<Self>,
    ) {
        self.error = None;

        // Already open: focus that tab and drop the picker tab if any.
        if let Some(repo_ix) = self.repo_tab_of(&root) {
            let close_picker =
                self.tabs.get(at).is_some_and(|t| matches!(t, TabEntry::NewTab)) && at != repo_ix;
            if close_picker {
                self.tabs.remove(at);
                self.adjust_active_after_remove(at);
            }
            // The removal above may have shifted the repo tab's index.
            self.active = self.repo_tab_of(&root).unwrap_or(repo_ix);
            self.persist(cx);
            return;
        }

        if at < self.tabs.len() && matches!(self.tabs[at], TabEntry::NewTab) {
            self.tabs[at] = TabEntry::Repo { path: root.clone(), workspace };
            self.active = at;
        } else {
            self.tabs.push(TabEntry::Repo { path: root.clone(), workspace });
            self.active = self.tabs.len() - 1;
        }
        self.remember(root.to_string_lossy().into_owned());
        self.persist(cx);
    }

    /// Startup helper: open `path` as a repo tab, or skip an invalid path.
    fn add_repo_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(repo) = Repo::open(path) else {
            self.error = Some(format!("Not a Git repository: \"{}\"", path.display()).into());
            return;
        };
        let root = normalize_root(repo.path);
        if self.repo_tab_of(&root).is_some() {
            return;
        }
        let workspace =
            cx.new(|cx| Workspace::new(root.to_string_lossy().into_owned(), window, cx));
        self.remember(root.to_string_lossy().into_owned());
        self.tabs.push(TabEntry::Repo { path: root, workspace });
    }

    fn repo_tab_of(&self, root: &Path) -> Option<usize> {
        self.tabs.iter().position(|t| match t {
            TabEntry::Repo { path, .. } => path == root,
            TabEntry::NewTab => false,
        })
    }

    fn adjust_active_after_remove(&mut self, removed: usize) {
        if self.tabs.is_empty() {
            self.active = 0;
            return;
        }
        if removed < self.active {
            self.active -= 1;
        } else if removed == self.active {
            self.active = self.active.min(self.tabs.len() - 1);
        }
    }

    fn remember(&mut self, root: String) {
        self.known.retain(|p| p != &root);
        self.known.insert(0, root);
        self.known.truncate(MAX_KNOWN);
    }

    fn build_data(&self) -> persist::Data {
        let open = self
            .tabs
            .iter()
            .filter_map(|t| match t {
                TabEntry::Repo { path, .. } => Some(path.to_string_lossy().into_owned()),
                TabEntry::NewTab => None,
            })
            .collect();
        persist::Data { known: self.known.clone(), open, active: self.active }
    }

    fn persist(&mut self, cx: &mut Context<Self>) {
        persist::save(&self.build_data());
        cx.notify();
    }
}

// ---- rendering --------------------------------------------------------------

/// git2 hands out the workdir with a trailing separator; normalize once so
/// stored paths (and their equality checks) stay clean.
fn normalize_root(path: PathBuf) -> PathBuf {
    path.components().collect()
}

/// Normalize + dedup the persisted known-repository list on load.
fn normalize_known(paths: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in paths {
        let normalized = normalize_root(PathBuf::from(&path)).to_string_lossy().into_owned();
        if !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    out
}

fn tab_label(path: &Path) -> SharedString {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or("Repository".into()).into()
}

impl Session {
    /// Tab strip drawn inside the title bar: left of it sit the native window
    /// buttons (TitleBar applies the 80px macOS padding), tabs align on the
    /// same row.
    fn render_tab_strip(&self, cx: &mut Context<Self>) -> Div {
        let owner = cx.entity().downgrade();
        let active = self.active.min(self.tabs.len() - 1);

        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, entry)| {
                let owner = owner.clone();
                let base = match entry {
                    TabEntry::Repo { path, .. } => {
                        Tab::new()
                            .label(tab_label(path))
                            // 0.7.1: `.icon()` renders an icon-only tab (the
                            // label is dropped) — use a prefix element so
                            // both icon and label show.
                            .prefix(Icon::new(IconName::FolderOpen).size_3())
                    }
                    TabEntry::NewTab => {
                        Tab::new().label("New tab").prefix(Icon::new(IconName::Plus).size_3())
                    }
                };
                base.suffix(
                    Button::new(format!("close-tab-{ix}"))
                        .ghost()
                        .xsmall()
                        // `X` is not in the default 101-icon set; `Close` is.
                        .icon(IconName::Close)
                        .on_click(move |_, _, cx| {
                            owner.update(cx, |this, cx| this.close_tab(ix, cx)).ok();
                        }),
                )
            })
            .collect::<Vec<_>>();

        h_flex()
            .min_w_0()
            .gap_1()
            .child(
                TabBar::new("session-tabs")
                    .xsmall()
                    .underline()
                    .selected_index(active)
                    .max_width(px(160.))
                    .on_click(move |ix, _, cx| {
                        owner.update(cx, |this, cx| this.select_tab(*ix, cx)).ok();
                    })
                    .children(tabs),
            )
            .child(
                Button::new("new-tab")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .on_click(cx.listener(|this, _, _, cx| this.open_new_tab(cx))),
            )
    }

    /// The picker page: open-folder button, error banner, recent repos.
    fn render_new_tab(&self, at: usize, owner: &WeakEntity<Session>, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let owner = owner.clone();

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                v_flex()
                    .w(px(520.))
                    .gap_2()
                    .child(
                        v_flex()
                            .items_center()
                            .pb_2()
                            .child(
                                Icon::new(IconName::FolderOpen)
                                    .size_6()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Open a repository"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("Pick a local Git working tree to open it in a tab."),
                            ),
                    )
                    .child(
                        h_flex().justify_center().child(
                            Button::new("pick-folder")
                                .primary()
                                .label("Choose folder…")
                                .icon(IconName::FolderOpen)
                                .on_click({
                                    let owner = owner.clone();
                                    move |_, window, cx| {
                                        owner
                                            .update(cx, |this, cx| this.pick_folder(at, window, cx))
                                            .ok();
                                    }
                                }),
                        ),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1()
                                .rounded_sm()
                                .bg(theme.danger.opacity(0.15))
                                .child(
                                    Icon::new(IconName::CircleAlert)
                                        .size_4()
                                        .text_color(theme.danger),
                                )
                                .child(div().text_sm().text_color(theme.danger).child(error)),
                        )
                    })
                    .when(!self.known.is_empty(), |this| {
                        this.child(
                            div()
                                .pt_2()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("RECENT REPOSITORIES"),
                        )
                        .child(
                            div()
                                .id("known-repos")
                                .max_h(px(320.))
                                .overflow_y_scroll()
                                .gap_0p5()
                                .flex()
                                .flex_col()
                                .children(self.known.iter().enumerate().map(|(ix, path)| {
                                    let number = ix + 1;
                                    known_repo_row(number, at, path, &owner, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// One row of the recent repositories list on the picker page.
fn known_repo_row(
    ix: usize,
    at: usize,
    path: &str,
    owner: &WeakEntity<Session>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let owner = owner.clone();
    let path_buf = PathBuf::from(path);
    let name = path_buf
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());

    h_flex()
        .id(SharedString::from(format!("known-repo-{ix}")))
        .w_full()
        .flex_none()
        .gap_2()
        .px_3()
        .py_1p5()
        .rounded_sm()
        .items_center()
        .cursor_pointer()
        .hover(|style| style.bg(theme.accent.opacity(0.4)))
        .child(Icon::new(IconName::Folder).size_4().text_color(theme.muted_foreground))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(name))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(path.to_string()),
                ),
        )
        .on_click(move |_, window, cx| {
            owner.update(cx, |this, cx| this.open_repo(&path_buf, at, window, cx)).ok();
        })
}

impl Render for Session {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active.min(self.tabs.len() - 1);
        // Active tab content is computed before the title bar so the borrows
        // of `self` never overlap.
        let owner = cx.entity().downgrade();
        let content = match self.tabs.get(active) {
            Some(TabEntry::Repo { workspace, .. }) => workspace.clone().into_any_element(),
            _ => self.render_new_tab(active, &owner, cx),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // ---- title bar with the tab strip ----
            .child(TitleBar::new().child(self.render_tab_strip(cx)))
            // ---- active tab content ----
            .child(div().flex_1().min_h_0().child(content))
    }
}

mod persist {
    //! Session file: known repos, open tabs and the last active tab.

    use std::fs;
    use std::path::PathBuf;

    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default)]
    pub struct Data {
        pub known: Vec<String>,
        pub open: Vec<String>,
        pub active: usize,
    }

    fn file() -> Option<PathBuf> {
        let mut path = dirs::data_dir()?;
        path.push("grit");
        if !path.is_dir() {
            fs::create_dir_all(&path).ok()?;
        }
        path.push("session.json");
        Some(path)
    }

    pub(super) fn load() -> Data {
        file()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub(super) fn save(data: &Data) {
        let Some(path) = file() else { return };
        if let Ok(content) = serde_json::to_string_pretty(data) {
            fs::write(path, content).ok();
        }
    }
}
