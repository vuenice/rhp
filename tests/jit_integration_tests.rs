use rhp::{
    execute, parse_and_check, run_source_with_vm, JitCompiler, JitConfig, NativeFrame, NativeSlot,
    PhpValue, RunMode, VM,
};

#[test]
fn test_hot_loop_jit_compilation_and_execution() {
    let source = r#"
        $sum = 0;
        $i = 0;
        while ($i < 1000) {
            $sum = $sum + $i;
            $i++;
        }
    "#;

    // Run with JIT enabled and a low threshold to trigger Cranelift compilation quickly
    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");

    let jit_config = JitConfig::new()
        .with_enabled(true)
        .with_hot_loop_threshold(10)
        .with_opt_level(2);

    let mut vm = VM::new(module, true).with_jit_config(jit_config);
    let _ = vm.run().expect("VM run should succeed");

    // Verify VM output variable
    let sum_val = vm.get_main_variable("sum").expect("sum variable should exist");
    assert_eq!(*sum_val, PhpValue::Int(499500));

    // Verify JIT engine stats
    let jit = vm.jit_engine().expect("JIT engine should be initialized");
    assert!(
        jit.compiled_loops_count() > 0,
        "At least one hot loop should have been compiled by Cranelift"
    );
}

#[test]
fn test_hot_function_invocation_compilation() {
    let source = r#"
        function computeSquare(int $n): int {
            return $n * $n;
        }

        $accum = 0;
        for ($k = 1; $k <= 25; $k++) {
            $accum = $accum + computeSquare($k);
        }
    "#;

    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");

    let jit_config = JitConfig::new()
        .with_enabled(true)
        .with_hot_function_threshold(5)
        .with_hot_loop_threshold(10);

    let mut vm = VM::new(module, true).with_jit_config(jit_config);
    let _ = vm.run().expect("VM run should succeed");

    let accum_val = vm.get_main_variable("accum").expect("accum should exist");
    // sum_{k=1..25} k^2 = (25 * 26 * 51) / 6 = 5525
    assert_eq!(*accum_val, PhpValue::Int(5525));

    let jit = vm.jit_engine().expect("JIT engine should be active");
    assert!(
        jit.compiled_functions_count() > 0,
        "Hot function computeSquare should have been compiled by Cranelift"
    );
}

#[test]
fn test_cranelift_compiler_direct_loop_compilation() {
    let config = JitConfig::new().with_opt_level(2);
    let mut compiler = JitCompiler::new(&config).expect("JitCompiler should initialize");

    // Create a synthetic bytecode chunk computing:
    // r0 = 0 ($sum)
    // r1 = 0 ($i)
    // header:
    // r2 = r1 < 100
    // jump_if_false r2, exit
    // r0 = r0 + r1
    // r1 = r1 + 1
    // jump header
    // exit:
    // return r0
    let mut chunk = rhp::BytecodeChunk::new();
    chunk.num_registers = 4;
    chunk.instructions = vec![
        rhp::OpCode::LoadInt {
            dst: rhp::Reg(0),
            value: 0,
        },
        rhp::OpCode::LoadInt {
            dst: rhp::Reg(1),
            value: 0,
        },
        // Loop header @2
        rhp::OpCode::Binary {
            dst: rhp::Reg(2),
            op: rhp::BinaryOp::Lt,
            lhs: rhp::Reg(1),
            rhs: rhp::Reg(3), // bound loaded via LoadInt
        },
        // Branch @3
        rhp::OpCode::JumpIfFalse {
            cond: rhp::Reg(2),
            target: 7, // jump to return
        },
        // Body @4
        rhp::OpCode::Binary {
            dst: rhp::Reg(0),
            op: rhp::BinaryOp::Add,
            lhs: rhp::Reg(0),
            rhs: rhp::Reg(1),
        },
        // Increment @5
        rhp::OpCode::IncDec {
            dst: rhp::Reg(1),
            src: rhp::Reg(1),
            is_inc: true,
        },
        // Backedge @6
        rhp::OpCode::Jump { target: 2 },
        // Return @7
        rhp::OpCode::Return {
            value: Some(rhp::Reg(0)),
        },
    ];

    let loop_fn = compiler
        .compile_loop("test_direct_loop", &chunk, 2, 6)
        .expect("Compilation of loop should succeed");

    let mut frame = NativeFrame::new(4);
    frame.slots[0] = NativeSlot::new_int(0); // sum
    frame.slots[1] = NativeSlot::new_int(0); // i
    frame.slots[3] = NativeSlot::new_int(100); // bound

    let exit_ip = unsafe { loop_fn(frame.as_mut_ptr()) };
    assert_eq!(exit_ip, 7); // Exited loop to IP 7

    // sum from 0 to 99 = 99 * 100 / 2 = 4950
    assert_eq!(frame.slots[0].int_val, 4950);
    assert_eq!(frame.slots[1].int_val, 100);
}

#[test]
fn test_hot_loop_with_bitwise_and_modulo_arithmetic() {
    let source = r#"
        $mask_accum = 0;
        for ($i = 0; $i < 200; $i++) {
            $mod = $i % 7;
            $shifted = $mod << 2;
            $mask = $shifted & 12;
            $mask_accum = $mask_accum + $mask;
        }
    "#;

    let (_, _val) = run_source_with_vm(source).expect("Source execution should succeed");
    let mut expected_accum = 0i32;
    for i in 0..200i32 {
        let m = i % 7;
        let s = m << 2;
        let mask = s & 12;
        expected_accum += mask;
    }

    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");
    let mut vm = VM::new(module, true).with_jit_config(
        JitConfig::new()
            .with_enabled(true)
            .with_hot_loop_threshold(16),
    );
    vm.run().expect("VM run should succeed");

    let final_accum = vm.get_main_variable("mask_accum").expect("mask_accum exists");
    assert_eq!(*final_accum, PhpValue::Int(expected_accum));
    assert!(
        vm.jit_engine().unwrap().compiled_loops_count() > 0,
        "Bitwise loop should be compiled by Cranelift"
    );
}

#[test]
fn test_collatz_conjecture_hot_loop() {
    let source = r#"
        $n = 27;
        $steps = 0;
        while ($n > 1) {
            if ($n % 2 == 0) {
                $n = $n / 2;
            } else {
                $n = 3 * $n + 1;
            }
            $steps++;
        }
    "#;

    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");
    let mut vm = VM::new(module, true).with_jit_config(
        JitConfig::new()
            .with_enabled(true)
            .with_hot_loop_threshold(10),
    );
    vm.run().expect("VM run should succeed");

    let steps = vm.get_main_variable("steps").expect("steps should exist");
    // Collatz sequence for 27 takes exactly 111 steps
    assert_eq!(*steps, PhpValue::Int(111));
    let n = vm.get_main_variable("n").expect("n should exist");
    assert_eq!(*n, PhpValue::Int(1));
}

#[test]
fn test_nested_loops_compilation() {
    let source = r#"
        $total = 0;
        for ($i = 0; $i < 30; $i++) {
            for ($j = 0; $j < 20; $j++) {
                $total = $total + 1;
            }
        }
    "#;

    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");
    let mut vm = VM::new(module, true).with_jit_config(
        JitConfig::new()
            .with_enabled(true)
            .with_hot_loop_threshold(5),
    );
    vm.run().expect("VM run should succeed");

    let total = vm.get_main_variable("total").expect("total should exist");
    assert_eq!(*total, PhpValue::Int(600));
}

#[test]
fn test_safe_deoptimization_and_fallback_to_interpreter() {
    // A loop containing an operation not supported in pure numerical JIT (e.g. echo or complex object)
    // should gracefully fall back to the interpreter without failing or corrupting state.
    let source = r#"
        $accum = 0;
        for ($i = 0; $i < 40; $i++) {
            echo "iter";
            $accum = $accum + $i;
        }
    "#;

    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");
    let mut vm = VM::new(module, true).with_jit_config(
        JitConfig::new()
            .with_enabled(true)
            .with_hot_loop_threshold(5),
    );
    vm.run().expect("VM run should succeed");

    let accum = vm.get_main_variable("accum").expect("accum should exist");
    // sum 0..39 = 39 * 40 / 2 = 780
    assert_eq!(*accum, PhpValue::Int(780));
    assert_eq!(vm.output().matches("iter").count(), 40);
}

#[test]
fn test_dual_engine_run_mode_build_with_jit() {
    let expr = rhp::parse_expr("5 * 8 + 2").expect("Parse expr");
    let mut env = rhp::Environment::new();

    let result = execute(
        RunMode::Build {
            jit_enabled: true,
            opt_level: 2,
        },
        &expr,
        &mut env,
    )
    .expect("Execute should succeed");

    assert_eq!(result, PhpValue::Int(42));
}

#[test]
fn test_jit_disabled_flag_skips_compilation() {
    let source = r#"
        $sum = 0;
        for ($i = 0; $i < 100; $i++) {
            $sum = $sum + $i;
        }
    "#;

    let typed = parse_and_check(source).expect("Parsing should succeed");
    let module = rhp::lower_typed_program(&typed).expect("Lowering should succeed");
    let mut vm = VM::new(module, false);
    vm.run().expect("VM run should succeed");

    assert!(!vm.is_jit_enabled());
    assert!(vm.jit_engine().is_none());
    assert_eq!(*vm.get_main_variable("sum").unwrap(), PhpValue::Int(4950));
}

#[test]
fn test_jit_optimization_levels_configuration() {
    for opt in [0, 1, 2] {
        let config = JitConfig::new().with_opt_level(opt);
        let compiler = JitCompiler::new(&config);
        assert!(compiler.is_ok(), "Compiler should initialize for opt_level {}", opt);
    }
}
