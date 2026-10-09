//! Diff panel: unified patch of the selected file, colored per line.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;

use crate::workspace::Workspace;

/// One rendered patch line.
struct PatchLine {
    text: String,
    kind: LineKind,
}

enum LineKind {
    /// Added content.
    Added,
    /// Removed content.
    Removed,
    /// Hunk header (@@ ... @@).
    Hunk,
    /// diff/index/---/+++ metadata.
    Meta,
    Context,
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;

    let Some(path) = &ws.selected_path else {
        return div();
    };

    let title = path.to_string_lossy().to_string();
    let source = if ws.selected_staged { "staged" } else { "unstaged" };

    let lines = parse_patch(&ws.diff_text);

    v_flex()
        .size_full()
        .min_h_0()
        // header: file path + source + close
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
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(div().min_w_0().truncate().text_color(muted).child(title.clone()))
                        .child(div().text_size(px(10.)).text_color(muted).child(source)),
                )
                .child(
                    Button::new("close-diff")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_selection(cx))),
                ),
        )
        // body
        .child(
            div()
                .id("diff-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .font_family(cx.theme().mono_font_family.clone())
                .children(if lines.is_empty() {
                    vec![
                        div()
                            .flex()
                            .justify_center()
                            .py_4()
                            .text_sm()
                            .text_color(muted)
                            .child("No changes"),
                    ]
                } else {
                    lines.into_iter().map(|line| patch_line_row(line, cx)).collect()
                }),
        )
}

fn patch_line_row(line: PatchLine, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (bg, fg) = match line.kind {
        LineKind::Added => (Some(theme.success.opacity(0.12)), theme.success),
        LineKind::Removed => (Some(theme.danger.opacity(0.12)), theme.danger),
        LineKind::Hunk => (None, theme.info),
        LineKind::Meta => (None, theme.muted_foreground),
        LineKind::Context => (None, theme.foreground),
    };

    let mut row = div()
        .w_full()
        .px_3()
        .text_size(px(11.))
        .whitespace_nowrap()
        .line_height(relative(1.4))
        .text_color(fg)
        .child(line.text);
    if let Some(bg) = bg {
        row = row.bg(bg);
    }
    row
}

/// Parse our unified patch text into colored lines.
fn parse_patch(patch: &str) -> Vec<PatchLine> {
    patch
        .lines()
        .map(|raw| {
            let kind = if raw.starts_with("---") || raw.starts_with("+++") {
                LineKind::Meta
            } else {
                match raw.chars().next() {
                    Some('+') => LineKind::Added,
                    Some('-') => LineKind::Removed,
                    Some('@') => LineKind::Hunk,
                    Some('d') if raw.starts_with("diff ") => LineKind::Meta,
                    Some('i') if raw.starts_with("index ") => LineKind::Meta,
                    _ => LineKind::Context,
                }
            };
            PatchLine { text: raw.to_string(), kind }
        })
        .collect()
}
