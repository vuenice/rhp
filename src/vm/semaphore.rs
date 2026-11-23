//! Semaphore — Async and multi-threaded concurrency permit coordinator.
//!
//! Aligns RHP with Rust and Tokio's `tokio::sync::Semaphore` pattern:
//! - Rate limiting outgoing HTTP/API requests, database connections, and background operations.
//! - Guarding critical sections with maximum concurrency permits.
//! - Supporting both scoped helper execution (`Semaphore::with($permits, $fn)`) and
//!   explicit handle acquisition/release (`$sem->acquire()`, `$sem->release()`, `$sem->run($fn)`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use parking_lot::RwLock;

use crate::vm::error::RuntimeErrorKind;
use crate::vm::facade::FacadeResult;
use crate::vm::value::{PhpValue, VmObject};

static SEMAPHORE_ID_GEN: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<RwLock<HashMap<u64, Arc<tokio::sync::Semaphore>>>> = OnceLock::new();

fn get_registry() -> &'static RwLock<HashMap<u64, Arc<tokio::sync::Semaphore>>> {
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_semaphore(id: u64) -> Option<Arc<tokio::sync::Semaphore>> {
    let guard = get_registry().read();
    guard.get(&id).cloned()
}

pub fn register_semaphore(permits: usize) -> (u64, Arc<tokio::sync::Semaphore>) {
    let id = SEMAPHORE_ID_GEN.fetch_add(1, Ordering::Relaxed);
    let sem = Arc::new(tokio::sync::Semaphore::new(permits));
    let mut guard = get_registry().write();
    guard.insert(id, sem.clone());
    (id, sem)
}

/// Create a new `Semaphore` object with configured permits.
pub fn create_semaphore_object(permits: usize) -> PhpValue {
    let (id, _) = register_semaphore(permits);
    let mut obj = VmObject::new("Semaphore");
    obj.set_property("id", PhpValue::Int(id as i64));
    obj.set_property("permits", PhpValue::Int(permits as i64));
    PhpValue::object(obj)
}

/// Dispatch static methods on the `Semaphore` facade.
pub fn dispatch_semaphore(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "create" | "new" => {
            let permits = args
                .first()
                .and_then(|v| v.to_i64())
                .unwrap_or(1)
                .max(1) as usize;
            Ok(create_semaphore_object(permits))
        }
        "available" | "availablePermits" => {
            if let Some(id_val) = args.first() {
                if let Some(id) = id_val.to_i64() {
                    if let Some(sem) = get_semaphore(id as u64) {
                        return Ok(PhpValue::Int(sem.available_permits() as i64));
                    }
                }
            }
            Ok(PhpValue::Int(0))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Semaphore".to_string(),
            method: method.to_string(),
        }),
    }
}

/// Acquire 1 permit on the semaphore identified by `id`, waiting if necessary.
pub fn acquire_permit(id: u64, timeout: Option<Duration>) -> Result<bool, String> {
    let sem = get_semaphore(id).ok_or_else(|| format!("Semaphore with id {} not found", id))?;
    if let Some(t) = timeout {
        let deadline = std::time::Instant::now() + t;
        loop {
            match sem.try_acquire() {
                Ok(permit) => {
                    permit.forget();
                    return Ok(true);
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    if std::time::Instant::now() >= deadline {
                        return Ok(false);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err("Semaphore closed".to_string());
                }
            }
        }
    } else {
        loop {
            match sem.try_acquire() {
                Ok(permit) => {
                    permit.forget();
                    return Ok(true);
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err("Semaphore closed".to_string());
                }
            }
        }
    }
}

/// Try to acquire 1 permit without blocking.
pub fn try_acquire_permit(id: u64) -> bool {
    if let Some(sem) = get_semaphore(id) {
        match sem.try_acquire() {
            Ok(permit) => {
                permit.forget();
                true
            }
            Err(_) => false,
        }
    } else {
        false
    }
}

/// Release 1 permit back to the semaphore.
pub fn release_permit(id: u64) {
    if let Some(sem) = get_semaphore(id) {
        sem.add_permits(1);
    }
}

/// Get current available permits.
pub fn available_permits(id: u64) -> usize {
    get_semaphore(id).map(|s| s.available_permits()).unwrap_or(0)
}
