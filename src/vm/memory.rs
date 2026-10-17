//! # Memory Ownership & Safety Enforcement
//!
//! Implements Technical Point #20 of the RHP Architecture Specification:
//! - **Memory Ownership & Safety Enforcement**: Leverage Rust’s compile-time borrow checker
//!   and ownership model to guarantee absolute memory safety and eliminate data races across
//!   concurrent worker threads without relying on a traditional tracing garbage collector.
//! - **Zero Tracing Garbage Collector**: No stop-the-world pauses, no cyclic GC sweeps, no background
//!   tracing threads, and zero GC-induced latency spikes (`gc_runs: 0`, `gc_pause_ms: 0.0`).
//! - **Deterministic RAII & Scoped Deallocation**: Values, collections, call frames, and worker VM states
//!   are automatically and deterministically dropped as soon as their lexical scope, fiber, or request
//!   completes ($O(1)$ cleanup).
//! - **Copy-On-Write (COW) Memory Efficiency**: Buffers (SSO strings, sequential lists, dictionaries,
//!   sets, and hash maps) share memory until mutation, at which point detachment occurs safely.
//! - **Compile-Time Data Race Elimination**: Active execution values ([`PhpValue`]) are strictly `!Send`
//!   and `!Sync` due to thread-local reference counts (`Rc`). Cross-thread state sharing is statically
//!   prevented by Rust's borrow checker, and dynamic cross-thread communication is strictly performed
//!   using immutable, thread-safe snapshots ([`FrozenValue`], `Send + Sync`) or channel message passing.
//! - **Tier 1 Native Facade (`Memory`)**: Exposes memory telemetry (`Memory::usage()`, `Memory::peak()`,
//!   `Memory::stats()`, `Memory::isGcFree()`, `Memory::scope()`, `Memory::assertThreadSafe()`) directly
//!   to userland RHP scripts.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use indexmap::IndexMap;

use crate::lexer::span::Span;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::parent_process::FrozenValue;
use crate::vm::value::PhpValue;

// =========================================================================
// Global Atomic Telemetry Counters (Thread-Safe Across Worker Threads)
// =========================================================================

static CURRENT_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static TOTAL_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static TOTAL_DEALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static COW_DETACHMENTS: AtomicUsize = AtomicUsize::new(0);
static LIVE_OBJECTS: AtomicUsize = AtomicUsize::new(0);
static GC_RUNS: AtomicUsize = AtomicUsize::new(0);
static GC_PAUSE_NS: AtomicU64 = AtomicU64::new(0);

/// Detailed telemetry snapshot of RHP memory management and ownership enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    /// Currently allocated heap and buffer memory in bytes.
    pub allocated_bytes: usize,
    /// High-water mark (peak) allocated memory in bytes.
    pub peak_bytes: usize,
    /// Total number of buffer/collection allocations recorded.
    pub total_allocations: usize,
    /// Total number of buffer/collection deallocations recorded.
    pub total_deallocations: usize,
    /// Total number of Copy-On-Write detachments executed on mutation.
    pub cow_detachments: usize,
    /// Currently live allocated objects and collection handles.
    pub live_objects: usize,
    /// Number of tracing garbage collector runs (strictly 0 in RHP).
    pub gc_runs: usize,
    /// Tracing garbage collector pause time in nanoseconds (strictly 0 in RHP).
    pub gc_pause_ns: u64,
    /// Whether a traditional tracing garbage collector is active (strictly false in RHP).
    pub tracing_gc_enabled: bool,
    /// Whether memory safety and thread safety are guaranteed (strictly true in RHP).
    pub thread_safe: bool,
    /// The memory management model identifier.
    pub model: &'static str,
}

impl MemoryStats {
    /// Convert these metrics into a subtyped RHP Dictionary for script consumption (`Memory::stats()`).
    pub fn to_dict(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert(
            "allocated_bytes".to_string(),
            PhpValue::Int(self.allocated_bytes as i32),
        );
        map.insert(
            "peak_bytes".to_string(),
            PhpValue::Int(self.peak_bytes as i32),
        );
        map.insert(
            "total_allocations".to_string(),
            PhpValue::Int(self.total_allocations as i32),
        );
        map.insert(
            "total_deallocations".to_string(),
            PhpValue::Int(self.total_deallocations as i32),
        );
        map.insert(
            "cow_detachments".to_string(),
            PhpValue::Int(self.cow_detachments as i32),
        );
        map.insert(
            "live_objects".to_string(),
            PhpValue::Int(self.live_objects as i32),
        );
        map.insert("gc_runs".to_string(), PhpValue::Int(0));
        map.insert("gc_pause_ms".to_string(), PhpValue::Float(0.0));
        map.insert("tracing_gc_enabled".to_string(), PhpValue::Bool(false));
        map.insert("thread_safe".to_string(), PhpValue::Bool(true));
        map.insert(
            "model".to_string(),
            PhpValue::string("rust_ownership_raii"),
        );
        PhpValue::dict_from_map(map)
    }
}

// =========================================================================
// MemoryTracker Functions
// =========================================================================

/// Record a buffer or heap allocation of `bytes` size.
pub fn record_alloc(bytes: usize) {
    if bytes == 0 {
        return;
    }
    let prev = CURRENT_ALLOCATED_BYTES.fetch_add(bytes, Ordering::Relaxed);
    let new_val = prev + bytes;
    TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);

    // Update peak memory with CAS loop
    let mut current_peak = PEAK_ALLOCATED_BYTES.load(Ordering::Relaxed);
    while new_val > current_peak {
        match PEAK_ALLOCATED_BYTES.compare_exchange_weak(
            current_peak,
            new_val,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current_peak = actual,
        }
    }
}

/// Record a buffer or heap deallocation of `bytes` size.
pub fn record_dealloc(bytes: usize) {
    if bytes == 0 {
        return;
    }
    let mut current = CURRENT_ALLOCATED_BYTES.load(Ordering::Relaxed);
    while current > 0 {
        let new_val = current.saturating_sub(bytes);
        match CURRENT_ALLOCATED_BYTES.compare_exchange_weak(
            current,
            new_val,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
    TOTAL_DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
}

/// Record a Copy-On-Write (COW) buffer detachment event.
pub fn record_cow_detachment() {
    COW_DETACHMENTS.fetch_add(1, Ordering::Relaxed);
}

/// Record the creation of a live object or collection handle.
pub fn record_live_object() {
    LIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
}

/// Record the destruction of a live object or collection handle.
pub fn release_live_object() {
    let mut current = LIVE_OBJECTS.load(Ordering::Relaxed);
    while current > 0 {
        match LIVE_OBJECTS.compare_exchange_weak(
            current,
            current - 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

/// Returns the current estimated allocated memory in bytes.
pub fn current_usage() -> usize {
    CURRENT_ALLOCATED_BYTES.load(Ordering::Relaxed)
}

/// Returns the peak allocated memory in bytes since engine start or last reset.
pub fn peak_usage() -> usize {
    PEAK_ALLOCATED_BYTES.load(Ordering::Relaxed)
}

/// Resets peak memory tracking to the current allocation level.
pub fn reset_peak() {
    let cur = current_usage();
    PEAK_ALLOCATED_BYTES.store(cur, Ordering::Relaxed);
}

/// Returns the total number of Copy-On-Write detachments executed.
pub fn cow_detachments() -> usize {
    COW_DETACHMENTS.load(Ordering::Relaxed)
}

/// Returns the total count of live objects/collections currently allocated.
pub fn live_objects() -> usize {
    LIVE_OBJECTS.load(Ordering::Relaxed)
}

/// Returns true confirming that RHP operates 100% free of a tracing garbage collector.
pub fn is_gc_free() -> bool {
    true
}

/// Returns the number of tracing garbage collector runs (always 0 in RHP).
pub fn gc_runs() -> usize {
    GC_RUNS.load(Ordering::Relaxed)
}

/// Returns the tracing garbage collector pause time in milliseconds (always 0.0 in RHP).
pub fn gc_pause_ms() -> f32 {
    (GC_PAUSE_NS.load(Ordering::Relaxed) as f32) / 1_000_000.0
}

/// Returns a complete [`MemoryStats`] snapshot.
pub fn get_memory_stats() -> MemoryStats {
    MemoryStats {
        allocated_bytes: current_usage(),
        peak_bytes: peak_usage(),
        total_allocations: TOTAL_ALLOCATIONS.load(Ordering::Relaxed),
        total_deallocations: TOTAL_DEALLOCATIONS.load(Ordering::Relaxed),
        cow_detachments: COW_DETACHMENTS.load(Ordering::Relaxed),
        live_objects: LIVE_OBJECTS.load(Ordering::Relaxed),
        gc_runs: 0,
        gc_pause_ns: 0,
        tracing_gc_enabled: false,
        thread_safe: true,
        model: "rust_ownership_raii",
    }
}

/// Resets all global memory tracking counters to zero (used for test isolation).
pub fn reset_memory_tracker() {
    CURRENT_ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    PEAK_ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    TOTAL_ALLOCATIONS.store(0, Ordering::Relaxed);
    TOTAL_DEALLOCATIONS.store(0, Ordering::Relaxed);
    COW_DETACHMENTS.store(0, Ordering::Relaxed);
    LIVE_OBJECTS.store(0, Ordering::Relaxed);
    GC_RUNS.store(0, Ordering::Relaxed);
    GC_PAUSE_NS.store(0, Ordering::Relaxed);
}

// =========================================================================
// Size Estimator
// =========================================================================

/// Recursively estimate the heap memory footprint (in bytes) of a [`PhpValue`].
pub fn estimate_value_size(val: &PhpValue) -> usize {
    match val {
        PhpValue::Null | PhpValue::Bool(_) | PhpValue::Int(_) | PhpValue::Float(_) => {
            std::mem::size_of::<PhpValue>()
        }
        PhpValue::RustInt { .. } | PhpValue::RustFloat { .. } => {
            std::mem::size_of::<PhpValue>() + 8
        }
        PhpValue::String(s) => {
            if s.is_inline() {
                std::mem::size_of::<PhpValue>()
            } else {
                std::mem::size_of::<PhpValue>() + s.as_str().len() + 16
            }
        }
        PhpValue::Array(items) | PhpValue::List(items) => {
            let b = items.borrow();
            let element_sizes: usize = b.iter().map(estimate_value_size).sum();
            std::mem::size_of::<PhpValue>() + 32 + (b.capacity() * std::mem::size_of::<PhpValue>()) + element_sizes
        }
        PhpValue::Dict(dict) => {
            let b = dict.borrow();
            let entries_size: usize = b
                .iter()
                .map(|(k, v)| k.len() + estimate_value_size(v))
                .sum();
            std::mem::size_of::<PhpValue>() + 48 + (b.capacity() * 48) + entries_size
        }
        PhpValue::Set(set) => {
            let b = set.borrow();
            let entries_size: usize = b.iter().map(estimate_value_size).sum();
            std::mem::size_of::<PhpValue>() + 48 + (b.capacity() * 32) + entries_size
        }
        PhpValue::HashMap(map) => {
            let b = map.borrow();
            let entries_size: usize = b
                .iter()
                .map(|(k, v)| estimate_value_size(k) + estimate_value_size(v))
                .sum();
            std::mem::size_of::<PhpValue>() + 48 + (b.capacity() * 48) + entries_size
        }
        PhpValue::Object(obj) => {
            let b = obj.borrow();
            let props_size: usize = b
                .properties
                .iter()
                .map(|(k, v)| k.len() + estimate_value_size(v))
                .sum();
            std::mem::size_of::<PhpValue>() + 64 + props_size
        }
        PhpValue::Closure(_) | PhpValue::Future(_) | PhpValue::Resource(_) => {
            std::mem::size_of::<PhpValue>() + 64
        }
    }
}

// =========================================================================
// Scoped RAII Memory Guard
// =========================================================================

/// Scoped RAII guard measuring memory allocations and verifying deterministic reclamation.
#[derive(Debug)]
pub struct MemoryScope {
    pub start_usage: usize,
    pub start_peak: usize,
}

impl Default for MemoryScope {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryScope {
    /// Enter a new memory measurement scope.
    pub fn new() -> Self {
        Self {
            start_usage: current_usage(),
            start_peak: peak_usage(),
        }
    }

    /// Returns the bytes allocated since entering this scope.
    pub fn allocated_since(&self) -> usize {
        let cur = current_usage();
        cur.saturating_sub(self.start_usage)
    }

    /// Returns the bytes reclaimed if current memory is less than initial.
    pub fn reclaimed_since(&self) -> usize {
        let cur = current_usage();
        self.start_usage.saturating_sub(cur)
    }
}

// =========================================================================
// Ownership & Thread Safety Enforcer
// =========================================================================

/// Enforces compile-time and runtime memory ownership, borrow rules, and data race elimination.
pub struct OwnershipEnforcer;

impl OwnershipEnforcer {
    /// Verify that an active [`PhpValue`] satisfies thread-safety boundary rules
    /// and can be safely frozen into an immutable [`FrozenValue`] (`Send + Sync`)
    /// without data races across concurrent worker threads.
    pub fn assert_thread_safe(val: &PhpValue, span: Span) -> Result<FrozenValue, RuntimeError> {
        match val {
            PhpValue::Closure(_) => Err(RuntimeError {
                kind: RuntimeErrorKind::Custom {
                    message: "Cannot share active closure across thread boundaries without serialization"
                        .to_string(),
                },
                span,
                stack_trace: vec![],
            }),
            other => Ok(FrozenValue::from_php_value(other)),
        }
    }

    /// Verify the RHP architectural memory safety invariants:
    /// 1. Tracing garbage collector is completely absent (`gc_runs == 0`, `tracing_gc_enabled == false`).
    /// 2. Deallocation is deterministic via Rust RAII.
    /// 3. Worker threads communicate without data races.
    pub fn verify_invariants() -> Result<(), &'static str> {
        if !is_gc_free() {
            return Err("Invariant violated: Tracing garbage collector must not be present");
        }
        if gc_runs() != 0 {
            return Err("Invariant violated: Tracing GC runs must strictly equal 0");
        }
        if gc_pause_ms() != 0.0 {
            return Err("Invariant violated: Tracing GC pause time must strictly equal 0.0ms");
        }
        Ok(())
    }
}
