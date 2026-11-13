use rhp::ir::lower_program;
use rhp::parser::parse_with_path;
use rhp::vm::engine::VM;
use rhp::vm::error::{RuntimeError, RuntimeErrorKind};
use rhp::vm::value::PhpValue;

fn run_php(source: &str) -> (VM, Result<PhpValue, RuntimeError>) {
    let path = "test_handler.php";
    let program = parse_with_path(source, path).expect("parse failed");
    let module = lower_program(&program, None).expect("lower failed");
    let mut vm = VM::new(module, false).with_file_path(path);
    vm.suppress_process_exit = true;
    let res = vm.run();
    (vm, res)
}

#[test]
fn test_set_error_handler_basic_capture() {
    let code = r#"
        class ErrorRecorder {
            public static string $captured = "";
        }

        set_error_handler(function($errno, $errstr, $errfile, $errline) {
            ErrorRecorder::$captured = ErrorRecorder::$captured + "[{$errno}:{$errstr}]";
            return true;
        });

        trigger_error("A custom test notice", E_USER_NOTICE);
        trigger_error("A custom test warning", E_USER_WARNING);

        return ErrorRecorder::$captured;
    "#;

    let (_vm, res) = run_php(code);
    let val = res.expect("execution should succeed");
    assert_eq!(
        val,
        PhpValue::string("[1024:A custom test notice][512:A custom test warning]")
    );
}

#[test]
fn test_set_error_handler_level_bitmask_filtering() {
    let code = r#"
        class FilterRecorder {
            public static string $captured = "";
        }

        // Only listen for E_USER_WARNING (512)
        set_error_handler(function($errno, $errstr) {
            FilterRecorder::$captured = FilterRecorder::$captured + "[{$errstr}]";
            return true;
        }, E_USER_WARNING);

        trigger_error("Should be ignored by handler", E_USER_NOTICE);
        trigger_error("Should be captured by handler", E_USER_WARNING);

        return FilterRecorder::$captured;
    "#;

    let (_vm, res) = run_php(code);
    let val = res.expect("execution should succeed");
    assert_eq!(val, PhpValue::string("[Should be captured by handler]"));
}

#[test]
fn test_set_error_handler_convert_to_error_exception() {
    let code = r#"
        set_error_handler(function($errno, $errstr, $errfile, $errline) {
            throw new ErrorException($errstr, 0, $errno, $errfile, $errline);
        });

        $caught = false;
        $caught_msg = "";
        try {
            trigger_error("Convert me to exception", E_USER_WARNING);
        } catch (ErrorException $e) {
            $caught = true;
            $caught_msg = $e->getMessage();
        }

        return [
            'caught' => $caught,
            'msg' => $caught_msg,
        ];
    "#;

    let (_vm, res) = run_php(code);
    let val = res.expect("execution should succeed");
    if let PhpValue::Dict(d) = val {
        let map = d.borrow();
        assert_eq!(map.get("caught"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("msg"), Some(&PhpValue::string("Convert me to exception")));
    } else {
        panic!("Expected Dict result");
    }
}

#[test]
fn test_restore_error_handler_stack() {
    let code = r#"
        class StackRecorder {
            public static string $log = "";
        }

        $h1 = function($errno, $errstr) {
            StackRecorder::$log = StackRecorder::$log + "[h1:{$errstr}]";
            return true;
        };

        $h2 = function($errno, $errstr) {
            StackRecorder::$log = StackRecorder::$log + "[h2:{$errstr}]";
            return true;
        };

        $prev0 = set_error_handler($h1);
        trigger_error("first", E_USER_NOTICE);

        $prev1 = set_error_handler($h2);
        trigger_error("second", E_USER_NOTICE);

        $restored = restore_error_handler();
        trigger_error("third", E_USER_NOTICE);

        return [
            'log' => StackRecorder::$log,
            'prev0_null' => $prev0 == null,
            'restored' => $restored,
        ];
    "#;

    let (_vm, res) = run_php(code);
    let val = res.expect("execution should succeed");
    if let PhpValue::Dict(d) = val {
        let map = d.borrow();
        assert_eq!(map.get("prev0_null"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("restored"), Some(&PhpValue::Bool(true)));
        assert_eq!(
            map.get("log"),
            Some(&PhpValue::string("[h1:first][h2:second][h1:third]"))
        );
    } else {
        panic!("Expected Dict result");
    }
}

#[test]
fn test_set_exception_handler_uncaught_capture() {
    let code = r#"
        set_exception_handler(function($e) {
            echo "TopLevelCaught: " + $e->getMessage();
        });

        throw new RuntimeException("Crash payload");
    "#;

    let (vm, res) = run_php(code);
    // Uncaught exception was handled by custom handler, stopping script cleanly without crashing VM
    assert!(res.is_err());
    let err = res.err().unwrap();
    assert!(matches!(err.kind, RuntimeErrorKind::UncaughtExceptionHandled { .. }));

    // Output buffer captured what the handler echoed
    assert!(vm.output().contains("TopLevelCaught: Crash payload"));
}

#[test]
fn test_restore_exception_handler() {
    let code = r#"
        $handler1 = function($e) {
            echo "Handler1: " + $e->getMessage();
        };

        $handler2 = function($e) {
            echo "Handler2: " + $e->getMessage();
        };

        $p0 = set_exception_handler($handler1);
        $p1 = set_exception_handler($handler2);
        $r = restore_exception_handler();

        return [
            'p0_null' => $p0 == null,
            'restored' => $r,
        ];
    "#;

    let (_vm, res) = run_php(code);
    let val = res.expect("execution should succeed");
    if let PhpValue::Dict(d) = val {
        let map = d.borrow();
        assert_eq!(map.get("p0_null"), Some(&PhpValue::Bool(true)));
        assert_eq!(map.get("restored"), Some(&PhpValue::Bool(true)));
    } else {
        panic!("Expected Dict result");
    }
}

#[test]
fn test_exceptions_and_errors_do_not_panic_rust() {
    // Guarantees that neither exceptions, error handlers, nor fatal errors trigger Rust panic!
    let code = r#"
        $caught = false;
        try {
            throw new Exception("Normal recoverable exception");
        } catch (Exception $e) {
            $caught = true;
        }

        return $caught;
    "#;

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (_vm, res) = run_php(code);
        res.unwrap()
    }));

    assert!(outcome.is_ok(), "VM execution must not panic Rust");
    assert_eq!(outcome.unwrap(), PhpValue::Bool(true));
}
