//! rustyTree: cross-platform GUI disk-usage analyzer.
//!
//! This library exposes the headless scan engine (`scan` module) so it can be
//! exercised by integration tests and, eventually, by alternative front-ends.
//! The `eframe`/`egui` UI lives in the binary (`src/main.rs`).

pub mod scan;
