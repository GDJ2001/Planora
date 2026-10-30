//! Process-local allocation accounting for optional performance reports.

use serde::{Deserialize, Serialize};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
};

static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static FREED_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_LIVE_BYTES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AllocationSnapshot {
    pub allocations: u64,
    pub allocated_bytes: u64,
    pub freed_bytes: u64,
    pub live_bytes: u64,
    pub peak_live_bytes: u64,
}

pub fn allocation_snapshot() -> AllocationSnapshot {
    let allocated = ALLOCATED_BYTES.load(Ordering::Relaxed);
    let freed = FREED_BYTES.load(Ordering::Relaxed);
    AllocationSnapshot {
        allocations: ALLOCATION_COUNT.load(Ordering::Relaxed),
        allocated_bytes: allocated,
        freed_bytes: freed,
        live_bytes: allocated.saturating_sub(freed),
        peak_live_bytes: PEAK_LIVE_BYTES.load(Ordering::Relaxed),
    }
}

fn record_alloc(size: usize) {
    ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
    let allocated = ALLOCATED_BYTES.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    let live = allocated.saturating_sub(FREED_BYTES.load(Ordering::Relaxed));
    PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
}

/// Instrumented system allocator. Its atomics are bounded and never allocate.
pub struct CountingAllocator;

// SAFETY: Every operation delegates to the system allocator with the exact
// layout supplied by the caller, and accounting has no effect on the pointer.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        FREED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size > layout.size() {
                record_alloc(new_size - layout.size());
            } else {
                FREED_BYTES.fetch_add((layout.size() - new_size) as u64, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}
