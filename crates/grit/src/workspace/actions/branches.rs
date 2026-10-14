//! Branch actions: checkout (with the upstream-sync policy), remote
//! branch checkout, inline branch creation, deletion, and the
//! create-branch / create-tag "here" dialogs' confirm side.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    input::Input,
    v_flex,
};
use gpui_kit::*;
use grit_core::{Oid, branch::BranchType};

use super::dialogs::ConfirmSpec;
use crate::workspace::{BranchOp, CreateTarget, Workspace};

/// How a checkout of a local branch should behave regarding its upstream
/// tip, resolved by [`Workspace::checkout_sync`] from the live relations.
enum CheckoutSync {
    /// No upstream, in step with it, or ahead-only: plain checkout.
    Plain,
    /// Strictly behind the upstream tip: check out, then fast-forward the
    /// branch up to it (`to_ref`, full git ref). Non-destructive.
    Level(String),
    /// Diverged from the upstream tip (`to_ref`, full git ref; `ahead`
    /// unique local commits at risk, `behind` upstream commits to take):
    /// confirm dialog, then force-checkout + reset.
    Diverged { to_ref: String, ahead: usize, behind: usize },
}

impl Workspace {
    pub(crate) fn checkout_branch(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repo.clone() else { return };
        match self.checkout_sync(name) {
            CheckoutSync::Plain => match repo.checkout_branch(name) {
                Ok(()) => self.refresh(cx),
                Err(e) => {
                    self.error = Some(e.to_string().into());
                    cx.notify();
                }
            },
            CheckoutSync::Level(to_ref) => {
                // Strictly behind the upstream: switch over, then
                // fast-forward the branch up to the tracking tip.
                match repo.checkout_branch(name) {
                    Ok(()) => {
                        self.refresh(cx);
                        self.run_branch_op(
                            BranchOp::FastForward { from: name.to_owned(), to_ref },
                            cx,
                        );
                    }
                    Err(e) => {
                        self.error = Some(e.to_string().into());
                        cx.notify();
                    }
                }
            }
            CheckoutSync::Diverged { to_ref, ahead, behind } => {
                self.open_force_checkout_dialog(name.to_owned(), to_ref, ahead, behind, window, cx);
            }
        }
    }

    /// Checkout a specific commit (detached HEAD), from the log's
    /// "Checkout this commit".
    pub(crate) fn checkout_commit(&mut self, oid: Oid, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.checkout_commit(oid) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    /// Create a local branch pointing at a commit (dialog result).
    pub(crate) fn create_branch_at(&mut self, name: &str, oid: Oid, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.create_branch_at(name, oid) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    /// Create a tag pointing at a commit (dialog result).
    pub(crate) fn create_tag_at(&mut self, name: &str, oid: Oid, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.create_tag(name, oid) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    /// Push a tag to the first configured remote (`git push <remote>`
    /// <tag>`; origin wins when several are configured). The release is
    /// published through the same credential story as branch pushes.
    pub(crate) fn push_tag(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let remote = match repo.remotes() {
            Ok(remotes) => remotes.first().cloned(),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
                return;
            }
        };
        let Some(remote) = remote else {
            self.error = Some("No remote configured to push the tag".into());
            cx.notify();
            return;
        };
        let args = vec!["push".to_string(), remote, name.to_string()];
        self.run_git_remote("push tag", args, cx);
    }

    /// Delete-tag dialog action: the remote copy goes first (`git push
    /// <remote> :refs/tags/<name>`) when a remote is configured, then the
    /// local ref leaves (`git tag -d`) — so a later fetch cannot
    /// resurrect the tag. Without a remote only the local ref goes.
    pub(crate) fn delete_tag_everywhere(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let mut steps = Vec::new();
        match repo.remotes() {
            Ok(remotes) => {
                if let Some(remote) = remotes.first() {
                    steps.push(vec![
                        "push".to_string(),
                        remote.clone(),
                        format!(":refs/tags/{name}"),
                    ]);
                }
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
                return;
            }
        }
        steps.push(vec!["tag".to_string(), "-d".to_string(), name.to_string()]);
        self.run_git_steps("delete tag", steps, cx);
    }

    /// "Create branch here" on a commit: small input dialog naming the
    /// branch to attach at `oid`.
    pub(crate) fn open_create_branch_dialog(
        &mut self,
        oid: Oid,
        summary: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_name_dialog(
            "Create branch here",
            format!("New local branch pointing at \"{summary}\"."),
            CreateTarget::Branch(oid),
            window,
            cx,
        );
    }

    /// "Create tag here" on a commit: small input dialog naming the tag to
    /// attach at `oid`.
    pub(crate) fn open_create_tag_dialog(
        &mut self,
        oid: Oid,
        summary: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_name_dialog(
            "Create tag here",
            format!("New tag pointing at \"{summary}\"."),
            CreateTarget::Tag(oid),
            window,
            cx,
        );
    }

    /// Shared input dialog: a hint line + the persistent `name_input` +
    /// Cancel / Create. Enter confirms through the input's subscription.
    pub(crate) fn open_name_dialog(
        &mut self,
        title: &'static str,
        question: String,
        target: CreateTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_create = Some(target);
        self.name_input.update(cx, |state, cx| state.set_value("", window, cx));
        let weak = cx.entity().downgrade();
        let input = self.name_input.clone();
        window.open_dialog(cx, move |dialog, _, dialog_cx| {
            let weak = weak.clone();
            let input = input.clone();
            dialog
                .title(title)
                .child(v_flex().gap_2().child(
                    div().text_color(dialog_cx.theme().muted_foreground).child(question.clone()),
                ))
                .child(div().flex_1().min_w_0().child(Input::new(&input).w_full()))
                .footer(
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("confirm").primary().label("Create").on_click(
                            move |_, window, cx| {
                                weak.update(cx, |ws, cx| ws.confirm_create_here(window, cx)).ok();
                            },
                        )),
                )
        });
    }

    /// Confirm the create-here dialog (Enter on the name input, or the
    /// Create button): consumes `pending_create` and builds the ref.
    pub(crate) fn confirm_create_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.pending_create.take() else { return };
        let name = self.name_input.read(cx).value().trim().to_string();
        self.name_input.update(cx, |state, cx| state.set_value("", window, cx));
        if name.is_empty() {
            self.error = Some("Name is empty".into());
            cx.notify();
            window.close_dialog(cx);
            return;
        }

        window.close_dialog(cx);
        match target {
            CreateTarget::Branch(oid) => self.create_branch_at(&name, oid, cx),
            CreateTarget::Tag(oid) => self.create_tag_at(&name, oid, cx),
        }
    }

    /// Create a local branch at HEAD from the inline input (Enter or + button).
    pub(crate) fn create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        let name = self.branch_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.error = Some("Branch name is empty".into());
            cx.notify();
            return;
        }

        match repo.create_branch(&name) {
            Ok(()) => {
                self.creating_branch = false;
                self.branch_input.update(cx, |state, cx| state.set_value("", window, cx));
                self.refresh(cx);
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    pub(crate) fn delete_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else { return };
        match repo.delete_branch(name) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    /// Checkout a remote-tracking branch (`origin/main`) as a local tracking
    /// branch (`main`).
    pub(crate) fn checkout_remote_branch(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A local branch already tracking `name` (created by an earlier
        // checkout) is reused: route through the plain-checkout policy so
        // a stale branch is leveled up (asked for force when diverged).
        if let Some(local) = self
            .branches
            .iter()
            .find(|b| b.kind == BranchType::Local && b.upstream.as_deref() == Some(name))
            .map(|b| b.name.clone())
        {
            self.checkout_branch(&local, window, cx);
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        match repo.checkout_remote_branch(name) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    pub(crate) fn toggle_create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.creating_branch = !self.creating_branch;
        if !self.creating_branch {
            self.branch_input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    /// How a checkout of the local branch `name` should behave regarding
    /// its upstream tip, resolved from the live branch relations.
    fn checkout_sync(&self, name: &str) -> CheckoutSync {
        let Some(local) =
            self.branches.iter().find(|b| b.kind == BranchType::Local && b.name == name)
        else {
            return CheckoutSync::Plain;
        };
        let Some(upstream_name) = local.upstream.clone() else {
            return CheckoutSync::Plain;
        };
        let Some(up) = self.branches.iter().find(|b| b.name == upstream_name) else {
            return CheckoutSync::Plain;
        };
        let Some((ahead, behind)) =
            self.branch_relations.get(&local.head).and_then(|row| row.get(&up.head)).copied()
        else {
            return CheckoutSync::Plain;
        };
        let to_ref = grit_core::branch::full_ref(up.kind, &up.name);
        match (ahead, behind) {
            (0, 0) => CheckoutSync::Plain, // in step with the upstream
            (0, b) if b > 0 => CheckoutSync::Level(to_ref),
            (a, b) if a > 0 && b > 0 => CheckoutSync::Diverged { to_ref, ahead: a, behind: b },
            _ => CheckoutSync::Plain, // ahead-only: nothing to level
        }
    }

    /// Force-checkout confirm dialog: a branch diverged from its upstream;
    /// proceeding resets the branch to the upstream tip and discards the
    /// local commits that are not on it.
    pub(crate) fn open_force_checkout_dialog(
        &mut self,
        name: String,
        to_ref: String,
        ahead: usize,
        behind: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op = BranchOp::ForceCheckout { branch: name.clone(), to_ref: to_ref.clone() };
        // Human spelling for the dialog; the op keeps the full ref.
        let display =
            to_ref.trim_start_matches("refs/heads/").trim_start_matches("refs/remotes/").to_owned();
        let question = format!("Check out \"{name}\" and reset it to \"{display}\"?");
        let detail = format!(
            "\"{name}\" has diverged from \"{display}\": {ahead} commit(s) ahead, {behind} \
             behind. A force checkout resets the branch to the upstream tip and discards \
             the {ahead} local commit(s) not on it (still reachable through the reflog)."
        );
        self.open_confirm_dialog(
            ConfirmSpec {
                title: "Force checkout?",
                question,
                detail,
                confirm_label: "Force checkout",
            },
            move |ws, cx| {
                if !ws.status.is_empty() {
                    ws.error = Some(
                        "Force checkout needs a clean working tree: commit or stash first.".into(),
                    );
                    cx.notify();
                    return;
                }
                ws.run_branch_op(op.clone(), cx)
            },
            window,
            cx,
        );
    }
}

// keep Rc in scope for the dialog closures shared with `dialogs`
#[allow(unused_imports)]
use Rc as _RcNeverUsed;
