//! The only JS-facing API. It runs in the engine worker, on web and on desktop.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Engine(cgs_engine::Engine);

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine(cgs_engine::Engine::new())
    }

    /// The encoded `ViewBlock` tile whose top-left cell is (`first_row`, `first_col`).
    pub fn view_block(&self, first_row: u32, first_col: u32) -> Vec<u8> {
        self.0.view_block(first_row, first_col).into_bytes()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// The crate version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
