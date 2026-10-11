//! Left column: local / remote branches.
//!
//! Click a branch to browse its history (no checkout). The menu button,
//! revealed on hover, gives checkout / pull / set-upstream / delete; the same
//! menu is available on right-click (context menu).

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use grit_core::branch::{Branch, BranchType};

use super::primitives::SectionLabel;
use crate::workspace::Workspace;
use grit_core::remote::RemoteOp;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;

    let local_rows: Vec<_> = ws
        .branches
        .iter()
        .filter(|b| b.kind == BranchType::Local)
        .map(|b| local_branch_row(ws, b, cx))
        .collect();
    let remote_rows: Vec<_> = ws
        .branches
        .iter()
        .filter(|b| b.kind == BranchType::Remote)
        .map(|b| remote_branch_row(ws, b, cx))
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
        // branch list
        .child(
            v_flex()
                .flex_1()
                .min_h_0()
                .id("branches")
                .overflow_y_scroll()
                .p_1()
                .gap_0p5()
                // Double-click on empty space: back to the current branch's history.
                .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                    if event.click_count() >= 2 && this.viewed_branch.is_some() {
                        this.view_branch_current(cx);
                    }
                }))
                .child(SectionLabel::new("LOCAL", cx))
                .children(local_rows)
                .child(div().mt_2().child(SectionLabel::new("REMOTES", cx)))
                .children(remote_rows),
        )
}

/// Row click = view this branch's history. Menu button (hover) = actions.
fn branch_row_shell(
    branch: &Branch,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
    foreground: gpui_kit::Hsla,
    is_current: bool,
    with_menu: bool,
) -> gpui_kit::AnyElement {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    let name = branch.name.clone();
    let is_viewed = ws.viewed_branch.as_deref() == Some(name.as_str());

    // Only the name area opens the branch's history; the right slot keeps the
    // badge and menu button outside the clickable area.
    let click_area = h_flex()
        .id(SharedString::from(format!("branch-{name}")))
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_1p5()
        .when(!is_viewed, |this| this.cursor_pointer())
        .on_click(cx.listener({
            let name = name.clone();
            move |this, _, _, cx| this.view_branch(&name, cx)
        }))
        .child(Icon::new(IconName::GitBranch).size_3().text_color(if is_current {
            cx.theme().primary
        } else if is_viewed {
            cx.theme().info
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
        .when(is_viewed, |this| this.bg(cx.theme().accent.opacity(0.3)))
        .when(!is_viewed, |this| this.hover(|style| style.bg(border.opacity(0.4))))
        .child(click_area);

    if !with_menu {
        return row.into_any_element();
    }

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
                move |menu, _, _| branch_menu(menu, &branch, is_current, &workspace)
            }),
    );

    // Same menu on right-click.
    row.context_menu({
        let branch = branch.clone();
        move |menu, _, _| branch_menu(menu, &branch, is_current, &workspace)
    })
    .into_any_element()
}

fn branch_menu(
    mut menu: PopupMenu,
    branch: &Branch,
    is_current: bool,
    workspace: &WeakEntity<Workspace>,
) -> PopupMenu {
    let name = branch.name.clone();

    // Checkout
    menu = menu.item(
        PopupMenuItem::new("Checkout").icon(IconName::GitBranch).disabled(is_current).on_click({
            let workspace = workspace.clone();
            let name = name.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.checkout_branch(&name, cx)).ok();
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

fn local_branch_row(
    ws: &Workspace,
    branch: &Branch,
    cx: &mut Context<Workspace>,
) -> gpui_kit::AnyElement {
    let is_current = ws.current_branch.as_ref().is_some_and(|c| c.name == branch.name);
    branch_row_shell(branch, ws, cx, cx.theme().foreground, is_current, true)
}

fn remote_branch_row(
    ws: &Workspace,
    branch: &Branch,
    cx: &mut Context<Workspace>,
) -> gpui_kit::AnyElement {
    branch_row_shell(branch, ws, cx, cx.theme().muted_foreground, false, false)
}
