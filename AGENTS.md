# AGENTS.md — git-desktop

Native Git client (GitKraken alternative) in Rust: GPUI Kit UI over a pure-Rust
git plumbing core. Early stage; expect moving ground.

## Commands

```sh
just ci          # full check: fmt-check + clippy (-D warnings) + test
just run         # run the app (default repo: cwd; pass a path: just run -- /path)
just test        # all tests; just test-core for plumbing only
just fmt         # format (must run BEFORE just ci — see below)
just check       # fast cargo check
```

- Run `just fmt` before `just ci`: `fmt-check` fails on unformatted code.
- clippy denies warnings; `unused dependency` warnings fail lint — wire deps
  up when you add them.
- Toolchain is nightly via `rust-toolchain.toml` (GPUI uses unstable std
  features like `cold_path`); rustup installs it automatically. All lint/fmt
  recipes call `+nightly` explicitly — keep that.

## Layout

- `crates/gk-core` — git plumbing, no UI, headless tests (`tempfile` repos in
  `tests/support`). Modules: `repo` (open/discover), `status` (staging),
  `commit`, `branch`, `diff` (unified patches).
- `crates/gk-desktop` — app. `src/workspace/mod.rs` owns all state + actions;
  `src/workspace/components/*` are render functions
  `render(&Workspace, &mut Context<Workspace>) -> Div`.
- Git access: local ops via `git2` (libgit2); remote ops (fetch/pull/push)
  shell out to the `git` binary (`gk-core::remote`) so SSH/keychain credentials
  come from the user's git config — they run on the background executor.

## git2 0.21 gotchas (verified in source, not docs)

- `reset_default` with pathspec `["."]` matches **nothing** — full unstage is
  `repo.reset(head, ResetType::Mixed)`.
- `entry.path()`, `c.summary()`, `c.message()`, `head.shorthand()` return
  `Result` — use `.ok()`, not `Option` combinators.
- `Oid::ZERO_SHA1` (not deprecated `Oid::zero()`); `Index::remove_all(pathspecs,
  None)` takes a callback `Option`, not `PathspecFlags`.
- `Repository::commit` parents are `&[&Commit]`, not `&[&Object]`.
- Untracked content in workdir diffs needs `opts.show_untracked_content(true)`.

## GPUI Kit 0.7.1 gotchas (component sources live in `~/.cargo/registry/.../gpui-component-0.7.1`)

- Lists that must scroll smoothly use gpui's `uniform_list(id, item_count,
  closure)` (exported via `gpui_kit::*`) — a plain scrollable `div` full of
  rows re-measures everything every scroll frame and lags. The closure gets
  `(&Range<usize>, &mut Window, &mut App)`: read live state through a captured
  `WeakEntity`, use plain `on_click` closures (no `cx.listener`). Items stack
  contiguously with uniform height (no gap support). Item width is
  content-based (`ListHorizontalSizingBehavior::FitList` by default) — add
  `.w_full()` on each row so it fills the list and `flex_1` works.
- **Never invent APIs** — check the installed crate source; the
  `.agents/skills/gpui-kit` catalog describes a newer ecosystem than 0.7.1
  (e.g. the `Diff` component does not exist in 0.7.1; `h_resizable(id)` takes
  no state argument — attach it with `.with_state(&Entity<ResizableState>)`).
- `ResizablePanelGroup` always renders `size_full()` (both axes, not with
  Styled) — it must live inside a constrained wrapper
  (`div().flex_1().min_h_0().child(group)`), otherwise it overflows the
  toolbar/status bar and pushes the whole content down. Give panels a
  `.size(px(...))` + `.size_range(min..)` or a content-tall panel (a big
  diff) grows beyond the window.
- Dialogs (0.7.1): `window.open_dialog(cx, move |dialog, window, dialog_cx| …)`
  — real signature is `(cx, build)`, the builder closure is `move` and you
  **must name its params**: the site docs (all versions) show
  `footer(|…| vec![Button])` as a closure factory, but 0.7.1 source takes
  `footer(impl IntoElement)` and never renders `button_props` buttons (that's
  AlertDialog-only). Build the footer yourself:
  `DialogFooter::new().child(Button…)` + `window.close_dialog(cx)`.
  Shadowing `cx` in the closure body while naming the param `_` borrows the
  outer mutable `Context` in a `+ 'static` closure → E0521.
- Return **concrete types** (`Div`, `Stateful<Div>`) from render helpers that
  use `cx.listener` inside: `-> impl IntoElement` captures `cx`'s lifetime
  (edition 2024 RPIT) and breaks the borrow checker during composition.
- `.map()` iterators that capture `cx` are lazy — collect eagerly into
  `Vec<_>` during render.
- `on_click` / `overflow_y_scroll` need a stateful element: call `.id(...)`
  first. gpui-kit `Button` calls `stop_propagation`, safe inside clickable rows.
- The `.v_flex()` / `.h_flex()` convenience methods exist on `Div` only —
  on a `Stateful<Div>` (after `.id()`) use `.flex().flex_col()` instead.
- No `whitespace_pre`; only `whitespace_normal` / `whitespace_nowrap`.
- Full Lucide catalog is `gpui_kit::assets::IconName`
  (`component::IconName` is a subset). Generated list is in
  `target/debug/build/gpui-kit-assets-*/out/icon_name.rs`.
- `List` renders its own section headers via `ListDelegate::render_section_header`
  and hides empty sections — do not duplicate titles in the surrounding panel.
- `InputState::value()` reads text; `set_value(value, window, cx)` clears it.
- `cx.theme()` returns the full `Theme`: colors (`success`, `danger`, `info`,
  `primary`, `muted_foreground`, …) **and** `mono_font_family`. There is no
  plain `warning` token in 0.7.1.

## Conventions

- **No raw hex/rgb colors in app UI** — semantic tokens from `cx.theme()`
  only. New roles go into the app's token layer, not call sites.
- UI text in English (app language), code comments in English.
- All changes must pass `just ci` before commit.
