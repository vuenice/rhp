use std::cell::RefCell;
use std::rc::Rc;

use rhp::vm::fiber::{set_current_yielder, YieldReason};
use rhp::vm::{run_source_with_dev_mode, run_source_with_vm, run_source_with_vm_and_jit, VM};
use rhp::{ExecutionError, PhpValue, RuntimeErrorKind};

#[test]
fn test_production_instruction_budget_tick_slicing_and_preemption() {
    // Production default: budget of 10,000 ticks, no loop warnings or hard limits
    let code = r#"
        $sum = 0;
        for ($i = 0; $i < 35000; $i++) {
            $sum = $sum + 1;
        }
        return $sum;
    "#;

    let typed = rhp::type_system::parse_and_check(code).unwrap();
    let module = rhp::ir::lower_typed_program(&typed).unwrap();
    let mut vm = VM::new(module, false).with_dev_mode(false);
    let result = vm.run().expect("Production loop should run to completion");

    assert_eq!(result, PhpValue::Int(35000));
    // 35,000 iterations with default 10,000 tick budget should trigger at least 3 preemptions
    assert!(
        vm.total_preemptions >= 3,
        "Expected at least 3 preemptions, got {}",
        vm.total_preemptions
    );
    // In production mode, output_buffer should contain no warnings
    assert!(
        !vm.output().contains("Warning: Loop"),
        "Production mode must not emit loop warnings"
    );
}

#[test]
fn test_fiber_yielder_suspend_called_on_preemption() {
    let suspend_count = Rc::new(RefCell::new(0usize));
    let suspend_count_clone = Rc::clone(&suspend_count);

    set_current_yielder(Some(Box::new(move |reason: YieldReason| {
        if reason == YieldReason::CooperativeYield {
            *suspend_count_clone.borrow_mut() += 1;
        }
    })));

    let code = r#"
        $count = 0;
        while ($count < 25000) {
            $count = $count + 1;
        }
        return $count;
    "#;

    let typed = rhp::type_system::parse_and_check(code).unwrap();
    let module = rhp::ir::lower_typed_program(&typed).unwrap();
    let mut vm = VM::new(module, false).with_instruction_budget(10_000);
    let val = vm.run().unwrap();
    assert_eq!(val, PhpValue::Int(25000));

    // Clear the thread-local yielder
    set_current_yielder(None);

    let recorded_suspends = *suspend_count.borrow();
    assert!(
        recorded_suspends >= 2,
        "CURRENT_YIELDER should have been invoked on tick exhaustion, got {}",
        recorded_suspends
    );
}

#[test]
fn test_dev_mode_loop_warning_at_1000_iterations() {
    let code = r#"
        $sum = 0;
        for ($i = 0; $i < 1500; $i++) {
            $sum = $sum + 1;
        }
        return $sum;
    "#;

    let (vm, val) = run_source_with_dev_mode(code, true)
        .expect("Loop under 100,000 iterations in Dev Mode should complete");
    assert_eq!(val, PhpValue::Int(1500));

    // Must emit a warning about exceeding 1,000 iterations
    let out = vm.output();
    assert!(
        out.contains("Warning: Loop in <main>") && out.contains("exceeded 1000 iterations in Dev Mode"),
        "Dev mode should emit warning when exceeding 1000 iterations. Got output: {}",
        out
    );

    // Ensure the warning was emitted only once
    let count = out.matches("exceeded 1000 iterations").count();
    assert_eq!(count, 1, "Warning should be emitted only once per loop");
}

#[test]
fn test_dev_mode_loop_hard_limit_at_100000_iterations() {
    // In Dev Mode, loop reaching 100,000 iterations must trigger InfiniteLoopLimitExceeded
    let code = r#"
        $i = 0;
        while (true) {
            $i = $i + 1;
        }
    "#;

    let res = run_source_with_dev_mode(code, true);
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("Infinite loop in Dev Mode must hit hard limit of 100,000 iterations"),
    };

    match err {
        ExecutionError::Runtime(rt) => match rt.kind {
            RuntimeErrorKind::InfiniteLoopLimitExceeded { limit, .. } => {
                assert_eq!(limit, 100_000);
            }
            other => panic!("Expected InfiniteLoopLimitExceeded, got {:?}", other),
        },
        other => panic!("Expected Runtime error, got {:?}", other),
    }
}

#[test]
fn test_dev_mode_exemption_via_ini_set() {
    // Developer can exempt the loop or disable limit by calling ini_set('max_loop_iterations', '0')
    let code = r#"
        ini_set('max_loop_iterations', '0');
        $count = 0;
        while ($count < 105000) {
            $count = $count + 1;
        }
        return $count;
    "#;

    let (_, val) = run_source_with_dev_mode(code, true)
        .expect("Loop with ini_set exemption should run past 100,000 iterations in Dev Mode");
    assert_eq!(val, PhpValue::Int(105000));
}

#[test]
fn test_ini_get_and_set_loop_limits_and_budget() {
    let code = r#"
        $b1 = ini_get('instruction_budget');
        ini_set('instruction_budget', '5000');
        $b2 = ini_get('instruction_budget');

        $l1 = ini_get('max_loop_iterations');
        ini_set('max_loop_iterations', '250000');
        $l2 = ini_get('max_loop_iterations');

        $w1 = ini_get('warn_loop_iterations');
        ini_set('warn_loop_iterations', '2000');
        $w2 = ini_get('warn_loop_iterations');

        return [$b1, $b2, $l1, $l2, $w1, $w2];
    "#;

    let (_vm, val) = run_source_with_vm(code).unwrap();
    if let PhpValue::Array(items) | PhpValue::List(items) = val {
        let b = items.borrow();
        assert_eq!(b[0], PhpValue::string("10000"));
        assert_eq!(b[1], PhpValue::string("5000"));
        assert_eq!(b[2], PhpValue::string("100000"));
        assert_eq!(b[3], PhpValue::string("250000"));
        assert_eq!(b[4], PhpValue::string("1000"));
        assert_eq!(b[5], PhpValue::string("2000"));
    } else {
        panic!("Expected array of configuration values, got {:?}", val);
    }
}

#[test]
fn test_cranelift_jit_loop_budget_and_preemption() {
    // Hot loop with JIT compilation enabled
    let code = r#"
        $sum = 0;
        for ($i = 0; $i < 30000; $i++) {
            $sum = $sum + 1;
        }
        return $sum;
    "#;

    let (vm, val) = run_source_with_vm_and_jit(code, true)
        .expect("JIT hot loop should execute with tick budgeting");
    assert_eq!(val, PhpValue::Int(30000));
    assert!(
        vm.total_preemptions >= 2,
        "JIT loop should record preemptions on tick exhaustion, got {}",
        vm.total_preemptions
    );
}
