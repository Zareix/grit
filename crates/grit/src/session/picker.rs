//! The repo-picker "new tab" page: open-folder button, error banner, and
//! the recent repositories list (one row per known repo).

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Session;

impl Session {
    /// The picker page: open-folder button, error banner, recent repos.
    pub(super) fn render_new_tab(
        &self,
        at: usize,
        owner: &WeakEntity<Session>,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let owner = owner.clone();

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                v_flex()
                    .w(px(520.))
                    .gap_2()
                    .child(
                        v_flex()
                            .items_center()
                            .pb_2()
                            .child(
                                Icon::new(IconName::FolderOpen)
                                    .size_6()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Open a repository"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("Pick a local Git working tree to open it in a tab."),
                            ),
                    )
                    .child(
                        h_flex().justify_center().child(
                            Button::new("pick-folder")
                                .primary()
                                .label("Choose folder…")
                                .icon(IconName::FolderOpen)
                                .on_click({
                                    let owner = owner.clone();
                                    move |_, window, cx| {
                                        owner
                                            .update(cx, |this, cx| this.pick_folder(at, window, cx))
                                            .ok();
                                    }
                                }),
                        ),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1()
                                .rounded_sm()
                                .bg(theme.danger.opacity(0.15))
                                .child(
                                    Icon::new(IconName::CircleAlert)
                                        .size_4()
                                        .text_color(theme.danger),
                                )
                                .child(div().text_sm().text_color(theme.danger).child(error)),
                        )
                    })
                    .when(!self.known.is_empty(), |this| {
                        this.child(
                            div()
                                .pt_2()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("RECENT REPOSITORIES"),
                        )
                        .child(
                            div()
                                .id("known-repos")
                                .max_h(px(320.))
                                .overflow_y_scroll()
                                .gap_0p5()
                                .flex()
                                .flex_col()
                                .children(self.known.iter().enumerate().map(|(ix, path)| {
                                    let number = ix + 1;
                                    known_repo_row(number, at, path, &owner, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// One row of the recent repositories list on the picker page.
fn known_repo_row(
    ix: usize,
    at: usize,
    path: &str,
    owner: &WeakEntity<Session>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let owner = owner.clone();
    let path_buf = PathBuf::from(path);
    let name = path_buf
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());

    h_flex()
        .id(SharedString::from(format!("known-repo-{ix}")))
        .w_full()
        .flex_none()
        .gap_2()
        .px_3()
        .py_1p5()
        .rounded_sm()
        .items_center()
        .cursor_pointer()
        .hover(|style| style.bg(theme.accent.opacity(0.4)))
        .child(Icon::new(IconName::Folder).size_4().text_color(theme.muted_foreground))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(name))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(path.to_string()),
                ),
        )
        .on_click(move |_, window, cx| {
            owner.update(cx, |this, cx| this.open_repo(&path_buf, at, window, cx)).ok();
        })
}
