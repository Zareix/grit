//! The log's ref chips (branches / tags): one chip, then a muted "+N"
//! whose HoverCard lists the remaining refs; chip right-clicks carry the
//! sidebar's branch / remote menus.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, h_flex,
    hover_card::HoverCard,
    menu::{ContextMenuExt as _, PopupMenu},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::REFS_W;
use crate::workspace::components::branches::{branch_menu, remote_branch_menu};
use crate::workspace::{CommitRef, CommitRefKind, Workspace};
use grit_core::Oid;
use grit_core::branch::BranchType;

/// The refs (branches / tags) pointing at a commit: a single chip, then a
/// muted "+N" whose HoverCard lists the remaining refs on hover.
pub(super) fn ref_chips(
    refs: &[CommitRef],
    oid: Oid,
    workspace: &WeakEntity<Workspace>,
    cx: &App,
) -> Div {
    let mut cell = h_flex().flex_none().w(px(REFS_W)).min_w_0().gap_1().overflow_hidden();
    let Some(first) = refs.first() else { return cell };
    cell = cell.child(ref_chip(first, workspace, 140., cx));

    let extra = refs.len() - 1;
    if extra > 0 {
        let more: Vec<CommitRef> = refs[1..].to_vec();
        let muted = cx.theme().muted_foreground;
        cell = cell.child(
            HoverCard::new(SharedString::from(format!("refs-more-{oid}")))
                // Below the trigger, left-aligned with it.
                .anchor(Anchor::TopLeft)
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
                        .child(format!("+{extra}")),
                )
                .child({
                    let chips: Vec<_> =
                        more.iter().map(|r| ref_chip(r, workspace, 220., cx)).collect();
                    v_flex().gap_0p5().children(chips)
                }),
        );
    }
    cell
}

/// One ref as a small colored chip (in the log row and in the "+N" hover
/// card): local branch = primary, tag = success, remote = info. `max_w`
/// bounds the name before an ellipsis kicks in. Branch chips open the same
/// context menus as the sidebar on right-click.
fn ref_chip(
    r: &CommitRef,
    workspace: &WeakEntity<Workspace>,
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
    div()
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
        )
        // Same menus as the sidebar, read live when the menu opens.
        .context_menu({
            let workspace = workspace.clone();
            let r = r.clone();
            move |menu, _, cx| ref_menu(menu, &r, &workspace, cx)
        })
        .into_any_element()
}

/// The branches sidebar's menus on the log's ref chips. Only branches act —
/// tags have no actions yet, and an empty menu never opens.
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
        CommitRefKind::Tag => menu,
    }
}
