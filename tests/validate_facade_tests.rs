use rhp::{
    compile_source, is_legacy_php_function, is_static_facade, is_static_only, run_source_with_vm,
    OpCode, PhpValue,
};

#[test]
fn test_validate_facade_registration_and_signature_mapping() {
    // 1. Verify Validate is registered as a native static facade
    assert!(is_static_facade("Validate"));
    assert!(is_static_only("Validate"));

    // 2. Verify legacy PHP functions are recognized
    let legacy_funcs = [
        ("is_null", "Validate", "null"),
        ("is_int", "Validate", "int"),
        ("is_integer", "Validate", "int"),
        ("is_long", "Validate", "int"),
        ("is_bool", "Validate", "bool"),
        ("is_boolean", "Validate", "bool"),
        ("is_float", "Validate", "float"),
        ("is_double", "Validate", "float"),
        ("is_real", "Validate", "float"),
        ("is_numeric", "Validate", "numeric"),
        ("is_scalar", "Validate", "scalar"),
    ];

    for (php_fn, facade, method) in legacy_funcs {
        assert!(
            is_legacy_php_function(php_fn),
            "Function {} should be recognized as legacy PHP function",
            php_fn
        );
        let mapping = rhp::get_legacy_php_function(php_fn)
            .unwrap_or_else(|| panic!("Mapping for {} should exist", php_fn));
        assert_eq!(mapping.php_name, php_fn);
        assert_eq!(mapping.rhp_facade, facade);
        assert_eq!(mapping.rhp_method, method);
    }
}

#[test]
fn test_validate_facade_direct_calls() {
    let source = r#"
        // Validate::null
        $n1 = Validate::null(null);
        $n2 = Validate::null(0);
        $n3 = Validate::null("");
        $n4 = Validate::null(false);

        // Validate::int
        $i1 = Validate::int(42);
        $i2 = Validate::int(0);
        $i3 = Validate::int(100);
        $i4 = Validate::int(3.14);
        $i5 = Validate::int("42");

        // Validate::bool
        $b1 = Validate::bool(true);
        $b2 = Validate::bool(false);
        $b3 = Validate::bool(1);
        $b4 = Validate::bool(null);
        $b5 = Validate::bool("true");

        // Validate::float
        $f1 = Validate::float(3.14);
        $f2 = Validate::float(2.718);
        $f3 = Validate::float(42);
        $f4 = Validate::float("3.14");

        // Validate::numeric
        $num1 = Validate::numeric(42);
        $num2 = Validate::numeric(3.14);
        $num3 = Validate::numeric("42");
        $num4 = Validate::numeric("  -13.37  ");
        $num5 = Validate::numeric("+0123.45e6");
        $num6 = Validate::numeric(".5");
        $num7 = Validate::numeric("abc");
        $num8 = Validate::numeric("42abc");
        $num9 = Validate::numeric(null);
        $num10 = Validate::numeric(true);

        // Validate::scalar
        $s1 = Validate::scalar(42);
        $s2 = Validate::scalar(3.14);
        $s3 = Validate::scalar("hello");
        $s4 = Validate::scalar(true);
        $s5 = Validate::scalar(false);
        $s6 = Validate::scalar(null);
        $s7 = Validate::scalar([1, 2, 3]);
        $s8 = Validate::scalar(new stdClass());
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Validate facade direct calls should execute");

    // null
    assert_eq!(vm.get_main_variable("$n1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$n2"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$n3"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$n4"), Some(&PhpValue::Bool(false)));

    // int
    assert_eq!(vm.get_main_variable("$i1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$i2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$i3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$i4"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$i5"), Some(&PhpValue::Bool(false)));

    // bool
    assert_eq!(vm.get_main_variable("$b1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$b2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$b3"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$b4"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$b5"), Some(&PhpValue::Bool(false)));

    // float
    assert_eq!(vm.get_main_variable("$f1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f3"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$f4"), Some(&PhpValue::Bool(false)));

    // numeric
    assert_eq!(vm.get_main_variable("$num1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$num2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$num3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$num4"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$num5"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$num6"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$num7"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$num8"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$num9"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$num10"), Some(&PhpValue::Bool(false)));

    // scalar
    assert_eq!(vm.get_main_variable("$s1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$s2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$s3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$s4"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$s5"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$s6"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$s7"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$s8"), Some(&PhpValue::Bool(false)));
}

#[test]
fn test_legacy_php_functions_lowering_and_execution() {
    let source = r#"
        $valNull = null;
        $valInt = 123;
        $valFloat = 45.67;
        $valBool = true;
        $valStr = "99.9";
        $valArr = [1, 2];

        // Legacy function calls
        $t_null = is_null($valNull);
        $f_null = is_null($valInt);

        $t_int = is_int($valInt);
        $t_integer = is_integer($valInt);
        $t_long = is_long($valInt);
        $f_int = is_int($valFloat);

        $t_bool = is_bool($valBool);
        $f_bool = is_bool($valInt);

        $t_float = is_float($valFloat);
        $t_double = is_double($valFloat);
        $t_real = is_real($valFloat);
        $f_float = is_float($valInt);

        $t_num1 = is_numeric($valInt);
        $t_num2 = is_numeric($valFloat);
        $t_num3 = is_numeric($valStr);
        $f_num = is_numeric($valArr);

        $t_scalar1 = is_scalar($valInt);
        $t_scalar2 = is_scalar($valFloat);
        $t_scalar3 = is_scalar($valBool);
        $t_scalar4 = is_scalar($valStr);
        $f_scalar1 = is_scalar($valNull);
        $f_scalar2 = is_scalar($valArr);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Legacy is_* functions should execute");

    assert_eq!(vm.get_main_variable("$t_null"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f_null"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$t_int"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_integer"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_long"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f_int"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$t_bool"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f_bool"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$t_float"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_double"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_real"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f_float"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$t_num1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_num2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_num3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f_num"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$t_scalar1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_scalar2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_scalar3"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$t_scalar4"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$f_scalar1"), Some(&PhpValue::Bool(false)));
    assert_eq!(vm.get_main_variable("$f_scalar2"), Some(&PhpValue::Bool(false)));
}

#[test]
fn test_is_functions_compile_to_validate_facade_call_opcode() {
    let source = r#"
        $a = 10;
        $r1 = is_null($a);
        $r2 = Validate::null($a);
        $r3 = is_int($a);
        $r4 = is_bool($a);
        $r5 = is_float($a);
        $r6 = is_numeric($a);
        $r7 = is_scalar($a);
    "#;

    let module = compile_source(source).expect("Source compilation should succeed");
    let main_chunk = &module.main_chunk;

    // Count OpCode::FacadeCall targeting Validate
    let mut validate_calls = Vec::new();
    for instr in &main_chunk.instructions {
        if let OpCode::FacadeCall { facade, method, .. } = instr {
            if let Some(rhp::Constant::String(facade_str)) = main_chunk.constants.get(facade.index()) {
                if facade_str == "Validate" {
                    if let Some(rhp::Constant::String(method_str)) = main_chunk.constants.get(method.index()) {
                        validate_calls.push(method_str.clone());
                    }
                }
            }
        }
    }

    // Both is_null and Validate::null compile to Validate::null, plus int, bool, float, numeric, scalar
    assert_eq!(
        validate_calls,
        vec!["null", "null", "int", "bool", "float", "numeric", "scalar"]
    );
}

#[test]
fn test_class_exists_and_function_exists_with_validate() {
    let source = r#"
        $validateClass = class_exists('Validate');
        $validateClassLower = class_exists('validate');
        $nullFunc = function_exists('is_null');
        $intFunc = function_exists('is_int');
        $boolFunc = function_exists('is_bool');
        $floatFunc = function_exists('is_float');
        $numFunc = function_exists('is_numeric');
        $scalarFunc = function_exists('is_scalar');
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Reflection functions should succeed");

    assert_eq!(vm.get_main_variable("$validateClass"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$validateClassLower"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$nullFunc"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$intFunc"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$boolFunc"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$floatFunc"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$numFunc"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$scalarFunc"), Some(&PhpValue::Bool(true)));
}
