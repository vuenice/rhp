//! # Opaque Handle Registry
//!
//! Enables Tier 2 native packages and database drivers (MongoDB, Cassandra, Cosmos DB, Cache)
//! to maintain persistent connection pools, clients, and cursors directly inside native Rust memory.
//!
//! RHP script-land only receives an opaque integer handle (`u64` ID), avoiding object serialization,
//! memory cloning, and cross-boundary pointer risks.
//!
//! ## Lifecycle & Request-Scope Isolation:
//! - Request cursors and leased connections live on the active fiber's registry.
//!   `FiberScopeGuard` binds that registry for the fiber and releases it on finish, fail, or drop.
//! - `clear_request_handles()` drops only the active fiber registry (or the thread fallback when
//!   no fiber is running). It does not touch the process-lifetime registry.
//! - Long-lived pools stay on `shared class` instances (`VmSharedObject::set_native_resource`)
//!   or in [`get_handle_registry`], which survives request teardown.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HandleError {
    #[error("Failed to acquire registry lock: poisoned")]
    LockPoisoned,
    #[error("Handle {0} not found")]
    NotFound(u64),
    #[error("Type mismatch for handle {id}: expected {expected}, found {found}")]
    TypeMismatch {
        id: u64,
        expected: String,
        found: String,
    },
}

/// Thread-safe registry for opaque native handles.
#[derive(Default)]
pub struct OpaqueHandleRegistry {
    next_id: AtomicU64,
    handles: RwLock<HashMap<u64, (String, Arc<dyn Any + Send + Sync>)>>,
}

thread_local! {
    /// Registry bound to the fiber currently running on this OS thread.
    static ACTIVE_REGISTRY: RefCell<Option<Arc<OpaqueHandleRegistry>>> = const { RefCell::new(None) };
    /// Used only when no fiber is active (unit tests and non-fiber callers).
    static FALLBACK_REGISTRY: RefCell<Option<Arc<OpaqueHandleRegistry>>> = const { RefCell::new(None) };
}

static GLOBAL_REGISTRY: OnceLock<OpaqueHandleRegistry> = OnceLock::new();

fn fallback_registry() -> Arc<OpaqueHandleRegistry> {
    FALLBACK_REGISTRY.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(Arc::new(OpaqueHandleRegistry::new()));
        }
        Arc::clone(slot.as_ref().expect("fallback registry is installed"))
    })
}

/// Swap the registry bound to this thread. Returns the previous binding.
///
/// `FiberScopeGuard` installs the running fiber's registry and restores the parent on exit.
pub(crate) fn swap_active_registry(
    next: Option<Arc<OpaqueHandleRegistry>>,
) -> Option<Arc<OpaqueHandleRegistry>> {
    ACTIVE_REGISTRY.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), next))
}

/// Access the request-scoped handle registry for the active fiber.
///
/// When no fiber is bound, this uses a thread-local fallback that
/// [`clear_request_handles`] can drop. It never reads the process-lifetime registry.
pub fn with_request_handle_registry<R>(f: impl FnOnce(&OpaqueHandleRegistry) -> R) -> R {
    let active = ACTIVE_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = active {
        f(&reg)
    } else {
        f(&fallback_registry())
    }
}

/// Drop handles owned by the active fiber.
///
/// With no fiber bound, this clears only the thread fallback. The process-lifetime
/// registry from [`get_handle_registry`] is left intact.
pub fn clear_request_handles() {
    let active = ACTIVE_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = active {
        reg.clear();
        return;
    }
    FALLBACK_REGISTRY.with(|cell| {
        if let Some(reg) = cell.borrow().as_ref() {
            reg.clear();
        }
    });
}

/// Process-lifetime handle registry.
///
/// Pools and clients stored here outlive a single request. Request teardown must not clear it.
/// Request cursors belong on the fiber registry via [`with_request_handle_registry`].
pub fn get_handle_registry() -> &'static OpaqueHandleRegistry {
    GLOBAL_REGISTRY.get_or_init(|| OpaqueHandleRegistry {
        next_id: AtomicU64::new(1),
        handles: RwLock::new(HashMap::new()),
    })
}

impl std::fmt::Debug for OpaqueHandleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpaqueHandleRegistry")
            .field("count", &self.count())
            .finish()
    }
}

impl OpaqueHandleRegistry {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            handles: RwLock::new(HashMap::new()),
        }
    }

    /// Register a typed native resource, returning `Ok(id)` or `Err(HandleError)` if the write lock is poisoned.
    pub fn try_insert<T: Send + Sync + 'static>(
        &self,
        type_name: impl Into<String>,
        resource: T,
    ) -> Result<u64, HandleError> {
        let mut guard = self.handles.write().map_err(|_| HandleError::LockPoisoned)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        guard.insert(id, (type_name.into(), Arc::new(resource)));
        Ok(id)
    }

    /// Register a typed native resource and return a unique opaque handle ID.
    /// Recovers from lock poisoning if necessary, ensuring an ID is NEVER returned without being stored.
    pub fn insert<T: Send + Sync + 'static>(&self, type_name: impl Into<String>, resource: T) -> u64 {
        let mut guard = match self.handles.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        guard.insert(id, (type_name.into(), Arc::new(resource)));
        id
    }

    /// Retrieve an opaque resource by handle ID and downcast to target type `T`.
    pub fn get<T: Send + Sync + 'static>(&self, id: u64) -> Option<Arc<T>> {
        let guard = match self.handles.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let (_, arc_dyn) = guard.get(&id)?;
        arc_dyn.clone().downcast::<T>().ok()
    }

    /// Check if a handle exists and belongs to the expected type name.
    pub fn has(&self, id: u64, expected_type: &str) -> bool {
        let guard = match self.handles.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((type_name, _)) = guard.get(&id) {
            type_name == expected_type
        } else {
            false
        }
    }

    /// Remove a handle from the registry (closing the connection or freeing memory).
    pub fn remove(&self, id: u64) -> bool {
        let mut guard = match self.handles.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.remove(&id).is_some()
    }

    /// Total number of active handles.
    pub fn count(&self) -> usize {
        let guard = match self.handles.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.len()
    }

    /// Clear all registered handles (useful for server restarts, request cleanup, and testing).
    pub fn clear(&self) {
        let mut guard = match self.handles.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockMongoClient {
        uri: String,
        pool_size: u32,
    }

    #[test]
    fn test_opaque_handle_lifecycle() {
        let registry = OpaqueHandleRegistry::new();

        let client = MockMongoClient {
            uri: "mongodb://cluster0.local".to_string(),
            pool_size: 10,
        };

        // 1. Insert client and receive handle
        let handle = registry.insert("Mongo", client);
        assert!(handle > 0);
        assert!(registry.has(handle, "Mongo"));
        assert!(!registry.has(handle, "Cassandra"));

        // 2. Fetch and use client via handle
        let fetched = registry.get::<MockMongoClient>(handle).expect("Client should exist");
        assert_eq!(fetched.uri, "mongodb://cluster0.local");
        assert_eq!(fetched.pool_size, 10);

        // 3. Drop handle
        assert!(registry.remove(handle));
        assert!(registry.get::<MockMongoClient>(handle).is_none());
    }

    #[test]
    fn test_try_insert_and_poison_recovery() {
        let registry = OpaqueHandleRegistry::new();
        let handle = registry.try_insert("TestType", 42u64).expect("Insert should succeed");
        assert_eq!(registry.get::<u64>(handle), Some(Arc::new(42u64)));

        // Verify insert always stores before returning
        let handle2 = registry.insert("TestType", 100u64);
        assert_eq!(registry.get::<u64>(handle2), Some(Arc::new(100u64)));
    }

    #[test]
    fn test_try_insert_lock_poison_does_not_mint_id() {
        let registry = OpaqueHandleRegistry::new();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = registry.handles.write().expect("lock");
            panic!("poison registry lock");
        }));

        let before = registry.next_id.load(Ordering::Relaxed);
        let err = registry.try_insert("TestType", 7u64);
        assert!(matches!(err, Err(HandleError::LockPoisoned)));
        assert_eq!(registry.count(), 0);
        assert_eq!(registry.next_id.load(Ordering::Relaxed), before);
    }

    #[test]
    fn test_request_scoped_handles_clear() {
        with_request_handle_registry(|reg| {
            let h = reg.insert("SessionCursor", "cursor_data".to_string());
            assert!(reg.has(h, "SessionCursor"));
        });

        clear_request_handles();

        with_request_handle_registry(|reg| {
            assert_eq!(reg.count(), 0);
        });
    }
}
