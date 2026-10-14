//! Shared dialogs (destructive-action confirm, launchers into IDEs and
//! terminals) and the stash actions with their confirm flow.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    v_flex,
};
use gpui_kit::*;

use crate::workspace::Workspace;

/// Text payload of a destructive-action confirm dialog (see
/// [`Workspace::open_confirm_dialog`]).
pub(crate) struct ConfirmSpec {
    pub(crate) title: &'static str,
    pub(crate) question: String,
    pub(crate) detail: String,
    pub(crate) confirm_label: &'static str,
}

impl Workspace {
    /// Reveal the file in Finder (`open -R`), on the background executor.
    pub(crate) fn show_in_finder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let abs = repo.path.join(&path);
        let job = cx.background_spawn(async move {
            match std::process::Command::new("open").args(["-R", &abs.to_string_lossy()]).status() {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("open -R failed with {status}")),
                Err(e) => Err(e.to_string()),
            }
        });
        self.run_launcher("show in finder", job, cx);
    }

    /// Discard confirm dialog: both staged and unstaged changes are lost.
    /// When the right-clicked file sits inside the current multi-selection,
    /// the confirmation covers all of its files instead.
    pub(crate) fn open_discard_dialog(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<PathBuf> =
            if self.selected_paths.len() > 1 && self.selected_paths.iter().any(|p| p == &path) {
                self.selected_paths.clone()
            } else {
                vec![path]
            };
        let count = paths.len();

        let question = if count == 1 {
            format!("Discard all changes to \"{}\"?", paths[0].to_string_lossy())
        } else {
            format!("Discard all changes to {count} files?")
        };
        self.open_confirm_dialog(
            ConfirmSpec {
                title: "Discard changes?",
                question,
                detail: "Both staged and unstaged changes are lost. This cannot be undone.".into(),
                confirm_label: "Discard",
            },
            move |ws, cx| ws.discard_files(paths.clone(), cx),
            window,
            cx,
        );
    }

    /// Delete confirm dialog: the file leaves the working tree.
    pub(crate) fn open_delete_dialog(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display = path.to_string_lossy().into_owned();
        let owned = path.clone();
        self.open_confirm_dialog(
            ConfirmSpec {
                title: "Delete file?",
                question: format!("Delete \"{display}\" from the working tree?"),
                detail: "It stays recoverable from history if it was previously committed.".into(),
                confirm_label: "Delete",
            },
            move |ws, cx| ws.delete_file(owned.clone(), cx),
            window,
            cx,
        );
    }

    /// Shared destructive-action confirm dialog: Cancel + danger confirmation
    /// button that runs `action`. Same shape as the set-upstream dialog.
    pub(crate) fn open_confirm_dialog(
        &mut self,
        spec: impl Into<ConfirmSpec>,
        action: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = spec.into();
        let weak = cx.entity().downgrade();
        let action = Rc::new(action);
        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            let weak = weak.clone();
            let action = action.clone();
            dialog
                .title(spec.title)
                .child(v_flex().gap_2().child(spec.question.clone()).child(
                    div().text_color(dialog_cx.theme().muted_foreground).child(spec.detail.clone()),
                ))
                // 0.7.1: footer takes a value; build ours (see set-upstream).
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("confirm").danger().label(spec.confirm_label).on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                weak.update(cx, |ws, cx| action(ws, cx)).ok();
                            },
                        )),
                )
        });
    }

    /// Stash the selected staging files (`git stash push -u -- <paths>`,
    /// background git job). Errors surface on the status bar like the other
    /// background ops.
    pub(crate) fn stash_selection(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = (!self.selected_paths.is_empty()).then(|| self.selected_paths.clone())
        else {
            self.error = Some("No file selected to stash".into());
            cx.notify();
            return;
        };
        let args = grit_core::stash::stash_push_args(&paths);
        self.run_git_steps("stash", vec![args], cx);
    }

    /// Reapply a stash entry, keeping it in the list. Sync git2 (local op);
    /// conflicts surface as conflicted staging entries on refresh.
    pub(crate) fn apply_stash(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.stash_apply(index) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                self.refresh(cx);
            }
        }
    }

    /// Reapply a stash entry and drop it.
    pub(crate) fn pop_stash(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(mut repo) = self.repo.clone() else { return };
        match repo.stash_pop(index) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                self.refresh(cx);
            }
        }
    }

    /// Discard-stash confirm dialog: the entry is gone afterwards.
    pub(crate) fn open_stash_drop_dialog(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = self
            .stashes
            .iter()
            .find(|s| s.index == index)
            .map(|s| s.message.clone())
            .unwrap_or_else(|| format!("stash@{{{index}}}"));
        self.open_confirm_dialog(
            ConfirmSpec {
                title: "Drop stash?",
                question: format!("Drop \"{message}\"? ({index})"),
                detail: "The stash entry is removed from the list; its commits stay \
                         recoverable from the reflog while git keeps them."
                    .into(),
                confirm_label: "Drop",
            },
            move |ws, cx| ws.drop_stash(index, cx),
            window,
            cx,
        );
    }

    /// Remove a stash entry from the list.
    pub(crate) fn drop_stash(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(mut repo) = self.repo.clone() else { return };
        match repo.stash_drop(index) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                self.refresh(cx);
            }
        }
    }

    /// Toggle the stash panel's collapsed state in the branches column.
    pub(crate) fn toggle_stashes_collapsed(&mut self, cx: &mut Context<Self>) {
        self.stashes_collapsed = !self.stashes_collapsed;
        cx.notify();
    }

    // ---- app launchers (IDE / terminal) ---------------------------------

    /// Open the repository in Zed; falls back to the system `open -a Zed`
    /// when the `zed` CLI is not on PATH.
    pub(crate) fn open_in_ide(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let path = repo.path.to_string_lossy().into_owned();
        let job = cx.background_spawn(async move {
            try_open(&[
                ("zed".to_string(), vec![path.clone()]),
                ("open".to_string(), vec!["-a".to_string(), "Zed".to_string(), path]),
            ])
        });
        self.run_launcher("ide", job, cx);
    }

    /// Open the repository in a Warp terminal; Warp is driven through its
    /// `warp://` deep link (new tab with the repo path), with the plain
    /// `open -a Warp <path>` launch as fallback.
    pub(crate) fn open_in_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let path = repo.path.to_string_lossy().into_owned();
        // Basic percent-encoding for the URL scheme (space only is realistic).
        let encoded = path.replace(' ', "%20");
        let job = cx.background_spawn(async move {
            try_open(&[
                ("open".to_string(), vec![format!("warp://action/new_tab?path={encoded}")]),
                ("open".to_string(), vec!["-a".to_string(), "Warp".to_string(), path]),
            ])
        });
        self.run_launcher("terminal", job, cx);
    }
}

/// Run each candidate launcher command until one exits successfully; the
/// returned `Err` carries the last failure. Commands run detached from the
/// UI on the background executor.
fn try_open(commands: &[(String, Vec<String>)]) -> Result<(), String> {
    let mut last = String::from("no launcher available");
    for (program, args) in commands {
        match std::process::Command::new(program).args(args).output() {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                last = if stderr.is_empty() {
                    format!("{program} failed with {}", out.status)
                } else {
                    format!("{program}: {stderr}")
                };
            }
            Err(e) => last = format!("{program}: {e}"),
        }
    }
    Err(last)
}
