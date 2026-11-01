//! The native entry point, for quick iteration. The bake-off's numbers come from
//! Chrome, through `bench/chrome/`. Run: `run <engine> <file> [edits] [top-row] [skip-calc]`.

use cgs_bench::run::{engine_by_name, run_file, RunOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let engine = engine_by_name(&args[1]).expect("unknown engine");
    let bytes = std::fs::read(&args[2]).expect("read workbook");
    let options = RunOptions {
        edits: args.get(3).and_then(|v| v.parse().ok()).unwrap_or(100),
        top_row_insert: args.get(4).is_none_or(|v| v == "1"),
        examples: true,
        skip_calc: args.get(5).is_some_and(|v| v == "1"),
    };
    // Deep formula chains recurse, and the default main-thread stack is small.
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || run_file(engine, &bytes, options))
        .expect("spawn");
    match worker.join() {
        Ok(report) => println!("{}", serde_json::to_string(&report).expect("json")),
        Err(_) => println!("{{\"ok\":false,\"error\":\"panic\"}}"),
    }
}
