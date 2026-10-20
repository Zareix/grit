//! Headless bench of `DiffFile::parse` — the diff-viewer install path that
//! runs on the UI thread in `Workspace::set_diff`.
//!
//! Synthesizes a realistic multi-file patch (like the gpui-kit doc's own
//! benchmark: ~6.5 MB / 200 files) and times parsing. Run:
//!
//! ```sh
//! cargo run --release -p grit --example bench_diff_parse
//! ```

use gpui_kit::component::diff::DiffFile;
use std::time::Instant;

/// A synthetic unified-diff patch: `files` files, each `lines_per_file`
/// changed lines of realistic code length.
fn synthetic_patch(files: usize, lines_per_file: usize) -> String {
    let mut out = String::with_capacity(files * lines_per_file * 60);
    for f in 0..files {
        out.push_str(&format!("diff --git a/src/file_{f}.rs b/src/file_{f}.rs\n"));
        out.push_str("index 0000000..1111111 100644\n");
        out.push_str(&format!("--- a/src/file_{f}.rs\n"));
        out.push_str(&format!("+++ b/src/file_{f}.rs\n"));
        out.push_str(&format!("@@ -10,{lines_per_file} +10,{lines_per_file} @@\n"));
        for l in 0..lines_per_file {
            let l = l + 10;
            out.push_str(&format!(
                "-    let value_{l} = compute_thing({l}, \"param_{l}\", Config {{ depth: {l} }});\n"
            ));
            out.push_str(&format!(
                "+    let value_{l} = compute_thing({l}, \"param_{l}\", Config {{ depth: {l}, mode: Mode::Fast }});\n"
            ));
        }
    }
    out
}

fn main() {
    for (files, lines) in [(1, 500), (10, 500), (50, 500), (200, 500)] {
        let patch = synthetic_patch(files, lines);
        let mb = patch.len() as f64 / (1024. * 1024.);
        // Warmup + 3 timed runs each; report the best.
        let _ = DiffFile::parse(&patch);
        let mut best = f64::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            let parsed = DiffFile::parse(&patch).expect("parse");
            best = best.min(start.elapsed().as_secs_f64() * 1000.);
            assert!(!parsed.is_empty());
        }
        println!(
            "parse patch: {files:>4} files, {lines} lines each ({mb:5.1} MB) -> {best:8.2} ms"
        );
    }
}
