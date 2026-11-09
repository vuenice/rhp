//! # Opaque Handle Registry
//!
//! Enables Tier 2 native packages and database drivers (MongoDB, Cassandra, Cosmos DB, Cache)
//! to maintain persistent connection pools, clients, and cursors directly inside native Rust memory.
//!
//! RHP script-land only receives an opaque integer handle (`u64` ID), avoiding object serialization,
//! memory cloning, and cross-boundary pointer risks.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

/// Global thread-safe registry for opaque native handles.
#[derive(Default)]
pub struct OpaqueHandleRegistry {
    next_id: AtomicU64,
    handles: RwLock<HashMap<u64, (String, Arc<dyn Any + Send + Sync>)>>,
}

static GLOBAL_REGISTRY: OnceLock<OpaqueHandleRegistry> = OnceLock::new();

pub fn get_handle_registry() -> &'static OpaqueHandleRegistry {
    GLOBAL_REGISTRY.get_or_init(|| OpaqueHandleRegistry {
        next_id: AtomicU64::new(1),
        handles: RwLock::new(HashMap::new()),
    })
}

impl OpaqueHandleRegistry {
    /// Register a typed native resource and return a unique opaque handle ID.
    pub fn insert<T: Send + Sync + 'static>(&self, type_name: impl Into<String>, resource: T) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let arc_resource: Arc<dyn Any + Send + Sync> = Arc::new(resource);
        if let Ok(mut guard) = self.handles.write() {
            guard.insert(id, (type_name.into(), arc_resource));
        }
        id
    }

    /// Retrieve an opaque resource by handle ID and downcast to target type `T`.
    pub fn get<T: Send + Sync + 'static>(&self, id: u64) -> Option<Arc<T>> {
        let guard = self.handles.read().ok()?;
        let (_, arc_dyn) = guard.get(&id)?;
        arc_dyn.clone().downcast::<T>().ok()
    }

    /// Check if a handle exists and belongs to the expected type name.
    pub fn has(&self, id: u64, expected_type: &str) -> bool {
        if let Ok(guard) = self.handles.read() {
            if let Some((type_name, _)) = guard.get(&id) {
                return type_name == expected_type;
            }
        }
        false
    }

    /// Remove a handle from the registry (closing the connection or freeing memory).
    pub fn remove(&self, id: u64) -> bool {
        if let Ok(mut guard) = self.handles.write() {
            guard.remove(&id).is_some()
        } else {
            false
        }
    }

    /// Total number of active handles.
    pub fn count(&self) -> usize {
        self.handles.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Clear all registered handles (useful for server restarts and testing).
    pub fn clear(&self) {
        if let Ok(mut guard) = self.handles.write() {
            guard.clear();
        }
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
}
