//! egui rendering helpers split by visual region.
//!
//! Each submodule exposes a `render(app, ui)` function that draws a single
//! pane. They share state via [`crate::app::RustyTreeApp`].

pub mod status;
pub mod toolbar;
pub mod tree_view;
