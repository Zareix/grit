//! Grit — a native Git client built in Rust with GPUI Kit.

mod about;
mod session;
mod workspace;

use gpui_kit::assets::{Assets as ComponentAssets, icon_assets};
use gpui_kit::component::TitleBar;
use gpui_kit::component::theme::Theme;
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
        Archive,
        Cloud,
        CloudDownload,
        Code,
        EllipsisVertical,
        FastForward,
        FileCode,
        GitCommitHorizontal,
        GitMerge,
        RotateCcw,
        Trash,
        GitBranch,
        Upload,
        Download,
        Tag,
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

actions!(grit, [Quit, NewTab, CloseTab, About]);

/// Route a tab-level action to the root [`Session`] of the active window.
/// Same global-handler pattern as gpui-kit's inspector toggle: the binding
/// has no key context, so the shortcut works wherever focus happens to be.
///
/// gpui-kit's `open_window` wraps the app view in a `base::Root`, so the
/// Session is reached through the Root's content view, not `window.root`.
fn route_tab_action(
    cx: &mut gpui_kit::App,
    operate: impl Fn(&mut Session, &mut Context<Session>) + 'static,
) {
    let Some(window) = cx.active_window() else { return };
    cx.defer(move |cx| {
        _ = window.update(cx, |_, window, cx| {
            let Some(Some(root)) = window.root::<gpui_kit::base::Root>() else { return };
            let Ok(session) = root.read(cx).view().clone().downcast::<Session>() else {
                return;
            };
            session.update(cx, |session, cx| operate(session, cx));
        });
    });
}

fn main() {
    // Optional --version: standard CLI flag, mirrored by the CI release
    // check that the repackaged binary matches the packaged version.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("grit {}", about::VERSION);
        return;
    }

    // Optional repo path from the CLI; without one the session shows the
    // tab picker page (or restores the persisted tabs).
    let repo_path = std::env::args().nth(1);

    application().with_assets(AppAssets).run(move |cx| {
        // Required before using any gpui-kit component.
        init(cx);

        // Follow the system appearance from startup (sync_system_appearance
        // with no window reads `cx.window_appearance()`); each window then
        // observes appearance changes below.
        Theme::sync_system_appearance(None, cx);

        // App menu. gpui builds no menu bar by default and on macOS ⌘Q is
        // carried by the menu bar's Quit item, so without this it does
        // nothing. The key equivalent comes from the `cmd-q` binding.
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-t", NewTab, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // Tab commands: bound without context (global), routed to the
        // active window's root Session.
        cx.on_action(|_: &NewTab, cx| route_tab_action(cx, |s, cx| s.open_new_tab(cx)));
        cx.on_action(|_: &CloseTab, cx| route_tab_action(cx, |s, cx| s.close_active_tab(cx)));
        cx.on_action(|_: &About, cx| about::open_about_window(cx));
        // "About" is the first item of the app menu, macOS convention.
        cx.set_menus([Menu::new("Grit").items([
            MenuItem::action("About Grit", About),
            MenuItem::separator(),
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
            // While this window is open, re-sync the theme whenever the
            // system light/dark appearance changes.
            window
                .observe_window_appearance(|window, cx| {
                    Theme::sync_system_appearance(Some(window), cx);
                })
                .detach();
            cx.new(|cx| Session::new(repo_path.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
