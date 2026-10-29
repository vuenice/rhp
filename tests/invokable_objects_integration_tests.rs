use rhp::{run_source, ExecutionError, PhpValue, RuntimeErrorKind};

#[test]
fn test_basic_invokable_object() {
    let source = r#"
        class CreateSite {
            public function __invoke(string $domain): string {
                return "site created: " . $domain;
            }
        }

        $createSite = new CreateSite();
        $result = $createSite("laravel.com");
        return $result;
    "#;

    let result = run_source(source).expect("Invokable object execution should succeed");
    assert_eq!(result, PhpValue::string("site created: laravel.com"));
}

#[test]
fn test_laravel_single_action_controller_pattern() {
    let source = r#"
        class CreateSiteController {
            private string $environment;

            public function __construct(string $env) {
                $this->environment = $env;
            }

            public function __invoke(array $data): string {
                return "Created site [" . $data["name"] . "] on " . $this->environment;
            }
        }

        $controller = new CreateSiteController("production");
        $response = $controller(["name" => "Acme Corp"]);
        return $response;
    "#;

    let result = run_source(source).expect("Laravel single-action controller should succeed");
    assert_eq!(result, PhpValue::string("Created site [Acme Corp] on production"));
}

#[test]
fn test_invokable_object_state_mutation() {
    let source = r#"
        class HitCounter {
            public int $hits = 0;

            public function __invoke(int $step = 1): int {
                $this->hits += $step;
                return $this->hits;
            }
        }

        $counter = new HitCounter();
        $c1 = $counter();
        $c2 = $counter(5);
        $c3 = $counter(10);
        return [$c1, $c2, $c3, $counter->hits];
    "#;

    let result = run_source(source).expect("Stateful invokable object execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = result {
        let b = arr.borrow();
        assert_eq!(b[0], PhpValue::Int(1));
        assert_eq!(b[1], PhpValue::Int(6));
        assert_eq!(b[2], PhpValue::Int(16));
        assert_eq!(b[3], PhpValue::Int(16));
    } else {
        panic!("Expected array/list, got {:?}", result);
    }
}

#[test]
fn test_invokable_object_recursion_via_this() {
    let source = r#"
        class Factorial {
            public function __invoke(int $n): int {
                if ($n <= 1) {
                    return 1;
                }
                $recurse = $this;
                return $n * $recurse($n - 1);
            }
        }

        $fact = new Factorial();
        return $fact(5);
    "#;

    let result = run_source(source).expect("Recursive invokable execution should succeed");
    assert_eq!(result, PhpValue::Int(120));
}

#[test]
fn test_invokable_object_trait_and_inheritance() {
    let source = r#"
        trait InvokableLogger {
            public function __invoke(string $msg): string {
                return "[LOG] " . $msg;
            }
        }

        class BaseService {
            use InvokableLogger;
        }

        class CustomService extends BaseService {}

        $svc = new CustomService();
        return $svc("Server started");
    "#;

    let result = run_source(source).expect("Inherited invokable execution should succeed");
    assert_eq!(result, PhpValue::string("[LOG] Server started"));
}

#[test]
fn test_invokable_object_as_callable_boundary_parameter() {
    let source = r#"
        class ActionHandler {
            public function __invoke(string $input): string {
                return "processed: " . $input;
            }
        }

        function dispatch(callable $action, string $param): string {
            return $action($param);
        }

        $handler = new ActionHandler();
        return dispatch($handler, "event_42");
    "#;

    let result = run_source(source).expect("Callable boundary acceptance should succeed");
    assert_eq!(result, PhpValue::string("processed: event_42"));
}

#[test]
fn test_immediate_instantiation_invokable_call() {
    let source = r#"
        class Formatter {
            public function __invoke(string $s): string {
                return ">>> " . $s . " <<<";
            }
        }

        $formatted = (new Formatter())("hello world");
        return $formatted;
    "#;

    let result = run_source(source).expect("Immediate instantiation invocation should succeed");
    assert_eq!(result, PhpValue::string(">>> hello world <<<"));
}

#[test]
fn test_invokable_stored_in_array() {
    let source = r#"
        class GreetingAction {
            public function __invoke(string $target): string {
                return "Hello, " . $target . "!";
            }
        }

        $routes = [
            "greet" => new GreetingAction(),
        ];

        $action = $routes["greet"];
        return $action("Laravel");
    "#;

    let result = run_source(source).expect("Array-stored invokable should succeed");
    assert_eq!(result, PhpValue::string("Hello, Laravel!"));
}

#[test]
fn test_non_invokable_object_call_fails() {
    let source = r#"
        class PlainObject {
            public string $name = "foo";
        }

        $obj = new PlainObject();
        return $obj("test");
    "#;

    let err = run_source(source).expect_err("Non-invokable object invocation must fail");
    match err {
        ExecutionError::Runtime(rt_err) => match &rt_err.kind {
            RuntimeErrorKind::NotCallable { type_name } => {
                assert_eq!(type_name, "PlainObject");
            }
            other => panic!("Expected NotCallable error, got {:?}", other),
        },
        other => panic!("Expected RuntimeError, got {:?}", other),
    }
}
