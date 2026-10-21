//! The stash panel in the branches column's bottom: header (title +
//! count + collapse toggle) and one row per entry with a right-click
//! menu (apply / pop / drop).

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;
use grit_core::Stash;

use crate::workspace::Workspace;

/// The stash panel, styled like the staging column's sections: fixed header
/// (title + count + collapse toggle) and stashes as single-line rows.
pub(super) fn render_stashes(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let workspace = cx.entity().downgrade();
    let count = ws.stashes.len();

    v_flex()
        .w_full()
        .h_full()
        .flex_none()
        // header, always visible (collapsible through the chevron)
        .child(
            h_flex()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .justify_between()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(h_flex().gap_2().child("Stashes").child(format!("{count}")))
                .child(
                    Button::new("toggle-stashes")
                        .ghost()
                        .xsmall()
                        .icon(if ws.stashes_collapsed {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_stashes_collapsed(cx))),
                ),
        )
        // rows, virtualized like the staging lists: the closure gets
        // `&mut App` only, state is read through the captured `WeakEntity`.
        .child(if ws.stashes_collapsed {
            v_flex().flex_1().min_h_0().into_any_element()
        } else {
            uniform_list("stash-list", ws.stashes.len(), {
                let workspace = workspace.clone();
                move |range, _window, cx| {
                    let Some(entity) = workspace.upgrade() else {
                        return Vec::new();
                    };
                    let ws = entity.read(cx);
                    range
                        .map(|ix| match ws.stashes.get(ix) {
                            Some(stash) => stash_row(stash, workspace.clone(), cx),
                            None => div().id(format!("stash-empty-{ix}")).into_any_element(),
                        })
                        .collect()
                }
            })
            .flex_1()
            .min_h_0()
            .w_full()
            .pb_1()
            .into_any_element()
        })
}

/// One stash entry line: archive icon + message, actions on right-click.
fn stash_row(stash: &Stash, workspace: WeakEntity<Workspace>, cx: &App) -> gpui_kit::AnyElement {
    let muted = cx.theme().muted_foreground;
    let border = cx.theme().border;
    let foreground = cx.theme().foreground;
    let message = stash.message.clone();
    let index = stash.index;

    div()
        .id(SharedString::from(format!("stash-{index}")))
        .w_full()
        .h_7()
        .px_2()
        .flex()
        .items_center()
        .gap_1p5()
        .rounded_sm()
        .cursor_pointer()
        .hover(|style| style.bg(border.opacity(0.4)))
        .child(Icon::new(IconName::Archive).size_3().text_color(muted))
        .child(div().min_w_0().truncate().text_size(px(12.)).text_color(foreground).child(message))
        .context_menu(move |menu, _, cx| stash_menu(menu, index, &workspace, cx))
        .into_any_element()
}

/// Right-click actions on a stash entry: reapply (keeping or taking it out
/// of the list) and drop (confirm dialog).
fn stash_menu(
    mut menu: PopupMenu,
    index: usize,
    workspace: &WeakEntity<Workspace>,
    _cx: &mut App,
) -> PopupMenu {
    menu = menu.item(PopupMenuItem::new("Apply").icon(IconName::RotateCcw).on_click({
        let workspace = workspace.clone();
        move |_, _, cx| {
            workspace.update(cx, |ws, cx| ws.apply_stash(index, cx)).ok();
        }
    }));
    menu = menu.item(PopupMenuItem::new("Pop").icon(IconName::ArrowDown).on_click({
        let workspace = workspace.clone();
        move |_, _, cx| {
            workspace.update(cx, |ws, cx| ws.pop_stash(index, cx)).ok();
        }
    }));
    menu = menu.separator();
    menu = menu.item(PopupMenuItem::new("Drop").icon(IconName::Trash).on_click({
        let workspace = workspace.clone();
        move |_, window, cx| {
            workspace.update(cx, |ws, cx| ws.open_stash_drop_dialog(index, window, cx)).ok();
        }
    }));
    menu
}
