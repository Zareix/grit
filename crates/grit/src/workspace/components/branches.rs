//! Left column: local / remote branches.
//!
//! Click a branch to scroll the commit log to its tip (no checkout). The
//! menu button, revealed on hover, gives checkout / pull / set-upstream /
//! delete; the same menu is available on right-click (context menu).
//! Remotes render as a GitKraken-style tree (one root per remote, a folder
//! per `/` segment) whose leaves offer checkout (as a tracking local
//! branch) and delete on the server, via right-click.

use std::collections::{BTreeMap, HashSet};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    list::ListItem,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    tree::{TreeItem, tree},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use grit_core::Oid;
use grit_core::branch::{Branch, BranchType, full_ref};

use super::primitives::SectionLabel;
use crate::workspace::{BranchOp, Workspace};
use grit_core::remote::RemoteOp;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;

    let local_rows: Vec<_> = ws
        .branches
        .iter()
        .filter(|b| b.kind == BranchType::Local)
        .map(|b| local_branch_row(ws, b, cx))
        .collect();

    v_flex()
        .w_full()
        .h_full()
        // header + "new branch" toggle
        .child(
            h_flex()
                .px_2()
                .py_1()
                .items_center()
                .justify_between()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child("Branches")
                .child(
                    Button::new("new-branch")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .selected(ws.creating_branch)
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.toggle_create_branch(window, cx)
                            }),
                        ),
                ),
        )
        // inline creation input
        .when(ws.creating_branch, |this| {
            this.child(
                h_flex()
                    .px_2()
                    .pb_1()
                    .gap_1()
                    .child(div().flex_1().min_w_0().child(Input::new(&ws.branch_input)))
                    .child(
                        Button::new("create-branch")
                            .primary()
                            .xsmall()
                            .icon(IconName::Plus)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.create_branch(window, cx)),
                            ),
                    ),
            )
        })
        // local branches: flat scrolling list — hugs its content but claims
        // at most half of the panel height, so the remotes tree stays visible
        // right below instead of being pushed to the bottom.
        .child(
            v_flex()
                .flex_none()
                .max_h(relative(0.5))
                .id("local-branches")
                .overflow_y_scroll()
                .p_1()
                .gap_0p5()
                .child(SectionLabel::new("LOCAL", cx))
                .children(local_rows),
        )
        // remotes: virtualized tree (scroll + expand/collapse handled by the
        // component; it fills the remaining height in its own scroll region).
        .child(
            v_flex()
                .flex_1()
                .min_h_0()
                .mt_1()
                .child(div().px_1().child(SectionLabel::new("REMOTES", cx)))
                .child(render_remote_tree(ws, cx)),
        )
}

// ---- remote tree --------------------------------------------------------

/// Synthetic root/field ids follow `remote:<remote>` and
/// `remote:<remote>/<segment>`; leaf ids are the full branch name, used by
/// the context menu and log reveal.
const ROOT_PREFIX: &str = "remote:";

/// GitKraken-style remote tree items: one root per remote, a folder per
/// intermediate `/` segment, and a leaf per branch tip.
pub(crate) fn remote_tree_items(ws: &Workspace) -> Vec<TreeItem> {
    let mut remotes: BTreeMap<String, RemoteNode> = BTreeMap::new();
    for branch in &ws.branches {
        if branch.kind != BranchType::Remote {
            continue;
        }
        let Some((remote, short)) = branch.name.split_once('/') else {
            continue;
        };
        let segments: Vec<&str> = short.split('/').collect();
        insert_node(remotes.entry(remote.to_owned()).or_default(), &segments, &branch.name);
    }

    remotes
        .into_iter()
        .map(|(remote, node)| {
            let id = format!("{ROOT_PREFIX}{remote}");
            TreeItem::new(id.clone(), remote)
                .expanded(!ws.remote_collapsed.contains(&id))
                .children(node.build(&id, &ws.remote_collapsed))
        })
        .collect()
}

/// A remote's branch-name hierarchy: one node per folder segment, leaves
/// collected in `branches`.
#[derive(Default)]
struct RemoteNode {
    folders: BTreeMap<String, RemoteNode>,
    branches: Vec<String>,
}

/// Pack one branch's `/` segments into folders (all but the last); the leaf
/// keeps the full branch name for actions.
fn insert_node(node: &mut RemoteNode, segments: &[&str], full: &str) {
    match segments {
        [_leaf] => node.branches.push(full.to_owned()),
        [name, rest @ ..] => {
            insert_node(node.folders.entry((*name).to_owned()).or_default(), rest, full)
        }
        [] => {}
    }
}

impl RemoteNode {
    /// Folders first (like the GitKraken panel), sorted; leaves last. Root /
    /// folder ids are synthetic and reusable for the collapse records;
    /// leaf ids are the full branch name.
    fn build(self, id_prefix: &str, collapsed: &HashSet<String>) -> Vec<TreeItem> {
        let mut items: Vec<TreeItem> = self
            .branches
            .into_iter()
            .map(|full| {
                let label = full.rsplit('/').next().unwrap_or(&full).to_owned();
                TreeItem::new(full.clone(), label)
            })
            .collect();
        for (name, child) in self.folders {
            let id = format!("{id_prefix}/{name}");
            items.push(
                TreeItem::new(id.clone(), name)
                    .expanded(!collapsed.contains(&id))
                    .children(child.build(&id, collapsed)),
            );
        }
        items
    }
}

/// Drop collapse records for tree items that no longer exist, so the set
/// does not grow stale as branches come and go.
pub(crate) fn prune_collapsed<'a>(
    items: impl Iterator<Item = &'a TreeItem>,
    collapsed: &mut HashSet<String>,
) {
    fn walk(item: &TreeItem, collapsed: &mut HashSet<String>) {
        collapsed.remove(item.id.as_ref());
        for child in &item.children {
            walk(child, collapsed);
        }
    }
    items.for_each(|item| walk(item, collapsed));
}

/// The remote tree itself: virtualized, folders expand/collapse in place,
/// leaf clicks reveal the branch in the log, right-click opens the leaf menu.
fn render_remote_tree(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let workspace = cx.entity().downgrade();
    let workspace_for_menu = workspace.clone();

    let tree = tree(&ws.remote_tree, move |ix, entry, _, _, cx| {
        let foreground = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;
        let is_folder = entry.is_folder();
        let depth = entry.depth();
        let id = entry.item().id.clone();
        let label = entry.item().label.clone();

        let (icon, color) = if depth == 0 {
            (IconName::Cloud, foreground)
        } else if is_folder {
            let icon = if entry.is_expanded() { IconName::FolderOpen } else { IconName::Folder };
            (icon, muted)
        } else {
            (IconName::GitBranch, muted)
        };

        let mut item = ListItem::new(ix)
            .w_full()
            .min_w_0()
            .rounded_sm()
            .text_size(px(12.))
            .py_0p5()
            .pl(px(16.) * depth as f32 + px(4.))
            .text_color(color)
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1p5()
                    .child(Icon::new(icon).size_3().text_color(color))
                    .child(div().min_w_0().truncate().child(label)),
            );

        // Folders expand/collapse through the tree itself; leaves reveal.
        if !is_folder {
            item = item.on_click({
                let workspace = workspace.clone();
                let name = id.clone();
                move |_, _, cx| {
                    workspace.update(cx, |ws, cx| ws.reveal_branch(&name, cx)).ok();
                }
            });
        }
        item
    })
    // Only leaves act; the empty menu never opens for folders.
    .context_menu(move |_ix, entry, menu, _, cx| {
        if entry.is_folder() {
            return menu;
        }
        let name = entry.item().id.to_string();
        remote_branch_menu(menu, &name, &workspace_for_menu, cx)
    });

    div().flex_1().min_h_0().px_1().child(tree)
}

/// Leaf menu: checkout as a local tracking branch, fast-forward / merge
/// moves coherent with the remote tip, and delete the branch on its server.
pub(crate) fn remote_branch_menu(
    mut menu: PopupMenu,
    name: &str,
    workspace: &WeakEntity<Workspace>,
    cx: &mut App,
) -> PopupMenu {
    // Checkout
    menu = menu.item(PopupMenuItem::new("Checkout").icon(IconName::GitBranch).on_click({
        let workspace = workspace.clone();
        let name = name.to_owned();
        move |_, window, cx| {
            workspace.update(cx, |ws, cx| ws.checkout_remote_branch(&name, window, cx)).ok();
        }
    }));

    menu = menu.separator();

    // Integration moves relative to this remote tip (fast-forward of the
    // checked-out branch onto it, merge into checked-out branch).
    if let Some(entity) = workspace.upgrade() {
        let subject = entity.read(cx).branches.iter().find(|b| b.name == name).cloned();
        if let Some(subject) = subject {
            menu = integration_items(menu, &subject, workspace, cx);
            menu = menu.separator();
        }
    }

    // Delete (removes the branch from the remote server)
    menu = menu.item(PopupMenuItem::new("Delete").icon(IconName::Trash).on_click({
        let workspace = workspace.clone();
        let name = name.to_owned();
        move |_, _, cx| {
            workspace.update(cx, |ws, cx| ws.delete_remote_branch(&name, cx)).ok();
        }
    }));

    menu
}

// ---- local branches -----------------------------------------------------

/// Row click = scroll the log to this branch's tip. Menu button (hover) =
/// actions; the same menu is available on right-click (context menu).
fn branch_row_shell(
    branch: &Branch,
    cx: &mut Context<Workspace>,
    foreground: gpui_kit::Hsla,
    is_current: bool,
) -> gpui_kit::AnyElement {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    let name = branch.name.clone();

    // Only the name area reveals the branch in the log; the right slot keeps
    // the menu button outside the clickable area.
    let click_area = h_flex()
        .id(SharedString::from(format!("branch-{name}")))
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_1p5()
        .cursor_pointer()
        .on_click(cx.listener({
            let name = name.clone();
            move |this, _, _, cx| this.reveal_branch(&name, cx)
        }))
        .child(Icon::new(IconName::GitBranch).size_3().text_color(if is_current {
            cx.theme().primary
        } else {
            muted
        }))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(foreground)
                // Current branch stands out by weight alone (no badge).
                .when(is_current, |this| this.font_weight(FontWeight::SEMIBOLD))
                .child(name.clone()),
        );

    let mut row = h_flex()
        .w_full()
        .h_7()
        .px_2()
        .gap_2()
        .items_center()
        .justify_between()
        .rounded_sm()
        .text_size(px(12.))
        .group("branch-row")
        .hover(|style| style.bg(border.opacity(0.4)))
        .child(click_area);

    let workspace = cx.entity().downgrade();

    // Menu button, invisible until the row is hovered.
    row = row.child(
        Button::new(format!("menu-{name}"))
            .ghost()
            .xsmall()
            .icon(IconName::EllipsisVertical)
            .opacity(0.)
            .group_hover("branch-row", |style| style.opacity(1.))
            .dropdown_menu({
                let workspace = workspace.clone();
                let branch = branch.clone();
                move |menu, _, cx| branch_menu(menu, &branch, is_current, &workspace, cx)
            }),
    );

    // Same menu on right-click.
    row.context_menu({
        let branch = branch.clone();
        move |menu, _, cx| branch_menu(menu, &branch, is_current, &workspace, cx)
    })
    .into_any_element()
}

pub(crate) fn branch_menu(
    mut menu: PopupMenu,
    branch: &Branch,
    is_current: bool,
    workspace: &WeakEntity<Workspace>,
    cx: &mut App,
) -> PopupMenu {
    let name = branch.name.clone();

    // Checkout
    menu = menu.item(
        PopupMenuItem::new("Checkout").icon(IconName::GitBranch).disabled(is_current).on_click({
            let workspace = workspace.clone();
            let name = name.clone();
            move |_, window, cx| {
                workspace.update(cx, |ws, cx| ws.checkout_branch(&name, window, cx)).ok();
            }
        }),
    );

    // Pull (only meaningful on the checked-out branch)
    let can_pull = is_current && branch.upstream.is_some();
    menu = menu.item(
        PopupMenuItem::new("Pull").icon(IconName::Download).disabled(!can_pull).on_click({
            let workspace = workspace.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.run_remote(RemoteOp::Pull, cx)).ok();
            }
        }),
    );

    // Integration moves (fast-forward / merge / rebase) coherent with the
    // branches' commits.
    menu = integration_items(menu, branch, workspace, cx);

    // Set upstream (retarget always allowed, but only via its own dialog)
    menu = menu.item(
        PopupMenuItem::new("Set upstream…").icon(IconName::Upload).disabled(!is_current).on_click(
            {
                let workspace = workspace.clone();
                let name = name.clone();
                move |_, window, cx| {
                    workspace.update(cx, |ws, cx| ws.open_upstream_dialog(&name, window, cx)).ok();
                }
            },
        ),
    );

    menu = menu.separator();

    // Delete (refused on the checked-out branch)
    menu = menu.item(
        PopupMenuItem::new("Delete").icon(IconName::Trash).disabled(is_current).on_click({
            let workspace = workspace.clone();
            let name = name.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.delete_branch(&name, cx)).ok();
            }
        }),
    );

    // The menu is anchored under the row; text weight is inherited deliberately.
    menu
}

/// Ahead/behind-coherent integration entries for a branch context menu,
/// restricted to the pair (clicked branch, current branch). Flat items —
/// one per valid move, labeled with the branch names and the counts that
/// make them coherent. Reads live relations through `workspace`.
///
/// `(ahead, behind)` counts commits unique to the clicked branch, then
/// commits unique to the current branch. Moves offered:
///
/// - clicked strictly behind: its ref *fast-forwards* to the current tip.
/// - clicked with unique commits (`ahead > 0`): it **merges into** the
///   current branch (git fast-forwards when the current branch has nothing
///   unique, creates a merge commit otherwise).
/// - when the current branch has nothing unique (`behind == 0`) and the
///   clicked branch has commits (`ahead > 0`), a plain *fast-forward jump*
///   of the current branch onto the tip is offered next to the merge —
///   both reach the same history state, merge just goes through the
///   merger machinery.
/// - diverged (`ahead > 0 && behind > 0`): Rebase the **current branch**
///   onto the clicked tip (`git rebase <base> <branch>` — the checked-out
///   branch replays its unique commits on the clicked tip and stays
///   checked out). Counts are phrased from the rebased branch's
///   perspective (its `ahead` = the subject's `behind`).
///
/// For a clicked *remote* tip the ref itself cannot move, so the current
/// branch is the one that acts (fast-forward jump/merge/rebase against the
/// remote tip); when the remote tip is fully contained in the current
/// branch, the offered fast-forward *advances the remote ref* via a
/// non-force push. Same counting rules.
fn integration_items(
    mut menu: PopupMenu,
    subject: &Branch,
    workspace: &WeakEntity<Workspace>,
    cx: &mut App,
) -> PopupMenu {
    let Some(entity) = workspace.upgrade() else { return menu };
    if subject.head == Oid::ZERO_SHA1 {
        return menu;
    }
    let ws = entity.read(cx);
    let Some(current) = ws.current_branch.as_ref().filter(|c| c.name != subject.name) else {
        return menu;
    };
    let Some((ahead, behind)) =
        ws.branch_relations.get(&subject.head).and_then(|row| row.get(&current.head)).copied()
    else {
        return menu;
    };
    if ahead == 0 && behind == 0 {
        return menu; // same tip: nothing to move
    }

    // (label, icon, op, needs confirm dialog)
    let mut items: Vec<(String, IconName, BranchOp, bool)> = Vec::new();
    if subject.kind == BranchType::Local {
        if ahead == 0 && behind > 0 {
            items.push((
                format!(
                    "Fast-forward \"{}\" to \"{}\" (behind {behind})",
                    subject.name, current.name
                ),
                IconName::FastForward,
                BranchOp::FastForward {
                    from: subject.name.clone(),
                    to_ref: full_ref(current.kind, &current.name),
                },
                false,
            ));
        }
        if ahead > 0 && behind == 0 {
            // The subject is strictly ahead: the current branch can jump
            // straight to it (fast-forward only), or pull it in through a
            // merge — both land on the same history state.
            items.push((
                format!(
                    "Fast-forward \"{}\" to \"{}\" (behind {ahead})",
                    current.name, subject.name
                ),
                IconName::FastForward,
                BranchOp::FastForward {
                    from: current.name.clone(),
                    to_ref: full_ref(subject.kind, &subject.name),
                },
                false,
            ));
        }
        if ahead > 0 {
            items.push((
                format!("Merge \"{}\" into \"{}\" (ahead {ahead})", subject.name, current.name),
                IconName::GitMerge,
                BranchOp::Merge { what_ref: full_ref(subject.kind, &subject.name) },
                false,
            ));
        }
        if behind > 0 && ahead > 0 {
            // Diverged: the checked-out branch replays its unique commits
            // onto the clicked tip (git rebase <base> <branch> — leaves the
            // user on their branch). Counts are phrased from the rebased
            // branch's perspective.
            items.push((
                format!(
                    "Rebase \"{}\" onto \"{}\" (ahead {behind} / behind {ahead})",
                    current.name, subject.name
                ),
                IconName::RotateCcw,
                BranchOp::Rebase {
                    branch: current.name.clone(),
                    onto_ref: full_ref(subject.kind, &subject.name),
                },
                true,
            ));
        }
    } else {
        // Remote tip: the local ref cannot move it, so the current branch (or
        // the remote branch itself via push) is the one that acts — same
        // counting rules as for a local subject.
        if ahead == 0 && behind > 0 {
            // Remote tip fully contained in the current branch: advance the
            // remote ref with a fast-forward push.
            items.push((
                format!(
                    "Fast-forward \"{}\" to \"{}\" (behind {behind})",
                    subject.name, current.name
                ),
                IconName::FastForward,
                BranchOp::FastForwardPush {
                    from: current.name.clone(),
                    remote_branch: subject.name.clone(),
                },
                true,
            ));
        }
        if ahead > 0 && behind == 0 {
            // The current branch is strictly behind the tip: it can jump
            // straight there (fast-forward only), or take the commits
            // through a merge — both land on the same history state.
            items.push((
                format!(
                    "Fast-forward \"{}\" to \"{}\" (behind {ahead})",
                    current.name, subject.name
                ),
                IconName::FastForward,
                BranchOp::FastForward {
                    from: current.name.clone(),
                    to_ref: full_ref(subject.kind, &subject.name),
                },
                false,
            ));
        }
        if ahead > 0 {
            // The current branch lacks the tip's commits: merge it in —
            // git fast-forwards when the current branch itself has
            // nothing unique.
            items.push((
                format!("Merge \"{}\" into \"{}\" (ahead {ahead})", subject.name, current.name),
                IconName::GitMerge,
                BranchOp::Merge { what_ref: full_ref(subject.kind, &subject.name) },
                false,
            ));
        }
        if behind > 0 && ahead > 0 {
            // Diverged: the checked-out branch replays its unique commits
            // onto the remote tip. Counts from the rebased branch's
            // perspective (its `ahead` is the subject's `behind`).
            items.push((
                format!(
                    "Rebase \"{}\" onto \"{}\" (ahead {behind} / behind {ahead})",
                    current.name, subject.name
                ),
                IconName::RotateCcw,
                BranchOp::Rebase {
                    branch: current.name.clone(),
                    onto_ref: full_ref(subject.kind, &subject.name),
                },
                true,
            ));
        }
    }

    if items.is_empty() {
        return menu;
    }
    menu = menu.separator();
    for (label, icon, op, confirm) in items {
        menu = menu.item(PopupMenuItem::new(label).icon(icon).on_click({
            let workspace = workspace.clone();
            move |_, window, cx| {
                workspace
                    .update(cx, |ws, cx| {
                        if confirm {
                            ws.open_branch_op_dialog(op.clone(), window, cx)
                        } else {
                            ws.run_branch_op(op.clone(), cx)
                        }
                    })
                    .ok();
            }
        }));
    }

    menu
}

fn local_branch_row(
    ws: &Workspace,
    branch: &Branch,
    cx: &mut Context<Workspace>,
) -> gpui_kit::AnyElement {
    let is_current = ws.current_branch.as_ref().is_some_and(|c| c.name == branch.name);
    branch_row_shell(branch, cx, cx.theme().foreground, is_current)
}
