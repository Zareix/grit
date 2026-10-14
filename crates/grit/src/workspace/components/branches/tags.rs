//! The tags panel in the branches column's bottom: fixed header (title +
//! count + collapse toggle) and one row per tag, most recent first.
//!
//! Rows reuse the stash-row mechanics: click reveals the tagged commit
//! in the log; the shared menu carries push and delete-behind-confirm.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _},
    v_flex,
};
use gpui_kit::*;
use grit_core::tags::Tag;

use super::tag_menu;
use crate::workspace::Workspace;

/// Minimum height of the panel (exactly its `h_7` header), so the size
/// range lets the user shrink it down to the header alone.
pub(crate) const TAGS_MIN_SIZE: gpui_kit::Pixels = gpui_kit::px(28.);

/// The tags panel, styled like the stash panel: fixed header and tags as
/// single-line rows, newest first. Resizable through the bottom group's
/// handle; no separate collapse control.
pub(super) fn render_tags(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let workspace = cx.entity().downgrade();

    v_flex()
        .w_full()
        .h_full()
        .flex_none()
        // header, fixed at the panel's minimum height
        .child(
            h_flex()
                .w_full()
                .h_7()
                .px_2()
                .gap_2()
                .items_center()
                .flex_none()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child("Tags"),
        )
        // rows, most recent first (the snapshot sorts by target date).
        // The rows sit in a `flex_none` wrapper inside the scroll area:
        // one fixed content block instead of N children of a flex_1
        // container, so the row pitch cannot stretch with the panel.
        .child(v_flex().flex_1().min_h_0().id("tag-list").overflow_y_scroll().pb_1().child(
            v_flex().flex_none().flex_col().children(
                ws.tags.iter().map(|tag| tag_row(tag, workspace.clone(), cx)).collect::<Vec<_>>(),
            ),
        ))
}

/// One tag line: tag icon + name, actions on right-click (and through the
/// hover-revealed menu button, like branch rows).
fn tag_row(
    tag: &Tag,
    workspace: WeakEntity<Workspace>,
    cx: &mut Context<Workspace>,
) -> gpui_kit::AnyElement {
    let border = cx.theme().border;
    let foreground = cx.theme().foreground;
    let name = tag.name.clone();

    div()
        .id(SharedString::from(format!("tag-{name}")))
        .group("tag-row")
        .w_full()
        .h_7()
        .px_2()
        .flex()
        .items_center()
        .gap_1p5()
        .rounded_sm()
        .cursor_pointer()
        .hover(|style| style.bg(border.opacity(0.4)))
        .on_click({
            let workspace = workspace.clone();
            let name = name.clone();
            move |_, _, cx| {
                workspace.update(cx, |ws, cx| ws.reveal_tag(&name, cx)).ok();
            }
        })
        .child(Icon::new(IconName::Tag).size_3().text_color(cx.theme().success))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(px(12.))
                .text_color(foreground)
                .child(name.clone()),
        )
        .child(
            Button::new(format!("menu-tag-{name}"))
                .ghost()
                .xsmall()
                .icon(IconName::EllipsisVertical)
                .opacity(0.)
                .group_hover("tag-row", |style| style.opacity(1.))
                .dropdown_menu({
                    let workspace = workspace.clone();
                    let name = name.clone();
                    move |menu, _, cx| tag_menu(menu, &name, &workspace, cx)
                }),
        )
        .context_menu({
            let workspace = workspace.clone();
            move |menu, _, cx| tag_menu(menu, &name, &workspace, cx)
        })
        .into_any_element()
}
