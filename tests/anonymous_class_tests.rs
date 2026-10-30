use rhp::{run_source, PhpValue};

#[test]
fn test_basic_anonymous_class() {
    let source = r#"
        $anon = new class {
            public function greeting(): string {
                return "hello from anon class";
            }
        };
        return $anon->greeting();
    "#;

    let result = run_source(source).expect("Basic anonymous class execution should succeed");
    assert_eq!(result, PhpValue::string("hello from anon class"));
}

#[test]
fn test_anonymous_class_with_constructor_and_properties() {
    let source = r#"
        $user = new class("Alice", 30) {
            public string $name;
            public int $age;

            public function __construct(string $name, int $age) {
                $this->name = $name;
                $this->age = $age;
            }

            public function describe(): string {
                return $this->name . " is " . string($this->age);
            }
        };
        return $user->describe();
    "#;

    let result = run_source(source).expect("Anonymous class with constructor should succeed");
    assert_eq!(result, PhpValue::string("Alice is 30"));
}

#[test]
fn test_anonymous_class_inheritance_and_instanceof() {
    let source = r#"
        class BaseService {
            public function baseName(): string {
                return "base_service";
            }
        }

        $service = new class extends BaseService {
            public function customName(): string {
                return $this->baseName() . "_extended";
            }
        };

        $name = $service->customName();
        $isBase = $service instanceof BaseService;

        return [
            "name" => $name,
            "is_base" => $isBase,
        ];
    "#;

    let result = run_source(source).expect("Anonymous class inheritance should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            ("name".to_string(), PhpValue::string("base_service_extended")),
            ("is_base".to_string(), PhpValue::Bool(true)),
        ])
    );
}

#[test]
fn test_anonymous_class_implements_interface() {
    let source = r#"
        interface GreeterInterface {
            public function greet(string $name): string;
        }

        $greeter = new class implements GreeterInterface {
            public function greet(string $name): string {
                return "Welcome, " . $name . "!";
            }
        };

        $msg = $greeter->greet("Bob");
        $isGreeter = $greeter instanceof GreeterInterface;

        return [
            "msg" => $msg,
            "is_greeter" => $isGreeter,
        ];
    "#;

    let result = run_source(source).expect("Anonymous class interface implementation should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            ("msg".to_string(), PhpValue::string("Welcome, Bob!")),
            ("is_greeter".to_string(), PhpValue::Bool(true)),
        ])
    );
}

#[test]
fn test_anonymous_class_parent_constructor_forwarding() {
    let source = r#"
        class Vehicle {
            public string $type;
            public function __construct(string $type) {
                $this->type = $type;
            }
        }

        $car = new class("Sedan") extends Vehicle {
            public function getType(): string {
                return "Vehicle type: " . $this->type;
            }
        };

        return $car->getType();
    "#;

    let result = run_source(source).expect("Parent constructor forwarding should succeed");
    assert_eq!(result, PhpValue::string("Vehicle type: Sedan"));
}

#[test]
fn test_chained_anonymous_class_instantiation() {
    let source = r#"
        $val = (new class {
            public function compute(): int {
                return 21 * 2;
            }
        })->compute();

        return $val;
    "#;

    let result = run_source(source).expect("Chained anonymous class call should succeed");
    assert_eq!(result, PhpValue::Int(42));
}

#[test]
fn test_anonymous_class_in_function_factory() {
    let source = r#"
        function createCounter(int $start): object {
            return new class($start) {
                public int $count;

                public function __construct(int $start) {
                    $this->count = $start;
                }

                public function increment(): int {
                    $this->count = $this->count + 1;
                    return $this->count;
                }
            };
        }

        $c = createCounter(5);
        $v1 = $c->increment();
        $v2 = $c->increment();

        return [$v1, $v2];
    "#;

    let result = run_source(source).expect("Anonymous class in function factory should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![PhpValue::Int(6), PhpValue::Int(7)])
    );
}

#[test]
fn test_anonymous_class_name_prefix() {
    let source = r#"
        $anon = new class {};
        return get_class($anon);
    "#;

    let result = run_source(source).expect("get_class on anonymous class should succeed");
    if let PhpValue::String(ref name) = result {
        assert!(name.starts_with("class@anonymous"), "Expected class name to start with class@anonymous, got: {}", name);
    } else {
        panic!("Expected string class name, got {:?}", result);
    }
}

#[test]
fn test_anonymous_class_with_trait() {
    let source = r#"
        trait LogHelper {
            public function formatLog(string $msg): string {
                return "[LOG]: " . $msg;
            }
        }

        $logger = new class {
            use LogHelper;

            public function log(string $msg): string {
                return $this->formatLog($msg);
            }
        };

        return $logger->log("system ready");
    "#;

    let result = run_source(source).expect("Anonymous class with trait should succeed");
    assert_eq!(result, PhpValue::string("[LOG]: system ready"));
}

#[test]
fn test_nested_anonymous_classes() {
    let source = r#"
        $outer = new class {
            public function makeInner(): object {
                return new class {
                    public function innerVal(): int {
                        return 100;
                    }
                };
            }
        };

        $inner = $outer->makeInner();
        return $inner->innerVal();
    "#;

    let result = run_source(source).expect("Nested anonymous class should succeed");
    assert_eq!(result, PhpValue::Int(100));
}

#[test]
fn test_readonly_anonymous_class() {
    let source = r#"
        $ro = new readonly class("immutable") {
            public string $val;
            public function __construct(string $val) {
                $this->val = $val;
            }
            public function getVal(): string {
                return $this->val;
            }
        };

        return $ro->getVal();
    "#;

    let result = run_source(source).expect("Readonly anonymous class should succeed");
    assert_eq!(result, PhpValue::string("immutable"));
}
