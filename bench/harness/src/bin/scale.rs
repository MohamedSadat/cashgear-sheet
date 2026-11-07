//! Phase 9's native counterpart of `bench/measure-scale.mjs`: the same engine, built for this PC
//! instead of WASM, through the same steps (open, read every sheet, build the graph, recalculate
//! everything, one edit). A counting allocator gives the heap's peak, which is what a native
//! desktop host would need where WASM stops at 4 GB. The file's own bytes are read into memory
//! first and left out of the figures.
//!
//! Usage: scale <workbook.xlsx>   (prints one JSON line)

use std::alloc::{GlobalAlloc, Layout, System};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use cgs_engine::{Engine, LoadPhase};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, size) };
        if !moved.is_null() {
            if size >= layout.size() {
                let now = NOW.fetch_add(size - layout.size(), Ordering::Relaxed) + size
                    - layout.size();
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                NOW.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const MIB: usize = 1024 * 1024;

fn main() {
    let path = std::env::args().nth(1).expect("Usage: scale <workbook.xlsx>");
    let bytes = std::fs::read(&path).expect("read the workbook");
    let file = bytes.len();
    // The file's bytes are in memory from here on, and not the engine's.
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let peak = || (PEAK.load(Ordering::Relaxed) - base) / MIB;

    let started = Instant::now();
    let mut engine = Engine::new();
    engine.open(Rc::new(bytes)).expect("open");
    loop {
        let status = engine.load_step(50.0).expect("read");
        if status.phase != LoadPhase::Reading {
            break;
        }
    }
    let read_ms = started.elapsed().as_millis();
    let read_mib = peak();
    while engine.prepare_step().expect("prepare") {}
    let ready_ms = started.elapsed().as_millis();
    let ready_mib = peak();

    let at = Instant::now();
    engine.calculate_now(true).expect("recalculate");
    while engine.calc_step(1000, &[]).expect("calculate") {}
    let full_recalc_ms = at.elapsed().as_millis();
    let full_recalc_mib = peak();

    let at = Instant::now();
    engine
        .apply_json(r#"{"kind":"input","sheet":0,"row":1,"col":1,"text":"7"}"#)
        .expect("edit");
    while engine.calc_step(1000, &[]).expect("calculate") {}
    let edit_ms = at.elapsed().as_millis();

    println!(
        "{}",
        serde_json::json!({
            "file": path,
            "fileMB": (file as f64 / 1e6 * 10.0).round() / 10.0,
            "readMs": read_ms,
            "readMiB": read_mib,
            "readyMs": ready_ms,
            "readyMiB": ready_mib,
            "fullRecalcMs": full_recalc_ms,
            "fullRecalcMiB": full_recalc_mib,
            "firstEditMs": edit_ms,
            "peakMiB": peak(),
        })
    );
}
