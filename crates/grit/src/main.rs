//! Grit — a native Git client built in Rust with GPUI Kit.

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
    [
        ArrowDown,
        ArrowUp,
        CloudDownload,
        Code,
        EllipsisVertical,
        FileCode,
        GitCommitHorizontal,
        RotateCcw,
        Trash
    ]
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

actions!(grit, [Quit]);

fn main() {
    // Optional repo path from the CLI; without one the session shows the
    // tab picker page (or restores the persisted tabs).
    let repo_path = std::env::args().nth(1);

    application().with_assets(AppAssets).run(move |cx| {
        // Required before using any gpui-kit component.
        init(cx);

        // App menu. gpui builds no menu bar by default and on macOS ⌘Q is
        // carried by the menu bar's Quit item, so without this it does
        // nothing. The key equivalent comes from the `cmd-q` binding.
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus([Menu::new("Grit").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit", Quit),
        ])]);

        // TitleBar::window_options() gives the app a custom title bar drawn
        // by the TitleBar element itself (tabs align with the native window
        // buttons; on macOS the traffic lights sit at (9, 9)).
        let mut options = TitleBar::window_options();
        // Open at ~80% of the screen, centered on it but nudged upward so
        // the macOS dock does not overlap the bottom of the window.
        const DOCK_NUDGE: f32 = 32.0;
        options.window_bounds = cx.primary_display().map(|display| {
            let screen = display.bounds();
            let size = size(screen.size.width * 0.8, screen.size.height * 0.8);
            let origin = point(
                screen.origin.x + (screen.size.width - size.width) / 2.,
                screen.origin.y + (screen.size.height - size.height) / 2. - px(DOCK_NUDGE),
            );
            WindowBounds::Windowed(Bounds { origin, size })
        });
        open_window(options, cx, move |window, cx| {
            cx.new(|cx| Session::new(repo_path.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
