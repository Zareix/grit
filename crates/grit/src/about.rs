//! The About window: app identity, version, and credits in a small
//! fixed-size window, opened from the "About Grit" menu item.

use gpui_kit::component::theme::Theme;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;

/// Single source of truth for the app version (menu + `--version`).
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

const REPO_URL: &str = "https://github.com/zareix/grit";

/// Open the compact About window: centered on screen, not resizable.
pub(crate) fn open_about_window(cx: &mut App) {
    let bounds = WindowBounds::centered(size(px(420.), px(310.)), cx);
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some("About Grit".into()), ..Default::default() }),
        window_bounds: Some(bounds),
        is_resizable: false,
        ..Default::default()
    };
    open_window(options, cx, |window, cx| {
        // Follow the system light/dark appearance, like the main window.
        window
            .observe_window_appearance(|window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            })
            .detach();
        cx.new(|_| AboutWindow)
    })
    .expect("failed to open About window");
}

/// Static view: no interaction beyond the repository link.
struct AboutWindow;

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let link = cx.theme().primary;

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .justify_center()
            .items_center()
            .gap_2()
            .child(div().text_size(px(28.)).font_weight(FontWeight::BOLD).child("Grit"))
            .child(div().text_size(px(13.)).text_color(muted).child(format!("Version {VERSION}")))
            .child(
                div()
                    .id("repo-link")
                    .text_size(px(13.))
                    .text_color(link)
                    .on_click(|_, _, cx| cx.open_url(REPO_URL))
                    .hover(|s| s.underline())
                    .child("github.com/zareix/grit"),
            )
            .child(div().h_3())
            .child(
                v_flex()
                    .w_full()
                    .px(px(48.))
                    .gap_1()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(div().child("A native Git client written in Rust."))
                    .child(div().child("Built with GPUI Kit, git2 (libgit2), and the Git CLI."))
                    .child(div().child("© 2026 Raphaël C. — MIT licensed.")),
            )
    }
}
