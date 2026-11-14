use rhp::ir::lower_program;
use rhp::parser::parse_with_path;
use rhp::type_system::check_program;
use rhp::vm::engine::VM;
use rhp::vm::value::PhpValue;

fn run_php(source: &str) -> PhpValue {
    let path = "test_script.php";
    let program = parse_with_path(source, path).expect("parse failed");
    let type_info = check_program(&program).expect("type check failed");
    let module = lower_program(&program, Some(&type_info)).expect("lower failed");
    let mut vm = VM::new(module, false).with_file_path(path);
    vm.run().expect("execution failed")
}

#[test]
fn test_at_operator_success_returns_result_and_null_err() {
    let code = r#"
        function helperFun(): string {
            return "hello world";
        }

        [$result, $err] = @helperFun();

        return [
            'result' => $result,
            'err_is_null' => $err == null,
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("result"), Some(&PhpValue::string("hello world")));
            assert_eq!(map.get("err_is_null"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_failure_populates_err_and_null_result() {
    let code = r#"
        function helperFun(): string {
            throw new Exception("something went wrong", 42);
        }

        [$result, $err] = @helperFun();

        return [
            'result_is_null' => $result == null,
            'err_is_exception' => $err instanceof Exception,
            'err_message' => $err != null ? $err->getMessage() : "",
            'err_code' => $err != null ? $err->getCode() : 0,
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("result_is_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err_is_exception"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err_message"), Some(&PhpValue::string("something went wrong")));
            assert_eq!(map.get("err_code"), Some(&PhpValue::Int(42)));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_void_function_returns_null_result() {
    let code = r#"
        $counter = 0;
        function helperVoid(): void {
            // side effect or no-op
        }

        [$result, $err] = @helperVoid();

        return [
            'result_is_null' => $result == null,
            'err_is_null' => $err == null,
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("result_is_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err_is_null"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_void_function_throwing_populates_err() {
    let code = r#"
        function helperVoidThrows(): void {
            throw new RuntimeException("void failed");
        }

        [$result, $err] = @helperVoidThrows();

        return [
            'result_is_null' => $result == null,
            'err_is_runtime_exception' => $err instanceof RuntimeException,
            'err_message' => $err != null ? $err->getMessage() : "",
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("result_is_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err_is_runtime_exception"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err_message"), Some(&PhpValue::string("void failed")));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_single_slot_destructuring_unlink() {
    let code = r#"
        // Nothing forces the check like [$result] = @unlink();
        [$result] = @unlink("non_existent_file_xyz_123.tmp");

        return [
            'result' => $result,
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            // unlink on non-existent file returns false
            assert_eq!(map.get("result"), Some(&PhpValue::Bool(false)));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_direct_tuple_assignment() {
    let code = r#"
        function compute(): int {
            return 100;
        }

        $tuple = @compute();

        return [
            'val0' => $tuple[0],
            'val1' => $tuple[1],
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("val0"), Some(&PhpValue::Int(100)));
            assert_eq!(map.get("val1"), Some(&PhpValue::Null));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_deep_call_stack_exception_unwinding() {
    let code = r#"
        function level3() {
            throw new InvalidArgumentException("deep failure");
        }
        function level2() {
            level3();
        }
        function level1() {
            level2();
        }

        [$result, $err] = @level1();

        return [
            'result_is_null' => $result == null,
            'err_class' => $err != null ? get_class($err) : "",
            'err_message' => $err != null ? $err->getMessage() : "",
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("result_is_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err_class"), Some(&PhpValue::string("InvalidArgumentException")));
            assert_eq!(map.get("err_message"), Some(&PhpValue::string("deep failure")));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_multiple_sequential_suppressions() {
    let code = r#"
        function throwErr(string $msg) {
            throw new Exception($msg);
        }
        function okVal(int $val): int {
            return $val * 2;
        }

        [$r1, $e1] = @throwErr("first");
        [$r2, $e2] = @okVal(21);
        [$r3, $e3] = @throwErr("third");

        return [
            'r1' => $r1,
            'e1_msg' => $e1 != null ? $e1->getMessage() : "",
            'r2' => $r2,
            'e2_null' => $e2 == null,
            'r3' => $r3,
            'e3_msg' => $e3 != null ? $e3->getMessage() : "",
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("r1"), Some(&PhpValue::Null));
            assert_eq!(map.get("e1_msg"), Some(&PhpValue::string("first")));
            assert_eq!(map.get("r2"), Some(&PhpValue::Int(42)));
            assert_eq!(map.get("e2_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("r3"), Some(&PhpValue::Null));
            assert_eq!(map.get("e3_msg"), Some(&PhpValue::string("third")));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_object_method_call() {
    let code = r#"
        class Worker {
            public function process(bool $shouldFail): string {
                if ($shouldFail) {
                    throw new RuntimeException("worker failed");
                }
                return "worker success";
            }
        }

        $w = new Worker();
        [$res1, $err1] = @$w->process(false);
        [$res2, $err2] = @$w->process(true);

        return [
            'res1' => $res1,
            'err1_null' => $err1 == null,
            'res2_null' => $res2 == null,
            'err2_msg' => $err2 != null ? $err2->getMessage() : "",
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("res1"), Some(&PhpValue::string("worker success")));
            assert_eq!(map.get("err1_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("res2_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("err2_msg"), Some(&PhpValue::string("worker failed")));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_closure_invocation() {
    let code = r#"
        $failClosure = function(): int {
            throw new Exception("closure error");
        };

        $passClosure = function(int $x): int {
            return $x + 10;
        };

        [$resFail, $errFail] = @$failClosure();
        [$resPass, $errPass] = @$passClosure(5);

        return [
            'resFail' => $resFail,
            'errFail_msg' => $errFail != null ? $errFail->getMessage() : "",
            'resPass' => $resPass,
            'errPass_null' => $errPass == null,
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("resFail"), Some(&PhpValue::Null));
            assert_eq!(map.get("errFail_msg"), Some(&PhpValue::string("closure error")));
            assert_eq!(map.get("resPass"), Some(&PhpValue::Int(15)));
            assert_eq!(map.get("errPass_null"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_nested_suppression() {
    let code = r#"
        function throwsAlways(): string {
            throw new Exception("inner error");
        }

        $outer = function(): string {
            [$innerRes, $innerErr] = @throwsAlways();
            if ($innerErr != null) {
                return "recovered from: " + $innerErr->getMessage();
            }
            return "unexpected";
        };

        [$outerRes, $outerErr] = @$outer();

        return [
            'outerRes' => $outerRes,
            'outerErr_null' => $outerErr == null,
        ];
    "#;

    let res = run_php(code);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("outerRes"), Some(&PhpValue::string("recovered from: inner error")));
            assert_eq!(map.get("outerErr_null"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("Expected Dict, got {:?}", other),
    }
}

#[test]
fn test_at_operator_type_system_tuple_inference() {
    use rhp::type_system::types::RhpType;

    let code = r#"
        function computeNumber(): int {
            return 42;
        }
        $tuple = @computeNumber();
    "#;

    let path = "type_test.php";
    let program = parse_with_path(code, path).expect("parse failed");
    let type_info = check_program(&program).expect("type check failed");

    // Check inferred type of $tuple in environment
    let var_type = type_info.top_level_inferences.get("$tuple").expect("$tuple must be inferred");
    match var_type {
        RhpType::Tuple(items) => {
            assert_eq!(items.len(), 2);
            // First slot: T|null (i.e. ?int)
            assert_eq!(items[0], RhpType::Nullable(Box::new(RhpType::Int)));
            // Second slot: Throwable|null (i.e. ?Throwable)
            assert_eq!(items[1], RhpType::Nullable(Box::new(RhpType::Class("Throwable".to_string()))));
        }
        other => panic!("Expected RhpType::Tuple, got {:?}", other),
    }
}
