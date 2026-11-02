//! The Phase 1 bake-off harness. See `bench/README.md`.

pub mod book;
pub mod formula;
pub mod run;
#[cfg(target_arch = "wasm32")]
mod wasm;
