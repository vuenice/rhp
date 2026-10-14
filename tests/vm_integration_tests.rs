use rhp::{
    compile_source, compile_to_bytecode, execute, lower_program, parse, parse_expr,
    register_custom_helper, run_source, run_source_with_file_id, run_source_with_vm, Environment,
    ExecutionError, PhpValue, RunMode, RuntimeErrorKind, RustFloatKind, RustIntKind, SourceMap, VM,
};

#[test]
fn test_dual_engine_run_mode_dev_and_build_sketch() {
    let expr = parse_expr("10 + 20 * 3").expect("Failed to parse expression");
    let mut env = Environment::new();

    // 1. Development Mode (`RunMode::Dev`) direct AST evaluation
    let dev_val = execute(
        RunMode::Dev {
            hot_reload: true,
            dap_port: Some(9000),
        },
        &expr,
        &mut env,
    )
    .expect("Dev mode evaluation should succeed");
    assert_eq!(dev_val, PhpValue::Int(70));

    // 2. Production / Build Mode (`RunMode::Build`) bytecode compilation + Register VM execution
    let build_val = execute(
        RunMode::Build {
            jit_enabled: true,
            opt_level: 3,
        },
        &expr,
        &mut env,
    )
    .expect("Build mode VM execution should succeed");
    assert_eq!(build_val, PhpValue::Int(70));

    // 3. Direct `compile_to_bytecode` + `VM::new(bytecode, jit_enabled).run()`
    let bytecode = compile_to_bytecode(&expr).unwrap();
    let vm_val = VM::new(bytecode, true).run().unwrap();
    assert_eq!(vm_val, PhpValue::Int(70));
}

#[test]
fn test_arithmetic_bitwise_unary_and_short_circuit_execution() {
    let source = r#"
        $a = 15;
        $b = 4;
        $sum = $a + $b;
        $diff = $a - $b;
        $prod = $a * $b;
        $mod = $a % $b;
        $pow = 2 ** 8;
        $bit = ($a & 7) | 16;
        $pre = ++$b;
        $post = $b++;

        $nullVal = null;
        $coalesced = $nullVal ?? 'fallback';
        $elvis = '' ?: 'default_str';
        $ternary = ($sum > 10) ? 'high' : 'low';
        $andShort = false && (10 / 0 == 1);
        $orShort = true || (10 / 0 == 1);

        return [$sum, $diff, $prod, $mod, $pow, $bit, $pre, $post, $b, $coalesced, $elvis, $ternary, $andShort, $orShort];
    "#;

    let result = run_source(source).expect("VM execution should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(19),
            PhpValue::Int(11),
            PhpValue::Int(60),
            PhpValue::Int(3),
            PhpValue::Int(256),
            PhpValue::Int(23),
            PhpValue::Int(5),
            PhpValue::Int(5),
            PhpValue::Int(6),
            PhpValue::string("fallback"),
            PhpValue::string("default_str"),
            PhpValue::string("high"),
            PhpValue::Bool(false),
            PhpValue::Bool(true),
        ])
    );
}

#[test]
fn test_functions_recursion_default_and_named_arguments_and_type_contracts() {
    let source = r#"
        function fib(int $n): int {
            if ($n <= 1) {
                return $n;
            }
            return fib($n - 1) + fib($n - 2);
        }

        function calculateTotal(int $quantity, float $price, float $taxRate = 0.05): float {
            $subtotal = $quantity * $price;
            $tax = $subtotal * $taxRate;
            $total: float = $subtotal + $tax;
            return $total;
        }

        $fib8 = fib(8);
        $totalDefault = calculateTotal(4, 25.0);
        $totalNamed = calculateTotal(price: 50.0, quantity: 2, taxRate: 0.10);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Function execution should pass");
    assert_eq!(vm.get_main_variable("$fib8"), Some(&PhpValue::Int(21)));
    assert_eq!(
        vm.get_main_variable("$totalDefault"),
        Some(&PhpValue::Float(105.0))
    );
    assert_eq!(
        vm.get_main_variable("$totalNamed"),
        Some(&PhpValue::Float(110.0))
    );
}

#[test]
fn test_runtime_boundary_type_contract_enforcement_and_stack_trace() {
    // Bypass static type-checker using `lower_program(..., None)` so a dynamic runtime type mismatch hits `OpCode::AssertType`
    let source = r#"function requireInt(int $x): int {
    return $x + 1;
}
function caller(mixed $val): int {
    return requireInt($val);
}
$boom = caller('not-an-int');
"#;

    let mut source_map = SourceMap::new();
    let file_id = source_map.add_file("contracts.rhp", source);
    let program = rhp::parse_with_file_id(source, file_id).expect("Parse should succeed");
    let module = lower_program(&program, None).expect("IR lowering should succeed");

    let mut vm = VM::new(module, false);
    let err = vm
        .run()
        .expect_err("AssertType must reject string passed to int parameter at runtime");

    match &err.kind {
        RuntimeErrorKind::TypeContractViolation {
            expected, actual, ..
        } => {
            assert_eq!(*expected, rhp::RhpType::Int);
            assert_eq!(actual, "string");
        }
        other => panic!("Expected TypeContractViolation, got {:?}", other),
    }

    // Verify full call stack trace (`requireInt` -> `caller` -> `<main>`)
    assert_eq!(err.stack_trace.len(), 3);
    assert_eq!(err.stack_trace[0].function_name, "requireInt");
    assert_eq!(err.stack_trace[1].function_name, "caller");
    assert_eq!(err.stack_trace[2].function_name, "<main>");

    let formatted = err.format_with_source_map(&source_map);
    assert!(formatted.contains("contracts.rhp"));
    assert!(formatted.contains("Stack trace:"));
}

#[test]
fn test_subtyped_collections_and_fluent_arrow_closure_chaining() {
    let source = r#"
        $multiplier = 3;
        $numbers = Arr::list([1, 2, 3, 4, 5])
            ->map(fn($n) => $n * $multiplier)
            ->filter(fn($n) => $n > 6);

        $sum = $numbers->reduce(fn($acc, $n) => $acc + $n, 0);

        $dict = Arr::dict(['host' => 'localhost', 'port' => 8080]);
        $host = $dict['host'];

        $set = Arr::set([10, 20, 20, 30, 10]);
        $setCount = $set->count();

        return [$numbers, $sum, $host, $setCount];
    "#;

    let result = run_source(source).expect("Collection chaining should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::list(vec![PhpValue::Int(9), PhpValue::Int(12), PhpValue::Int(15)]),
            PhpValue::Int(36),
            PhpValue::string("localhost"),
            PhpValue::Int(3),
        ])
    );
}

#[test]
fn test_rust_numeric_wrappers_and_fluent_pipelines() {
    let source = r#"
        $i64Val = Int::i64(100)->add(50)->subtract(30);
        $f64Val = Float::f64(3.14159)->toDecimal(2);
        $u8Clamped = Int::u8(200)->add(10);

        return [$i64Val, $f64Val, $u8Clamped];
    "#;

    let result = run_source(source).expect("Rust numeric wrappers should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::RustInt {
                kind: RustIntKind::I64,
                value: 120,
            },
            PhpValue::RustFloat {
                kind: RustFloatKind::F64,
                value: 3.14,
            },
            PhpValue::RustInt {
                kind: RustIntKind::U8,
                value: 210,
            },
        ])
    );
}

#[test]
fn test_facades_legacy_phpfun_and_custom_global_helpers() {
    register_custom_helper(
        "double_ greeting",
        vec![("$val".to_string(), None)],
        "return $val;",
    )
    .ok();
    register_custom_helper("my_passthrough", vec![("$x".to_string(), None)], "return $x;")
        .expect("Register custom helper");

    let source = r#"
        $slug = Str::slug('Laravel Style Facades');
        $hasWord = Str::contains('Runtime engine', 'engine');
        $merged = array_merge([1, 2], [3, 4]);
        $len = strlen('hello');
        $joined = implode('-', ['a', 'b', 'c']);
        $custom = my_passthrough('custom_ok');

        return [$slug, $hasWord, $merged, $len, $joined, $custom];
    "#;

    let result = run_source(source).expect("Facade and legacy helper execution should pass");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("laravel-style-facades"),
            PhpValue::Bool(true),
            PhpValue::list(vec![
                PhpValue::Int(1),
                PhpValue::Int(2),
                PhpValue::Int(3),
                PhpValue::Int(4),
            ]),
            PhpValue::Int(5),
            PhpValue::string("a-b-c"),
            PhpValue::string("custom_ok"),
        ])
    );
}

#[test]
fn test_classes_inheritance_constructor_promotion_readonly_enums_and_try_catch() {
    let source = r#"
        enum OrderStatus: string {
            case Pending = 'pending';
            case Paid = 'paid';
        }

        interface Payable {
            public function total(): float;
        }

        class BaseEntity {
            public function describe(): string {
                return 'entity';
            }
        }

        class Order extends BaseEntity implements Payable {
            public const CURRENCY = 'USD';

            public function __construct(
                public readonly int $id,
                private float $amount
            ) {}

            public function total(): float {
                return $this->amount;
            }
        }

        $order = new Order(42, 99.5);
        $isPayable = $order instanceof Payable;
        $isBase = $order instanceof BaseEntity;
        $status = OrderStatus::Paid;
        $currency = Order::CURRENCY;

        $caughtMsg = 'none';
        try {
            $t = $order?->total() ?? 0.0;
            if ($t > 50.0) {
                throw 'Over limit';
            }
        } catch (Exception $e) {
            $caughtMsg = $e;
        }

        return [$order->id, $order->total(), $order->describe(), $isPayable, $isBase, $status->value, $currency, $caughtMsg];
    "#;

    let result = run_source(source).expect("OOP and exception handling should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(42),
            PhpValue::Float(99.5),
            PhpValue::string("entity"),
            PhpValue::Bool(true),
            PhpValue::Bool(true),
            PhpValue::string("paid"),
            PhpValue::string("USD"),
            PhpValue::string("Over limit"),
        ])
    );
}

#[test]
fn test_readonly_property_mutation_outside_constructor_fails_at_runtime() {
    // Lower without static checker so runtime readonly enforcement in `OpCode::SetProp` is tested directly
    let source = r#"
        class Account {
            public function __construct(public readonly int $id) {}
        }
        $acc = new Account(1);
        $acc->id = 2;
    "#;

    let program = parse(source).unwrap();
    let module = lower_program(&program, None).unwrap();
    let err = VM::new(module, false)
        .run()
        .expect_err("Mutating readonly property outside constructor must fail");

    assert_eq!(
        err.kind,
        RuntimeErrorKind::ReadonlyPropertyModification {
            class: "Account".to_string(),
            property: "id".to_string(),
        }
    );
}

#[test]
fn test_async_future_concurrent_core_destructuring_and_post_response_defer_order() {
    let source = r#"
        $id = 7;
        [$user, $posts] = Future::wait([
            fn() => DB::table('users')->find($id),
            fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
        ]);

        $fastest = Future::any([
            fn() => 'primary_ok',
            fn() => 'secondary_ok'
        ]);

        $delayed = Future::delayed(10, fn() => 'resumed')->wait();

        echo 'body;';
        defer {
            echo 'defer1;';
        }
        defer {
            echo 'defer2;';
        }

        return [$user->id, $posts['status'], $fastest, $delayed];
    "#;

    let mut source_map = SourceMap::new();
    let file_id = source_map.add_file("async_defer.rhp", source);
    let (vm, result) =
        run_source_with_file_id(source, file_id).expect("Future and defer execution should pass");

    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(7),
            PhpValue::string("ok"),
            PhpValue::string("primary_ok"),
            PhpValue::string("resumed"),
        ])
    );

    // Verify `defer` blocks executed strictly AFTER `<main>` in LIFO order
    assert_eq!(vm.output(), "body;defer2;defer1;");
}

#[test]
fn test_control_flow_loops_foreach_destructuring_switch_and_match() {
    let source = r#"
        $pairs = [[1, 10], [2, 20], [3, 30]];
        $total = 0;
        foreach ($pairs as $idx => [$k, $v]) {
            $total += ($k * $v);
        }

        $code = 201;
        $statusText = match ($code) {
            200, 201 => 'Created/OK',
            404 => 'Missing',
            default => 'Other',
        };

        $switchOut = '';
        switch ($code) {
            case 200:
                $switchOut = 'two-hundred';
                break;
            case 201:
                $switchOut = 'two-oh-one';
                break;
            default:
                $switchOut = 'default';
                break;
        }

        return [$total, $statusText, $switchOut];
    "#;

    let result = run_source(source).expect("Loops, switch, and match should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(140),
            PhpValue::string("Created/OK"),
            PhpValue::string("two-oh-one"),
        ])
    );
}

#[test]
fn test_unhandled_match_error_and_division_by_zero_diagnostics() {
    let match_src = r#"
        $x = 999;
        $res = match ($x) {
            1 => 'one',
            2 => 'two',
        };
    "#;

    match run_source(match_src).expect_err("Unhandled match value must error") {
        ExecutionError::Runtime(err) => {
            assert_eq!(
                err.kind,
                RuntimeErrorKind::UnhandledMatchError {
                    subject: "999".to_string()
                }
            );
        }
        other => panic!("Expected RuntimeError, got {:?}", other),
    }

    let div_zero_src = "$x = 100 / 0;";
    let module = compile_source(div_zero_src).unwrap();
    let err = VM::new(module, false)
        .run()
        .expect_err("Division by zero must error");
    assert_eq!(err.kind, RuntimeErrorKind::DivisionByZero);
}

#[test]
fn test_bun_inspired_improvements_i32_f32_sso_and_shapes() {
    use rhp::vm::value::{RhpString, VmObject};

    // 1. Verify i32 and f32 default runtime values and operations
    let source = r#"
        $intVal = 42;
        $floatVal = 3.5;
        $divFloat = 7 / 2;
        $divInt = 8 / 2;
        $addFloat = $intVal + $floatVal;
    "#;
    let (vm, _) = run_source_with_vm(source).expect("Execution should pass");
    assert_eq!(vm.get_main_variable("$intVal"), Some(&PhpValue::Int(42_i32)));
    assert_eq!(vm.get_main_variable("$floatVal"), Some(&PhpValue::Float(3.5_f32)));
    assert_eq!(vm.get_main_variable("$divFloat"), Some(&PhpValue::Float(3.5_f32)));
    assert_eq!(vm.get_main_variable("$divInt"), Some(&PhpValue::Int(4_i32)));
    assert_eq!(vm.get_main_variable("$addFloat"), Some(&PhpValue::Float(45.5_f32)));

    // 2. Verify Small-String-Optimization (SSO) on RhpString
    let short_s = RhpString::new("small_string");
    assert!(matches!(short_s, RhpString::Inline { len: 12, .. }));
    assert_eq!(short_s.as_str(), "small_string");

    let long_s = RhpString::new("this_is_a_very_long_string_that_exceeds_twenty_three_bytes");
    assert!(matches!(long_s, RhpString::Heap(_)));
    assert_eq!(long_s.as_str(), "this_is_a_very_long_string_that_exceeds_twenty_three_bytes");

    // 3. Verify Shape-based slot indexing on VmObject
    let mut obj = VmObject::new("User");
    obj.set_property("id", PhpValue::Int(1_i32));
    obj.set_property("status", PhpValue::string("active"));

    assert_eq!(obj.slots.len(), 2);
    assert_eq!(obj.slots[0], PhpValue::Int(1_i32));
    assert_eq!(obj.slots[1], PhpValue::string("active"));
    assert_eq!(obj.get_property("id"), Some(&PhpValue::Int(1_i32)));
    assert_eq!(obj.get_property("status"), Some(&PhpValue::string("active")));

    let shape = obj.shape.borrow();
    assert_eq!(shape.property_indices.get("id"), Some(&0));
    assert_eq!(shape.property_indices.get("status"), Some(&1));
}

#[test]
fn test_dynamic_value_enum_and_copy_on_write_semantics() {
    use rhp::vm::value::{PhpValue, RhpString};

    // 1. Core PhpValue enum representation & Ref-counting inspection
    let scalar_int = PhpValue::Int(42);
    assert_eq!(scalar_int.ref_count(), 1);
    assert!(!scalar_int.is_shared());

    let arr = PhpValue::array(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]);
    assert_eq!(arr.ref_count(), 1);
    assert!(!arr.is_shared());

    let arr_alias = arr.clone();
    assert_eq!(arr.ref_count(), 2);
    assert_eq!(arr_alias.ref_count(), 2);
    assert!(arr.is_shared());
    assert!(arr_alias.is_shared());

    // 2. COW detachment on array mutation
    let mut arr_mut = arr_alias;
    arr_mut.ensure_array_mut();
    // After detachment, arr_mut has its own buffer with ref_count 1, while arr returns to ref_count 1
    assert_eq!(arr_mut.ref_count(), 1);
    assert_eq!(arr.ref_count(), 1);
    assert!(!arr_mut.is_shared());
    assert!(!arr.is_shared());

    // 3. VM runtime: Array Copy-on-Write on assignment and mutation
    let source_arr_cow = r#"
        $orig = [10, 20, 30];
        $copied = $orig;
        $copied[0] = 99;
        $copied[] = 100;
    "#;
    let (vm, _) = run_source_with_vm(source_arr_cow).expect("Array COW execution should pass");
    let orig = vm.get_main_variable("$orig").unwrap();
    let copied = vm.get_main_variable("$copied").unwrap();

    assert_eq!(
        orig,
        &PhpValue::array(vec![
            PhpValue::Int(10),
            PhpValue::Int(20),
            PhpValue::Int(30),
        ])
    );
    assert_eq!(
        copied,
        &PhpValue::array(vec![
            PhpValue::Int(99),
            PhpValue::Int(20),
            PhpValue::Int(30),
            PhpValue::Int(100),
        ])
    );

    // 4. VM runtime: Array Copy-on-Write across function calls
    let source_func_cow = r#"
        function mutateList(array $items): array {
            $items[0] = 777;
            $items[] = 888;
            return $items;
        }

        $myList = [1, 2, 3];
        $returnedList = mutateList($myList);
    "#;
    let (vm, _) = run_source_with_vm(source_func_cow).expect("Function COW execution should pass");
    let my_list = vm.get_main_variable("$myList").unwrap();
    let ret_list = vm.get_main_variable("$returnedList").unwrap();

    assert_eq!(
        my_list,
        &PhpValue::array(vec![
            PhpValue::Int(1),
            PhpValue::Int(2),
            PhpValue::Int(3),
        ])
    );
    assert_eq!(
        ret_list,
        &PhpValue::array(vec![
            PhpValue::Int(777),
            PhpValue::Int(2),
            PhpValue::Int(3),
            PhpValue::Int(888),
        ])
    );

    // 5. VM runtime: Object Reference Semantics (objects share reference handle)
    let source_obj_ref = r#"
        class Customer {
            public string $name = 'Initial';
        }

        $c1 = new Customer();
        $c2 = $c1;
        $c2->name = 'Updated';

        $c3 = clone $c1;
        $c3->name = 'Cloned';
    "#;
    let (vm, _) = run_source_with_vm(source_obj_ref).expect("Object ref execution should pass");
    let c1 = vm.get_main_variable("$c1").unwrap();
    let c2 = vm.get_main_variable("$c2").unwrap();
    let c3 = vm.get_main_variable("$c3").unwrap();

    match (c1, c2, c3) {
        (PhpValue::Object(o1), PhpValue::Object(o2), PhpValue::Object(o3)) => {
            // $c1 and $c2 share the exact same underlying object handle
            assert!(std::rc::Rc::ptr_eq(o1, o2));
            assert_eq!(o1.borrow().get_property("name"), Some(&PhpValue::string("Updated")));
            assert_eq!(o2.borrow().get_property("name"), Some(&PhpValue::string("Updated")));

            // $c3 is an independent instance created via explicit language clone
            assert!(!std::rc::Rc::ptr_eq(o1, o3));
            assert_eq!(o3.borrow().get_property("name"), Some(&PhpValue::string("Cloned")));
        }
        _ => panic!("Expected objects"),
    }

    // 6. String COW and In-place Mutation
    let mut s = RhpString::new("hello");
    assert!(s.is_inline());
    assert_eq!(s.ref_count(), 1);

    // In-place COW push_str without heap allocation
    s.push_str(" world");
    assert!(s.is_inline());
    assert_eq!(s.as_str(), "hello world");

    // Exceeding SSO capacity promotes to Heap with COW
    s.push_str(" with a very long string exceeding 23 bytes");
    assert!(s.is_heap());
    assert_eq!(s.ref_count(), 1);

    // Heap string sharing & COW modification
    let mut s_shared = s.clone();
    assert_eq!(s.ref_count(), 2);
    assert!(s.is_shared());

    s_shared.push_str(" - mutated");
    // After write, s_shared detached via COW
    assert_eq!(s_shared.ref_count(), 1);
    assert_eq!(s.ref_count(), 1);
    assert!(s_shared.as_str().ends_with("- mutated"));
    assert!(!s.as_str().ends_with("- mutated"));

    // 7. VM string index mutation with COW
    let source_str_idx = r#"
        $greeting = 'hello';
        $greeting[0] = 'j';
    "#;
    let (vm, _) = run_source_with_vm(source_str_idx).expect("String index mutation should pass");
    assert_eq!(vm.get_main_variable("$greeting"), Some(&PhpValue::string("jello")));
}

#[test]
fn test_vm_runtime_type_casting_and_collection_boundary_contracts() {
    // 1. Runtime Type Casting: (int)$x, (string)$x, (float)$x, (bool)$x
    let source_casts = r#"
        $rawNum = "42";
        $asInt = (int)$rawNum;
        $asFloat = (float)"3.14";
        $asStr = (string)999;
        $asBool = (bool)1;
        $asBoolFalse = (bool)0;
    "#;
    let (vm_casts, _) = run_source_with_vm(source_casts).expect("Type casting must succeed");
    assert_eq!(vm_casts.get_main_variable("$asInt"), Some(&PhpValue::Int(42)));
    assert_eq!(vm_casts.get_main_variable("$asFloat"), Some(&PhpValue::Float(3.14)));
    assert_eq!(vm_casts.get_main_variable("$asStr"), Some(&PhpValue::string("999")));
    assert_eq!(vm_casts.get_main_variable("$asBool"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm_casts.get_main_variable("$asBoolFalse"), Some(&PhpValue::Bool(false)));

    // 2. Runtime Collection & Scalar Boundary Contracts: List $arr, Dict $arr, int $id, string $title
    let source_contracts = r#"
        function formatEntry(int $id, string $title, List $items, Dict $config): List {
            return $items;
        }

        $myList = Arr::list([1, 2, 3]);
        $myDict = Arr::dict(['enabled' => true]);
        $res = formatEntry(101, 'Report', $myList, $myDict);
    "#;
    let (vm_contracts, _) = run_source_with_vm(source_contracts).expect("Collection contracts must execute cleanly");
    assert_eq!(
        vm_contracts.get_main_variable("$res"),
        Some(&PhpValue::list(vec![PhpValue::Int(1), PhpValue::Int(2), PhpValue::Int(3)]))
    );
}



