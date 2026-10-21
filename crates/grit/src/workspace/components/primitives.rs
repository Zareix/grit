//! Small reusable pieces, shared across panels.
//!
//! Pure `RenderOnce` components: plain data in, one element out — no app
//! state, no retained entities.

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::*;
use grit_core::github::PrInfo;
use grit_core::status::ChangeKind;

/// Change-kind label + semantic color, shared by badge + text usages.
pub fn kind_colors(
    kind: ChangeKind,
    success: gpui_kit::Hsla,
    info: gpui_kit::Hsla,
    danger: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
) -> (&'static str, gpui_kit::Hsla) {
    match kind {
        ChangeKind::Added => ("A", success),
        ChangeKind::Modified => ("M", info),
        ChangeKind::Deleted => ("D", danger),
        ChangeKind::Renamed | ChangeKind::TypeChange => ("R/T", muted),
        ChangeKind::Conflicted => ("U", danger),
    }
}

/// The A/M/D/U change badge. The letter carries the meaning; the color is
/// only reinforcement (semantic tokens: success/info/danger, neutral for
/// renamed/type-change).
#[derive(IntoElement)]
pub struct KindBadge {
    label: &'static str,
    color: gpui_kit::Hsla,
}

impl KindBadge {
    pub fn new(
        kind: ChangeKind,
        success: gpui_kit::Hsla,
        info: gpui_kit::Hsla,
        danger: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> Self {
        let (label, color) = kind_colors(kind, success, info, danger, muted);
        Self { label, color }
    }
}

impl RenderOnce for KindBadge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .w_5()
            .flex_none()
            .text_center()
            .text_size(px(11.))
            .text_color(self.color)
            .child(self.label)
    }
}

/// Muted uppercase section label ("LOCAL", "REMOTES", …).
#[derive(IntoElement)]
pub struct SectionLabel {
    title: &'static str,
    color: gpui_kit::Hsla,
}

impl SectionLabel {
    pub fn new(title: &'static str, cx: &App) -> Self {
        Self { title, color: cx.theme().muted_foreground }
    }
}

impl RenderOnce for SectionLabel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .px_2()
            .py_0p5()
            .text_size(px(10.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.color)
            .child(self.title)
    }
}

/// The `#42` open-PR chip beside a branch row; draft PRs render muted.
#[derive(IntoElement)]
pub struct PrBadge {
    label: String,
    color: gpui_kit::Hsla,
}

impl PrBadge {
    pub fn new(pr: &PrInfo, cx: &App) -> Self {
        let color = if pr.draft { cx.theme().muted_foreground } else { cx.theme().info };
        Self { label: format!("#{}", pr.number), color }
    }
}

impl RenderOnce for PrBadge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .px_1()
            .rounded_sm()
            .text_size(px(10.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.color)
            .bg(self.color.opacity(0.15))
            .child(self.label)
    }
}

/// Panel header: title + count of items, used across data panels.
#[derive(IntoElement)]
pub struct PanelHeader {
    title: &'static str,
    count: usize,
}

impl PanelHeader {
    pub fn new(title: &'static str, count: usize) -> Self {
        Self { title, count }
    }
}

impl RenderOnce for PanelHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .gap_2()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(self.title)
            .child(self.count.to_string())
    }
}
