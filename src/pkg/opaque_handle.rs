//! # Opaque Handle Registry
//!
//! Enables Tier 2 native packages and database drivers (MongoDB, Cassandra, Cosmos DB, Cache)
//! to maintain persistent connection pools, clients, and cursors directly inside native Rust memory.
//!
//! RHP script-land only receives an opaque integer handle (`u64` ID), avoiding object serialization,
//! memory cloning, and cross-boundary pointer risks.
//!
//! ## Lifecycle & Request-Scope Isolation:
//! - Native handles created within a request scope are deterministically freed when the request ends
//!   via `clear_request_handles()`, guaranteeing zero handle leakage across requests.
//! - Long-lived persistent drivers belong on `shared class` instances (via `VmSharedObject::set_native_resource`).

use std::any::Any;
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
    static REQUEST_REGISTRY: OpaqueHandleRegistry = OpaqueHandleRegistry::new();
}

static GLOBAL_REGISTRY: OnceLock<OpaqueHandleRegistry> = OnceLock::new();

/// Access the request-scoped handle registry for the active request thread.
pub fn with_request_handle_registry<R>(f: impl FnOnce(&OpaqueHandleRegistry) -> R) -> R {
    REQUEST_REGISTRY.with(|reg| f(reg))
}

/// Clear all request-scoped handles (deterministically invoked at request completion).
pub fn clear_request_handles() {
    REQUEST_REGISTRY.with(|reg| {
        reg.clear();
    });
}

/// Access the handle registry (cleared on request termination to guarantee worker isolation).
pub fn get_handle_registry() -> &'static OpaqueHandleRegistry {
    GLOBAL_REGISTRY.get_or_init(|| OpaqueHandleRegistry {
        next_id: AtomicU64::new(1),
        handles: RwLock::new(HashMap::new()),
    })
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
        let registry = get_handle_registry();
        registry.clear();

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
