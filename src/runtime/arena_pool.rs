//! # Per-Worker Request Arena Pool
//!
//! Each Worker process maintains a thread-local pool of warm `RequestArena`
//! instances. After a request completes, the arena resets in O(1) via `drop_request()`
//! and returns to the pool so its pre-allocated slabs can be reused for the next request.
//!
//! This avoids `mmap`/`munmap` syscalls on the hot path and keeps memory
//! bounded: the pool holds at most `MAX_CACHED_ARENAS` idle arenas, and any
//! surplus is simply dropped (the allocator frees the backing slabs).

use std::cell::RefCell;

use crate::vm::arena::RequestArena;

/// Maximum number of idle arenas cached per worker thread.
const MAX_CACHED_ARENAS: usize = 32;

/// Thread-local pool of warm `RequestArena` instances.
struct ArenaPool {
    idle: Vec<RequestArena>,
}

impl ArenaPool {
    fn new() -> Self {
        Self { idle: Vec::with_capacity(MAX_CACHED_ARENAS) }
    }

    /// Acquire a warm arena (or allocate a fresh one).
    fn acquire(&mut self) -> RequestArena {
        self.idle.pop().unwrap_or_default()
    }

    /// Return a used arena to the pool after resetting it.
    fn release(&mut self, mut arena: RequestArena) {
        if self.idle.len() < MAX_CACHED_ARENAS {
            let _ = arena.drop_request();
            self.idle.push(arena);
        }
        // If pool is full, `arena` drops here — backing slabs are freed.
    }
}

thread_local! {
    static POOL: RefCell<ArenaPool> = RefCell::new(ArenaPool::new());
}

/// Acquire a warm `RequestArena` from the per-thread pool.
///
/// The returned arena has been reset and is ready for a new request.
/// Callers MUST call [`release_arena`] when the request is complete.
pub fn acquire_arena() -> RequestArena {
    POOL.with(|p| p.borrow_mut().acquire())
}

/// Return a completed request's arena to the per-thread pool.
///
/// The arena is reset internally in O(1); no need to clear it before calling.
pub fn release_arena(arena: RequestArena) {
    POOL.with(|p| p.borrow_mut().release(arena));
}

/// Statistics about the current thread's arena pool.
#[derive(Debug, Clone)]
pub struct ArenaPoolStats {
    /// Number of warm arenas currently idle in the pool.
    pub idle_count: usize,
    /// Maximum arenas the pool will cache.
    pub max_cached: usize,
}

/// Return stats for the current thread's arena pool.
pub fn arena_pool_stats() -> ArenaPoolStats {
    POOL.with(|p| ArenaPoolStats {
        idle_count: p.borrow().idle.len(),
        max_cached: MAX_CACHED_ARENAS,
    })
}
