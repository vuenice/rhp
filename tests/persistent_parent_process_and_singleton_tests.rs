use std::sync::Arc;
use std::thread;

use rhp::{
    parse, run_source, run_source_with_vm, EngineState, ExecutionError, ParentProcess, PhpValue,
    RuntimeErrorKind, TypeErrorKind,
};

#[test]
fn test_singleton_keyword_parsing_and_ast() {
    let src = r#"
        singleton class DatabasePool {
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

    let program = parse(src).expect("Source with singleton class should parse successfully");
    assert_eq!(program.statements.len(), 1);

    if let rhp::StmtKind::ClassDecl(ref cls) = program.statements[0].kind {
        assert_eq!(cls.name.name, "DatabasePool");
        assert!(cls.modifiers.is_singleton, "is_singleton modifier should be true");
        assert!(!cls.modifiers.is_static_only);
    } else {
        panic!("Expected Class statement");
    }
}

#[test]
fn test_singleton_attribute_parsing_and_ast() {
    let src = r#"
        #[Singleton]
        class CacheService {
            public static function ping(): string {
                return "pong";
            }
        }
    "#;

    let program = parse(src).expect("Source with #[Singleton] attribute should parse successfully");
    if let rhp::StmtKind::ClassDecl(ref cls) = program.statements[0].kind {
        assert_eq!(cls.name.name, "CacheService");
        assert!(cls.modifiers.is_singleton, "is_singleton modifier should be true from attribute");
    } else {
        panic!("Expected Class statement");
    }
}

#[test]
fn test_singleton_methods_within_other_singleton_classes() {
    // Both static and non-static methods of a singleton class can be used within another singleton class!
    let src = r#"
        singleton class ConfigManager {
            private string $env = "production";

            public function getEnv(): string {
                return $this->env;
            }

            public static function version(): string {
                return "1.0.0";
            }
        }

        singleton class AppKernel {
            public function bootAndInspect(ConfigManager $config): string {
                // Calling non-static method of ConfigManager inside AppKernel (both singleton) -> ALLOWED!
                $envName = $config->getEnv();

                // Calling static method of ConfigManager inside AppKernel -> ALLOWED!
                $ver = ConfigManager::version();

                return $envName . "-v" . $ver;
            }

            public function instantiateAndCall(): string {
                // Instantiating another singleton class within a singleton class -> ALLOWED!
                $cfg = new ConfigManager();
                return $cfg->getEnv();
            }
        }

        $cfg = new ConfigManager();
        $kernel = new AppKernel();
        $result = $kernel->bootAndInspect($cfg);
        $selfInstResult = $kernel->instantiateAndCall();
    "#;

    let (vm, _) = run_source_with_vm(src).expect("Singleton calls within singleton classes must succeed");
    assert_eq!(
        vm.get_main_variable("$result"),
        Some(&PhpValue::string("production-v1.0.0"))
    );
    assert_eq!(
        vm.get_main_variable("$selfInstResult"),
        Some(&PhpValue::string("production"))
    );
}

#[test]
fn test_static_methods_of_singleton_class_inside_non_singleton_class() {
    // Non-singleton classes can use static methods of a singleton class!
    let src = r#"
        singleton class MetricsRegistry {
            public static int $counter = 100;

            public static function getCount(): int {
                return self::$counter;
            }

            public function nonStaticMethod(): string {
                return "secret-internal";
            }
        }

        class HttpRequestHandler {
            public function handle(): int {
                // Non-singleton class calling static method of singleton class -> ALLOWED!
                return MetricsRegistry::getCount() + 5;
            }
        }

        $handler = new HttpRequestHandler();
        $ans = $handler->handle();
    "#;

    let (vm, _) = run_source_with_vm(src)
        .expect("Static methods of singleton class inside non-singleton class must succeed");
    assert_eq!(vm.get_main_variable("$ans"), Some(&PhpValue::Int(105)));
}

#[test]
fn test_non_static_methods_of_singleton_forbidden_in_non_singleton_class_type_check() {
    // Calling a non-static method of a singleton class inside a non-singleton class must fail at type check!
    let src = r#"
        singleton class ProcessService {
            public function executeTask(): string {
                return "task-done";
            }

            public static function ping(): string {
                return "pong";
            }
        }

        class UserWorker {
            public function run(ProcessService $svc): string {
                // Illegal: calling non-static method of singleton class inside non-singleton class!
                return $svc->executeTask();
            }
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Calling non-static method of singleton class from non-singleton class must fail type check");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::NonStaticSingletonMethodCallForbidden {
                singleton_class,
                method,
                caller_class,
            } => {
                assert_eq!(singleton_class, "ProcessService");
                assert_eq!(method, "executeTask");
                assert_eq!(caller_class, "UserWorker");
            }
            other => panic!("Expected NonStaticSingletonMethodCallForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_non_static_methods_of_singleton_forbidden_in_non_singleton_class_runtime() {
    // If dynamically dispatched without type hint, runtime VM must halt with fatal error!
    let src = r#"
        singleton class AuthTokenManager {
            public function issueToken(): string {
                return "jwt-token-123";
            }
        }

        class InsecureClient {
            public function attack(mixed $untypedManager): string {
                // Untyped dynamic call inside non-singleton class
                return $untypedManager->issueToken();
            }
        }

        $auth = new AuthTokenManager();
        $client = new InsecureClient();
        $client->attack($auth);
    "#;

    let err = run_source(src)
        .expect_err("Dynamic non-static call to singleton inside non-singleton class must fail at runtime");

    match err {
        ExecutionError::Runtime(rt_err) => match rt_err.kind {
            RuntimeErrorKind::NonStaticSingletonMethodCallForbidden {
                singleton_class,
                method,
                caller_class,
            } => {
                assert_eq!(singleton_class, "AuthTokenManager");
                assert_eq!(method, "issueToken");
                assert_eq!(caller_class, "InsecureClient");
            }
            other => panic!("Expected NonStaticSingletonMethodCallForbidden, got: {:?}", other),
        },
        other => panic!("Expected runtime error, got: {:?}", other),
    }
}

#[test]
fn test_singleton_instantiation_forbidden_in_non_singleton_class() {
    // Instantiating a singleton class inside a non-singleton class must fail!
    let src = r#"
        singleton class CoreKernel {
            public function tick(): int {
                return 1;
            }
        }

        class NormalClass {
            public function tryInstantiate(): CoreKernel {
                // Instantiating a singleton inside a non-singleton class is forbidden!
                return new CoreKernel();
            }
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Instantiating singleton in non-singleton class must fail type check");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::SingletonInstantiationForbidden {
                singleton_class,
                caller_class,
            } => {
                assert_eq!(singleton_class, "CoreKernel");
                assert_eq!(caller_class, "NormalClass");
            }
            other => panic!("Expected SingletonInstantiationForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_non_singleton_extending_singleton_class_forbidden() {
    // A non-singleton class cannot extend a singleton class!
    let src = r#"
        singleton class BaseSingletonService {
            public function internalWork(): string {
                return "work";
            }
        }

        class SubService extends BaseSingletonService {
            public function hijack(): string {
                return $this->internalWork();
            }
        }
    "#;

    let err = rhp::type_system::parse_and_check(src)
        .expect_err("Non-singleton extending singleton must fail type check");

    match err {
        rhp::FrontendError::Type(type_err) => match type_err.kind {
            TypeErrorKind::NonSingletonExtendsSingletonForbidden {
                class_name,
                parent_name,
            } => {
                assert_eq!(class_name, "SubService");
                assert_eq!(parent_name, "BaseSingletonService");
            }
            other => panic!("Expected NonSingletonExtendsSingletonForbidden, got: {:?}", other),
        },
        other => panic!("Expected Type error, got: {:?}", other),
    }
}

#[test]
fn test_persistent_parent_process_boot_and_immutable_engine_state() {
    let framework_code = r#"
        singleton class DatabasePool {
            public static string $url = "postgres://cluster.internal:5432/main";
            public static int $poolSize = 32;

            public static function getConnectionString(): string {
                return self::$url;
            }

            public static function getPoolSize(): int {
                return self::$poolSize;
            }
        }

        singleton class FrameworkConfig {
            public static string $appName = "RHP-Enterprise";

            public static function appName(): string {
                return self::$appName;
            }
        }

        $bootLog = "Bootstrapped " . FrameworkConfig::appName() . " on " . DatabasePool::getConnectionString();
    "#;

    // Boot framework inside persistent parent process into immutable Arc<EngineState>
    let state: Arc<EngineState> = ParentProcess::boot(framework_code)
        .expect("Parent process boot into Arc<EngineState> should succeed");

    assert!(state.is_singleton_class("DatabasePool"));
    assert!(state.is_singleton_class("FrameworkConfig"));
    assert_eq!(state.singleton_classes.len(), 2);

    // Verify parent process pre-compiled state without per-request bootstrap overhead
    let (worker_vm, result) = state
        .execute_request_with_vm(r#"
            class RequestHandler {
                public function handle(): string {
                    // Uses static methods of singletons pre-compiled in Arc<EngineState>
                    $app = FrameworkConfig::appName();
                    $db = DatabasePool::getConnectionString();
                    return $app . " -> " . $db;
                }
            }

            $h = new RequestHandler();
            $response = $h->handle();
            return $response;
        "#)
        .expect("Worker request execution using pre-compiled parent state should succeed");

    assert_eq!(
        result,
        PhpValue::string("RHP-Enterprise -> postgres://cluster.internal:5432/main")
    );
    assert_eq!(
        worker_vm.get_main_variable("$response"),
        Some(&PhpValue::string("RHP-Enterprise -> postgres://cluster.internal:5432/main"))
    );
}

#[test]
fn test_concurrent_worker_requests_sharing_parent_engine_state() {
    let parent_code = r#"
        singleton class SharedCache {
            public static int $hits = 500;

            public static function getHits(): int {
                return self::$hits;
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code)
        .expect("ParentProcess boot should succeed");

    // Spawn 8 concurrent threads simulating Tokio worker tasks / request fibers
    // All threads share the immutable Arc<EngineState> with ZERO bootstrap overhead
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
fn test_worker_calling_non_static_singleton_method_rejected_at_request_time() {
    let parent_code = r#"
        singleton class SecureVault {
            public function decryptKey(): string {
                return "secret-key-material";
            }

            public static function status(): string {
                return "ready";
            }
        }
    "#;

    let state = ParentProcess::boot(parent_code).expect("ParentProcess boot should succeed");

    // Request with non-singleton class attempting to call non-static method
    let request_code = r#"
        class EvilController {
            public function steal(SecureVault $vault): string {
                return $vault->decryptKey();
            }
        }
    "#;

    let err = state
        .execute_request(request_code)
        .expect_err("Request attempting to call non-static singleton method from non-singleton must fail");

    match err {
        ExecutionError::Compilation(rhp::CompilationError::Type(type_err)) => {
            match type_err.kind {
                TypeErrorKind::NonStaticSingletonMethodCallForbidden {
                    singleton_class,
                    method,
                    caller_class,
                } => {
                    assert_eq!(singleton_class, "SecureVault");
                    assert_eq!(method, "decryptKey");
                    assert_eq!(caller_class, "EvilController");
                }
                other => panic!("Expected NonStaticSingletonMethodCallForbidden, got: {:?}", other),
            }
        }
        other => panic!("Expected compilation type error, got: {:?}", other),
    }
}
