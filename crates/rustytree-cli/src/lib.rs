//! Library half of the rustyTree CLI.
//!
//! Splitting `app` and `ui` out of the binary into a library lets the
//! integration tests under `tests/` drive `RustyTreeApp` end-to-end —
//! create a tempdir, dispatch commands, render into a `TestBackend`,
//! inspect the resulting buffer — without paying the cost of spawning
//! a real terminal subprocess.
//!
//! `main.rs` is the binary entrypoint: it owns the alternate-screen /
//! raw-mode dance and the event loop, and consumes [`app::RustyTreeApp`]
//! and [`ui`] from this library. The terminal lifecycle and event
//! loop deliberately stay in the binary because they're inseparable
//! from real `crossterm` I/O — there's nothing meaningful to test
//! against `TestBackend` for them.

pub mod app;
pub mod ui;
