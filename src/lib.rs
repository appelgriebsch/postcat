//! postcat — a keyboard-first API client for the terminal.
//!
//! The binary is a thin shell around this library: [`app::App`] holds all state,
//! [`keys`] turns input events into state transitions, and [`ui::draw`] renders a
//! frame. That split is what makes the end-to-end tests in `tests/` possible —
//! they drive the same three pieces against a `TestBackend` and a real socket.

pub mod app;
pub mod clip;
pub mod http;
pub mod keys;
pub mod model;
pub mod theme;
pub mod ui;
