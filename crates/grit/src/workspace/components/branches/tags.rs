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
use gpui_kit::prelude::FluentBuilder as _;
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
        // rows, most recent first (the snapshot sorts by target date),
        // virtualized like the staging lists: the closure gets `&mut App`
        // only, state is read through the captured `WeakEntity`.
        .child(
            uniform_list("tag-list", ws.tags.len(), {
                let workspace = workspace.clone();
                move |range, _window, cx| {
                    let Some(entity) = workspace.upgrade() else {
                        return Vec::new();
                    };
                    let ws = entity.read(cx);
                    range
                        .map(|ix| match ws.tags.get(ix) {
                            Some(tag) => {
                                let on_remote = ws
                                    .remote_tags
                                    .as_ref()
                                    .is_some_and(|known| known.contains(tag.name.as_str()));
                                tag_row(tag, on_remote, workspace.clone(), cx)
                            }
                            None => div().id(format!("tag-empty-{ix}")).into_any_element(),
                        })
                        .collect()
                }
            })
            .flex_1()
            .min_h_0()
            .w_full()
            .pb_1(),
        )
}

/// One tag line: tag icon + name, a cloud marker when the tag is already
/// published (remote tags are learned in the background), and actions on
/// right-click plus the hover-revealed menu button, like branch rows.
fn tag_row(
    tag: &Tag,
    on_remote: bool,
    workspace: WeakEntity<Workspace>,
    cx: &App,
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
        .when(on_remote, |this| {
            this.child(Icon::new(IconName::Cloud).size_3().text_color(cx.theme().info.opacity(0.8)))
        })
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
                    move |menu, _, cx| tag_menu(menu, &name, on_remote, &workspace, cx)
                }),
        )
        .context_menu({
            let workspace = workspace.clone();
            move |menu, _, cx| tag_menu(menu, &name, on_remote, &workspace, cx)
        })
        .into_any_element()
}
