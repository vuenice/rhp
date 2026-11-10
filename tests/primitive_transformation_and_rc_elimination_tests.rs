use rhp::{
    compile_to_bytecode, execute, parse_expr, Environment, OpCode, PhpValue, Reg, RhpType,
    RunMode,
};

#[test]
fn test_dev_and_build_direct_transformations_int_float_bool_null() {
    let mut env = Environment::new();

    // 1. Dev Mode (RunMode::Dev): int directly transforms to i32, float to f32, bool to bool, null to None
    let expr_int = parse_expr("int('123')").unwrap();
    let dev_int = execute(
        RunMode::Dev {
            hot_reload: false,
            dap_port: None,
        },
        &expr_int,
        &mut env,
    )
    .unwrap();
    assert_eq!(dev_int, PhpValue::Int(123_i64));
    assert_eq!(dev_int.to_i64(), Some(123_i64));
    assert_eq!(dev_int.requires_rc(), false);
    assert_eq!(dev_int.is_scalar_primitive(), true);

    let expr_float = parse_expr("float('3.14')").unwrap();
    let dev_float = execute(
        RunMode::Dev {
            hot_reload: false,
            dap_port: None,
        },
        &expr_float,
        &mut env,
    )
    .unwrap();
    assert_eq!(dev_float, PhpValue::Float(3.14_f64));
    assert_eq!(dev_float.to_f64(), Some(3.14_f64));
    assert_eq!(dev_float.requires_rc(), false);
    assert_eq!(dev_float.is_scalar_primitive(), true);

    let expr_bool = parse_expr("bool(1)").unwrap();
    let dev_bool = execute(
        RunMode::Dev {
            hot_reload: false,
            dap_port: None,
        },
        &expr_bool,
        &mut env,
    )
    .unwrap();
    assert_eq!(dev_bool, PhpValue::Bool(true));
    assert_eq!(dev_bool.to_bool(), Some(true));
    assert_eq!(dev_bool.requires_rc(), false);
    assert_eq!(dev_bool.is_scalar_primitive(), true);

    let expr_null = parse_expr("null").unwrap();
    let dev_null = execute(
        RunMode::Dev {
            hot_reload: false,
            dap_port: None,
        },
        &expr_null,
        &mut env,
    )
    .unwrap();
    assert_eq!(dev_null, PhpValue::Null);
    assert_eq!(dev_null.as_none(), None);
    assert_eq!(dev_null.requires_rc(), false);
    assert_eq!(dev_null.is_scalar_primitive(), true);

    // 2. Build Mode (RunMode::Build): Bytecode VM with direct opcodes
    let build_int = execute(
        RunMode::Build {
            jit_enabled: true,
            opt_level: 2,
        },
        &expr_int,
        &mut env,
    )
    .unwrap();
    assert_eq!(build_int, PhpValue::Int(123_i64));

    let build_float = execute(
        RunMode::Build {
            jit_enabled: true,
            opt_level: 2,
        },
        &expr_float,
        &mut env,
    )
    .unwrap();
    assert_eq!(build_float, PhpValue::Float(3.14_f64));

    let build_bool = execute(
        RunMode::Build {
            jit_enabled: true,
            opt_level: 2,
        },
        &expr_bool,
        &mut env,
    )
    .unwrap();
    assert_eq!(build_bool, PhpValue::Bool(true));

    let build_null = execute(
        RunMode::Build {
            jit_enabled: true,
            opt_level: 2,
        },
        &expr_null,
        &mut env,
    )
    .unwrap();
    assert_eq!(build_null, PhpValue::Null);
    assert_eq!(build_null.as_none(), None);
}

#[test]
fn test_build_mode_emits_direct_native_opcodes_without_constant_pool() {
    let expr_exact = parse_expr("2.5").unwrap();
    let chunk_exact = compile_to_bytecode(&expr_exact).unwrap();
    assert_eq!(
        chunk_exact.instructions[0],
        OpCode::LoadFloat {
            dst: Reg(0),
            bits: 2.5_f64.to_bits(),
        }
    );
    let expr_float = parse_expr("2.718").unwrap();
    let chunk_float = compile_to_bytecode(&expr_float).unwrap();
    assert_eq!(
        chunk_float.instructions[0],
        OpCode::LoadFloat {
            dst: Reg(0),
            bits: 2.718_f64.to_bits(),
        }
    );

    // Verify int literal emits OpCode::LoadInt directly
    let expr_int = parse_expr("42").unwrap();
    let chunk_int = compile_to_bytecode(&expr_int).unwrap();
    assert_eq!(
        chunk_int.instructions[0],
        OpCode::LoadInt {
            dst: Reg(0),
            value: 42_i32,
        }
    );

    // Verify bool literal emits OpCode::LoadBool directly
    let expr_bool = parse_expr("true").unwrap();
    let chunk_bool = compile_to_bytecode(&expr_bool).unwrap();
    assert_eq!(
        chunk_bool.instructions[0],
        OpCode::LoadBool {
            dst: Reg(0),
            value: true,
        }
    );

    // Verify null literal emits OpCode::LoadNull directly
    let expr_null = parse_expr("null").unwrap();
    let chunk_null = compile_to_bytecode(&expr_null).unwrap();
    assert_eq!(
        chunk_null.instructions[0],
        OpCode::LoadNull { dst: Reg(0) }
    );
}

#[test]
fn test_null_directly_used_as_none_and_from_option() {
    // 1. None converts into PhpValue::Null
    let none_val: Option<i32> = None;
    let php_null: PhpValue = none_val.into();
    assert_eq!(php_null, PhpValue::Null);

    // 2. Some(T) converts into PhpValue
    let some_val: Option<i32> = Some(42);
    let php_int: PhpValue = some_val.into();
    assert_eq!(php_int, PhpValue::Int(42));

    let some_float: Option<f32> = Some(1.25);
    let php_fl: PhpValue = some_float.into();
    assert_eq!(php_fl, PhpValue::Float(1.25));

    let some_bool: Option<bool> = Some(true);
    let php_b: PhpValue = some_bool.into();
    assert_eq!(php_b, PhpValue::Bool(true));

    // 3. as_none() maps PhpValue::Null to None, and non-null to Some(&PhpValue)
    assert_eq!(php_null.as_none(), None);
    assert_eq!(php_int.as_none(), Some(&PhpValue::Int(42)));
    assert_eq!(php_null.into_option(), None);
}

#[test]
fn test_zero_rc_overhead_for_all_primitives() {
    let null_val = PhpValue::Null;
    let bool_val = PhpValue::from(true);
    let int_val = PhpValue::from(100_i32);
    let float_val = PhpValue::from(50.5_f32);

    // None of these primitive scalars require Rc
    assert!(!null_val.requires_rc());
    assert!(!bool_val.requires_rc());
    assert!(!int_val.requires_rc());
    assert!(!float_val.requires_rc());

    // All are zero-cost unboxed scalars
    assert!(null_val.is_scalar_primitive());
    assert!(bool_val.is_scalar_primitive());
    assert!(int_val.is_scalar_primitive());
    assert!(float_val.is_scalar_primitive());

    // Check ref_count is always 1 and unshared
    assert_eq!(null_val.ref_count(), 1);
    assert_eq!(bool_val.ref_count(), 1);
    assert_eq!(int_val.ref_count(), 1);
    assert_eq!(float_val.ref_count(), 1);
    assert!(!null_val.is_shared());
    assert!(!bool_val.is_shared());
    assert!(!int_val.is_shared());
    assert!(!float_val.is_shared());

    // Verify RhpType native Rust mapping and Rc check
    assert_eq!(RhpType::Int.to_native_rust_type(), "i64");
    assert_eq!(RhpType::Float.to_native_rust_type(), "f64");
    assert_eq!(RhpType::Bool.to_native_rust_type(), "bool");
    assert_eq!(RhpType::Null.to_native_rust_type(), "None");

    assert!(!RhpType::Int.requires_rc());
    assert!(!RhpType::Float.requires_rc());
    assert!(!RhpType::Bool.requires_rc());
    assert!(!RhpType::Null.requires_rc());

    assert!(RhpType::Int.is_scalar_primitive());
    assert!(RhpType::Float.is_scalar_primitive());
    assert!(RhpType::Bool.is_scalar_primitive());
    assert!(RhpType::Null.is_scalar_primitive());
}

#[test]
fn test_script_execution_with_cased_and_primitive_hints() {
    let source = r#"
        function compute(int $count, float $rate, bool $active, ?int $fallback): float {
            if (!$active) {
                return 0.0;
            }
            float $res = float($count) * $rate;
            if ($fallback != null) {
                $res = $res + float($fallback);
            }
            return $res;
        }

        $result = compute(10, 2.5, true, null);
    "#;

    let (vm, _) = rhp::run_source_with_vm(source).expect("Script execution should pass");
    assert_eq!(vm.get_main_variable("$result"), Some(&PhpValue::Float(25.0_f64)));
}

#[test]
fn test_native_f64_float_operations_and_ieee754_semantics() {
    // 1. IEEE-754 precision: 0.1 + 0.2 is strictly 0.30000000000000004 without decimal rounding
    let source = r#"
        $sum = 0.1 + 0.2;
        $sub = 1.0 - 0.9;
        $mul = 2.5 * 4.0;
        $div = 7.0 / 2.0;
        $pow = 2.0 ** 3.0;
        $mod = 5.5 % 2.0;
    "#;

    let (vm, _) = rhp::run_source_with_vm(source).expect("Float operations should succeed");
    assert_eq!(vm.get_main_variable("$sum"), Some(&PhpValue::Float(0.1_f64 + 0.2_f64)));
    assert_eq!(vm.get_main_variable("$sum"), Some(&PhpValue::Float(0.30000000000000004_f64)));
    assert_eq!(vm.get_main_variable("$sub"), Some(&PhpValue::Float(1.0_f64 - 0.9_f64)));
    assert_eq!(vm.get_main_variable("$mul"), Some(&PhpValue::Float(10.0_f64)));
    assert_eq!(vm.get_main_variable("$div"), Some(&PhpValue::Float(3.5_f64)));
    assert_eq!(vm.get_main_variable("$pow"), Some(&PhpValue::Float(8.0_f64)));
    assert_eq!(vm.get_main_variable("$mod"), Some(&PhpValue::Float(1.5_f64)));

    // 2. Value equality and canonicalization: -0.0 and +0.0 have the same bits
    let pos_zero = PhpValue::float(0.0);
    let neg_zero = PhpValue::float(-0.0);
    assert_eq!(pos_zero, neg_zero);
    if let (PhpValue::Float(a), PhpValue::Float(b)) = (&pos_zero, &neg_zero) {
        assert_eq!(a.to_bits(), b.to_bits());
        assert_eq!(a.to_bits(), 0.0_f64.to_bits());
    } else {
        panic!("Expected Float variants");
    }

    // 3. Facade Float helper operations
    let source_facade = r#"
        $b_add = Float::bAdd(0.1, 0.2);
        $add = Float::add(0.1, 0.2);
    "#;
    let (vm2, _) = rhp::run_source_with_vm(source_facade).expect("Float facade should succeed");
    assert_eq!(vm2.get_main_variable("$b_add"), Some(&PhpValue::Float(0.30000000000000004_f64)));
    assert_eq!(vm2.get_main_variable("$add"), Some(&PhpValue::Float(0.30000000000000004_f64)));
}

