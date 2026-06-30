//! Daily Construction Progress Report: Rust/WASM static form.
//!
//! `pdf`, `model`, and `metrics` are pure Rust (host-testable). The `app`
//! module holds the browser glue and only compiles for the wasm target.

pub mod metrics;
pub mod model;
pub mod pdf;

#[cfg(target_arch = "wasm32")]
mod app;
