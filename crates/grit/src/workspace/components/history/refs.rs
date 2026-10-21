//! The log's ref chips (branches / tags): one chip, then a muted "+N"
//! whose HoverCard lists the remaining refs; chip right-clicks carry the
//! sidebar's branch / remote menus.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, h_flex,
    hover_card::HoverCard,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::REFS_W;
use crate::workspace::components::branches::{branch_menu, remote_branch_menu, tag_menu};
use crate::workspace::{CommitRef, CommitRefKind, Workspace};
use grit_core::Oid;
use grit_core::branch::BranchType;

/// The refs (branches / tags) pointing at a commit: a single chip, then a
/// muted "+N" whose HoverCard lists the remaining refs (display only) and
/// whose right-click opens a menu with one submenu per ref — the card's
/// chips must stay non-interactive: a context menu opened from inside the
/// HoverCard is treated as out-of-card hover, so the card closes (and
/// takes the menu with it).
pub(super) fn ref_chips(
    refs: &[CommitRef],
    oid: Oid,
    workspace: &WeakEntity<Workspace>,
    cx: &App,
) -> Div {
    let mut cell = h_flex().flex_none().w(px(REFS_W)).min_w_0().gap_1().overflow_hidden();
    let Some(first) = refs.first() else { return cell };
    cell = cell.child(ref_chip(first, workspace, true, 140., cx));

    let extra = refs.len() - 1;
    if extra > 0 {
        let more: Vec<CommitRef> = refs[1..].to_vec();
        let muted = cx.theme().muted_foreground;
        cell = cell.child(
            HoverCard::new(SharedString::from(format!("refs-more-{oid}")))
                // Above the trigger: the log rows below must stay readable
                // (the card otherwise covers the refs cell / next row).
                .anchor(Anchor::BottomLeft)
                .open_delay(Duration::from_millis(300))
                .close_delay(Duration::from_millis(150))
                .flex_none()
                .trigger(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(muted)
                        .hover(|style| style.text_color(cx.theme().foreground))
                        // The overflow menu: one submenu per hidden ref.
                        .context_menu({
                            let workspace = workspace.clone();
                            let more = more.clone();
                            move |menu, window, cx| {
                                overflow_menu(menu, &more, &workspace, window, cx)
                            }
                        })
                        .child(format!("+{extra}")),
                )
                // Display-only chips: no menus inside the card. Built
                // lazily via `.content` — the builder only runs when the
                // card opens (gpui-kit's documented perf pattern), instead
                // of eagerly re-building every chip on every row render.
                .content({
                    let workspace = workspace.clone();
                    move |_state, _window, cx| {
                        let chips: Vec<_> =
                            more.iter().map(|r| ref_chip(r, &workspace, false, 220., cx)).collect();
                        v_flex().gap_0p5().children(chips)
                    }
                }),
        );
    }
    cell
}

/// The "+N" overflow menu: a labeled submenu per hidden ref, carrying the
/// same actions as the sidebar menus on its rows.
fn overflow_menu(
    mut menu: PopupMenu,
    refs: &[CommitRef],
    workspace: &WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(entity) = workspace.upgrade() else { return menu };
    for r in refs {
        // Build each submenu with a fresh menu, then attach it by label.
        let icon = match r.kind {
            CommitRefKind::Local => IconName::GitBranch,
            CommitRefKind::Remote => IconName::GitBranch,
            CommitRefKind::Tag => IconName::Tag,
        };
        let entity = entity.clone();
        let sub = PopupMenu::build(window, cx, {
            let workspace = workspace.clone();
            let r = r.clone();
            move |menu, _window, cx| match r.kind {
                CommitRefKind::Local => {
                    let branch = entity
                        .read(cx)
                        .branches
                        .iter()
                        .find(|b| b.kind == BranchType::Local && b.name == r.name)
                        .cloned();
                    if let Some(branch) = branch {
                        let is_current = branch.name == r.name;
                        branch_menu(menu, &branch, is_current, &workspace, cx)
                    } else {
                        menu
                    }
                }
                CommitRefKind::Remote => remote_branch_menu(menu, &r.name, &workspace, cx),
                CommitRefKind::Tag => {
                    let on_remote = entity
                        .read(cx)
                        .remote_tags
                        .as_ref()
                        .is_some_and(|known| known.contains(r.name.as_str()));
                    tag_menu(menu, &r.name, on_remote, &workspace, cx)
                }
            }
        });
        menu = menu.item(PopupMenuItem::submenu(r.name.clone(), sub).icon(icon));
    }
    // The menu is anchored at the "+N"; label with the ref name.
    menu
}

/// One ref as a small colored chip (in the log row and in the "+N" hover
/// card): local branch = primary, tag = success, remote = info. `max_w`
/// bounds the name before an ellipsis kicks in. When `interactive`, the
/// chip opens the ref's own menu on right-click (log rows only — the
/// hover card's chips are display-only).
fn ref_chip(
    r: &CommitRef,
    workspace: &WeakEntity<Workspace>,
    interactive: bool,
    max_w: f32,
    cx: &App,
) -> gpui_kit::AnyElement {
    let color = match r.kind {
        CommitRefKind::Local => cx.theme().primary,
        CommitRefKind::Remote => cx.theme().info,
        CommitRefKind::Tag => cx.theme().success,
    };
    // Only tags carry their own icon; branch chips stay bare.
    let icon = match r.kind {
        CommitRefKind::Tag => Some(IconName::Tag),
        _ => None,
    };
    let base = div()
        .min_w_0()
        .px_1()
        .rounded_sm()
        .bg(color.opacity(0.15))
        .text_size(px(11.))
        .text_color(color)
        .when(r.is_current, |this| this.font_weight(FontWeight::SEMIBOLD))
        .child(
            h_flex()
                .min_w_0()
                .gap_1()
                .when_some(icon, |this, icon| {
                    this.child(Icon::new(icon).size_3().text_color(color))
                })
                .child(div().max_w(px(max_w)).truncate().child(r.name.clone())),
        );
    // Same menus as the sidebar, read live when the menu opens — chips
    // painted directly in the row carry their own context menu; the
    // card's chips stay display-only. Kept off `.when`: the closure
    // would have to return the ContextMenu wrapper type.
    if interactive {
        base.context_menu({
            let workspace = workspace.clone();
            let r = r.clone();
            move |menu, _, cx| ref_menu(menu, &r, &workspace, cx)
        })
        .into_any_element()
    } else {
        base.into_any_element()
    }
}

/// The branches sidebar's menus on the log's ref chips. Tags act from
/// here too (push / delete), branches delegate to the sidebar menus.
fn ref_menu(
    menu: PopupMenu,
    r: &CommitRef,
    workspace: &WeakEntity<Workspace>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(entity) = workspace.upgrade() else { return menu };
    match r.kind {
        CommitRefKind::Local => {
            // Clone the branch up front: `branch_menu` mutably re-uses `cx`.
            let branch = entity
                .read(cx)
                .branches
                .iter()
                .find(|b| b.kind == BranchType::Local && b.name == r.name)
                .cloned();
            if let Some(branch) = branch {
                branch_menu(menu, &branch, r.is_current, workspace, cx)
            } else {
                menu
            }
        }
        CommitRefKind::Remote => remote_branch_menu(menu, &r.name, workspace, cx),
        CommitRefKind::Tag => {
            let on_remote = entity
                .read(cx)
                .remote_tags
                .as_ref()
                .is_some_and(|known| known.contains(r.name.as_str()));
            tag_menu(menu, &r.name, on_remote, workspace, cx)
        }
    }
}
