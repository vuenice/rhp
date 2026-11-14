use std::sync::Arc;
use std::thread;

use rhp::{
    parse, run_source, run_source_with_vm, ExecutionError, ParentProcess, PhpValue,
    RuntimeErrorKind, TypeErrorKind,
};

#[test]
fn test_shared_keyword_parsing_and_ast() {
    let src = r#"
        shared class DatabasePool {
            public static string $driver = "pgsql";
            private int $maxConnections = 50;

            public function getLimit(): int {
                return $this->maxConnections;
            }

            public static function getDriver(): string {
                return self::$driver;
            }
        }
    "#;

    let program = parse(src).expect("Source with shared class should parse successfully");
    assert_eq!(program.statements.len(), 1);

    if let rhp::StmtKind::ClassDecl(ref cls) = program.statements[0].kind {
        assert_eq!(cls.name.name, "DatabasePool");
        assert!(cls.modifiers.is_shared, "is_shared modifier should be true");
        assert!(!cls.modifiers.is_static_only);
    } else {
        panic!("Expected Class statement");
    }
}

#[test]
fn test_shared_attribute_parsing_and_ast() {
    let src = r#"
        #[Shared]
        class CacheService {
            public static function ping(): string {
                return "pong";
            }
        }
    "#;

    let program = parse(src).expect("Source with #[Shared] attribute should parse successfully");
    if let rhp::StmtKind::ClassDecl(ref cls) = program.statements[0].kind {
        assert_eq!(cls.name.name, "CacheService");
        assert!(cls.modifiers.is_shared, "is_shared modifier should be true from attribute");
    } else {
        panic!("Expected Class statement");
    }
}

#[test]
fn test_instance_methods_of_shared_class_allowed_everywhere() {
    // Instance methods on shared classes are essential for routers, DB managers, config repos.
    // They are completely allowed to be called from non-shared classes!
    let src = r#"
        shared class Router {
            public function dispatch(string $path): string {
                return "route:" + $path;
            }
        }

        class HttpController {
            public function handle(Router $router): string {
                // Instance method call on shared class is ALLOWED
                return $router->dispatch("/home");
            }
        }
    "#;

    let res = rhp::type_system::parse_and_check(src);
    assert!(res.is_ok(), "Calling instance methods of shared class must succeed: {:?}", res.err());
}

#[test]
fn test_static_methods_of_shared_class_inside_non_shared_class() {
    // Non-shared classes can use static methods of a shared class!
    let src = r#"
        shared class MetricsRegistry {
            public readonly static int $counter = 100;

            public static function getCount(): int {
                return self::$counter;
            }

            public function nonStaticMethod(): string {
                return "secret-internal";
            }
        }

        class HttpRequestHandler {
            public function handle(): int {
                // Non-shared class calling static method of shared class -> ALLOWED!
                return MetricsRegistry::getCount() + 5;
            }
        }

        $handler = new HttpRequestHandler();
        $ans = $handler->handle();
    "#;

    let (vm, _) = run_source_with_vm(src)
        .expect("Static methods of shared class inside non-shared class must succeed");
    assert_eq!(vm.get_main_variable("$ans"), Some(&PhpValue::Int(105)));
}

#[test]
fn test_shared_instantiation_forbidden_unconditionally_type_check() {
    // SharedInstantiationForbidden is unconditional: direct `new SharedClass()` is forbidden!
    let src = r#"
        shared class CoreKernel {
            public function tick(): int {
                return 1;
            }
        }

        class NormalClass {
            public function tryInstantiate(): CoreKernel {
                return new CoreKernel();
            }
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Instantiating shared class must fail type check");

    match err {
        rhp::FrontendError::Type(type_err) => {
            let msg = type_err.to_string();
            match type_err.kind {
                TypeErrorKind::SharedInstantiationForbidden {
                    shared_class,
                } => {
                    assert_eq!(shared_class, "CoreKernel");
                    assert!(msg.contains("shared instances are created by the container at boot") || msg.contains("Shared instances are created by the container at boot"), "Got: {}", msg);
                }
                other => panic!("Expected SharedInstantiationForbidden, got: {:?}", other),
            }
        }
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_shared_instantiation_forbidden_unconditionally_runtime() {
    // Dynamic instantiation of shared class must fail at runtime!
    let src = r#"
        shared class CoreKernel {
            public function tick(): int {
                return 1;
            }
        }

        $cls = "CoreKernel";
        $kernel = new $cls();
    "#;

    let err = run_source(src)
        .expect_err("Dynamic instantiation of shared class must fail at runtime");

    match err {
        ExecutionError::Runtime(rt_err) => {
            let msg = rt_err.to_string();
            match rt_err.kind {
                RuntimeErrorKind::SharedInstantiationForbidden {
                    shared_class,
                } => {
                    assert_eq!(shared_class, "CoreKernel");
                    assert!(msg.contains("shared instances are created by the container at boot") || msg.contains("Shared instances are created by the container at boot"), "Got: {}", msg);
                }
                other => panic!("Expected SharedInstantiationForbidden, got: {:?}", other),
            }
        }
        other => panic!("Expected runtime error, got: {:?}", other),
    }
}

#[test]
fn test_non_shared_extends_shared_forbidden() {
    let src = r#"
        shared class BasePool {}
        class ChildPool extends BasePool {}
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Non-shared extending shared class must fail");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::NonSharedExtendsSharedForbidden {
                class_name,
                parent_name,
            } => {
                assert_eq!(class_name, "ChildPool");
                assert_eq!(parent_name, "BasePool");
            }
            other => panic!("Expected NonSharedExtendsSharedForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_shared_class_cannot_inherit_mutable_properties() {
    let src = r#"
        class BaseRepository {
            public string $table = "users";
        }

        shared class UserRepository extends BaseRepository {
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Shared class extending class with mutable properties must fail");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::SharedClassInheritedMutablePropertyForbidden {
                shared_class,
                parent_class,
                property,
            } => {
                assert_eq!(shared_class, "UserRepository");
                assert_eq!(parent_class, "BaseRepository");
                assert_eq!(property, "table");
            }
            other => panic!("Expected SharedClassInheritedMutablePropertyForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_shared_class_mutable_property_forbidden() {
    let src = r#"
        shared class CounterStore {
            public int $count = 0;
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Shared class declaring mutable property must fail");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::SharedClassMutablePropertyForbidden {
                shared_class,
                property,
            } => {
                assert_eq!(shared_class, "CounterStore");
                assert_eq!(property, "count");
            }
            other => panic!("Expected SharedClassMutablePropertyForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_shared_class_scoped_service_injection_forbidden() {
    let src = r#"
        class Request {}

        shared class RequestTracker {
            public function __construct(public readonly Request $request) {}
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Shared class injecting Request must fail");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::SharedClassCapturesScopedService {
                shared_class,
                param,
                service_type,
            } => {
                assert_eq!(shared_class, "RequestTracker");
                assert_eq!(param, "request");
                assert_eq!(service_type, "Request");
            }
            other => panic!("Expected SharedClassCapturesScopedService, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_shared_class_non_static_closure_forbidden() {
    let src = r#"
        shared class HookManager {
            public readonly \Closure $hook = function() { return 42; };
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Shared class with non-static closure default must fail");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::SharedFieldClosureCaptureForbidden {
                shared_class,
                property,
            } => {
                assert_eq!(shared_class, "HookManager");
                assert_eq!(property, "hook");
            }
            other => panic!("Expected SharedFieldClosureCaptureForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_shared_class_mutable_static_forbidden() {
    let src = r#"
        shared class StateHolder {
            public static int $badState = 0;
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Shared class with mutable static property must fail");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::SharedClassMutableStaticForbidden {
                shared_class,
                property,
            } => {
                assert_eq!(shared_class, "StateHolder");
                assert_eq!(property, "badState");
            }
            other => panic!("Expected SharedClassMutableStaticForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_parent_process_and_shared_isolation() {
    let parent_code = r#"
        shared class GlobalCounter {
            public readonly static int $base = 1000;

            public static function getBase(): int {
                return self::$base;
            }
        }

        $boot_timestamp = 1700000000;
    "#;

    let state = ParentProcess::boot(parent_code).expect("ParentProcess boot should succeed");

    assert!(state.is_shared_class("GlobalCounter"));
    assert!(!state.is_shared_class("NonExistentClass"));

    let req1_code = r#"
        class RequestHandler1 {
            public function process(): int {
                return GlobalCounter::getBase() + 42;
            }
        }
        $handler = new RequestHandler1();
        return $handler->process();
    "#;
    let res1 = state
        .execute_request(req1_code)
        .expect("Request 1 execution should succeed");
    assert_eq!(res1, PhpValue::Int(1042));

    let req2_code = r#"
        class RequestHandler2 {
            public function process(): int {
                return GlobalCounter::getBase() + 99;
            }
        }
        $handler = new RequestHandler2();
        return $handler->process();
    "#;
    let res2 = state
        .execute_request(req2_code)
        .expect("Request 2 execution should succeed");
    assert_eq!(res2, PhpValue::Int(1099));
}

#[test]
fn test_parent_process_concurrent_workers_reading_shared() {
    let parent_code = r#"
        shared class SharedCache {
            public readonly static int $hits = 500;

            public static function getHits(): int {
                return self::$hits;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("ParentProcess boot should succeed");

    let mut handles = Vec::new();
    for i in 0..8 {
        let state_clone = Arc::clone(&state);
        let handle = thread::spawn(move || {
            let req_code = format!(
                r#"
                class WorkerTask_{} {{
                    public function compute(int $idx): int {{
                        return SharedCache::getHits() + $idx;
                    }}
                }}
                $worker = new WorkerTask_{}();
                return $worker->compute({});
                "#,
                i, i, i
            );
            let val = state_clone
                .execute_request(&req_code)
                .expect("Worker thread request execution should succeed");
            val.to_i32().expect("Result should be i32")
        });
        handles.push(handle);
    }

    for (i, h) in handles.into_iter().enumerate() {
        let res = h.join().expect("Worker thread should complete successfully");
        assert_eq!(res, 500 + i as i32);
    }
}

#[test]
fn test_worker_extending_shared_rejected_and_calling_instance_method_allowed() {
    let parent_code = r#"
        shared class SecureVault {
            public function decryptKey(): string {
                return "secret-key-material";
            }

            public static function status(): string {
                return "ready";
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("ParentProcess boot should succeed");

    // 1. Request with non-shared class extending shared class must fail with NonSharedExtendsSharedForbidden
    let request_code = r#"
        class EvilController extends SecureVault {
        }
    "#;

    let err = state
        .execute_request(request_code)
        .expect_err("Non-shared class extending shared class must fail");

    match err {
        ExecutionError::Compilation(rhp::CompilationError::Type(type_err)) => {
            match type_err.kind {
                TypeErrorKind::NonSharedExtendsSharedForbidden {
                    class_name,
                    parent_name,
                } => {
                    assert_eq!(class_name, "EvilController");
                    assert_eq!(parent_name, "SecureVault");
                }
                other => panic!("Expected NonSharedExtendsSharedForbidden, got: {:?}", other),
            }
        }
        other => panic!("Expected compilation type error, got: {:?}", other),
    }

    // 2. Request with non-shared class calling instance method on shared class -> ALLOWED!
    let valid_request_code = r#"
        class GoodController {
            public function run(SecureVault $vault): string {
                return $vault->decryptKey();
            }
        }
    "#;
    assert!(state.execute_request(valid_request_code).is_ok());
}

#[test]
fn test_boot_phase_db_and_future_calls_forbidden() {
    // 1. Top-level DB::table call in boot phase must fail immediately
    let db_boot_code = r#"
        $users = DB::table('users')->all();
    "#;
    let err = ParentProcess::boot(db_boot_code)
        .expect_err("Calling DB::table in boot phase must fail");
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("BootPhaseViolation"),
        "Expected BootPhaseViolation for DB::table, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("All boot-time execution must remain strictly single-threaded"),
        "Expected single-threaded guidance, got: {}",
        err_msg
    );

    // 2. Top-level Future::run call in boot phase must fail immediately
    let future_boot_code = r#"
        $f = Future::run(fn() => 42);
    "#;
    let err2 = ParentProcess::boot(future_boot_code)
        .expect_err("Calling Future::run in boot phase must fail");
    let err2_msg = err2.to_string();
    assert!(
        err2_msg.contains("BootPhaseViolation"),
        "Expected BootPhaseViolation for Future::run, got: {}",
        err2_msg
    );
}

#[test]
fn test_shared_class_arc_rwlock_concurrency_and_zero_copy() {
    let src = r#"
        shared class AppConfig {
            public readonly string $appName;
            public readonly int $version;

            public function __construct(string $name, int $ver) {
                $this->appName = $name;
                $this->version = $ver;
            }

            public function getAppName(): string {
                return $this->appName;
            }

            public function getVersion(): int {
                return $this->version;
            }
        }

        class NormalRequestState {
            public string $requestId = "req-1";
        }
    "#;

    let (mut vm, _) = run_source_with_vm(src).expect("Booting VM should succeed");

    // 1. Normal class MUST be represented by single-threaded Rc<RefCell<VmObject>>
    let normal_inst = vm
        .instantiate_and_construct("NormalRequestState", vec![], rhp::Span::default())
        .expect("Normal class instantiation should succeed");
    assert!(
        matches!(normal_inst, PhpValue::Object(_)),
        "Normal class must be represented by Rc<RefCell<VmObject>>"
    );

    // 2. Shared class MUST be represented by Arc<VmSharedObject>
    let shared_inst = vm
        .instantiate_and_construct(
            "AppConfig",
            vec![PhpValue::string("RhpCore"), PhpValue::Int(3)],
            rhp::Span::default(),
        )
        .expect("Shared class instantiation should succeed");

    let shared_arc = match &shared_inst {
        PhpValue::SharedObject(arc) => {
            assert_eq!(arc.class_name, "AppConfig");
            assert_eq!(arc.get_property("appName"), Some(PhpValue::string("RhpCore")));
            assert_eq!(arc.get_property("version"), Some(PhpValue::Int(3)));
            assert!(arc.is_readonly("appName"));
            assert!(arc.is_readonly("version"));
            Arc::clone(arc)
        }
        other => panic!("Expected PhpValue::SharedObject, got: {:?}", other),
    };

    // 3. Compile-time Send + Sync check
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<rhp::VmSharedObject>();
    assert_send_sync::<rhp::SharedValue>();
    assert_send_sync::<Arc<rhp::VmSharedObject>>();

    // 4. Multi-threaded concurrency: 16 threads concurrently dispatching instance methods
    let mut handles = Vec::new();
    for thread_idx in 0..16 {
        let arc_clone = Arc::clone(&shared_arc);
        let handle = thread::spawn(move || {
            let shared_val = PhpValue::SharedObject(arc_clone);
            let worker_src = r##"
                shared class AppConfig {
                    public readonly string $appName;
                    public readonly int $version;

                    public function getAppName(): string {
                        return $this->appName;
                    }

                    public function getVersion(): int {
                        return $this->version;
                    }
                }

                class RequestWorker {
                    public function describe(AppConfig $cfg, string $id): string {
                        return $cfg->getAppName() + "#" + string($cfg->getVersion()) + "@" + $id;
                    }
                }
            "##;
            let (mut worker_vm, _) = run_source_with_vm(worker_src).expect("Worker VM should boot");
            let worker = worker_vm
                .instantiate_and_construct("RequestWorker", vec![], rhp::Span::default())
                .expect("Worker instantiation should succeed");

            let res = worker_vm
                .invoke_method_sync(
                    worker,
                    "describe",
                    vec![shared_val.clone(), PhpValue::string(thread_idx.to_string())],
                    rhp::Span::default(),
                )
                .expect("Invoking describe on worker should succeed");

            assert_eq!(
                res,
                PhpValue::string(format!("RhpCore#3@{}", thread_idx))
            );
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Worker thread panicked");
    }
}

#[test]
fn test_shared_atomic_multi_threaded_concurrency() {
    use std::sync::atomic::{AtomicI64, Ordering};

    let atomic = Arc::new(AtomicI64::new(0));
    let shared_atomic = PhpValue::SharedAtomic(Arc::clone(&atomic));

    let mut handles = Vec::new();
    for _ in 0..16 {
        let atomic_ref = Arc::clone(&atomic);
        let handle = thread::spawn(move || {
            for _ in 0..1000 {
                atomic_ref.fetch_add(1, Ordering::SeqCst);
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(atomic.load(Ordering::SeqCst), 16000);
    assert_eq!(shared_atomic.to_i64(), Some(16000));
}

#[test]
fn test_parent_process_zero_copy_shared_object_arc() {
    use rhp::{FrozenValue, VmSharedObject};

    let shared_obj = VmSharedObject::new("SharedConfig");
    let _ = shared_obj.set_property("env".to_string(), PhpValue::string("production"));
    shared_obj.set_readonly("env".to_string(), true);
    let original_arc = Arc::new(shared_obj);
    let php_val = PhpValue::SharedObject(Arc::clone(&original_arc));

    // Freeze into FrozenValue
    let frozen = FrozenValue::from_php_value(&php_val);
    let frozen_arc = match &frozen {
        FrozenValue::SharedObject(arc) => Arc::clone(arc),
        other => panic!("Expected FrozenValue::SharedObject, got {:?}", other),
    };

    // Both point to the exact same pointer address (zero-copy!)
    assert!(
        Arc::ptr_eq(&original_arc, &frozen_arc),
        "FrozenValue::SharedObject must retain the exact same Arc allocation without copying"
    );

    // Unfreeze back to PhpValue
    let thawed = frozen.to_php_value();
    let thawed_arc = match &thawed {
        PhpValue::SharedObject(arc) => Arc::clone(arc),
        other => panic!("Expected PhpValue::SharedObject, got {:?}", other),
    };

    assert!(
        Arc::ptr_eq(&original_arc, &thawed_arc),
        "Thawed PhpValue::SharedObject must retain the exact same Arc allocation (zero-copy!)"
    );
    assert_eq!(thawed_arc.get_property("env"), Some(PhpValue::string("production")));
}

struct MockDbPool {
    connection_string: String,
    max_connections: u32,
}

#[test]
fn test_shared_class_native_driver_pool_attachment() {
    use rhp::{FrozenValue, VmSharedObject};

    let pool = Arc::new(MockDbPool {
        connection_string: "postgres://cluster.local:5432/rhp_prod".to_string(),
        max_connections: 50,
    });

    let shared_service = VmSharedObject::new("DatabasePoolService");
    shared_service.set_native_resource("pool", pool);

    let shared_arc = Arc::new(shared_service);
    let php_val = PhpValue::SharedObject(Arc::clone(&shared_arc));

    // Freeze into Parent Process state
    let frozen = FrozenValue::from_php_value(&php_val);
    let thawed = frozen.to_php_value();

    if let PhpValue::SharedObject(so) = thawed {
        let fetched_pool = so
            .get_native_resource::<MockDbPool>("pool")
            .expect("Native pool should be retrievable from shared class");
        assert_eq!(fetched_pool.connection_string, "postgres://cluster.local:5432/rhp_prod");
        assert_eq!(fetched_pool.max_connections, 50);
    } else {
        panic!("Expected PhpValue::SharedObject");
    }
}

#[test]
fn test_request_opaque_handle_isolation_across_requests() {
    use rhp::pkg::{get_handle_registry, with_request_handle_registry};

    // Request 1: inserts an opaque handle into the request registry
    let h1 = with_request_handle_registry(|reg| {
        reg.insert("SessionCursor", "cursor_req_1".to_string())
    });
    assert!(with_request_handle_registry(|reg| reg.has(h1, "SessionCursor")));

    // Global registry handle for Request 1
    let g1 = get_handle_registry().insert("GlobalCursor", "cursor_req_1".to_string());
    assert!(get_handle_registry().has(g1, "GlobalCursor"));

    // Simulate Request 1 completion / tear-down
    rhp::clear_request_context();

    // Request 2 runs: must NOT see handles from Request 1
    assert!(!with_request_handle_registry(|reg| reg.has(h1, "SessionCursor")));
    assert_eq!(with_request_handle_registry(|reg| reg.count()), 0);

    assert!(!get_handle_registry().has(g1, "GlobalCursor"));
    assert_eq!(get_handle_registry().count(), 0);
}


