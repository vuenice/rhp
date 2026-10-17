//! # Request-Scoped Arena Allocation & Static AST Cycle Prevention Tests
//!
//! Verifies:
//! 1. Compile-Time AST Protection Against Cyclic References:
//!    - Flagging closures capturing `$this` assigned into `$this->property` (`CyclicClosureCaptureForbidden`).
//!    - Flagging explicit `use ($this)` in anonymous functions (`CyclicClosureCaptureForbidden`).
//!    - Requiring explicit `Weak::to($parent)` for bidirectional links (`CyclicReferenceRisk`).
//! 2. Explicit Language-Level Weak References (`Weak` Facade):
//!    - `Weak::to($object)` returns a `WeakReference` handle without incrementing strong reference counts.
//!    - `->get()` retrieves target while alive; returns `null` once target is dropped.
//!    - `->isValid()` returns boolean liveness.
//! 3. Request-Scoped Arena Bump Allocation:
//!    - Pre-allocated continuous chunk bump allocation.
//!    - Zero-overhead instantaneous $O(1)$ request drop/reset.
//!    - Virtual memory buffer reuse across requests with zero system malloc overhead.
//!    - Resource `DropList` cleanup for non-memory OS handles.
//! 4. Parent vs. Child Worker Integration:
//!    - Worker requests execute within request arena and instantly drop all allocations on response completion.
//!    - Immutable parent process state (`Arc<EngineState>`) remains pristine and leak-free.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rhp::type_system::error::TypeErrorKind;
use rhp::type_system::{parse_and_check, FrontendError};
use rhp::vm::arena::{reset_global_arena_stats, RequestArena};
use rhp::vm::parent_process::ParentProcess;
use rhp::vm::value::{PhpValue, VmObject};
use rhp::vm::worker::{WorkerPool, WorkerRequest};

// =========================================================================
// 1. Compile-Time Static AST Cycle Prevention Tests
// =========================================================================

#[test]
fn test_closure_capturing_this_into_property_rejected_at_compile_time() {
    let script = r#"
    class Service {
        public $handler;

        public function register(): void {
            $this->handler = function(): int {
                return $this->compute();
            };
        }

        public function compute(): int {
            return 42;
        }
    }
    "#;

    let res = parse_and_check(script);
    assert!(res.is_err(), "Assigning closure capturing $this into $this->property must fail");
    let err = res.err().unwrap();
    if let FrontendError::Type(te) = err {
        match te.kind {
            TypeErrorKind::CyclicClosureCaptureForbidden { ref target } => {
                assert!(target.contains("$this->handler"));
            }
            other => panic!("Expected CyclicClosureCaptureForbidden, got {:?}", other),
        }
    } else {
        panic!("Expected FrontendError::Type");
    }
}

#[test]
fn test_arrow_function_capturing_this_into_property_rejected_at_compile_time() {
    let script = r#"
    class Listener {
        public $callback;

        public function bind(): void {
            $this->callback = fn(): int => $this->getValue();
        }

        public function getValue(): int {
            return 100;
        }
    }
    "#;

    let res = parse_and_check(script);
    assert!(res.is_err(), "Assigning arrow function capturing $this into $this->property must fail");
    let err = res.err().unwrap();
    if let FrontendError::Type(te) = err {
        match te.kind {
            TypeErrorKind::CyclicClosureCaptureForbidden { ref target } => {
                assert!(target.contains("$this->callback"));
            }
            other => panic!("Expected CyclicClosureCaptureForbidden, got {:?}", other),
        }
    } else {
        panic!("Expected FrontendError::Type");
    }
}

#[test]
fn test_static_closure_assigned_to_property_is_allowed() {
    let script = r#"
    class SafeService {
        public $transformer;

        public function register(): void {
            $this->transformer = static fn(int $x): int => $x * 2;
        }
    }
    "#;

    let res = parse_and_check(script);
    assert!(res.is_ok(), "Static closures do not capture $this and must be allowed");
}

#[test]
fn test_explicit_use_this_capture_rejected_at_compile_time() {
    let script = r#"
    class Worker {
        public function run(): void {
            $fn = function() use ($this): int {
                return 1;
            };
        }
    }
    "#;

    let res = parse_and_check(script);
    assert!(res.is_err(), "Explicit 'use ($this)' capture must be rejected at compile time");
    let err = res.err().unwrap();
    if let FrontendError::Type(te) = err {
        match te.kind {
            TypeErrorKind::CyclicClosureCaptureForbidden { ref target } => {
                assert_eq!(target, "$this");
            }
            other => panic!("Expected CyclicClosureCaptureForbidden for $this, got {:?}", other),
        }
    } else {
        panic!("Expected FrontendError::Type");
    }
}

#[test]
fn test_bidirectional_parent_link_without_weak_reference_rejected() {
    let script = r#"
    class Node {
        public $parent;

        public function attach(Node $p): void {
            $this->parent = $p;
        }
    }
    "#;

    let res = parse_and_check(script);
    assert!(res.is_err(), "Direct strong parent assignment must be flagged as cyclic risk");
    let err = res.err().unwrap();
    if let FrontendError::Type(te) = err {
        match te.kind {
            TypeErrorKind::CyclicReferenceRisk { ref target } => {
                assert!(target.contains("->parent"));
            }
            other => panic!("Expected CyclicReferenceRisk, got {:?}", other),
        }
    } else {
        panic!("Expected FrontendError::Type");
    }
}

#[test]
fn test_bidirectional_parent_link_with_weak_wrapper_is_allowed() {
    let script = r#"
    class Node {
        public $parent;

        public function attach(Node $p): void {
            $this->parent = Weak::to($p);
        }
    }
    "#;

    let res = parse_and_check(script);
    assert!(res.is_ok(), "Parent assignment wrapped in Weak::to(...) must pass static type check");
}

// =========================================================================
// 2. Explicit Language-Level Weak References (`Weak` Facade) Tests
// =========================================================================

#[test]
fn test_weak_reference_does_not_increment_strong_count() {
    let mut obj = VmObject::new("User");
    obj.set_property("name", PhpValue::string("Alice"));
    let target = Rc::new(RefCell::new(obj));

    assert_eq!(Rc::strong_count(&target), 1);

    // Create weak reference via PhpValue::weak_ref
    let weak_val = PhpValue::weak_ref(&target);

    // Strong count MUST remain strictly 1
    assert_eq!(Rc::strong_count(&target), 1);

    if let PhpValue::Object(ref weak_rc) = weak_val {
        let weak_obj = weak_rc.borrow();
        assert_eq!(weak_obj.class_name, "WeakReference");
        let upgraded = weak_obj.weak_target.as_ref().unwrap().upgrade();
        assert!(upgraded.is_some());
        assert_eq!(upgraded.unwrap().borrow().get_property("name").unwrap().to_php_string(), "Alice");
    } else {
        panic!("Expected PhpValue::Object");
    }

    // Drop target
    drop(target);

    // Upgrading dropped target must now return None
    if let PhpValue::Object(ref weak_rc) = weak_val {
        let weak_obj = weak_rc.borrow();
        let upgraded = weak_obj.weak_target.as_ref().unwrap().upgrade();
        assert!(upgraded.is_none(), "Target was dropped, upgrade must yield None");
    }
}

#[test]
fn test_weak_facade_runtime_get_and_is_valid() {
    let parent = ParentProcess::boot(
        r#"
        class ParentNode {
            public string $name = "Root";
        }

        class ChildNode {
            public $parentRef;

            public function setParent(ParentNode $p): void {
                $this->parentRef = Weak::to($p);
            }

            public function getParentName(): string {
                $p = $this->parentRef->get();
                if ($p !== null) {
                    return $p->name;
                }
                return "detached";
            }

            public function isParentAlive(): bool {
                return $this->parentRef->isValid();
            }
        }
        "#,
    )
    .expect("Boot must succeed");

    let script = r#"
        $parent = new ParentNode();
        $child = new ChildNode();
        $child->setParent($parent);

        $alive = $child->isParentAlive();
        $name = $child->getParentName();

        return [$alive, $name];
    "#;

    let res = parent.execute_request(script).expect("Execution must succeed");
    match res {
        PhpValue::List(items) | PhpValue::Array(items) => {
            let b = items.borrow();
            assert_eq!(b[0], PhpValue::Bool(true));
            assert_eq!(b[1], PhpValue::string("Root"));
        }
        _ => panic!("Expected List or Array result, got {:?}", res),
    }
}

// =========================================================================
// 3. Request-Scoped Arena Bump Allocation Tests
// =========================================================================

#[test]
fn test_request_arena_bump_allocation_and_o1_drop() {
    let mut arena = RequestArena::with_chunk_size(1024);

    assert_eq!(arena.allocated_bytes(), 0);
    assert_eq!(arena.capacity(), 1024);
    assert_eq!(arena.reset_count(), 0);

    // Allocate string slices in arena
    let s1 = arena.alloc_str("Hello RHP Arena");
    let s2 = arena.alloc_str("High performance request memory");

    assert_eq!(s1, "Hello RHP Arena");
    assert_eq!(s2, "High performance request memory");
    assert!(arena.allocated_bytes() > 0);
    let bytes_before_drop = arena.allocated_bytes();

    // Instantaneous O(1) Drop
    let drop_stats = arena.drop_request();
    assert_eq!(drop_stats.bytes_reclaimed, bytes_before_drop);
    assert_eq!(drop_stats.reset_count, 1);
    assert_eq!(arena.allocated_bytes(), 0);
    assert_eq!(arena.reset_count(), 1);

    // Reusing the same arena for next request requires ZERO reallocation
    assert_eq!(arena.capacity(), 1024);
    let s3 = arena.alloc_str("Next Request In Same Arena Buffer");
    assert_eq!(s3, "Next Request In Same Arena Buffer");
    assert!(arena.allocated_bytes() > 0);
}

#[test]
fn test_request_arena_resource_drop_list() {
    let mut arena = RequestArena::with_chunk_size(1024);
    let db_rolled_back = Arc::new(AtomicBool::new(false));
    let db_clone = db_rolled_back.clone();

    // Register external non-memory resource cleanup hook
    arena.register_resource_drop(move || {
        db_clone.store(true, Ordering::SeqCst);
    });

    assert!(!db_rolled_back.load(Ordering::SeqCst));

    // Request finishes -> instant O(1) drop triggers DropList
    let drop_stats = arena.drop_request();
    assert_eq!(drop_stats.resources_closed, 1);
    assert!(db_rolled_back.load(Ordering::SeqCst), "External resource hook must run on arena drop");
}

#[test]
fn test_memory_arena_stats_telemetry_in_scripts() {
    reset_global_arena_stats();

    let parent = ParentProcess::boot("").expect("Boot must succeed");
    let script = r#"
        $stats = Memory::arenaStats();
        $resetCount = $stats['reset_count'];
        $isZeroGc = $stats['zero_gc_drop'];

        return [$resetCount, $isZeroGc];
    "#;

    let res = parent.execute_request(script).expect("Execution must succeed");
    match res {
        PhpValue::List(items) | PhpValue::Array(items) => {
            let b = items.borrow();
            assert_eq!(b[0], PhpValue::Int(0));
            assert_eq!(b[1], PhpValue::Bool(true));
        }
        _ => panic!("Expected List or Array result, got {:?}", res),
    }
}

// =========================================================================
// 4. Parent vs. Child Worker Process Tiering Tests
// =========================================================================

#[test]
fn test_worker_request_executes_with_instant_arena_drop() {
    let parent = ParentProcess::boot(
        r#"
        function handleRequest(string $payload): string {
            $temp = Str::upper($payload) . " PROCESSED";
            return $temp;
        }
        "#,
    )
    .expect("Parent boot must succeed");

    let mut pool = WorkerPool::builder()
        .workers(2)
        .build(parent.clone())
        .expect("Worker pool build must succeed");

    let response = pool.execute(WorkerRequest::call(
        "handleRequest",
        vec![PhpValue::string("rhp_fast_payload")],
    ));

    assert!(response.is_ok());
    assert_eq!(
        response.value().to_php_value().to_php_string(),
        "RHP_FAST_PAYLOAD PROCESSED"
    );

    // Verify request arena drop telemetry is populated on completion
    let arena_stats = response.arena_drop();
    assert!(arena_stats.is_some(), "Worker response must record arena drop telemetry");
    let stats = arena_stats.unwrap();
    assert_eq!(stats.reset_count, 1, "Arena reset count must increment on request completion");

    let _ = pool.shutdown();
}

#[test]
fn test_parent_state_unaffected_by_repeated_worker_requests() {
    let parent = ParentProcess::boot(
        r#"
        class GlobalConfig {
            public static string $appName = "RHP Enterprise";
        }
        function getAppName(): string {
            return GlobalConfig::$appName;
        }
        "#,
    )
    .expect("Parent boot must succeed");

    let mut pool = WorkerPool::builder()
        .workers(4)
        .build(parent.clone())
        .expect("Worker pool build must succeed");

    // Execute 20 concurrent requests
    let mut requests = Vec::new();
    for i in 0..20 {
        requests.push(WorkerRequest::call("getAppName", Vec::<PhpValue>::new()).with_id(format!("req_{}", i)));
    }

    let responses = pool.execute_batch(requests);
    assert_eq!(responses.len(), 20);

    for resp in responses {
        assert!(resp.is_ok());
        assert_eq!(resp.value().to_php_value().to_php_string(), "RHP Enterprise");
        assert!(resp.arena_drop().is_some());
        assert_eq!(resp.arena_drop().unwrap().reset_count, 1);
    }

    // Verify parent state remains unchanged and leak-free
    let direct_call = parent
        .call_function("getAppName", Vec::new())
        .expect("Parent call must succeed");
    assert_eq!(direct_call.to_php_string(), "RHP Enterprise");

    let _ = pool.shutdown();
}
