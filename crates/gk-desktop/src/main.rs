//! git-desktop — a Git client built in Rust with GPUI Kit.

mod workspace;

use gpui_kit::assets::{Assets as ComponentAssets, icon_assets};
use gpui_kit::*;
use std::borrow::Cow;

use crate::workspace::Workspace;

// Extra catalog icons used by the app but not embedded by the default
// `Assets` source (which keeps only the 101 legacy component icons).
// Icons already in the default set resolve through the `ComponentAssets`
// fallback below.
icon_assets!(ExtraIcons, [ArrowDown, ArrowUp, CloudDownload, EllipsisVertical, FileCode]);

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
    // Repo path comes from the CLI, defaulting to the current directory.
    let repo_path = std::env::args().nth(1).unwrap_or_else(|| ".".into());

    application().with_assets(AppAssets).run(move |cx| {
        // Required before using any gpui-kit component.
        init(cx);

        open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| Workspace::new(repo_path.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
