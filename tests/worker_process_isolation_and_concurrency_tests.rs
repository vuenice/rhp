//! # Technical Point #14 Integration Tests: Worker Process Isolation & Concurrency
//!
//! Verifies:
//! - Spawning multi-threaded worker tasks and handling incoming requests concurrently against shared parent state.
//! - Process-level copy-on-write model (`WorkerModel::ProcessCow`) utilizing kernel MMU page-table isolation.
//! - Strict Copy-on-Write (CoW) memory isolation: mutations to static properties, globals, and superglobals
//!   do NOT leak across concurrent/subsequent requests or contaminate the parent state.
//! - Fault isolation & panic boundaries: exceptions, runtime errors, or panics in user requests are caught
//!   cleanly and do not kill the worker pool or crash the parent server.
//! - Superglobals injection: `$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE` are accessible inside request VMs.
//! - Direct pre-compiled function calls (`WorkerPayload::Call`) with zero parsing overhead.
//! - Worker pool metrics tracking and graceful shutdown.

use std::time::Duration;

use rhp::{
    deserialize_response, serialize_response, FrozenValue, IndexMap, ParentProcess, PhpValue,
    WorkerError, WorkerModel, WorkerPool, WorkerPoolConfig, WorkerRequest, WorkerResponse,
    WorkerResponseStatus,
};

#[test]
fn test_multi_threaded_worker_pool_concurrency() {
    let parent_code = r#"
        class OrderCalculator {
            public static int $taxRate = 15;

            public static function computeTotal(int $subtotal, int $shipping): int {
                $tax = ($subtotal * self::$taxRate) / 100;
                return $subtotal + $shipping + $tax;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code)
        .expect("ParentProcess boot should succeed");

    // Spawn multi-threaded worker pool with 4 workers
    let pool = state
        .spawn_worker_threads(4)
        .expect("Worker pool creation should succeed");

    assert_eq!(pool.num_workers(), 4);
    assert_eq!(pool.model(), WorkerModel::MultiThreaded);

    // Prepare 16 concurrent requests
    let mut requests = Vec::new();
    for i in 0..16 {
        let req_source = format!(
            r#"
            $subtotal = {};
            $shipping = 10;
            return OrderCalculator::computeTotal($subtotal, $shipping);
            "#,
            i * 100
        );
        requests.push(WorkerRequest::new(req_source).with_id(format!("order_req_{}", i)));
    }

    // Execute all 16 requests in parallel across workers
    let responses = pool.execute_batch(requests);
    assert_eq!(responses.len(), 16);

    for (i, resp) in responses.into_iter().enumerate() {
        assert_eq!(resp.status, WorkerResponseStatus::Success);
        let subtotal = (i as i32) * 100;
        let expected = subtotal + 10 + (subtotal * 15) / 100;
        assert_eq!(
            resp.to_php_value(),
            PhpValue::Int(expected),
            "Failed matching expected order total for request {}",
            i
        );
    }

    let stats = pool.stats();
    assert_eq!(stats.total_requests, 16);
    assert_eq!(stats.successful_requests, 16);
    assert_eq!(stats.failed_requests, 0);
}

#[test]
fn test_copy_on_write_state_isolation_static_properties() {
    let parent_code = r#"
        class ServerConfig {
            public static int $counter = 100;
            public static string $activeZone = "us-east-1";

            public static function mutate(int $delta, string $zone): int {
                self::$counter = self::$counter + $delta;
                self::$activeZone = $zone;
                return self::$counter;
            }

            public static function get(): int {
                return self::$counter;
            }

            public static function getZone(): string {
                return self::$activeZone;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(2).expect("Spawn workers");

    // Request 1: Mutates static properties in its own isolated VM
    let req1 = WorkerRequest::new(
        r#"
        ServerConfig::mutate(50, "eu-central-1");
        echo "mutated;";
        return ServerConfig::get();
        "#,
    )
    .with_id("req_mutate");

    let resp1 = pool.execute(req1);
    assert_eq!(resp1.status, WorkerResponseStatus::Success);
    assert_eq!(resp1.to_php_value(), PhpValue::Int(150));
    assert_eq!(resp1.output(), "mutated;");

    // Request 2: Executes subsequently on worker pool
    // MUST see pristine initial parent value (100 and "us-east-1"), NOT the mutated value!
    let req2 = WorkerRequest::new(
        r#"
        $c = ServerConfig::get();
        $z = ServerConfig::getZone();
        return $z . ":" . $c;
        "#,
    )
    .with_id("req_inspect");

    let resp2 = pool.execute(req2);
    assert_eq!(resp2.status, WorkerResponseStatus::Success);
    assert_eq!(
        resp2.to_php_value(),
        PhpValue::string("us-east-1:100"),
        "State from request 1 must NOT leak into request 2!"
    );

    // Verify parent EngineState itself is completely pristine
    let parent_counter = state
        .static_properties
        .get(&("ServerConfig".to_string(), "counter".to_string()))
        .expect("Parent static property counter");
    assert_eq!(parent_counter, &FrozenValue::Int(100));

    let parent_zone = state
        .static_properties
        .get(&("ServerConfig".to_string(), "activeZone".to_string()))
        .expect("Parent static property activeZone");
    assert_eq!(parent_zone, &FrozenValue::String("us-east-1".to_string()));
}

#[test]
fn test_superglobals_injection_and_request_isolation() {
    let parent_code = r#"
        class RequestInspector {
            public static function handle(): string {
                global $_GET, $_POST, $_SERVER;
                $user = $_GET["user"];
                $action = $_POST["action"];
                $method = $_SERVER["REQUEST_METHOD"];
                return $method . " -> " . $user . " performs " . $action;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(2).expect("Pool spawn");

    let req = WorkerRequest::new("return RequestInspector::handle();")
        .with_id("req_superglobals")
        .with_get("user", "alice")
        .with_post("action", "checkout")
        .with_server("REQUEST_METHOD", "POST")
        .with_server("REQUEST_URI", "/api/v1/checkout");

    let resp = pool.execute(req);
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(
        resp.to_php_value(),
        PhpValue::string("POST -> alice performs checkout")
    );

    // Next request with different params is isolated
    let req2 = WorkerRequest::new(
        r#"
        global $_GET;
        return $_GET["user"];
        "#,
    )
    .with_get("user", "bob");

    let resp2 = pool.execute(req2);
    assert_eq!(resp2.status, WorkerResponseStatus::Success);
    assert_eq!(resp2.to_php_value(), PhpValue::string("bob"));
}

#[test]
fn test_fault_isolation_and_panic_boundary() {
    let parent_code = r#"
        class SafeService {
            public static function ping(): string {
                return "pong";
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(2).expect("Spawn worker pool");

    // 1. Send a request that causes a runtime DivisionByZero
    let bad_req = WorkerRequest::new(
        r#"
        $divisor = 0;
        return 100 / $divisor;
        "#,
    )
    .with_id("req_div_zero");

    let bad_resp = pool.execute(bad_req);
    assert_eq!(bad_resp.status, WorkerResponseStatus::RuntimeError);
    assert!(bad_resp.error().is_some());
    assert!(bad_resp.error().unwrap().contains("Division"));

    // 2. Send a request that has a syntax/compilation error
    let syntax_req = WorkerRequest::new("this is not valid rhp syntax {{{")
        .with_id("req_syntax_error");
    let syntax_resp = pool.execute(syntax_req);
    assert_eq!(syntax_resp.status, WorkerResponseStatus::CompilationError);
    assert!(syntax_resp.error().is_some());

    // 3. Crucial verification: The worker pool is NOT dead!
    // It immediately and safely handles subsequent valid requests without degraded state.
    let good_req = WorkerRequest::new("return SafeService::ping();")
        .with_id("req_recover");
    let good_resp = pool.execute(good_req);
    assert_eq!(good_resp.status, WorkerResponseStatus::Success);
    assert_eq!(good_resp.to_php_value(), PhpValue::string("pong"));

    let stats = pool.stats();
    assert_eq!(stats.total_requests, 3);
    assert_eq!(stats.successful_requests, 1);
    assert_eq!(stats.failed_requests, 2);
}

#[test]
fn test_direct_precompiled_function_call_concurrency() {
    let parent_code = r#"
        function calculateHash(int $id, string $salt): string {
            return "hash:" . $id . "-" . $salt;
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let pool = state.spawn_worker_threads(4).expect("Spawn pool");

    // Dispatch direct function calls with ZERO parsing overhead
    let mut requests = Vec::new();
    for i in 0..8 {
        requests.push(
            WorkerRequest::call("calculateHash", vec![FrozenValue::Int(i), FrozenValue::String("sec".to_string())])
                .with_id(format!("call_{}", i)),
        );
    }

    let responses = pool.execute_batch(requests);
    assert_eq!(responses.len(), 8);

    for (i, resp) in responses.into_iter().enumerate() {
        assert_eq!(resp.status, WorkerResponseStatus::Success);
        assert_eq!(
            resp.to_php_value(),
            PhpValue::string(format!("hash:{}-sec", i))
        );
    }
}

#[test]
fn test_process_cow_worker_model() {
    let parent_code = r#"
        class HeavyFramework {
            public static int $stateVersion = 42;

            public static function executeJob(int $id): string {
                self::$stateVersion = self::$stateVersion + $id;
                echo "job_executed;";
                return "res_" . self::$stateVersion;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");

    // Spawn Process-Level Copy-on-Write worker pool
    let config = WorkerPoolConfig::new(2).with_model(WorkerModel::ProcessCow);
    let pool = state.spawn_worker_pool(config).expect("Spawn ProcessCow pool");

    assert_eq!(pool.model(), WorkerModel::ProcessCow);

    // Request 1 under ProcessCow
    let req1 = WorkerRequest::new("return HeavyFramework::executeJob(10);")
        .with_id("cow_req_1");
    let resp1 = pool.execute(req1);
    assert_eq!(resp1.status, WorkerResponseStatus::Success);
    assert_eq!(resp1.to_php_value(), PhpValue::string("res_52"));
    assert_eq!(resp1.output(), "job_executed;");

    // Request 2 under ProcessCow: must NOT see stateVersion = 52, must start from 42!
    let req2 = WorkerRequest::new("return HeavyFramework::executeJob(5);")
        .with_id("cow_req_2");
    let resp2 = pool.execute(req2);
    assert_eq!(resp2.status, WorkerResponseStatus::Success);
    assert_eq!(resp2.to_php_value(), PhpValue::string("res_47"));

    // Verify parent memory state was completely isolated and unaltered
    let initial_val = state
        .static_properties
        .get(&("HeavyFramework".to_string(), "stateVersion".to_string()))
        .expect("Parent stateVersion");
    assert_eq!(initial_val, &FrozenValue::Int(42));
}

#[test]
fn test_ipc_binary_serialization_round_trip() {
    let mut dict = IndexMap::new();
    dict.insert("route".to_string(), PhpValue::string("users.show"));
    dict.insert("id".to_string(), PhpValue::Int(101));

    let sample_resp = WorkerResponse {
        request_id: "test_ipc_123".to_string(),
        status: WorkerResponseStatus::Success,
        value: FrozenValue::Dict(vec![
            ("route".to_string(), FrozenValue::String("users.show".to_string())),
            ("id".to_string(), FrozenValue::Int(101)),
        ]),
        output: "Hello from isolated worker\n".to_string(),
        duration: Duration::from_micros(450),
        error: None,
        worker_id: 3,
        fiber_stats: None,
        arena_drop: None,
        trace_id: Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string()),
        span_id: Some("00f067aa0ba902b7".to_string()),
    };

    let serialized = serialize_response(&sample_resp);
    assert!(!serialized.is_empty());

    let deserialized = deserialize_response(&serialized).expect("Deserialization must succeed");
    assert_eq!(deserialized.request_id, "test_ipc_123");
    assert_eq!(deserialized.status, WorkerResponseStatus::Success);
    assert_eq!(deserialized.output, "Hello from isolated worker\n");
    assert_eq!(deserialized.duration, Duration::from_micros(450));
    assert_eq!(deserialized.error, None);
    assert_eq!(deserialized.worker_id, 3);
    assert_eq!(deserialized.trace_id(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
    assert_eq!(deserialized.span_id(), Some("00f067aa0ba902b7"));

    // Verify complex rehydrated value
    let php_val = deserialized.to_php_value();
    if let PhpValue::Dict(map) = php_val {
        let b = map.borrow();
        assert_eq!(b.get("route"), Some(&PhpValue::string("users.show")));
        assert_eq!(b.get("id"), Some(&PhpValue::Int(101)));
    } else {
        panic!("Expected Dict PhpValue");
    }
}

#[test]
fn test_worker_pool_builder_and_timeout() {
    let parent_code = r#"
        class QuickService {
            public static function quickWork(): string {
                return "done";
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");

    let pool = WorkerPool::builder()
        .workers(2)
        .model(WorkerModel::MultiThreaded)
        .queue_capacity(64)
        .timeout(Duration::from_millis(1500))
        .build(state)
        .expect("Build worker pool");

    let resp = pool.execute_source("return QuickService::quickWork();");
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.to_php_value(), PhpValue::string("done"));
}

#[test]
fn test_worker_pool_graceful_shutdown() {
    let parent_code = r#"
        class WorkerService {
            public static function work(): int {
                return 42;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("Parent boot");
    let mut pool = state.spawn_worker_threads(2).expect("Spawn pool");

    let resp = pool.execute_source("return WorkerService::work();");
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.to_php_value(), PhpValue::Int(42));

    // Shutdown pool
    pool.shutdown().expect("Shutdown should succeed");

    // Subsequent dispatch must fail with PoolShutdown
    let after_shutdown_res = pool.dispatch(WorkerRequest::new("return 1;"));
    assert_eq!(after_shutdown_res.unwrap_err(), WorkerError::PoolShutdown);
}
