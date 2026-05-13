//! Headless filesystem scan engine.
//!
//! Module map:
//! - [`platform`]: cfg-gated helpers that extract platform-specific metadata
//!   (allocated size, owner, mtime) behind a uniform API.
//! - [`tree`]: in-memory size tree built from scan results.
//! - [`events`]: scan event channel + cancellation handle.
//! - [`walker`]: jwalk-based parallel walker that produces a [`tree::Tree`].
//!
//! The UI layer never touches the filesystem directly; it owns a
//! [`events::ScanHandle`] and polls it each frame.

pub mod events;
pub mod platform;
pub mod tree;
pub mod walker;

pub use events::{ScanError, ScanEvent, ScanHandle, ScanProgress, start_scan};
pub use tree::{Node, NodeId, NodeKind, Tree};
