//! Remote branches rendered as a GitKraken-style tree: one root per
//! remote, a folder per intermediate `/` segment, a leaf per branch tip.
//! Folders expand/collapse in place, leaf clicks reveal the branch in the
//! log, right-click opens the leaf menu.

use std::collections::{BTreeMap, HashSet};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, h_flex,
    list::ListItem,
    menu::{PopupMenu, PopupMenuItem},
    tree::{TreeItem, tree},
};
use gpui_kit::*;
use grit_core::branch::BranchType;

use super::{Workspace, github_menu_items, integration_items};
use crate::workspace::components::primitives::PrBadge;

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
pub(super) fn render_remote_tree(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
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

        // Leaves with an open PR show its `#n` chip after the label.
        if !is_folder {
            let pr = workspace.upgrade().and_then(|entity| {
                let ws = entity.read(cx);
                let remote = ws.github.remote.as_deref()?;
                let short = id.strip_prefix(&format!("{remote}/"))?;
                ws.github.prs.get(short).cloned()
            });
            if let Some(pr) = pr {
                item = item.child(PrBadge::new(&pr, cx));
            }
        }

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
    // Only leaves act; folders carry no menu except the GitHub repo root.
    .context_menu(move |_ix, entry, menu, _, cx| {
        if entry.is_folder() {
            if entry.depth() == 0 {
                let remote = entry.item().id.strip_prefix(ROOT_PREFIX).unwrap_or_default();
                return github_repo_menu(menu, remote, &workspace_for_menu, cx);
            }
            return menu;
        }
        let name = entry.item().id.to_string();
        remote_branch_menu(menu, &name, &workspace_for_menu, cx)
    });

    div().flex_1().min_h_0().px_1().child(tree)
}

/// Leaves menu: checkout as a local tracking branch, fast-forward / merge
/// moves coherent with the remote tip, delete on the server, and the
/// GitHub entries (PR / branch commits).
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

    // GitHub web entries (PR page / commits view), keyed by short name.
    let short = name.split_once('/').map(|(_, s)| s).unwrap_or(name).to_owned();
    github_menu_items(menu, &short, workspace, cx)
}

/// The remote root's (depth-0) menu: open the repository on GitHub — only
/// when this remote's URL resolved to GitHub coordinates (otherwise the
/// empty menu never opens, like before).
pub(crate) fn github_repo_menu(
    mut menu: PopupMenu,
    remote: &str,
    workspace: &WeakEntity<Workspace>,
    cx: &mut App,
) -> PopupMenu {
    let Some(entity) = workspace.upgrade() else { return menu };
    let url = {
        let ws = entity.read(cx);
        ws.github
            .remote
            .as_deref()
            .filter(|r| *r == remote)
            .and_then(|_| ws.github.coords.as_ref().map(|c| c.web_url()))
    };
    if let Some(url) = url {
        menu = menu.item(
            PopupMenuItem::new("Open repository on GitHub").icon(IconName::Github).on_click({
                let workspace = workspace.clone();
                move |_, _, cx| {
                    workspace.update(cx, |ws, cx| ws.open_github_url(url.clone(), cx)).ok();
                }
            }),
        );
    }
    menu
}
