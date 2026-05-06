//! sen-tui — Terminal UI frontend built on ratatui/crossterm.
//!
//! Enable the `tui` feature (on by default) to compile the ratatui-based
//! terminal interface. Without it this crate is a no-op stub that still
//! compiles cleanly in headless / minimal environments.

#[cfg(feature = "tui")]
pub use senweavercoding::tui;
