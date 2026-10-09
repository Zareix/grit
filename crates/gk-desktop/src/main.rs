//! git-desktop — a Git client built in Rust with GPUI Kit.

mod workspace;

use gpui_kit::*;

use crate::workspace::Workspace;

fn main() {
    // Repo path comes from the CLI, defaulting to the current directory.
    let repo_path = std::env::args().nth(1).unwrap_or_else(|| ".".into());

    application().with_assets(assets::Assets).run(move |cx| {
        // Required before using any gpui-kit component.
        init(cx);

        open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| Workspace::new(repo_path.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
