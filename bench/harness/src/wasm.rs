//! The Chrome entry point, called from `bench/chrome/worker.js`.

use wasm_bindgen::prelude::wasm_bindgen;

use crate::run::{engine_by_name, run_file, RunOptions};

/// Runs one workbook in this worker's fresh module, and returns the report as JSON.
#[wasm_bindgen]
pub fn run_workbook(
    engine: &str,
    bytes: &[u8],
    edits: usize,
    top_row_insert: bool,
    examples: bool,
    skip_calc: bool,
) -> String {
    let Some(kind) = engine_by_name(engine) else {
        return format!("{{\"ok\":false,\"error\":\"unknown engine {engine}\"}}");
    };
    let report = run_file(
        kind,
        bytes,
        RunOptions {
            edits,
            top_row_insert,
            examples,
            skip_calc,
        },
    );
    serde_json::to_string(&report)
        .unwrap_or_else(|error| format!("{{\"ok\":false,\"error\":\"report: {error}\"}}"))
}
