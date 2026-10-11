# Grit — justfile
# Usage: just <recipe>   (or `just` to list)

default:
    @just --list

# --- dev ---------------------------------------------------------------

# Build the workspace (debug)
build:
    cargo build

# Build the desktop app and run it
run:
    cargo run -p grit

# Fast syntax/type check, no codegen
check:
    cargo check --workspace --all-targets

# --- test --------------------------------------------------------------

# Run all tests (unit + integration)
test:
    cargo test --workspace

# Test only the core Git plumbing (fast, no UI)
test-core:
    cargo test -p grit-core

# --- lint & format -----------------------------------------------------

fmt:
    cargo fmt --all

# Verify formatting without writing (CI)
fmt-check:
    cargo fmt --all -- --check

# Run clippy on everything, deny warnings
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format + lint in one pass
ci: fmt-check lint test

# --- maintenance -------------------------------------------------------

# Update dependencies and refresh Cargo.lock
update:
    cargo update

# Remove build artifacts
clean:
    cargo clean
