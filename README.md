# git-desktop

A Git client built in Rust with [GPUI Kit](https://gpui-kit.com/) — the goal: a
native, GPU-accelerated alternative to GitKraken (no webview, no Electron).

## Workspace

```
crates/
  gk-core/      Git plumbing: status, staging, commits, branches (pure Rust + git2)
  gk-desktop/   Desktop app: GPUI + gpui-kit UI
```

## Status

WIP — early days. Current focus: core plumbing (staging, commit, branches),
then the commit graph.

## Build & run

```sh
cargo run -p gk-desktop
```

## Tests

```sh
cargo test
```
