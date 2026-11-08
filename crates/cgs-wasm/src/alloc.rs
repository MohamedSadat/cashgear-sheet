//! The engine's allocator: Rust's own dlmalloc, with WASM's memory grown in large steps.
//!
//! dlmalloc asks for more memory as little as it needs, and in Chrome each `memory.grow` costs
//! about a quarter of a millisecond, which came to a sixth of the time reading a big book took
//! (bench/profile-read.mjs). Growing 64 MiB at a time asks about sixty times on the way to 4 GiB.
//! Memory is never given back, as before, and near the ceiling it grows by exactly what is asked.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::wasm32::memory_grow;
use core::cell::UnsafeCell;
use core::ptr;

use dlmalloc::{Allocator, Dlmalloc};

const PAGE: usize = 64 * 1024;
const STEP: usize = 64 << 20;

/// Pages from WASM's memory.
struct Pages;

/// Grows memory by `bytes`, rounded up to pages. Returns the new region, or null.
fn grow(bytes: usize) -> (*mut u8, usize, u32) {
    let pages = bytes.div_ceil(PAGE);
    let previous = memory_grow(0, pages);
    if previous == usize::MAX {
        return (ptr::null_mut(), 0, 0);
    }
    let base = previous * PAGE;
    let size = pages * PAGE;
    // The last bytes of the address space can't be handed out: a pointer past them would wrap.
    if base.wrapping_add(size) == 0 {
        return (base as *mut u8, size - 16, 0);
    }
    (base as *mut u8, size, 0)
}

// SAFETY: WASM here has one thread, and regions come only from `memory.grow`.
unsafe impl Allocator for Pages {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        if size < STEP {
            let region = grow(STEP);
            if !region.0.is_null() {
                return region;
            }
        }
        grow(size)
    }

    fn remap(&self, _: *mut u8, _: usize, _: usize, _: bool) -> *mut u8 {
        ptr::null_mut()
    }

    fn free_part(&self, _: *mut u8, _: usize, _: usize) -> bool {
        false
    }

    fn free(&self, _: *mut u8, _: usize) -> bool {
        false
    }

    fn can_release_part(&self, _: u32) -> bool {
        false
    }

    fn allocates_zeros(&self) -> bool {
        true
    }

    fn page_size(&self) -> usize {
        PAGE
    }
}

struct Global(UnsafeCell<Dlmalloc<Pages>>);

// SAFETY: WASM here has one thread, so nothing reaches the allocator from two at once.
unsafe impl Sync for Global {}

#[global_allocator]
static ALLOCATOR: Global = Global(UnsafeCell::new(Dlmalloc::new_with_allocator(Pages)));

// SAFETY: each call has the only access to the allocator, as there is one thread, and the
// contracts are dlmalloc's own, which std's WASM allocator relies on in the same way.
unsafe impl GlobalAlloc for Global {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { (*self.0.get()).malloc(layout.size(), layout.align()) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { (*self.0.get()).calloc(layout.size(), layout.align()) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { (*self.0.get()).free(pointer, layout.size(), layout.align()) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        unsafe { (*self.0.get()).realloc(pointer, layout.size(), layout.align(), size) }
    }
}
