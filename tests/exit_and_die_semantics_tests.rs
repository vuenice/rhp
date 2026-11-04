use rhp::{
    clear_exit_hook, run_source_with_vm, set_exit_hook, ExecutionError, PhpValue, RuntimeErrorKind,
};
use std::sync::{Arc, Mutex};

static TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_exit_executes_shutdown_functions_and_destructors() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let captured_exit = Arc::new(Mutex::new(None));
    let cap_clone = Arc::clone(&captured_exit);
    set_exit_hook(move |code| {
        *cap_clone.lock().unwrap() = Some(code);
    });

    let src = r#"
        class ResourceCleaner {
            public string $name;

            public function __construct(string $name) {
                $this->name = $name;
            }

            public function __destruct() {
                echo "destruct:" . $this->name . ";";
            }
        }

        register_shutdown_function(function() {
            echo "shutdown_fn_1;";
        });

        register_shutdown_function(function() {
            echo "shutdown_fn_2;";
        });

        $res1 = new ResourceCleaner("first");
        $res2 = new ResourceCleaner("second");

        echo "running_main;";
        exit(0);
        echo "unreachable;";
    "#;

    let typed = rhp::type_system::parse_and_check(src).unwrap();
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = rhp::lower_typed_program(&typed).unwrap();
    let mut vm = rhp::VM::new(module, false).with_type_hierarchy(hierarchy);
    let _ = vm.run();
    clear_exit_hook();

    assert_eq!(
        vm.output(),
        "running_main;shutdown_fn_1;shutdown_fn_2;destruct:second;destruct:first;"
    );
    assert_eq!(*captured_exit.lock().unwrap(), Some(0));
}

#[test]
fn test_exit_is_uncatchable_by_try_catch() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let captured_exit = Arc::new(Mutex::new(None));
    let cap_clone = Arc::clone(&captured_exit);
    set_exit_hook(move |code| {
        *cap_clone.lock().unwrap() = Some(code);
    });

    let src = r#"
        class Guard {
            public function __destruct() {
                echo "guard_destructed;";
            }
        }

        register_shutdown_function(function() {
            echo "shutdown_called;";
        });

        $g = new Guard();

        try {
            echo "before_exit;";
            exit(42);
            echo "after_exit;";
        } catch (\Throwable $e) {
            echo "caught_throwable;";
        } catch (\Exception $e) {
            echo "caught_exception;";
        }

        echo "should_not_reach_here;";
    "#;

    let res = run_source_with_vm(src);
    clear_exit_hook();

    assert_eq!(*captured_exit.lock().unwrap(), Some(42));

    match res {
        Err(ExecutionError::Runtime(rt_err)) => {
            assert_eq!(rt_err.kind, RuntimeErrorKind::Exit(42));
        }
        Ok(_) => panic!("exit(42) should terminate via Exit signal"),
        Err(other) => panic!("Unexpected error: {:?}", other),
    }
}

#[test]
fn test_die_switched_functionality_hard_exits_without_shutdown_or_destructors() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let captured_exit = Arc::new(Mutex::new(None));
    let cap_clone = Arc::clone(&captured_exit);
    set_exit_hook(move |code| {
        *cap_clone.lock().unwrap() = Some(code);
    });

    let src = r#"
        class ShouldNotDestruct {
            public function __destruct() {
                echo "SHOULD_NOT_DESTRUCT;";
            }
        }

        register_shutdown_function(function() {
            echo "SHOULD_NOT_RUN_SHUTDOWN;";
        });

        $obj = new ShouldNotDestruct();
        echo "about_to_die;";
        die("died_immediately;");
        echo "unreachable;";
    "#;

    let res = run_source_with_vm(src);
    clear_exit_hook();

    // die() uses execute_process_exit directly:
    // It prints the die message, skips shutdown functions and destructors, and returns Die signal when hooked.
    assert_eq!(*captured_exit.lock().unwrap(), Some(0));

    match res {
        Err(ExecutionError::Runtime(rt_err)) => {
            assert_eq!(rt_err.kind, RuntimeErrorKind::Die(0));
        }
        Ok(_) => panic!("die() should terminate immediately with Die signal"),
        Err(other) => panic!("Unexpected error: {:?}", other),
    }
}

#[test]
fn test_exit_with_string_message_and_numeric_code() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let captured_exit = Arc::new(Mutex::new(None));
    let cap_clone = Arc::clone(&captured_exit);
    set_exit_hook(move |code| {
        *cap_clone.lock().unwrap() = Some(code);
    });

    let src = r#"
        register_shutdown_function(function() {
            echo "after_exit_string;";
        });

        exit("Script stopping now;");
    "#;

    let typed = rhp::type_system::parse_and_check(src).unwrap();
    let hierarchy = typed.type_info.hierarchy.clone();
    let module = rhp::lower_typed_program(&typed).unwrap();
    let mut vm = rhp::VM::new(module, false).with_type_hierarchy(hierarchy);
    let _ = vm.run();
    clear_exit_hook();

    assert_eq!(*captured_exit.lock().unwrap(), Some(0));
    assert_eq!(vm.output(), "Script stopping now;after_exit_string;");
}

#[test]
fn test_normal_completion_also_runs_shutdown_and_destructors() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let src = r#"
        class AutoClean {
            public string $id;
            public function __construct(string $id) {
                $this->id = $id;
            }
            public function __destruct() {
                echo "clean:" . $this->id . ";";
            }
        }

        register_shutdown_function(function($msg) {
            echo "shutdown:" . $msg . ";";
        }, "custom_arg");

        $a = new AutoClean("A");
        $b = new AutoClean("B");
        echo "done_main;";
    "#;

    let (vm, val) = run_source_with_vm(src).expect("Script should run normally");
    assert_eq!(val, PhpValue::Null);
    assert_eq!(vm.output(), "done_main;shutdown:custom_arg;clean:B;clean:A;");
}
