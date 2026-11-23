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
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HandleError {
    #[error("Failed to acquire registry lock: poisoned")]
    LockPoisoned,
    #[error("Handle {0} not found")]
    NotFound(u64),
    #[error("Handle {0} has expired or belongs to a different request")]
    Expired(u64),
    #[error("Type mismatch for handle {id}: expected {expected}, found {found}")]
    TypeMismatch {
        id: u64,
        expected: String,
        found: String,
    },
}

/// Marker type for request-lifetime handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestScoped;

/// Marker type for process-lifetime handles (pools and long-lived clients).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessScoped;

/// Type-safe handle wrapper encoding the scope (RequestScoped or ProcessScoped).
#[derive(Debug, PartialEq, Eq)]
pub struct Handle<S> {
    id: u64,
    _marker: PhantomData<S>,
}

impl<S> Handle<S> {
    pub const fn new(id: u64) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl<S> Clone for Handle<S> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _marker: PhantomData,
        }
    }
}

impl<S> Copy for Handle<S> {}

/// An entry stored in the handle registry.
#[derive(Clone)]
pub struct HandleEntry {
    pub type_name: String,
    pub resource: Arc<dyn Any + Send + Sync>,
    pub owner: Option<String>,
}

impl std::fmt::Debug for HandleEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleEntry")
            .field("type_name", &self.type_name)
            .field("owner", &self.owner)
            .finish()
    }
}

/// Thread-safe registry for opaque native handles.
#[derive(Default)]
pub struct OpaqueHandleRegistry {
    next_id: AtomicU64,
    handles: RwLock<HashMap<u64, HandleEntry>>,
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

/// Drop handles owned by the active fiber and sweep any process registry entries owned by it.
///
/// With no fiber bound, this clears only the thread fallback. The process-lifetime
/// registry from [`get_handle_registry`] is left intact except for any entries tagged
/// with the active fiber/request.
pub fn clear_request_handles() {
    let active = ACTIVE_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = active {
        reg.clear();
        if let Some(owner) = crate::vm::fiber::current_fiber_name() {
            get_handle_registry().release_owned_by(&owner);
        }
        return;
    }
    FALLBACK_REGISTRY.with(|cell| {
        if let Some(reg) = cell.borrow().as_ref() {
            reg.clear();
        }
    });
    if let Some(owner) = crate::vm::fiber::current_fiber_name() {
        get_handle_registry().release_owned_by(&owner);
    }
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

/// Register a request-scoped handle. Always bound to the active fiber's registry.
pub fn register_request<T: Send + Sync + 'static>(
    type_name: impl Into<String>,
    resource: T,
) -> Handle<RequestScoped> {
    let owner = crate::vm::fiber::current_fiber_name();
    let id = with_request_handle_registry(|reg| {
        reg.insert_with_owner(type_name, resource, owner)
    });
    Handle::new(id)
}

/// Register a process-lifetime handle (intended for pools and long-lived clients).
pub fn register_process<T: Send + Sync + 'static>(
    type_name: impl Into<String>,
    resource: T,
) -> Handle<ProcessScoped> {
    let id = get_handle_registry().insert_unowned(type_name, resource);
    Handle::new(id)
}

/// Register a process-lifetime handle with an explicit owner tag for teardown backstop.
pub fn register_process_with_owner<T: Send + Sync + 'static>(
    type_name: impl Into<String>,
    resource: T,
    owner: Option<String>,
) -> Handle<ProcessScoped> {
    let id = get_handle_registry().insert_with_owner(type_name, resource, owner);
    Handle::new(id)
}

/// Retrieve a request-scoped handle, ensuring owner validation.
pub fn get_request_handle<T: Send + Sync + 'static>(
    handle: Handle<RequestScoped>,
) -> Result<Arc<T>, HandleError> {
    let owner = crate::vm::fiber::current_fiber_name();
    with_request_handle_registry(|reg| {
        reg.get_checked::<T>(handle.id(), owner.as_deref())
    })
}

/// Retrieve a process-scoped handle.
pub fn get_process_handle<T: Send + Sync + 'static>(
    handle: Handle<ProcessScoped>,
) -> Result<Arc<T>, HandleError> {
    get_handle_registry()
        .get::<T>(handle.id())
        .ok_or(HandleError::NotFound(handle.id()))
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
    /// If an execution fiber is active, automatically tags the handle with the fiber owner name.
    pub fn try_insert<T: Send + Sync + 'static>(
        &self,
        type_name: impl Into<String>,
        resource: T,
    ) -> Result<u64, HandleError> {
        self.try_insert_with_owner(type_name, resource, crate::vm::fiber::current_fiber_name())
    }

    /// Register a typed native resource with an explicit owner tag.
    pub fn try_insert_with_owner<T: Send + Sync + 'static>(
        &self,
        type_name: impl Into<String>,
        resource: T,
        owner: Option<String>,
    ) -> Result<u64, HandleError> {
        let mut guard = self.handles.write().map_err(|_| HandleError::LockPoisoned)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        guard.insert(
            id,
            HandleEntry {
                type_name: type_name.into(),
                resource: Arc::new(resource),
                owner,
            },
        );
        Ok(id)
    }

    /// Register a typed native resource and return a unique opaque handle ID.
    /// Automatically tags with active fiber owner name if executing inside a fiber.
    pub fn insert<T: Send + Sync + 'static>(&self, type_name: impl Into<String>, resource: T) -> u64 {
        let owner = crate::vm::fiber::current_fiber_name();
        self.insert_with_owner(type_name, resource, owner)
    }

    /// Register a process-lifetime resource without any owner tag (will never be swept by request teardown).
    pub fn insert_unowned<T: Send + Sync + 'static>(&self, type_name: impl Into<String>, resource: T) -> u64 {
        self.insert_with_owner(type_name, resource, None)
    }

    /// Register a typed native resource with an explicit owner tag and return a unique opaque handle ID.
    pub fn insert_with_owner<T: Send + Sync + 'static>(
        &self,
        type_name: impl Into<String>,
        resource: T,
        owner: Option<String>,
    ) -> u64 {
        let mut guard = match self.handles.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        guard.insert(
            id,
            HandleEntry {
                type_name: type_name.into(),
                resource: Arc::new(resource),
                owner,
            },
        );
        id
    }

    /// Retrieve an opaque resource by handle ID and downcast to target type `T`.
    /// Rejects cross-request access if the handle is owned by a different fiber.
    pub fn get<T: Send + Sync + 'static>(&self, id: u64) -> Option<Arc<T>> {
        let guard = match self.handles.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = guard.get(&id)?;
        if let Some(entry_owner) = &entry.owner {
            if let Some(current_owner) = crate::vm::fiber::current_fiber_name() {
                if entry_owner != &current_owner {
                    return None;
                }
            }
        }
        entry.resource.clone().downcast::<T>().ok()
    }

    /// Retrieve an opaque resource by handle ID, returning `Err(HandleError::Expired)`
    /// if the handle is tagged with an owner that does not match `expected_owner`.
    pub fn get_checked<T: Send + Sync + 'static>(
        &self,
        id: u64,
        expected_owner: Option<&str>,
    ) -> Result<Arc<T>, HandleError> {
        let guard = match self.handles.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = guard.get(&id).ok_or(HandleError::NotFound(id))?;
        if let Some(entry_owner) = &entry.owner {
            if let Some(expected) = expected_owner {
                if entry_owner != expected {
                    return Err(HandleError::Expired(id));
                }
            } else if let Some(current_owner) = crate::vm::fiber::current_fiber_name() {
                if entry_owner != &current_owner {
                    return Err(HandleError::Expired(id));
                }
            }
        }
        entry
            .resource
            .clone()
            .downcast::<T>()
            .map_err(|_| HandleError::TypeMismatch {
                id,
                expected: std::any::type_name::<T>().to_string(),
                found: entry.type_name.clone(),
            })
    }

    /// Check if a handle exists, belongs to the expected type, and is not owned by a different fiber.
    pub fn has(&self, id: u64, expected_type: &str) -> bool {
        let guard = match self.handles.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(entry) = guard.get(&id) {
            if let Some(entry_owner) = &entry.owner {
                if let Some(current_owner) = crate::vm::fiber::current_fiber_name() {
                    if entry_owner != &current_owner {
                        return false;
                    }
                }
            }
            entry.type_name == expected_type
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

    /// Sweep and remove all handles owned by a specific fiber/request name. Returns count of freed handles.
    pub fn release_owned_by(&self, owner: &str) -> usize {
        let mut guard = match self.handles.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let before = guard.len();
        guard.retain(|_, entry| entry.owner.as_deref() != Some(owner));
        before - guard.len()
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

    #[test]
    fn test_typed_handles_and_owner_sweep() {
        let registry = OpaqueHandleRegistry::new();
        let h_proc = registry.insert_unowned("ProcessPool", "pg_pool".to_string());
        let h_req = registry.insert_with_owner("Cursor", "cursor_1".to_string(), Some("req_42".to_string()));

        assert_eq!(registry.count(), 2);
        assert_eq!(registry.release_owned_by("req_42"), 1);
        assert_eq!(registry.count(), 1);
        assert!(registry.has(h_proc, "ProcessPool"));
        assert!(!registry.has(h_req, "Cursor"));
    }

    #[test]
    fn test_owner_checked_lookup() {
        let registry = OpaqueHandleRegistry::new();
        let id = registry.insert_with_owner("Cursor", "data".to_string(), Some("fiber_alpha".to_string()));

        // Matching owner succeeds
        assert!(registry.get_checked::<String>(id, Some("fiber_alpha")).is_ok());

        // Mismatched owner fails with Expired
        let err = registry.get_checked::<String>(id, Some("fiber_beta"));
        assert!(matches!(err, Err(HandleError::Expired(_))));
    }

    #[test]
    fn test_typed_handle_registration_api() {
        let pool = register_process("MyPool", 12345u64);
        assert_eq!(get_process_handle::<u64>(pool).unwrap(), Arc::new(12345u64));

        let cur = register_request("MyCursor", "query_res".to_string());
        assert_eq!(get_request_handle::<String>(cur).unwrap(), Arc::new("query_res".to_string()));
    }
}
