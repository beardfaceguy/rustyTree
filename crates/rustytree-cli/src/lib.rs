//! Library half of the rustyTree CLI.
//!
//! Splitting `app` and `ui` out of the binary into a library lets the
//! integration tests under `tests/` drive `RustyTreeApp` end-to-end —
//! create a tempdir, dispatch commands, render into a `TestBackend`,
//! inspect the resulting buffer — without paying the cost of spawning
//! a real terminal subprocess.
//!
//! `main.rs` is now a thin entrypoint that owns the alternate-screen /
//! raw-mode dance and calls `rustytree_cli::run`.

pub mod app;
pub mod ui;
