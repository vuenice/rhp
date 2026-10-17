//! # Technical Point #16 Integration Tests: Synchronous-by-Default Execution Model & Local Execution Fibers
//!
//! Verifies:
//! - **Synchronous-by-Default Execution Ergonomics:** Developers write linear, sequential code
//!   calling standard helpers (`File::read()`, `DB::table()`, `HTTP::get()`) without `async`/`await`
//!   boilerplate or promise chains.
//! - **Local Execution Fiber Lifecycle:** Tracks fiber states (`Created`, `Running`, `Yielded`, `WaitingIo`,
//!   `Completed`, `Failed`) with high-resolution CPU and I/O duration telemetry.
//! - **Fiber Facade API:** `Fiber::current()`, `Fiber::isFiber()`, `Fiber::yield()`, `Fiber::stats()`,
//!   `Fiber::context()`, and `Fiber::all()`.
//! - **Cooperative Fiber Scheduling:** `FiberScheduler` runs multiple local execution fibers
//!   cooperatively, interleaving execution and yielding without blocking OS worker threads.
//! - **Worker Pool Integration:** Requests executed by `WorkerPool` automatically run inside
//!   dedicated execution fibers, populating `WorkerResponse::fiber_stats()`.
//! - **Static Facade Invariant Guards:** Instantiation (`new Fiber()`) and inheritance (`extends Fiber`)
//!   are strictly forbidden with fatal compile-time / runtime errors.
//! - **Fault Containment:** Unhandled errors within a fiber transition it to `Failed` without crashing
//!   the scheduler or affecting concurrent fibers.

use std::fs;

use rhp::{
    parse, run_source_in_fiber, run_source_in_fiber_with_context, FiberScheduler, FiberStepResult,
    IndexMap, IoKind, ParentProcess, ParseErrorKind, PhpValue, RuntimeError, RuntimeErrorKind,
    WorkerRequest, WorkerResponseStatus,
};

#[test]
fn test_synchronous_by_default_linear_ergonomics() {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("rhp_sync_model_test.txt");
    fs::write(&test_file, "rhp synchronous fiber content").expect("Write test file");
    let test_path = test_file.to_str().unwrap().replace('\\', "/");

    let source = format!(
        r#"
        // 1. Synchronous-by-default File read without await/promise boilerplate
        $content = File::read("{}");

        // 2. Synchronous-by-default Database query builder
        $query = DB::table("posts");
        $post = $query->find(42);

        // 3. Synchronous-by-default HTTP call
        $response = HTTP::get("https://api.example.com/status");

        // 4. Pure linear evaluation: inspect results immediately
        $upperContent = Str::upper($content);
        $postId = $post->id;
        $httpStatus = $response->status;

        return Arr::list([$upperContent, $postId, $httpStatus]);
        "#,
        test_path
    );

    let (result, stats) = run_source_in_fiber(&source, "sync_request_fiber")
        .expect("Synchronous source execution must succeed");

    // Verify sequential return values
    match result {
        PhpValue::List(items_rc) => {
            let items = items_rc.borrow();
            assert_eq!(items.len(), 3);
            assert_eq!(
                items[0],
                PhpValue::string("RHP SYNCHRONOUS FIBER CONTENT")
            );
            assert_eq!(items[1], PhpValue::Int(42));
            assert_eq!(items[2], PhpValue::Int(200));
        }
        other => panic!("Expected List return, got {:?}", other),
    }

    // Verify fiber telemetry captured all sequential I/O operations
    assert_eq!(stats.name, "sync_request_fiber");
    assert!(stats.is_completed);
    assert!(stats.io_count >= 3, "Expected at least 3 I/O calls (File, DB, HTTP), got {}", stats.io_count);
    assert_eq!(stats.io_breakdown.get("File"), Some(&1));
    assert!(stats.io_breakdown.get("DB").unwrap_or(&0) >= &1);
    assert_eq!(stats.io_breakdown.get("HTTP"), Some(&1));

    let _ = fs::remove_file(&test_file);
}

#[test]
fn test_local_execution_fiber_lifecycle_and_telemetry() {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("rhp_fiber_lifecycle_test.txt");
    let test_path = test_file.to_str().unwrap().replace('\\', "/");

    let source = format!(
        r#"
        // Step 1: Write file (I/O event #1)
        File::write("{}", "data block");

        // Step 2: Check existence (I/O event #2)
        $exists = File::exists("{}");

        // Step 3: Query DB table (I/O event #3)
        $users = DB::table("users")->get();

        // Step 4: Delete file (I/O event #4)
        File::delete("{}");

        return $exists;
        "#,
        test_path, test_path, test_path
    );

    let (result, stats) = run_source_in_fiber(&source, "telemetry_fiber")
        .expect("Fiber execution should succeed");

    assert_eq!(result, PhpValue::Bool(true));
    assert_eq!(stats.name, "telemetry_fiber");
    assert!(stats.is_completed);
    assert!(stats.io_count >= 4, "Expected at least 4 I/O events, got {}", stats.io_count);
    assert_eq!(stats.io_breakdown.get("File"), Some(&3));
    assert_eq!(stats.io_breakdown.get("DB"), Some(&2));
    assert!(stats.total_duration >= stats.cpu_duration);
}

#[test]
fn test_fiber_facade_in_script_execution() {
    let source = r#"
        // 1. Fiber::isFiber() check
        $inside = Fiber::isFiber();

        // 2. Fiber::current() inspection
        $curr = Fiber::current();
        $fiberName = $curr["name"];
        $fiberState = $curr["state"];

        // 3. Fiber-local context storage
        Fiber::context("request_id", "req-xyz-789");
        Fiber::context("user_role", "admin");
        $retrievedReqId = Fiber::context("request_id");

        // 4. Cooperative yield
        $yieldOk = Fiber::yield();

        // 5. Query stats
        $stats = Fiber::stats();
        $yields = $stats["yield_count"];

        return Arr::dict([
            "is_fiber" => $inside,
            "name" => $fiberName,
            "state" => $fiberState,
            "req_id" => $retrievedReqId,
            "yield_ok" => $yieldOk,
            "yield_count" => $yields,
        ]);
    "#;

    let (result, stats) = run_source_in_fiber(source, "facade_probe_fiber")
        .expect("Fiber facade script must execute cleanly");

    if let PhpValue::Dict(map_rc) = result {
        let map = map_rc.borrow();
        assert_eq!(map.get("is_fiber"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("name"), Some(&PhpValue::string("facade_probe_fiber")));
        assert_eq!(map.get("state"), Some(&PhpValue::string("Running")));
        assert_eq!(map.get("req_id"), Some(&PhpValue::string("req-xyz-789")));
        assert_eq!(map.get("yield_ok"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("yield_count"), Some(&PhpValue::Int(1)));
    } else {
        panic!("Expected Dict return, got {:?}", result);
    }

    assert_eq!(stats.yield_count, 1);
}

#[test]
fn test_fiber_context_prepopulation_and_isolation() {
    let source = r#"
        $tenant = Fiber::context("tenant");
        $auth = Fiber::context("auth_user");
        $missing = Fiber::context("non_existent");

        // Mutate context inside fiber
        Fiber::context("status", "processed");
        $newStatus = Fiber::context("status");

        return Arr::list([$tenant, $auth, $missing, $newStatus]);
    "#;

    let mut ctx = IndexMap::new();
    ctx.insert("tenant".to_string(), PhpValue::string("enterprise-alpha"));
    ctx.insert("auth_user".to_string(), PhpValue::Int(994));

    let (result, _) = run_source_in_fiber_with_context(source, "context_fiber", ctx)
        .expect("Context-populated fiber should succeed");

    if let PhpValue::List(items_rc) = result {
        let items = items_rc.borrow();
        assert_eq!(items[0], PhpValue::string("enterprise-alpha"));
        assert_eq!(items[1], PhpValue::Int(994));
        assert_eq!(items[2], PhpValue::Null);
        assert_eq!(items[3], PhpValue::string("processed"));
    } else {
        panic!("Expected List, got {:?}", result);
    }
}

#[test]
fn test_cooperative_fiber_scheduler_multitasking() {
    let mut scheduler = FiberScheduler::new();

    // Spawn Fiber 1: does two yields then finishes
    let mut f1_step = 0;
    let f1_id = scheduler.spawn("fiber_worker_1", move |_fiber| {
        f1_step += 1;
        match f1_step {
            1 => FiberStepResult::Yield,
            2 => FiberStepResult::Yield,
            _ => FiberStepResult::Done(PhpValue::string("F1 finished")),
        }
    });

    // Spawn Fiber 2: does an I/O wait then finishes
    let mut f2_step = 0;
    let f2_id = scheduler.spawn("fiber_worker_2", move |_fiber| {
        f2_step += 1;
        match f2_step {
            1 => FiberStepResult::WaitIo(IoKind::DbTable {
                table: "orders".to_string(),
            }),
            _ => FiberStepResult::Done(PhpValue::Int(777)),
        }
    });

    // Spawn Fiber 3: simple run-to-completion task
    let f3_id = scheduler.spawn_simple("fiber_worker_3", || {
        Ok(PhpValue::Bool(true))
    });

    // Run all fibers to completion on the cooperative scheduler
    let results = scheduler.run_all();
    assert_eq!(results.len(), 3);

    assert_eq!(
        results.get(&f1_id).unwrap().as_ref().unwrap(),
        &PhpValue::string("F1 finished")
    );
    assert_eq!(
        results.get(&f2_id).unwrap().as_ref().unwrap(),
        &PhpValue::Int(777)
    );
    assert_eq!(
        results.get(&f3_id).unwrap().as_ref().unwrap(),
        &PhpValue::Bool(true)
    );

    let stats = scheduler.stats();
    assert_eq!(stats.total_spawned, 3);
    assert_eq!(stats.total_completed, 3);
    assert_eq!(stats.total_failed, 0);
    assert!(stats.total_context_switches >= 5);
    assert_eq!(stats.total_io_events, 1);
}

#[test]
fn test_worker_pool_request_fibers_and_telemetry() {
    let parent_code = r#"
        class OrderService {
            public static function process(int $orderId): array {
                // Sequential standard helpers inside worker fiber
                $table = DB::table("orders");
                $record = $table->find($orderId);
                $resp = HTTP::get("https://shipping.internal/verify");
                return Arr::dict([
                    "order_id" => $orderId,
                    "status" => $resp->status,
                ]);
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("ParentProcess boot failed");
    let pool = state
        .spawn_worker_threads(4)
        .expect("Failed to create worker pool");

    // Execute 8 concurrent requests
    let mut requests = Vec::new();
    for i in 1..=8 {
        let req_src = format!("return OrderService::process({});", i * 10);
        requests.push(WorkerRequest::new(req_src).with_id(format!("order_req_{}", i)));
    }

    let responses = pool.execute_batch(requests);
    assert_eq!(responses.len(), 8);

    for (i, resp) in responses.into_iter().enumerate() {
        assert_eq!(resp.status, WorkerResponseStatus::Success);
        let val = resp.to_php_value();
        if let PhpValue::Dict(d) = val {
            let map = d.borrow();
            let expected_id = ((i + 1) as i32) * 10;
            assert_eq!(map.get("order_id"), Some(&PhpValue::Int(expected_id)));
            assert_eq!(map.get("status"), Some(&PhpValue::Int(200)));
        } else {
            panic!("Expected Dict response, got {:?}", val);
        }

        // Verify request ran within a local execution fiber with captured telemetry
        let f_stats = resp.fiber_stats().expect("Response must contain fiber stats");
        assert!(f_stats.is_completed);
        assert!(f_stats.io_count >= 2, "Expected at least 2 I/O events (DB, HTTP), got {}", f_stats.io_count);
        assert!(f_stats.io_breakdown.get("DB").unwrap_or(&0) >= &1);
        assert_eq!(f_stats.io_breakdown.get("HTTP"), Some(&1));
    }
}

#[test]
fn test_fiber_static_facade_guards() {
    // 1. Instantiation forbidden at parse-time / compile-time
    let parse_err = parse("$f = new Fiber();").unwrap_err();
    assert_eq!(
        parse_err.kind,
        ParseErrorKind::StaticFacadeInstantiationForbidden {
            class_name: "Fiber".to_string()
        }
    );

    // 2. Inheritance forbidden at parse-time / compile-time
    let inherit_err = parse("class CustomFiber extends Fiber {}").unwrap_err();
    assert_eq!(
        inherit_err.kind,
        ParseErrorKind::StaticFacadeInheritanceForbidden {
            class_name: "Fiber".to_string()
        }
    );
}

#[test]
fn test_fiber_fault_containment_and_error_state() {
    let mut scheduler = FiberScheduler::new();

    // Fiber 1 throws runtime division by zero
    let f1_id = scheduler.spawn_simple("failing_fiber", || {
        Err(RuntimeError::new(
            RuntimeErrorKind::DivisionByZero,
            rhp::Span::default(),
        ))
    });

    // Fiber 2 succeeds normally
    let f2_id = scheduler.spawn_simple("healthy_fiber", || {
        Ok(PhpValue::string("All good"))
    });

    let results = scheduler.run_all();
    assert_eq!(results.len(), 2);

    let f1_res = results.get(&f1_id).unwrap();
    assert!(f1_res.is_err(), "Fiber 1 must fail");
    assert_eq!(f1_res.as_ref().unwrap_err().kind, RuntimeErrorKind::DivisionByZero);

    let f2_res = results.get(&f2_id).unwrap();
    assert!(f2_res.is_ok(), "Fiber 2 must succeed despite Fiber 1 failure");
    assert_eq!(f2_res.as_ref().unwrap(), &PhpValue::string("All good"));

    let f1 = scheduler.get_fiber(f1_id).unwrap();
    assert!(f1.state.is_failed());
    let f2 = scheduler.get_fiber(f2_id).unwrap();
    assert!(f2.state.is_completed());

    let stats = scheduler.stats();
    assert_eq!(stats.total_completed, 1);
    assert_eq!(stats.total_failed, 1);
}

#[test]
fn test_future_delayed_timer_fiber_yielding() {
    let source = r#"
        $res = Future::delayed(10, fn() => "delayed_success")->wait();
        return $res;
    "#;

    let (result, stats) = run_source_in_fiber(source, "timer_fiber")
        .expect("Future::delayed inside fiber should succeed");

    assert_eq!(result, PhpValue::string("delayed_success"));
    assert_eq!(stats.name, "timer_fiber");
    assert!(stats.yield_count >= 1);
    assert_eq!(stats.io_breakdown.get("Timer"), Some(&1));
}
