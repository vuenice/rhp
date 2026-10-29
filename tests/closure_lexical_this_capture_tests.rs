use rhp::{run_source_with_vm, PhpValue};

#[test]
fn test_closure_automatic_this_prop_and_method_access() {
    let source = r#"
        class Calculator {
            public int $base = 100;

            public function getAdder(): callable {
                return function(int $x): int {
                    return $this->base + $this->computeOffset($x);
                };
            }

            public function computeOffset(int $x): int {
                return $x * 2;
            }
        }

        $calc = new Calculator();
        $adder = $calc->getAdder();
        $result = $adder(25);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Execution should succeed");
    assert_eq!(
        vm.get_main_variable("$result"),
        Some(&PhpValue::Int(150)) // 100 + 25 * 2
    );
}

#[test]
fn test_closure_callbacks_with_list_map() {
    let source = r#"
        class ItemFormatter {
            private string $prefix = "item_";

            public function formatAll(List $items): List {
                return $items->map(function(string $item): string {
                    return $this->formatSingle($item);
                });
            }

            private function formatSingle(string $val): string {
                return $this->prefix . $val;
            }
        }

        $formatter = new ItemFormatter();
        $list = Arr::list(["first", "second", "third"]);
        $formatted = $formatter->formatAll($list);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("List map with closure callback should succeed");
    assert_eq!(
        vm.get_main_variable("$formatted"),
        Some(&PhpValue::list(vec![
            PhpValue::string("item_first"),
            PhpValue::string("item_second"),
            PhpValue::string("item_third"),
        ]))
    );
}

#[test]
fn test_closure_mutates_this_property() {
    let source = r#"
        class Counter {
            public int $count = 0;

            public function getIncrementer(): callable {
                return function(): int {
                    $this->count = $this->count + 1;
                    return $this->count;
                };
            }
        }

        $c = new Counter();
        $inc = $c->getIncrementer();
        $r1 = $inc();
        $r2 = $inc();
        $finalCount = $c->count;
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Closure mutating $this property should succeed");
    assert_eq!(vm.get_main_variable("$r1"), Some(&PhpValue::Int(1)));
    assert_eq!(vm.get_main_variable("$r2"), Some(&PhpValue::Int(2)));
    assert_eq!(vm.get_main_variable("$finalCount"), Some(&PhpValue::Int(2)));
}

#[test]
fn test_closure_with_explicit_use_and_lexical_this() {
    let source = r#"
        class Multiplier {
            private int $multiplier = 3;

            public function makeOperation(int $bonus): callable {
                return function(int $x) use ($bonus): int {
                    return ($x * $this->multiplier) + $bonus;
                };
            }
        }

        $m = new Multiplier();
        $op = $m->makeOperation(50);
        $res = $op(10);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Closure with use ($var) and lexical $this should succeed");
    assert_eq!(
        vm.get_main_variable("$res"),
        Some(&PhpValue::Int(80)) // 10 * 3 + 50
    );
}

#[test]
fn test_nested_non_static_closures_capture_this_transitively() {
    let source = r#"
        class Greeter {
            public string $greeting = "Welcome";

            public function createPipeline(): callable {
                return function(string $title): callable {
                    return function(string $name) use ($title): string {
                        return $this->greeting . ", " . $title . " " . $name . "!";
                    };
                };
            }
        }

        $g = new Greeter();
        $pipeline = $g->createPipeline();
        $titled = $pipeline("Dr.");
        $message = $titled("Watson");
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Nested closures capturing $this should succeed");
    assert_eq!(
        vm.get_main_variable("$message"),
        Some(&PhpValue::string("Welcome, Dr. Watson!"))
    );
}

#[test]
fn test_arrow_function_enclosing_closure_captures_this() {
    let source = r#"
        class Dispatcher {
            public int $tag = 999;

            public function getGenerator(): callable {
                return fn() => function(): int {
                    return $this->tag;
                };
            }
        }

        $d = new Dispatcher();
        $arrow = $d->getGenerator();
        $closure = $arrow();
        $val = $closure();
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Arrow enclosing non-static closure should succeed");
    assert_eq!(vm.get_main_variable("$val"), Some(&PhpValue::Int(999)));
}

#[test]
fn test_static_closure_inside_method_does_not_capture_this() {
    let source = r#"
        class StrictClass {
            public int $value = 42;

            public function getStaticFn(): callable {
                return static function(int $x): int {
                    return $x * 10;
                };
            }
        }

        $obj = new StrictClass();
        $fn = $obj->getStaticFn();
        $res = $fn(7);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Static closure execution should succeed");
    assert_eq!(vm.get_main_variable("$res"), Some(&PhpValue::Int(70)));
}

#[test]
fn test_closure_in_static_method_cannot_access_this() {
    let source = r#"
        class StaticContainer {
            public static function factory(): callable {
                return function(): int {
                    return $this->foo;
                };
            }
        }

        $fn = StaticContainer::factory();
        $fn();
    "#;

    let res = run_source_with_vm(source);
    assert!(res.is_err(), "Non-static closure in static method accessing $this must fail at runtime on null $this");
}
