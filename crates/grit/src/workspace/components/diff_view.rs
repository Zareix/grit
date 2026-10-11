//! Diff panel: the selected file's patch, rendered by the gpui-kit `Diff`
//! component (virtualized rows, line numbers, tree-sitter syntax colors,
//! unified or side-by-side layout).

use crate::workspace::Workspace;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    diff::{Diff, DiffHunkSeparator, DiffMode},
    h_flex, v_flex,
};
use gpui_kit::*;

/// Whether `path` has a viewable image extension (per GPUI's `img` list).
pub(crate) fn is_viewable_image(path: &std::path::Path) -> bool {
    let extensions = Img::extensions();
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| extensions.contains(&e.as_str()))
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;

    let Some(path) = &ws.selected_path else {
        return div();
    };

    let full = path.to_string_lossy().to_string();
    let name =
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| full.clone());
    // The Diff component shows a per-file summary below the header when a
    // file has no rows (binary change, pure rename…), so the viewer renders
    // whenever files are installed; "No changes" only for a truly empty diff.
    let has_files = !ws.diff.read(cx).files().is_empty();
    // Image files show a live preview instead of the (binary) patch. The file
    // must exist: a deletion leaves no image to display.
    let is_image = is_viewable_image(path) && path.is_file();

    let source = if ws.diff_from_commit {
        match &ws.selected_commit {
            Some(commit) => {
                let short = commit.id.to_string().chars().take(7).collect::<String>();
                format!("in {short}")
            }
            None => "commit".to_string(),
        }
    } else if ws.selected_staged {
        "staged".to_string()
    } else {
        "unstaged".to_string()
    };

    v_flex()
        .size_full()
        .min_h_0()
        // header: file name + source chip on top, close right
        .child(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(
                    h_flex()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .gap_1()
                        .child(Icon::new(IconName::FileCode).size_3().text_color(muted))
                        .child(
                            v_flex().min_w_0().gap_0p5().child(
                                h_flex()
                                    .min_w_0()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().foreground)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .px_1()
                                            .rounded_sm()
                                            .text_size(px(10.))
                                            .text_color(muted)
                                            .bg(border.opacity(0.5))
                                            .child(source),
                                    ),
                            ),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("toggle-diff-mode")
                                .ghost()
                                .xsmall()
                                .label(if ws.split_mode(cx) { "Unified" } else { "Split" })
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_diff_mode(cx))),
                        )
                        .child(
                            Button::new("close-diff")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|this, _, _, cx| this.clear_selection(cx))),
                        ),
                ),
        )
        // body: image preview for image files (no diff), otherwise the Diff
        // component, which owns its scrolling; do not nest another scrollable
        // region (unified rows, or two columns in split).
        .child(div().flex_1().min_h_0().child(if is_image {
            image_preview(path).into_any_element()
        } else if has_files {
            Diff::new(&ws.diff)
                .hunk_separator(DiffHunkSeparator::LineInfo)
                .soft_wrap(true)
                .h_full()
                .into_any_element()
        } else {
            div()
                .flex()
                .justify_center()
                .py_4()
                .text_sm()
                .text_color(muted)
                .child("No changes")
                .into_any_element()
        }))
}

/// Centered, size-constrained preview of an image file on disk. GPUI's `img`
/// loads and decodes the file asynchronously with its own asset cache and
/// `ObjectFit::Contain` by default, so the image scales to the available
/// space without overflowing.
fn image_preview(path: &std::path::Path) -> impl IntoElement {
    div()
        .flex_1()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .overflow_hidden()
        .child(img(path.to_path_buf()).max_w_full().max_h_full())
}

impl Workspace {
    /// Whether the diff viewer currently shows the side-by-side layout.
    pub(crate) fn split_mode(&self, cx: &App) -> bool {
        self.diff.read(cx).mode() == DiffMode::Split
    }
}
