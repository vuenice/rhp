use rhp::{run_source, run_source_with_vm, ExecutionError, PhpValue};

#[test]
fn test_model_builder_forwarding_and_late_static_binding() {
    let source = r#"
        class Builder {
            public string $model_class;
            public array $conditions = [];

            public function __construct(string $model_class) {
                $this->model_class = $model_class;
            }

            public function where(string $col, mixed $val): Builder {
                $this->conditions[] = [$col, $val];
                return $this;
            }

            public function get(): array {
                return [
                    'model' => $this->model_class,
                    'conditions' => $this->conditions,
                ];
            }
        }

        class Model {
            public static function __forward(): object {
                return new Builder(static::class);
            }
        }

        class User extends Model {}
        class Post extends Model {}

        $user_builder = User::where('id', 1);
        $user_model = $user_builder->model_class;

        $post_builder = Post::where('title', 'hello');
        $post_model = $post_builder->model_class;
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$user_model"), Some(&PhpValue::string("User")));
    assert_eq!(vm.get_main_variable("$post_model"), Some(&PhpValue::string("Post")));
}

#[test]
fn test_evaluation_order_factory_runs_before_arguments() {
    let source = r#"
        class Tracker {
            public static string $order = '';
        }

        class Builder {
            public function process(mixed $arg): int {
                return 42;
            }
        }

        class Model {
            public static function __forward(): object {
                Tracker::$order = Tracker::$order . 'forward;';
                return new Builder();
            }
        }

        class User extends Model {}

        function get_arg(): int {
            Tracker::$order = Tracker::$order . 'arg;';
            return 100;
        }

        $res = User::process(get_arg());
        $order = Tracker::$order;
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$order"), Some(&PhpValue::string("forward;arg;")));
}

#[test]
fn test_real_static_method_precedence_over_forwarding() {
    let source = r#"
        class Builder {
            public function find(int $id): string {
                return 'builder find: ' . $id;
            }
        }

        class Model {
            public static function __forward(): object {
                return new Builder();
            }

            public static function find(int $id): string {
                return 'model static find: ' . $id;
            }
        }

        class User extends Model {}

        $res = User::find(99);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("model static find: 99")));
}

#[test]
fn test_typo_diagnostics_compile_time_rewrite() {
    let source = r#"
        class Builder {
            public function where(string $col, mixed $val): Builder {
                return $this;
            }
        }

        class Model {
            public static function __forward(): object {
                return new Builder();
            }
        }

        class User extends Model {}

        User::wher('id', 1);
    "#;

    let result = run_source(source);
    match result {
        Err(ExecutionError::Runtime(err)) => {
            let msg = err.kind.to_string();
            assert!(
                msg.contains("Call to undefined method 'wher' on 'Builder', reached via 'User::__forward()'"),
                "Expected forwarded method error, got: {}",
                msg
            );
        }
        other => panic!("Expected runtime error, got: {:?}", other),
    }
}

#[test]
fn test_typo_diagnostics_dynamic_call() {
    let source = r#"
        class Builder {
            public function where(string $col, mixed $val): Builder {
                return $this;
            }
        }

        class Model {
            public static function __forward(): object {
                return new Builder();
            }
        }

        class User extends Model {}

        $cls = 'User';
        $cls::wher('id', 1);
    "#;

    let result = run_source(source);
    match result {
        Err(ExecutionError::Runtime(err)) => {
            let msg = err.kind.to_string();
            assert!(
                msg.contains("Call to undefined method 'wher' on 'Builder', reached via 'User::__forward()'"),
                "Expected forwarded method error, got: {}",
                msg
            );
        }
        other => panic!("Expected runtime error, got: {:?}", other),
    }
}

#[test]
fn test_dynamic_class_static_forwarding() {
    let source = r#"
        class Builder {
            public string $m;
            public function __construct(string $m) {
                $this->m = $m;
            }
            public function query(): string {
                return 'queried ' . $this->m;
            }
        }

        class Model {
            public static function __forward(): object {
                return new Builder(static::class);
            }
        }

        class Product extends Model {}

        $target = 'Product';
        $res = $target::query();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("queried Product")));
}

#[test]
fn test_facade_pattern_with_forward() {
    let source = r#"
        class CacheStore {
            public function get(string $key): string {
                return 'stored_' . $key;
            }
        }

        class Cache {
            public static function __forward(): object {
                return new CacheStore();
            }
        }

        $val = Cache::get('foo');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$val"), Some(&PhpValue::string("stored_foo")));
}

#[test]
fn test_call_static_fallback_when_no_forward_hook() {
    let source = r#"
        class LegacyProxy {
            public static function __callStatic(string $method, array $args): string {
                return 'intercepted ' . $method . ' with ' . count($args) . ' args';
            }
        }

        $res = LegacyProxy::customCall(1, 2, 3);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::string("intercepted customCall with 3 args")));
}

#[test]
fn test_subclass_override_forward() {
    let source = r#"
        class BuilderA {
            public function run(): string { return 'builder A'; }
        }
        class BuilderB {
            public function run(): string { return 'builder B'; }
        }
        class Model {
            public static function __forward(): object { return new BuilderA(); }
        }
        class SubModel extends Model {
            public static function __forward(): object { return new BuilderB(); }
        }

        $a = Model::run();
        $b = SubModel::run();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$a"), Some(&PhpValue::string("builder A")));
    assert_eq!(vm.get_main_variable("$b"), Some(&PhpValue::string("builder B")));
}
