//! Shared scaffolding for the end-to-end tests: a terminal harness that drives
//! the real app, and a real HTTP server to talk to.
#![allow(dead_code)]

pub mod harness;
pub mod server;

pub use harness::Harness;
pub use server::TestServer;
