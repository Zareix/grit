//! Left column: local / remote branches, checkout on click, inline creation.

use gk_core::branch::{Branch, BranchType};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;

    // Collect eagerly: the map closures borrow `cx`, and lazy iterators would
    // keep the borrow alive across the rest of the render.
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
        .map(|b| remote_branch_row(b, muted))
        .collect();

    v_flex()
        .w_full()
        .h_full()
        .flex_none()
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
                .child(section_label("LOCAL", muted))
                .children(local_rows)
                .child(section_label("REMOTES", muted).mt_2())
                .children(remote_rows),
        )
}

fn section_label(title: &'static str, color: gpui_kit::Hsla) -> Div {
    h_flex()
        .px_2()
        .py_0p5()
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(title)
}

fn local_branch_row(ws: &Workspace, branch: &Branch, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let name = branch.name.clone();
    let is_current = ws.current_branch.as_ref().is_some_and(|c| c.name == branch.name);
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;

    let row = h_flex()
        .w_full()
        .px_2()
        .py_0p5()
        .gap_2()
        .items_center()
        .justify_between()
        .rounded_sm()
        .text_size(px(12.))
        .id(SharedString::from(format!("branch-{name}")))
        .when(is_current, |this| this.font_weight(FontWeight::SEMIBOLD))
        .when(!is_current, |this| {
            this.cursor_pointer().hover(|style| style.bg(border.opacity(0.4))).on_click(
                cx.listener({
                    let name = name.clone();
                    move |this, _, _, cx| this.checkout_branch(&name, cx)
                }),
            )
        })
        .child(
            h_flex()
                .min_w_0()
                .items_center()
                .gap_1p5()
                .child(Icon::new(IconName::GitBranch).size_3().text_color(if is_current {
                    cx.theme().primary
                } else {
                    muted
                }))
                .child(div().min_w_0().truncate().child(name.clone())),
        );

    if is_current {
        row
    } else {
        // The Button stops propagation, so this does not trigger the row checkout.
        row.child(
            Button::new(format!("delete-{name}")).ghost().xsmall().icon(IconName::Trash).on_click(
                cx.listener({
                    let name = name.clone();
                    move |this, _, _, cx| this.delete_branch(&name, cx)
                }),
            ),
        )
    }
}

fn remote_branch_row(branch: &Branch, muted: gpui_kit::Hsla) -> Div {
    h_flex()
        .w_full()
        .px_2()
        .py_0p5()
        .gap_1p5()
        .items_center()
        .text_size(px(12.))
        .text_color(muted)
        .child(Icon::new(IconName::GitBranch).size_3())
        .child(div().min_w_0().truncate().child(branch.name.clone()))
}
