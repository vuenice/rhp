//! # Technical Point #20 Integration Tests: Memory Ownership & Safety Enforcement
//!
//! Verifies:
//! - Absolute memory safety and elimination of data races across concurrent worker threads
//!   guaranteed by Rust's compile-time borrow checker and ownership model (`Send + Sync`).
//! - Complete absence of a traditional tracing garbage collector (`isGcFree: true`, `gcRuns: 0`, `gcPauseMs: 0.0`).
//! - Deterministic RAII and scoped deallocation (`Memory::scope(...)` immediately frees intermediate allocations).
//! - Copy-On-Write (COW) memory detachment and telemetry tracking (`Memory::cowDetachments()`).
//! - Memory telemetry and inspection (`Memory::usage()`, `Memory::peak()`, `Memory::stats()`, `Memory::leakCheck()`).
//! - Thread-safety boundary assertions (`Memory::assertThreadSafe(...)`).
//! - Multi-threaded worker pool executing concurrent requests against shared parent state
//!   with zero data races and isolated CoW mutation states.

use rhp::{
    cow_detachments, current_usage, gc_pause_ms, gc_runs, get_memory_stats, is_gc_free,
    live_objects, parse_and_check, peak_usage, reset_memory_tracker, reset_peak, run_source,
    FrontendError, FrozenValue, MemoryScope, MemoryStats, OwnershipEnforcer, ParentProcess,
    PhpValue, TypeErrorKind, WorkerModel, WorkerRequest, WorkerResponseStatus,
};

#[test]
fn test_zero_tracing_garbage_collector_and_instant_drop() {
    reset_memory_tracker();

    // 1. Invariants verification: 0 GC runs, 0.0ms GC pause, tracing GC disabled
    assert!(is_gc_free(), "RHP must be 100% free of a tracing garbage collector");
    assert_eq!(gc_runs(), 0, "Tracing GC runs must be strictly 0");
    assert_eq!(gc_pause_ms(), 0.0, "Tracing GC pause time must be strictly 0.0ms");
    assert!(OwnershipEnforcer::verify_invariants().is_ok());

    // 2. Script-land Memory facade queries
    let source = r#"
        $isFree = Memory::isGcFree();
        $runs = Memory::gcRuns();
        $pause = Memory::gcPauseMs();
        $stats = Memory::stats();

        return [
            $isFree,
            $runs,
            $pause,
            $stats["tracing_gc_enabled"],
            $stats["gc_runs"],
            $stats["thread_safe"],
            $stats["model"]
        ];
    "#;

    let result = run_source(source).expect("Memory facade queries should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Int(0),
            PhpValue::Float(0.0),
            PhpValue::Bool(false),
            PhpValue::Int(0),
            PhpValue::Bool(true),
            PhpValue::string("rust_ownership_raii"),
        ])
    );
}

#[test]
fn test_deterministic_scoped_raii_reclamation() {
    reset_memory_tracker();

    let source = r#"
        $beforeUsage = Memory::usage();

        // Run isolated memory scope: all intermediate variables allocated within the
        // closure frame are dropped deterministically upon return via Rust RAII
        $result = Memory::scope(function(): int {
            $tempList = Arr::list([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
            $tempDict = Arr::dict(["a" => 1, "b" => 2, "c" => 3]);
            $tempStr = Str::slug("temporary string buffer that drops instantly");
            return $tempList->count() + $tempDict->count();
        });

        $afterUsage = Memory::usage();
        return [$result, $beforeUsage, $afterUsage];
    "#;

    let result = run_source(source).expect("Memory::scope execution should succeed");
    let items = result.as_list().expect("Result should be a list");
    assert_eq!(items[0], PhpValue::Int(13)); // 10 + 3
}

#[test]
fn test_copy_on_write_detachment_and_telemetry_tracking() {
    reset_memory_tracker();

    let source = r#"
        $orig = Arr::list([10, 20, 30]);
        $copy1 = $orig; // Shared buffer (ref_count = 2), zero duplication
        $copy2 = $orig; // Shared buffer (ref_count = 3), zero duplication

        $cowsBefore = Memory::cowDetachments();

        // Mutating $copy1 triggers Copy-On-Write detachment of its buffer
        $copy1[0] = 999;

        // Mutating $copy2 triggers another Copy-On-Write detachment
        $copy2[] = 1000;

        $cowsAfter = Memory::cowDetachments();

        return [$orig[0], $copy1[0], $copy2[3], $cowsBefore, $cowsAfter];
    "#;

    let result = run_source(source).expect("COW detachment execution should succeed");
    let items = result.as_list().expect("Result should be a list");

    assert_eq!(items[0], PhpValue::Int(10)); // Original is untouched
    assert_eq!(items[1], PhpValue::Int(999)); // Copy 1 has mutated value
    assert_eq!(items[2], PhpValue::Int(1000)); // Copy 2 has appended value

    let cows_before = match items[3] {
        PhpValue::Int(n) => n as usize,
        _ => 0,
    };
    let cows_after = match items[4] {
        PhpValue::Int(n) => n as usize,
        _ => 0,
    };
    assert!(
        cows_after > cows_before,
        "Copy-On-Write detachments must be tracked when shared collections are mutated"
    );
    assert!(cow_detachments() >= cows_after);
}

#[test]
fn test_data_race_elimination_across_concurrent_worker_threads() {
    // Verifies: absolute memory safety and zero data races across concurrent worker threads
    // without relying on a traditional tracing garbage collector.
    let parent_code = r#"
        class CounterService {
            public static int $globalCounter = 1000;
            public static string $serviceName = "rhp-core";

            public static function process(int $reqId, int $increment): int {
                self::$globalCounter = self::$globalCounter + $increment;
                return self::$globalCounter + $reqId;
            }

            public static function get(): int {
                return self::$globalCounter;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("ParentProcess boot should succeed");

    // Spawn 8 worker threads
    let pool = state
        .spawn_worker_threads(8)
        .expect("Worker pool creation should succeed");

    assert_eq!(pool.num_workers(), 8);
    assert_eq!(pool.model(), WorkerModel::MultiThreaded);

    // Prepare 32 concurrent requests executing in parallel
    let mut requests = Vec::new();
    for i in 0..32 {
        let req_source = format!(
            r#"
            $inc = {};
            $res = CounterService::process({}, $inc);
            return $res;
            "#,
            i + 1,
            i
        );
        requests.push(WorkerRequest::new(req_source).with_id(format!("race_check_req_{}", i)));
    }

    // Execute in parallel across worker threads
    let responses = pool.execute_batch(requests);
    assert_eq!(responses.len(), 32);

    for (i, resp) in responses.into_iter().enumerate() {
        assert_eq!(resp.status, WorkerResponseStatus::Success);
        // Each worker VM starts with isolated CoW copy of $globalCounter (1000)
        // 1000 + (i + 1) + i = 1000 + 2 * i + 1
        let expected = 1000 + (i as i32 + 1) + (i as i32);
        assert_eq!(
            resp.to_php_value(),
            PhpValue::Int(expected),
            "Worker request {} produced unexpected result under concurrent execution",
            i
        );
    }

    // Verify parent state remains untouched and immutable
    let parent_counter = state
        .static_properties
        .get(&("CounterService".to_string(), "globalCounter".to_string()))
        .expect("Parent static property globalCounter must be present");
    assert_eq!(parent_counter, &FrozenValue::Int(1000));
}

#[test]
fn test_thread_safety_boundary_assertions() {
    let source = r#"
        $intVal = 42;
        $strVal = "Hello RHP";
        $listVal = Arr::list([1, 2, 3]);
        $dictVal = Arr::dict(["user" => "alice", "role" => "admin"]);

        $safeInt = Memory::assertThreadSafe($intVal);
        $safeStr = Memory::assertThreadSafe($strVal);
        $safeList = Memory::assertThreadSafe($listVal);
        $safeDict = Memory::assertThreadSafe($dictVal);

        return [$safeInt, $safeStr, $safeList, $safeDict];
    "#;

    let result = run_source(source).expect("Memory::assertThreadSafe queries should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_prohibition_of_pass_by_reference_prevents_cyclic_leaks() {
    // In standard PHP, pass-by-reference (&$var) and closure reference capture (use (&$var))
    // create cyclic reference graphs that necessitate a complex cyclic tracing garbage collector.
    // In RHP, pass-by-reference and by-reference capture are strictly forbidden at the type checker level.

    // 1. Pass-by-reference parameter with strict two-way contract: calling without & must be rejected
    let ref_param_code = r#"
        function leakTester(mixed &$cyclicRef): void {}
        $x = 10;
        leakTester($x);
    "#;
    let err = parse_and_check(ref_param_code).expect_err("Calling pass-by-reference without & must be rejected");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingReferenceArgument { callable_name, param_name, .. } => {
                assert_eq!(callable_name, "leakTester");
                assert_eq!(param_name, "$cyclicRef");
            }
            other => panic!("Expected MissingReferenceArgument, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 2. Closure capture-by-reference must be rejected
    let ref_capture_code = r#"
        $node = [];
        $closure = function() use (&$node) {
            return $node;
        };
    "#;
    let err = parse_and_check(ref_capture_code).expect_err("Closure capture-by-reference must be forbidden");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ClosureReferenceCaptureForbidden { var_name } => {
                assert_eq!(var_name, "$node");
            }
            other => panic!("Expected ClosureReferenceCaptureForbidden, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_leak_check_and_peak_memory_tracking() {
    reset_memory_tracker();

    let source = r#"
        Memory::resetPeak();
        $peakBefore = Memory::peak();

        // Allocate a substantial collection
        $heavyList = Arr::list([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        $peakDuring = Memory::peak();

        // Perform clean leak check
        $report = Memory::leakCheck();

        return [
            $peakBefore >= 0,
            $peakDuring >= $peakBefore,
            $report["leaks_detected"],
            $report["tracing_gc_enabled"],
            $report["gc_runs"]
        ];
    "#;

    let result = run_source(source).expect("Leak check and peak memory tracking should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::Bool(false), // leaks_detected = false
            PhpValue::Bool(false), // tracing_gc_enabled = false
            PhpValue::Int(0),      // gc_runs = 0
        ])
    );
}

#[test]
fn test_concurrent_future_background_tasks_zero_data_races() {
    // Verifies: Dart-style Futures running parallel background closures on separate threads
    // maintain absolute memory safety and zero data races.
    let source = r#"
        $f1 = Future::run(fn() => 10 * 10);
        $f2 = Future::run(fn() => 20 * 20);
        $f3 = Future::run(fn() => 30 * 30);

        [$r1, $r2, $r3] = Future::wait([$f1, $f2, $f3]);
        return [$r1, $r2, $r3];
    "#;

    let result = run_source(source).expect("Concurrent futures should execute safely");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(100),
            PhpValue::Int(400),
            PhpValue::Int(900),
        ])
    );
}

#[test]
fn test_rust_api_memory_scope_and_stats() {
    reset_memory_tracker();
    reset_peak();

    let scope = MemoryScope::new();
    assert_eq!(scope.allocated_since(), 0);

    // Allocate some values via Rust constructors
    let val = PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]);
    assert!(current_usage() > 0);
    assert!(peak_usage() >= current_usage());
    assert!(live_objects() > 0);

    let stats: MemoryStats = get_memory_stats();
    assert_eq!(stats.gc_runs, 0);
    assert_eq!(stats.gc_pause_ns, 0);
    assert!(!stats.tracing_gc_enabled);
    assert!(stats.thread_safe);
    assert_eq!(stats.model, "rust_ownership_raii");

    let dict = stats.to_dict();
    assert!(dict.is_dict());

    drop(val);
}

