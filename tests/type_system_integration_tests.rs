use rhp::{
    parse_and_check, parse_and_check_with_file_id, CollectionSubtype, FrontendError, RhpType,
    SourceMap, StmtKind, TypeErrorKind,
};

#[test]
fn test_strict_function_and_method_boundary_signatures_enforced() {
    // 1. Standalone function missing parameter type must be rejected
    let missing_param_fn = r#"
        function processOrder($orderId, $amount: float): bool {
            return true;
        }
    "#;
    let err = parse_and_check(missing_param_fn).expect_err("Expected MissingParameterType");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingParameterType {
                callable_kind,
                callable_name,
                param_name,
            } => {
                assert_eq!(callable_kind, "function");
                assert_eq!(callable_name, "processOrder");
                assert_eq!(param_name, "$orderId");
            }
            other => panic!("Expected MissingParameterType, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 2. Standalone function missing return type must be rejected
    let missing_ret_fn = r#"
        function processOrder($orderId: int, $amount: float) {
            return true;
        }
    "#;
    let err = parse_and_check(missing_ret_fn).expect_err("Expected MissingReturnType");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingReturnType {
                callable_kind,
                callable_name,
            } => {
                assert_eq!(callable_kind, "function");
                assert_eq!(callable_name, "processOrder");
            }
            other => panic!("Expected MissingReturnType, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 3. Class method missing parameter type must be rejected
    let missing_method_param = r#"
        class UserController {
            public function show($id): string {
                return "user";
            }
        }
    "#;
    let err = parse_and_check(missing_method_param).expect_err("Expected MissingParameterType on method");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingParameterType {
                callable_kind,
                callable_name,
                param_name,
            } => {
                assert_eq!(callable_kind, "method");
                assert_eq!(callable_name, "UserController::show");
                assert_eq!(param_name, "$id");
            }
            other => panic!("Expected MissingParameterType, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 4. Class method missing return type must be rejected (except __construct / __destruct)
    let missing_method_ret = r#"
        class UserController {
            public function __construct($prefix: string) {}
            public function show($id: int) {
                return "user";
            }
        }
    "#;
    let err = parse_and_check(missing_method_ret).expect_err("Expected MissingReturnType on method");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingReturnType {
                callable_kind,
                callable_name,
            } => {
                assert_eq!(callable_kind, "method");
                assert_eq!(callable_name, "UserController::show");
            }
            other => panic!("Expected MissingReturnType, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 5. Constructor declaring non-void return type must be rejected
    let bad_ctor_ret = r#"
        class UserController {
            public function __construct($prefix: string): int {}
        }
    "#;
    let err = parse_and_check(bad_ctor_ret).expect_err("Expected ConstructorReturnTypeForbidden");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::ConstructorReturnTypeForbidden { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_reference_mechanics_pass_by_reference_and_capture_forbidden() {
    // 1. Untyped pass-by-reference parameter `&$items` fails missing parameter type
    let untyped_ref = "function mutate(&$items): void {}";
    let err = parse_and_check(untyped_ref).expect_err("Expected MissingParameterType");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingParameterType {
                callable_name,
                param_name,
                ..
            } => {
                assert_eq!(callable_name, "mutate");
                assert_eq!(param_name, "$items");
            }
            other => panic!("Expected MissingParameterType, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 2. Typed pass-by-reference parameter `array &$items` is valid, but calling without & is rejected
    let typed_ref = r#"
        function mutate(&$items: array): void {}
        $list = [1, 2, 3];
        mutate($list);
    "#;
    let err = parse_and_check(typed_ref).expect_err("Expected MissingReferenceArgument when calling without &");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::MissingReferenceArgument { callable_name, param_name, .. } => {
                assert_eq!(callable_name, "mutate");
                assert_eq!(param_name, "$items");
            }
            other => panic!("Expected MissingReferenceArgument, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 3. Passing by reference to a non-reference parameter is rejected
    let unexpected_ref = r#"
        function inspect($items: array): void {}
        $list = [1, 2, 3];
        inspect(&$list);
    "#;
    let err = parse_and_check(unexpected_ref).expect_err("Expected UnexpectedReferenceArgument");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::UnexpectedReferenceArgument { callable_name, param_name, .. } => {
                assert_eq!(callable_name, "inspect");
                assert_eq!(param_name, "$items");
            }
            other => panic!("Expected UnexpectedReferenceArgument, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 4. Passing a literal or expression by reference is rejected
    let literal_ref = r#"
        function mutate(&$val: int): void {}
        mutate(&10);
    "#;
    let err = parse_and_check(literal_ref).expect_err("Expected ReferenceArgumentMustBeVariable");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ReferenceArgumentMustBeVariable { callable_name, .. } => {
                assert_eq!(callable_name, "mutate");
            }
            other => panic!("Expected ReferenceArgumentMustBeVariable, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 5. Valid pass-by-reference with &$x at both declaration and call site
    let valid_ref = r#"
        function mutate(&$items: array): void {}
        $list = [1, 2, 3];
        mutate(&$list);
    "#;
    assert!(parse_and_check(valid_ref).is_ok());

    // 3. Closure capture-by-reference `use (&$count)`
    let closure_ref = r#"
        $count = 0;
        $inc = function($step: int) use (&$count): int {
            return $count + $step;
        };
    "#;
    let err = parse_and_check(closure_ref).expect_err("Expected ClosureReferenceCaptureForbidden");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ClosureReferenceCaptureForbidden { var_name } => {
                assert_eq!(var_name, "$count");
            }
            other => panic!("Expected ClosureReferenceCaptureForbidden, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 4. Value capture `use ($count)` is valid
    let valid_closure = r#"
        $count = 10;
        $add = function($step: int) use ($count): int {
            return $count + $step;
        };
    "#;
    assert!(parse_and_check(valid_closure).is_ok());
}

#[test]
fn test_dynamic_type_inference_within_function_bodies() {
    let source = r#"
        function calculateTotal($quantity: int, $unitPrice: float, $currency: string): float {
            $subtotal = $quantity * $unitPrice;
            $tax = $subtotal * 0.18;
            $label = "Currency: " + $currency;
            $isTaxable = $tax > 0.0;
            return $subtotal + $tax;
        }

        $finalAmount = calculateTotal(5, 19.99, 'USD');
    "#;

    let typed = parse_and_check(source).expect("Type checking must succeed");

    // Verify inferred variable types inside `calculateTotal` body
    assert_eq!(
        typed.type_info.inferred_var_type("calculateTotal", "$quantity"),
        Some(&RhpType::Int)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("calculateTotal", "$unitPrice"),
        Some(&RhpType::Float)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("calculateTotal", "$subtotal"),
        Some(&RhpType::Float)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("calculateTotal", "$tax"),
        Some(&RhpType::Float)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("calculateTotal", "$label"),
        Some(&RhpType::String)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("calculateTotal", "$isTaxable"),
        Some(&RhpType::Bool)
    );

    // Verify top-level call return type inference
    assert_eq!(
        typed.type_info.top_level_var_type("$finalAmount"),
        Some(&RhpType::Float)
    );
}

#[test]
fn test_dynamic_reassignment_vs_locked_typed_variables_in_body() {
    // Untyped variables inside function bodies can be dynamically reassigned to new types
    let ergonomic_dynamic_body = r#"
        function formatPayload($id: int): string {
            $data = $id * 10;
            $data = "ID-" + string($data);
            return $data;
        }
    "#;
    let typed = parse_and_check(ergonomic_dynamic_body)
        .expect("Dynamic reassignment of untyped local variable must succeed");
    assert_eq!(
        typed.type_info.inferred_var_type("formatPayload", "$data"),
        Some(&RhpType::String)
    );

    // Explicitly typed variable `int $locked = 10;` cannot be reassigned to `string`
    let locked_var_violation = r#"
        function brokenLock($id: int): int {
            $locked: int = $id + 1;
            $locked = "cannot assign string to locked int";
            return $locked;
        }
    "#;
    let err = parse_and_check(locked_var_violation).expect_err("Expected VariableTypeMismatch");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::VariableTypeMismatch {
                var_name,
                expected,
                found,
            } => {
                assert_eq!(var_name, "$locked");
                assert_eq!(expected, "int");
                assert_eq!(found, "string");
            }
            other => panic!("Expected VariableTypeMismatch, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // Prefix typed variable `int $x = 0;` cannot be reassigned to `string`
    let prefix_typed_violation = r#"
        $x: int = 0;
        $x = 'hello';
    "#;
    let err3 = parse_and_check(prefix_typed_violation).expect_err("Expected VariableTypeMismatch on prefix typed var");
    match err3 {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::VariableTypeMismatch {
                var_name,
                expected,
                found,
            } => {
                assert_eq!(var_name, "$x");
                assert_eq!(expected, "int");
                assert_eq!(found, "string");
            }
            other => panic!("Expected VariableTypeMismatch, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_extended_rust_numeric_types_and_fluent_wrappers() {
    let source = r#"
        function computeMetrics($bigId: int, $counter: int, $ratio: float): float {
            $generatedId: int = $bigId + 1;
            $stored: int = 42;
            return $ratio;
        }
    "#;

    let typed = parse_and_check(source).expect("int is i64 and float is the only float declaration");

    assert_eq!(
        typed.type_info.inferred_var_type("computeMetrics", "$bigId"),
        Some(&RhpType::Int)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("computeMetrics", "$counter"),
        Some(&RhpType::Int)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("computeMetrics", "$ratio"),
        Some(&RhpType::Float)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("computeMetrics", "$generatedId"),
        Some(&RhpType::Int)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("computeMetrics", "$stored"),
        Some(&RhpType::Int)
    );
}

#[test]
fn test_collection_subtypes_and_contextual_closure_inference() {
    let source = r#"
        $numbers = Arr::list([1, 2, 3, 4, 5])
            ->map(fn($n) => $n * 2)
            ->filter(fn($n) => $n > 4);

        $scaledFloats = Arr::list([10, 20, 30])
            ->map(fn($n) => $n * 1.5);

        $firstMatch = $numbers->find(fn($n) => $n == 6);
        $dict = Arr::dict(['a' => 'alpha', 'b' => 'beta']);
        $uniqueSet = Arr::set([1, 2, 2, 3]);
        $complexMap = Arr::hashMap(['key' => 100]);
    "#;

    let typed = parse_and_check(source).expect("Collection subtypes and chaining must type-check");

    assert_eq!(
        typed.type_info.top_level_var_type("$numbers"),
        Some(&RhpType::Collection(CollectionSubtype::List(Box::new(
            RhpType::Int
        ))))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$scaledFloats"),
        Some(&RhpType::Collection(CollectionSubtype::List(Box::new(
            RhpType::Float
        ))))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$firstMatch"),
        Some(&RhpType::Nullable(Box::new(RhpType::Int)))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$dict"),
        Some(&RhpType::Collection(CollectionSubtype::Dict(Box::new(
            RhpType::String
        ))))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$uniqueSet"),
        Some(&RhpType::Collection(CollectionSubtype::Set(Box::new(
            RhpType::Int
        ))))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$complexMap"),
        Some(&RhpType::Collection(CollectionSubtype::HashMap(
            Box::new(RhpType::Mixed),
            Box::new(RhpType::Int)
        )))
    );
}

#[test]
fn test_async_future_concurrent_core_and_destructuring_inference() {
    let source = r#"
        class User {}

        function findUser($id: int): User {
            return new User();
        }

        [$user, $posts] = Future::all([
            fn() => findUser(1),
            fn() => HTTP::get("https://api.example.com/posts?user=1")->json()
        ]);

        $delayedMsg = Future::delayed(500, fn() => 'Execution resumed after 500ms');
        $bgFuture = Future::run(fn() => Str::slug('Async Task'));
        $resolvedSlug = $bgFuture->wait();
    "#;

    let typed = parse_and_check(source).expect("Async Future concurrent core must type-check");

    assert_eq!(
        typed.type_info.top_level_var_type("$user"),
        Some(&RhpType::Class("User".to_string()))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$posts"),
        Some(&RhpType::Array(Box::new(RhpType::Mixed)))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$delayedMsg"),
        Some(&RhpType::Future(Box::new(RhpType::String)))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$bgFuture"),
        Some(&RhpType::Future(Box::new(RhpType::String)))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$resolvedSlug"),
        Some(&RhpType::String)
    );
}

#[test]
fn test_boundary_call_argument_and_return_type_verification() {
    // 1. ArgumentTypeMismatch on user function call
    let bad_arg = r#"
        function addNumbers($a: int, $b: int): int {
            return $a + $b;
        }
        $res = addNumbers(10, "not an int");
    "#;
    let err = parse_and_check(bad_arg).expect_err("Expected ArgumentTypeMismatch");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ArgumentTypeMismatch {
                callable_name,
                param_name,
                arg_index,
                expected,
                found,
            } => {
                assert_eq!(callable_name, "addNumbers");
                assert_eq!(param_name, "$b");
                assert_eq!(arg_index, 2);
                assert_eq!(expected, "int");
                assert_eq!(found, "string");
            }
            other => panic!("Expected ArgumentTypeMismatch, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 2. ArityMismatch (too few arguments)
    let too_few_args = r#"
        function addNumbers($a: int, $b: int): int {
            return $a + $b;
        }
        $res = addNumbers(10);
    "#;
    let err = parse_and_check(too_few_args).expect_err("Expected ArityMismatch");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ArityMismatch {
                callable_name,
                min_args,
                max_args,
                found_args,
            } => {
                assert_eq!(callable_name, "addNumbers");
                assert_eq!(min_args, 2);
                assert_eq!(max_args, 2);
                assert_eq!(found_args, 1);
            }
            other => panic!("Expected ArityMismatch, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 3. ReturnTypeMismatch inside function body
    let bad_return = r#"
        function computeScore($base: int): int {
            $msg = "score is " + string($base);
            return $msg;
        }
    "#;
    let err = parse_and_check(bad_return).expect_err("Expected ReturnTypeMismatch");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::ReturnTypeMismatch {
                callable_name,
                expected,
                found,
                ..
            } => {
                assert_eq!(callable_name, "computeScore");
                assert_eq!(expected, "int");
                assert_eq!(found, "string");
            }
            other => panic!("Expected ReturnTypeMismatch, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 4. VoidReturnWithValue when returning value from `: void` function
    let bad_void = r#"
        function logEvent($msg: string): void {
            return 42;
        }
    "#;
    let err = parse_and_check(bad_void).expect_err("Expected VoidReturnWithValue");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::VoidReturnWithValue { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 5. MissingReturnStatement when non-empty body fails to return
    let missing_ret_stmt = r#"
        function getTax($price: float): float {
            $tax = $price * 0.18;
        }
    "#;
    let err = parse_and_check(missing_ret_stmt).expect_err("Expected MissingReturnStatement");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::MissingReturnStatement { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_class_interface_inheritance_and_readonly_property_contracts() {
    // 1. Valid class implementing interface and initializing readonly property in __construct
    let valid_oop = r#"
        interface PaymentGateway {
            public function charge($cents: int, $currency: string): bool;
        }

        class StripeGateway implements PaymentGateway {
            private readonly $secretKey: string;

            public function __construct($secretKey: string) {
                $this->secretKey = $secretKey;
            }

            public function charge($cents: int, $currency: string): bool {
                return $cents > 0;
            }
        }

        function processPayment($gateway: PaymentGateway, $amount: int): bool {
            return $gateway->charge($amount, 'USD');
        }

        $stripe = new StripeGateway('sk_live_123');
        $paid = processPayment($stripe, 5000);
    "#;

    let typed = parse_and_check(valid_oop).expect("Valid OOP interface & subtype polymorphism must pass");
    assert_eq!(
        typed.type_info.top_level_var_type("$stripe"),
        Some(&RhpType::Class("StripeGateway".to_string()))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$paid"),
        Some(&RhpType::Bool)
    );

    // 2. Interface contract violation (wrong return type on implementing method)
    let broken_iface = r#"
        interface PaymentGateway {
            public function charge($cents: int, $currency: string): bool;
        }

        class BrokenGateway implements PaymentGateway {
            public function charge($cents: int, $currency: string): string {
                return "ok";
            }
        }
    "#;
    let err = parse_and_check(broken_iface).expect_err("Expected InterfaceContractViolation");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::InterfaceContractViolation { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 3. Readonly property mutated outside constructor
    let readonly_mutation = r#"
        class Config {
            public readonly $env: string;

            public function __construct($env: string) {
                $this->env = $env;
            }

            public function overrideEnv($newEnv: string): void {
                $this->env = $newEnv;
            }
        }
    "#;
    let err = parse_and_check(readonly_mutation).expect_err("Expected ReadonlyPropertyMutation");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::ReadonlyPropertyMutation { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_branch_type_unification_and_source_map_diagnostics() {
    let source = r#"
        function resolveValue($asNumber: bool): int|string {
            if ($asNumber) {
                $val = 42;
            } else {
                $val = "forty-two";
            }
            return $val;
        }
    "#;

    let typed = parse_and_check(source).expect("Branch union inference must satisfy int|string return");
    assert_eq!(
        typed.type_info.inferred_var_type("resolveValue", "$val"),
        Some(&RhpType::Union(vec![RhpType::Int, RhpType::String]))
    );

    // Verify exact SourceMap diagnostic formatting on TypeError
    let bad_source = "function broken($x: int): int {\n    return \"wrong\";\n}";
    let mut sm = SourceMap::new();
    let file_id = sm.add_file("broken.rhp", bad_source);
    let err = parse_and_check_with_file_id(bad_source, file_id).unwrap_err();
    let diag = sm.format_diagnostic(&err.span(), &err.to_string());
    assert!(diag.contains("--> broken.rhp:2:"));
    assert!(diag.contains("return \"wrong\";"));
}

#[test]
fn test_collection_and_scalar_type_hint_boundary_contracts() {
    let source = r#"
        function transformData($id: int, $prefix: string, $items: List, $attributes: Dict): List {
            return $items;
        }

        $list = Arr::list([10, 20, 30]);
        $dict = Arr::dict(['status' => 'active', 'env' => 'prod']);
        $result = transformData(1, "item_", $list, $dict);
    "#;

    let typed = parse_and_check(source).expect("Scalar and collection boundary contracts must type-check");
    assert_eq!(
        typed.type_info.inferred_var_type("transformData", "$id"),
        Some(&RhpType::Int)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("transformData", "$prefix"),
        Some(&RhpType::String)
    );
    assert_eq!(
        typed.type_info.inferred_var_type("transformData", "$items"),
        Some(&RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::Mixed))))
    );
    assert_eq!(
        typed.type_info.inferred_var_type("transformData", "$attributes"),
        Some(&RhpType::Collection(CollectionSubtype::Dict(Box::new(RhpType::Mixed))))
    );
    assert_eq!(
        typed.type_info.top_level_var_type("$result"),
        Some(&RhpType::Collection(CollectionSubtype::List(Box::new(RhpType::Mixed))))
    );

    // Verify passing mismatched type (e.g., int passed to List parameter) produces ArgumentTypeMismatch
    let bad_call = r#"
        function acceptList($arr: List): void {}
        acceptList(123);
    "#;
    let err = parse_and_check(bad_call).expect_err("ArgumentTypeMismatch expected for List $arr");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::ArgumentTypeMismatch { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_plus_concatenation_strict_typing_and_dot_removal() {
    // 1. Valid string concatenation using +
    let valid_concat = r#"
        function greet($first: string, $last: string): string {
            return $first + " " + $last;
        }
    "#;
    let typed = parse_and_check(valid_concat).expect("string + string should typecheck");
    assert_eq!(
        typed.type_info.inferred_var_type("greet", "$first"),
        Some(&RhpType::String)
    );

    // 2. Valid string concatenation assignment using +=
    let valid_assign = r#"
        function appendMsg($msg: string): string {
            $msg += " world";
            return $msg;
        }
    "#;
    parse_and_check(valid_assign).expect("string += string should typecheck");

    // 3. Mismatched string + int must produce InvalidBinaryOperation
    let bad_concat_str_int = r#"
        function bad($s: string, $n: int): string {
            return $s + $n;
        }
    "#;
    let err = parse_and_check(bad_concat_str_int).expect_err("string + int should be rejected");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::InvalidBinaryOperation { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 4. Mismatched int + string must produce InvalidBinaryOperation
    let bad_concat_int_str = r#"
        function bad($n: int, $s: string): string {
            return $n + $s;
        }
    "#;
    let err = parse_and_check(bad_concat_int_str).expect_err("int + string should be rejected");
    match err {
        FrontendError::Type(te) => assert!(matches!(
            te.kind,
            TypeErrorKind::InvalidBinaryOperation { .. }
        )),
        other => panic!("Expected TypeError, got {:?}", other),
    }

    // 5. Legacy '.' operator must be rejected by parser with educational message
    let dot_code = r#"
        function oldDot($a: string, $b: string): string {
            return $a . $b;
        }
    "#;
    let err = parse_and_check(dot_code).expect_err("dot operator should be rejected at parse time");
    match err {
        FrontendError::Parse(pe) => {
            let msg = pe.to_string();
            assert!(
                msg.contains("The '.' concatenation operator has been removed. Use '+' for string concatenation."),
                "Unexpected parser message: {}",
                msg
            );
        }
        other => panic!("Expected ParseError, got {:?}", other),
    }

    // 6. Legacy '.=' operator must be rejected by parser with educational message
    let dot_assign_code = r#"
        function oldDotAssign($a: string): string {
            $a .= " test";
            return $a;
        }
    "#;
    let err = parse_and_check(dot_assign_code).expect_err("dot-assign operator should be rejected at parse time");
    match err {
        FrontendError::Parse(pe) => {
            let msg = pe.to_string();
            assert!(
                msg.contains("The '.=' operator has been removed. Use '+=' for string concatenation assignment."),
                "Unexpected parser message: {}",
                msg
            );
        }
        other => panic!("Expected ParseError, got {:?}", other),
    }
}

#[test]
fn test_null_union_matches_postfix_nullable() {
    let source = r#"
        function find($id: int?, $other: int|null): User|null {
            return null;
        }
        function both($value: int|string|null): void {}
    "#;
    let program = rhp::parse(source).expect("parse nullable unions");
    let StmtKind::FunctionDecl(find) = &program.statements[0].kind else {
        panic!("expected find");
    };
    let posted = RhpType::from_type_hint(find.params[0].type_hint.as_ref().unwrap(), None).unwrap();
    let union = RhpType::from_type_hint(find.params[1].type_hint.as_ref().unwrap(), None).unwrap();
    let ret = RhpType::from_type_hint(find.return_type.as_ref().unwrap(), None).unwrap();
    assert_eq!(posted, union);
    assert_eq!(posted, RhpType::Nullable(Box::new(RhpType::Int)));
    assert!(ret.is_object_type());
    assert_eq!(ret.to_string(), "User?");

    let StmtKind::FunctionDecl(both) = &program.statements[1].kind else {
        panic!("expected both");
    };
    let mixed_union = RhpType::from_type_hint(both.params[0].type_hint.as_ref().unwrap(), None).unwrap();
    assert!(matches!(mixed_union, RhpType::Union(_)));
    assert_eq!(mixed_union.to_string(), "int|string|null");
}

#[test]
fn test_void_function_result_cannot_be_assigned() {
    let bare = r#"
        function ping() {}
        ping();
    "#;
    parse_and_check(bare).expect("a void call used as a statement is valid");

    let assigned = r#"
        function ping() {}
        $x = ping();
    "#;
    let err = parse_and_check(assigned).expect_err("assigning a void call must fail");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::VoidAssignment { function_name } => {
                assert_eq!(function_name, "ping");
            }
            other => panic!("Expected VoidAssignment, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }

    let declared = r#"
        function ping() {}
        $y: int = ping();
    "#;
    let err = parse_and_check(declared).expect_err("declaring from a void call must fail");
    match err {
        FrontendError::Type(te) => match te.kind {
            TypeErrorKind::VoidAssignment { function_name } => {
                assert_eq!(function_name, "ping");
            }
            other => panic!("Expected VoidAssignment, got {:?}", other),
        },
        other => panic!("Expected TypeError, got {:?}", other),
    }
}

#[test]
fn test_abstract_method_without_return_type_is_void() {
    let source = r#"
        interface Drawer {
            public function draw();
        }
        class Canvas implements Drawer {
            public function draw() {}
        }
    "#;
    parse_and_check(source).expect("an omitted return type on an abstract method is void");
}

#[test]
fn test_unawaited_future_emits_compiler_warning() {
    let source = r#"
        function runTask(): void {
            Future::run(fn() => 123);
        }
    "#;
    let typed = parse_and_check(source).expect("type check should pass with warning");
    let warning = typed
        .type_info
        .warnings
        .iter()
        .find(|w| w.code == "unawaited-future");
    assert!(
        warning.is_some(),
        "Expected unawaited-future warning, got: {:?}",
        typed.type_info.warnings
    );
}


