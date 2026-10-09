//! git-desktop — a Git client built in Rust with GPUI Kit.

mod session;
mod workspace;

use gpui_kit::assets::{Assets as ComponentAssets, icon_assets};
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::borrow::Cow;

use crate::session::Session;

// Extra catalog icons used by the app but not embedded by the default
// `Assets` source (which keeps only the 101 legacy component icons).
// Icons already in the default set resolve through the `ComponentAssets`
// fallback below.
icon_assets!(
    ExtraIcons,
    [ArrowDown, ArrowUp, CloudDownload, Code, EllipsisVertical, FileCode, GitCommitHorizontal]
);

/// Composite asset source: app icons first, then the default component set.
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        ComponentAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ComponentAssets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn main() {
    // Optional repo path from the CLI; without one the session shows the
    // tab picker page (or restores the persisted tabs).
    let repo_path = std::env::args().nth(1);

    application().with_assets(AppAssets).run(move |cx| {
        // Required before using any gpui-kit component.
        init(cx);

        // TitleBar::window_options() gives the app a custom title bar drawn
        // by the TitleBar element itself (tabs align with the native window
        // buttons; on macOS the traffic lights sit at (9, 9)).
        open_window(TitleBar::window_options(), cx, move |window, cx| {
            cx.new(|cx| Session::new(repo_path.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
