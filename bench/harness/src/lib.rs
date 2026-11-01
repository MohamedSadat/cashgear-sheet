//! The Phase 1 bake-off harness. See `bench/README.md`.

pub mod book;
pub mod formula;
#[cfg(any(feature = "formualizer", feature = "ironcalc"))]
pub mod run;
#[cfg(all(
    target_arch = "wasm32",
    any(feature = "formualizer", feature = "ironcalc")
))]
mod wasm;
