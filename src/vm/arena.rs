//! # Request-Scoped Arena Allocation (Bump Allocator)
//!
//! Provides high-performance, request-scoped memory allocation for worker threads
//! and fibers with instantaneous, zero-overhead $O(1)$ cleanup upon request completion.
//!
//! Key Architecture Guarantees:
//! - **Zero GC Pauses**: Memory allocated during a request is not traversed or traced.
//! - **$O(1)$ Instantaneous Drop**: Dropping/resetting the arena resets bump pointers to offset 0,
//!   instantly reclaiming all temporary strings, arrays, and objects in a single CPU instruction.
//! - **Buffer Reuse**: Committed virtual memory chunks remain allocated across requests,
//!   eliminating kernel syscalls and memory fragmentation for subsequent HTTP requests.
//! - **Resource DropList**: External operating system resources (file descriptors, sockets,
//!   uncommitted DB transactions) are tracked and explicitly cleaned up, while pure data memory
//!   is wiped in bulk.

use std::sync::atomic::{AtomicUsize, Ordering};

use indexmap::IndexMap;

use crate::vm::value::PhpValue;

/// Default capacity for each arena chunk (64 KB).
pub const DEFAULT_ARENA_CHUNK_SIZE: usize = 64 * 1024;

// Global atomic telemetry counters for arena metrics across all worker threads.
static ARENA_CURRENT_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static ARENA_PEAK_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static ARENA_TOTAL_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static ARENA_TOTAL_RESETS: AtomicUsize = AtomicUsize::new(0);

/// Telemetry metrics for the request arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestArenaStats {
    /// Currently allocated bytes in the active request arena.
    pub allocated_bytes: usize,
    /// Peak allocated bytes across request executions.
    pub peak_bytes: usize,
    /// Total pre-allocated chunk capacity in bytes.
    pub capacity_bytes: usize,
    /// Total number of instantaneous O(1) arena drops/resets executed.
    pub reset_count: usize,
    /// Number of pre-allocated memory chunks.
    pub chunk_count: usize,
}

impl RequestArenaStats {
    /// Convert metrics into a subtyped RHP Dictionary for script consumption (`Memory::arenaStats()`).
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
            "capacity_bytes".to_string(),
            PhpValue::Int(self.capacity_bytes as i32),
        );
        map.insert(
            "reset_count".to_string(),
            PhpValue::Int(self.reset_count as i32),
        );
        map.insert(
            "chunk_count".to_string(),
            PhpValue::Int(self.chunk_count as i32),
        );
        map.insert("zero_gc_drop".to_string(), PhpValue::Bool(true));
        PhpValue::dict_from_map(map)
    }
}

/// Statistics recorded upon dropping a request arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDropStats {
    /// Total bytes reclaimed by this drop.
    pub bytes_reclaimed: usize,
    /// Lifetime reset count for this arena.
    pub reset_count: usize,
    /// Total resources closed via DropList.
    pub resources_closed: usize,
}

/// Savepoint marker for nested memory scopes (`Memory::scope()`).
#[derive(Debug, Clone, Copy)]
pub struct ArenaMarker {
    pub chunk_idx: usize,
    pub offset: usize,
    pub allocated_bytes: usize,
}

/// Request-scoped bump allocator bound to a worker VM's request lifecycle.
pub struct RequestArena {
    /// Pre-allocated continuous memory buffers.
    chunks: Vec<Vec<u8>>,
    /// Current active chunk index.
    current_chunk: usize,
    /// Current bump offset within the active chunk.
    offset: usize,
    /// Standard size for newly allocated chunks.
    chunk_size: usize,
    /// Total bytes currently allocated during this request.
    allocated_bytes: usize,
    /// High-water mark of allocated bytes for this arena instance.
    peak_bytes: usize,
    /// Total number of O(1) request drops executed by this arena.
    reset_count: usize,
    /// Destructor hooks for non-memory external resources.
    drop_list: Vec<Box<dyn FnOnce() + Send>>,
}

impl Default for RequestArena {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestArena {
    /// Create a new request arena pre-seeded with a default chunk.
    pub fn new() -> Self {
        Self::with_chunk_size(DEFAULT_ARENA_CHUNK_SIZE)
    }

    /// Create a new request arena with a customized chunk size.
    pub fn with_chunk_size(chunk_size: usize) -> Self {
        let mut initial_chunk = Vec::with_capacity(chunk_size);
        // Ensure buffer is initialized
        initial_chunk.resize(chunk_size, 0);

        ARENA_TOTAL_CAPACITY.fetch_add(chunk_size, Ordering::Relaxed);

        Self {
            chunks: vec![initial_chunk],
            current_chunk: 0,
            offset: 0,
            chunk_size,
            allocated_bytes: 0,
            peak_bytes: 0,
            reset_count: 0,
            drop_list: Vec::new(),
        }
    }

    /// Total capacity across all pre-allocated chunks.
    pub fn capacity(&self) -> usize {
        self.chunks.iter().map(|c| c.len()).sum()
    }

    /// Bytes currently allocated in the current request.
    pub fn allocated_bytes(&self) -> usize {
        self.allocated_bytes
    }

    /// Peak bytes allocated across requests.
    pub fn peak_bytes(&self) -> usize {
        self.peak_bytes
    }

    /// Total number of O(1) drops executed.
    pub fn reset_count(&self) -> usize {
        self.reset_count
    }

    /// Pre-allocate raw aligned memory within the arena.
    pub fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        if size == 0 {
            return std::ptr::null_mut();
        }

        // Align current offset
        let align_mask = align.saturating_sub(1);
        let aligned_offset = (self.offset + align_mask) & !align_mask;

        if aligned_offset + size <= self.chunks[self.current_chunk].len() {
            let ptr = unsafe {
                self.chunks[self.current_chunk]
                    .as_mut_ptr()
                    .add(aligned_offset)
            };
            self.offset = aligned_offset + size;
            self.record_alloc(size);
            ptr
        } else {
            // Need next chunk
            self.current_chunk += 1;
            if self.current_chunk >= self.chunks.len() {
                let new_size = self.chunk_size.max(size);
                let mut new_chunk = Vec::with_capacity(new_size);
                new_chunk.resize(new_size, 0);
                ARENA_TOTAL_CAPACITY.fetch_add(new_size, Ordering::Relaxed);
                self.chunks.push(new_chunk);
            }

            let ptr = self.chunks[self.current_chunk].as_mut_ptr();
            self.offset = size;
            self.record_alloc(size);
            ptr
        }
    }

    /// Allocate and copy a byte slice into the arena.
    pub fn alloc_bytes(&mut self, bytes: &[u8]) -> &'static [u8] {
        if bytes.is_empty() {
            return &[];
        }
        let ptr = self.alloc(bytes.len(), 1);
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            std::slice::from_raw_parts(ptr, bytes.len())
        }
    }

    /// Allocate and copy a string slice into the arena.
    pub fn alloc_str(&mut self, s: &str) -> &'static str {
        let bytes = self.alloc_bytes(s.as_bytes());
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }

    /// Register a non-memory resource cleanup hook (e.g. database rollback, file descriptor).
    pub fn register_resource_drop(&mut self, cleanup: impl FnOnce() + Send + 'static) {
        self.drop_list.push(Box::new(cleanup));
    }

    /// Capture an arena marker for scoped sub-allocation (`Memory::scope()`).
    pub fn mark(&self) -> ArenaMarker {
        ArenaMarker {
            chunk_idx: self.current_chunk,
            offset: self.offset,
            allocated_bytes: self.allocated_bytes,
        }
    }

    /// Roll back allocations to a previous marker ($O(1)$ scoped deallocation).
    pub fn reset_to_marker(&mut self, marker: ArenaMarker) -> usize {
        let reclaimed = self.allocated_bytes.saturating_sub(marker.allocated_bytes);
        self.current_chunk = marker.chunk_idx.min(self.chunks.len().saturating_sub(1));
        self.offset = marker.offset;
        self.allocated_bytes = marker.allocated_bytes;
        ARENA_CURRENT_ALLOCATED.fetch_sub(reclaimed, Ordering::Relaxed);
        reclaimed
    }

    /// The Instantaneous $O(1)$ Drop Mechanism:
    ///
    /// The moment the request finishes and the response is flushed to the client,
    /// the entire arena is dropped in a single operation:
    /// 1. Runs non-memory resource drops (files, DB transactions).
    /// 2. Resets bump pointers to 0.
    /// 3. Retains pre-allocated virtual memory buffers for the next incoming request.
    pub fn drop_request(&mut self) -> RequestDropStats {
        let resources_closed = self.drop_list.len();
        // 1. Run external resource drops
        for cleanup in self.drop_list.drain(..) {
            cleanup();
        }

        let bytes_reclaimed = self.allocated_bytes;

        // 2. Instantaneous O(1) Pointer Reset
        self.current_chunk = 0;
        self.offset = 0;
        self.allocated_bytes = 0;
        self.reset_count += 1;

        // Update atomic counters
        ARENA_CURRENT_ALLOCATED.store(0, Ordering::Relaxed);
        ARENA_TOTAL_RESETS.fetch_add(1, Ordering::Relaxed);

        RequestDropStats {
            bytes_reclaimed,
            reset_count: self.reset_count,
            resources_closed,
        }
    }

    /// Get current telemetry statistics for this arena.
    pub fn stats(&self) -> RequestArenaStats {
        RequestArenaStats {
            allocated_bytes: self.allocated_bytes,
            peak_bytes: self.peak_bytes,
            capacity_bytes: self.capacity(),
            reset_count: self.reset_count,
            chunk_count: self.chunks.len(),
        }
    }

    #[inline]
    fn record_alloc(&mut self, bytes: usize) {
        self.allocated_bytes += bytes;
        if self.allocated_bytes > self.peak_bytes {
            self.peak_bytes = self.allocated_bytes;
        }

        ARENA_CURRENT_ALLOCATED.fetch_add(bytes, Ordering::Relaxed);

        // Update global peak
        let mut cur_peak = ARENA_PEAK_ALLOCATED.load(Ordering::Relaxed);
        while self.allocated_bytes > cur_peak {
            match ARENA_PEAK_ALLOCATED.compare_exchange_weak(
                cur_peak,
                self.allocated_bytes,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur_peak = actual,
            }
        }
    }
}

impl Drop for RequestArena {
    fn drop(&mut self) {
        let cap = self.capacity();
        ARENA_TOTAL_CAPACITY.fetch_sub(cap, Ordering::Relaxed);
        for cleanup in self.drop_list.drain(..) {
            cleanup();
        }
    }
}

/// Retrieve global telemetry statistics for all active arenas.
pub fn get_global_arena_stats() -> RequestArenaStats {
    RequestArenaStats {
        allocated_bytes: ARENA_CURRENT_ALLOCATED.load(Ordering::Relaxed),
        peak_bytes: ARENA_PEAK_ALLOCATED.load(Ordering::Relaxed),
        capacity_bytes: ARENA_TOTAL_CAPACITY.load(Ordering::Relaxed),
        reset_count: ARENA_TOTAL_RESETS.load(Ordering::Relaxed),
        chunk_count: 1,
    }
}

/// Reset global arena telemetry counters (for test isolation).
pub fn reset_global_arena_stats() {
    ARENA_CURRENT_ALLOCATED.store(0, Ordering::Relaxed);
    ARENA_PEAK_ALLOCATED.store(0, Ordering::Relaxed);
    ARENA_TOTAL_CAPACITY.store(0, Ordering::Relaxed);
    ARENA_TOTAL_RESETS.store(0, Ordering::Relaxed);
}
