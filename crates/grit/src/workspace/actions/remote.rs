//! Remote operations: push / pull / fetch, the set-upstream dialog, the
//! remote branch deletion, and the branch-to-branch ops (fast-forward,
//! merge, rebase, force-checkout) run as background git command sequences.

use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    v_flex,
};
use gpui_kit::*;
use grit_core::remote::RemoteOp;

use super::dialogs::ConfirmSpec;
use crate::workspace::{BranchOp, Workspace};

impl Workspace {
    /// Push; opens the set-upstream dialog when the current branch does not
    /// track a remote yet.
    pub(crate) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote_busy.is_some() {
            return;
        }
        let branch = match &self.current_branch {
            Some(branch) => branch.clone(),
            None => {
                self.error = Some("Cannot push: no current branch".into());
                cx.notify();
                return;
            }
        };

        if branch.upstream.is_none() {
            self.open_upstream_dialog(&branch.name, window, cx);
            return;
        }

        self.run_remote(RemoteOp::Push, cx);
    }

    /// Dialog asking to publish a branch as `<remote>/<branch>` and set it as
    /// upstream (`push --set-upstream`).
    pub(crate) fn open_upstream_dialog(
        &mut self,
        branch_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Any configured remote works (the dialog offers the first one); a
        // remotes() read failure surfaces as an error, an empty list as the
        // "no remote configured" message.
        let Some(repo) = self.repo.as_ref() else {
            self.error = Some("Cannot set upstream: no repository open".into());
            cx.notify();
            return;
        };
        let remote = match repo.remotes() {
            Ok(remotes) => remotes.into_iter().next(),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
                return;
            }
        };
        let Some(remote) = remote else {
            self.error = Some("Cannot set upstream: no remote configured".into());
            cx.notify();
            return;
        };
        let branch_name = branch_name.to_string();
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            dialog
                .title("Set upstream branch?")
                .child(
                    v_flex()
                        .gap_2()
                        .child(format!("\"{}\" does not track a remote branch yet.", branch_name))
                        .child(div().text_color(dialog_cx.theme().muted_foreground).child(
                            format!("Publish it as {remote}/{branch_name} and set it as upstream?"),
                        )),
                )
                // 0.7.1: footer takes a value; the styled OK/Cancel buttons from
                // button_props are only rendered by AlertDialog — build ours.
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm")
                                .primary()
                                .label("Push and set upstream")
                                .on_click({
                                    let weak = weak.clone();
                                    let remote = remote.clone();
                                    let branch_name = branch_name.clone();
                                    move |_, window, cx| {
                                        window.close_dialog(cx);
                                        weak.update(cx, |ws, cx| {
                                            ws.set_upstream_and_push(
                                                remote.clone(),
                                                branch_name.clone(),
                                                cx,
                                            )
                                        })
                                        .ok();
                                    }
                                }),
                        ),
                )
        });
    }

    /// Created by the set-upstream dialog: push with `--set-upstream`.
    pub(crate) fn set_upstream_and_push(
        &mut self,
        remote: String,
        branch: String,
        cx: &mut Context<Self>,
    ) {
        let args = vec!["push".to_string(), "--set-upstream".to_string(), remote, branch];
        self.run_git_remote("push", args, cx);
    }

    /// Delete the remote branch on its server (`git push <remote> --delete
    /// <short>`); runs on the background thread like other remote ops.
    pub(crate) fn delete_remote_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let remotes = match repo.remotes() {
            Ok(remotes) => remotes,
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
                return;
            }
        };
        let Some((remote, short)) = remotes
            .iter()
            .find_map(|r| name.strip_prefix(&format!("{r}/")).map(|s| (r.clone(), s.to_string())))
        else {
            self.error =
                Some(format!("Cannot delete remote branch: unknown remote in {name}").into());
            cx.notify();
            return;
        };

        let args = vec!["push".to_string(), remote, "--delete".to_string(), short];
        self.run_git_remote("delete", args, cx);
    }

    /// Run a remote operation on a background thread (network + credentials),
    /// then refresh the UI.
    pub(crate) fn run_remote(&mut self, op: RemoteOp, cx: &mut Context<Self>) {
        if self.remote_busy.is_some() {
            return;
        }
        let args = [op.name().to_string()];
        self.run_git_remote(op.name(), args.to_vec(), cx);
    }

    /// Run a local branch-to-branch op through the git binary on the
    /// background thread (a failed merge/rebase leaves the conflicts in the
    /// worktree; refresh surfaces them as conflicted status entries).
    ///
    /// Arg spellings: a fast-forward of the checked-out branch goes through
    /// `merge --ff-only` so the working tree follows; any other branch
    /// moves its ref directly; the remote fast-forward is a plain push of
    /// the local tip onto the remote branch (longest remote prefix wins);
    /// a force checkout resets the branch to the target tip (the
    /// checked-out branch in place, the others before switching).
    pub(crate) fn run_branch_op(&mut self, op: BranchOp, cx: &mut Context<Self>) {
        if self.remote_busy.is_some() {
            return;
        }
        let current = self.current_branch.as_ref().map(|b| b.name.clone());

        let steps: Vec<Vec<String>> = match &op {
            BranchOp::FastForward { from, to_ref } => {
                vec![if current.as_deref() == Some(from.as_str()) {
                    vec!["merge".into(), "--ff-only".into(), to_ref.clone()]
                } else {
                    vec!["branch".into(), "--force".into(), from.clone(), to_ref.clone()]
                }]
            }
            BranchOp::FastForwardPush { from, remote_branch } => {
                let Some(repo) = self.repo.as_ref() else { return };
                let remotes = match repo.remotes() {
                    Ok(remotes) => remotes,
                    Err(e) => {
                        self.error = Some(e.to_string().into());
                        cx.notify();
                        return;
                    }
                };
                // Resolve the longest configured remote prefix, like
                // `checkout_remote_branch` does (an old remote name shared
                // by another must not shadow a longer one).
                let mut hit: Option<(String, String)> = None;
                for remote in remotes {
                    if let Some(short) = remote_branch.strip_prefix(&format!("{remote}/"))
                        && hit.as_ref().is_none_or(|(_, s)| short.len() < s.len())
                    {
                        hit = Some((remote, short.to_owned()));
                    }
                }
                let Some((remote, short)) = hit else {
                    self.error =
                        Some(format!("Cannot fast-forward {remote_branch}: unknown remote").into());
                    cx.notify();
                    return;
                };
                vec![vec!["push".into(), remote, format!("refs/heads/{from}:refs/heads/{short}")]]
            }
            BranchOp::ForceCheckout { branch, to_ref } => {
                if current.as_deref() == Some(branch.as_str()) {
                    // The branch is already checked out: reset it in place
                    // (checked-out refs cannot be force-moved).
                    vec![vec!["reset".into(), "--hard".into(), to_ref.clone()]]
                } else {
                    vec![
                        vec!["branch".into(), "--force".into(), branch.clone(), to_ref.clone()],
                        vec!["checkout".into(), branch.clone()],
                    ]
                }
            }
            BranchOp::Merge { what_ref } => vec![vec!["merge".into(), what_ref.clone()]],
            BranchOp::Rebase { branch, onto_ref } => {
                // `<branch>` must be the SHORT branch name: a full ref makes
                // git treat it as a commit and rebase "detached HEAD",
                // leaving the branch ref unmoved.
                vec![vec!["rebase".into(), onto_ref.clone(), branch.clone()]]
            }
        };
        self.run_git_steps(op.label(), steps, cx);
    }

    /// Destructive branch ops run through a confirm dialog first.
    pub(crate) fn open_branch_op_dialog(
        &mut self,
        op: BranchOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, question, detail) = match &op {
            BranchOp::FastForward { from, to_ref } => (
                "Fast-forward branch?",
                format!("Fast-forward \"{from}\" to {to_ref}?"),
                "The branch moves to the target tip. Only fast-forward moves are attempted; \
                 the working tree follows when the branch is checked out."
                    .into(),
            ),
            BranchOp::Merge { what_ref } => (
                "Merge branch?",
                format!("Merge {what_ref} into the current branch?"),
                "Diverged histories are combined with a merge commit; conflicts, when any, \
                 surface in the staging panel."
                    .into(),
            ),
            BranchOp::Rebase { branch, onto_ref } => (
                "Rebase branch?",
                format!("Rebase \"{branch}\" onto {onto_ref}?"),
                "The branch's unique commits are replayed onto the target. Already-rebased \
                 commits are dropped; conflicts, when any, surface in the staging panel."
                    .into(),
            ),
            BranchOp::FastForwardPush { from, remote_branch } => (
                "Fast-forward remote branch?",
                format!("Fast-forward \"{remote_branch}\" to \"{from}\"?"),
                "The remote branch is advanced to the local tip with a plain push. Only \
                 accepted as a fast-forward — it refuses (without force) if the remote tip \
                 has moved on."
                    .into(),
            ),
            BranchOp::ForceCheckout { branch, to_ref } => (
                "Force checkout?",
                format!("Check out \"{branch}\" and reset it to \"{to_ref}\"?"),
                "The branch is set to the target tip and the working tree follows. Local \
                 commits not on the target are discarded (still reachable through the \
                 reflog)."
                    .into(),
            ),
        };
        let label = op.confirm_label();
        self.open_confirm_dialog(
            ConfirmSpec { title, question, detail, confirm_label: label },
            move |ws, cx| ws.run_branch_op(op.clone(), cx),
            window,
            cx,
        );
    }
}
