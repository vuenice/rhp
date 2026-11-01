use rhp::ir::lower_program;
use rhp::parser::parse_with_path;
use rhp::vm::engine::VM;
use rhp::vm::value::PhpValue;

fn run_php(source: &str, file_path: Option<&str>) -> PhpValue {
    let path = file_path.unwrap_or("test_script.php");
    let program = parse_with_path(source, path).expect("parse failed");
    let module = lower_program(&program, None).expect("lower failed");
    let mut vm = VM::new(module, false).with_file_path(path);
    vm.run().expect("execution failed")
}

#[allow(dead_code)]
fn run_php_err(source: &str, file_path: Option<&str>) -> Result<PhpValue, rhp::vm::error::RuntimeError> {
    let path = file_path.unwrap_or("test_script.php");
    let program = parse_with_path(source, path).expect("parse failed");
    let module = lower_program(&program, None).expect("lower failed");
    let mut vm = VM::new(module, false).with_file_path(path);
    vm.run()
}

#[test]
fn test_exception_instantiation_site_captured_not_thrown() {
    let code = r#"
        function createException() {
            return new Exception("captured at instantiation");
        }

        function throwLater($e) {
            throw $e;
        }

        $ex = createException();
        $line_at_create = $ex->getLine();
        $file_at_create = $ex->getFile();

        $caught_line = 0;
        $caught_file = "";
        try {
            throwLater($ex);
        } catch (Exception $e) {
            $caught_line = $e->getLine();
            $caught_file = $e->getFile();
        }

        return [
            'line_match' => $line_at_create === $caught_line,
            'file_match' => $file_at_create === $caught_file,
            'file' => $caught_file,
            'line' => $caught_line
        ];
    "#;

    let res = run_php(code, Some("my_app/service.php"));
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("line_match"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("file_match"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("file"), Some(&PhpValue::string("my_app/service.php")));
            // Line 3 is where `new Exception(...)` is created
            assert_eq!(map.get("line"), Some(&PhpValue::Int(3)));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_exception_previous_chaining() {
    let code = r#"
        $e1 = new Exception("first root error", 101);
        $e2 = new RuntimeException("second middle error", 102, $e1);
        $e3 = new Exception("third top error", 103, $e2);

        $prev_of_e1 = $e1->getPrevious();
        $prev_of_e2 = $e2->getPrevious();
        $prev_of_e3 = $e3->getPrevious();

        return [
            'e1_prev_is_null' => $prev_of_e1 === null,
            'e2_prev_msg' => $prev_of_e2->getMessage(),
            'e2_prev_code' => $prev_of_e2->getCode(),
            'e3_prev_msg' => $prev_of_e3->getMessage(),
            'e3_prev_code' => $prev_of_e3->getCode(),
            'e3_grandparent_msg' => $prev_of_e3->getPrevious()->getMessage(),
        ];
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("e1_prev_is_null"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("e2_prev_msg"), Some(&PhpValue::string("first root error")));
            assert_eq!(map.get("e2_prev_code"), Some(&PhpValue::Int(101)));
            assert_eq!(map.get("e3_prev_msg"), Some(&PhpValue::string("second middle error")));
            assert_eq!(map.get("e3_prev_code"), Some(&PhpValue::Int(102)));
            assert_eq!(map.get("e3_grandparent_msg"), Some(&PhpValue::string("first root error")));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_lazy_get_trace_caller_frames() {
    let code = r#"
        class OrderService {
            public static function process() {
                return self::validate();
            }

            public static function validate() {
                return new Exception("invalid order");
            }
        }

        function trigger() {
            return OrderService::process();
        }

        $e = trigger();
        $trace = $e->getTrace();
        $str = $e->getTraceAsString();

        return [
            'trace_count' => count($trace),
            'first_func' => $trace[0]['function'],
            'first_class' => $trace[0]['class'],
            'second_func' => $trace[1]['function'],
            'third_func' => $trace[2]['function'],
            'has_main_in_str' => str_contains($str, '{main}'),
        ];
    "#;

    let res = run_php(code, Some("order.php"));
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("trace_count"), Some(&PhpValue::Int(3)));
            assert_eq!(map.get("first_func"), Some(&PhpValue::string("validate")));
            assert_eq!(map.get("first_class"), Some(&PhpValue::string("OrderService")));
            assert_eq!(map.get("second_func"), Some(&PhpValue::string("process")));
            assert_eq!(map.get("third_func"), Some(&PhpValue::string("trigger")));
            assert_eq!(map.get("has_main_in_str"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_zend_exception_ignore_args_behavior() {
    let code = r#"
        function helperWithSecrets($user, $apiKey) {
            return new Exception("secret failure");
        }

        // 1. By default, exception_ignore_args is enabled (zend.exception_ignore_args = 1)
        $ini_default = ini_get("zend.exception_ignore_args");
        $e1 = helperWithSecrets("admin", "sk-secret-12345");
        $trace1 = $e1->getTrace();
        $has_args_default = array_key_exists('args', $trace1[0]);

        // 2. Change ini_set to disable ignore_args (0)
        ini_set("zend.exception_ignore_args", "0");
        $ini_changed = ini_get("zend.exception_ignore_args");
        $e2 = helperWithSecrets("alice", "sk-live-67890");
        $trace2 = $e2->getTrace();
        $has_args_disabled = array_key_exists('args', $trace2[0]);
        $captured_user = $has_args_disabled ? $trace2[0]['args'][0] : "";

        return [
            'ini_default' => $ini_default,
            'has_args_default' => $has_args_default,
            'ini_changed' => $ini_changed,
            'has_args_disabled' => $has_args_disabled,
            'captured_user' => $captured_user,
        ];
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("ini_default"), Some(&PhpValue::string("1")));
            assert_eq!(map.get("has_args_default"), Some(&PhpValue::Bool(false)));
            assert_eq!(map.get("ini_changed"), Some(&PhpValue::string("0")));
            assert_eq!(map.get("has_args_disabled"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("captured_user"), Some(&PhpValue::string("alice")));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_php_hierarchy_subtyping_typeerror_not_caught_as_exception() {
    let code = r#"
        $caught_as = "none";
        try {
            throw new TypeError("type mismatch");
        } catch (Exception $e) {
            $caught_as = "Exception";
        } catch (Error $e) {
            $caught_as = "Error";
        } catch (Throwable $e) {
            $caught_as = "Throwable";
        }

        return $caught_as;
    "#;

    let res = run_php(code, None);
    assert_eq!(res, PhpValue::string("Error"));
}

#[test]
fn test_php_hierarchy_subtyping_runtimeexception_not_caught_as_error() {
    let code = r#"
        $caught_as = "none";
        try {
            throw new RuntimeException("runtime failure");
        } catch (Error $e) {
            $caught_as = "Error";
        } catch (Exception $e) {
            $caught_as = "Exception";
        } catch (Throwable $e) {
            $caught_as = "Throwable";
        }

        return $caught_as;
    "#;

    let res = run_php(code, None);
    assert_eq!(res, PhpValue::string("Exception"));
}

#[test]
fn test_php_hierarchy_subtyping_divisionbyzero_and_arithmetic_error() {
    let code = r#"
        $caught_as = "none";
        try {
            throw new DivisionByZeroError("division by zero");
        } catch (ArithmeticError $e) {
            $caught_as = "ArithmeticError";
        } catch (Error $e) {
            $caught_as = "Error";
        }

        return $caught_as;
    "#;

    let res = run_php(code, None);
    assert_eq!(res, PhpValue::string("ArithmeticError"));
}

#[test]
fn test_spl_exception_hierarchy() {
    let code = r#"
        $results = [];

        try {
            throw new BadMethodCallException("bad method");
        } catch (LogicException $e) {
            $results[] = 'LogicException';
        }

        try {
            throw new OutOfBoundsException("out of bounds");
        } catch (RuntimeException $e) {
            $results[] = 'RuntimeException';
        }

        try {
            throw new InvalidArgumentException("invalid arg");
        } catch (Exception $e) {
            $results[] = 'Exception';
        }

        return $results;
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Array(a) => {
            let vec = a.borrow();
            assert_eq!(vec.len(), 3);
            assert_eq!(vec[0], PhpValue::string("LogicException"));
            assert_eq!(vec[1], PhpValue::string("RuntimeException"));
            assert_eq!(vec[2], PhpValue::string("Exception"));
        }
        PhpValue::List(l) => {
            let vec = l.borrow();
            assert_eq!(vec.len(), 3);
            assert_eq!(vec[0], PhpValue::string("LogicException"));
            assert_eq!(vec[1], PhpValue::string("RuntimeException"));
            assert_eq!(vec[2], PhpValue::string("Exception"));
        }
        other => panic!("expected Array or List, got {:?}", other),
    }
}

#[test]
fn test_error_exception_severity_and_custom_file_line() {
    let code = r#"
        $e = new ErrorException(
            "Notice converted to ErrorException",
            42,
            8, // E_NOTICE
            "custom_source.php",
            999
        );

        return [
            'message' => $e->getMessage(),
            'code' => $e->getCode(),
            'severity' => $e->getSeverity(),
            'file' => $e->getFile(),
            'line' => $e->getLine(),
        ];
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("message"), Some(&PhpValue::string("Notice converted to ErrorException")));
            assert_eq!(map.get("code"), Some(&PhpValue::Int(42)));
            assert_eq!(map.get("severity"), Some(&PhpValue::Int(8)));
            assert_eq!(map.get("file"), Some(&PhpValue::string("custom_source.php")));
            assert_eq!(map.get("line"), Some(&PhpValue::Int(999)));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_custom_user_exception_class_hierarchy_and_constructor() {
    let code = r#"
        class CustomAppException extends RuntimeException {
            public $customMeta;

            public function __construct($message, $code = 0, $meta = "default") {
                parent::__construct($message, $code);
                $this->customMeta = $meta;
            }
        }

        $ex = new CustomAppException("custom app failure", 500, "user_123");

        $caught = false;
        $caught_as_throwable = false;
        try {
            throw $ex;
        } catch (RuntimeException $e) {
            $caught = true;
            if ($e instanceof Throwable) {
                $caught_as_throwable = true;
            }
        }

        return [
            'message' => $ex->getMessage(),
            'code' => $ex->getCode(),
            'meta' => $ex->customMeta,
            'caught' => $caught,
            'caught_as_throwable' => $caught_as_throwable,
            'file' => $ex->getFile(),
            'line' => $ex->getLine(),
        ];
    "#;

    let res = run_php(code, Some("app/handler.php"));
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("message"), Some(&PhpValue::string("custom app failure")));
            assert_eq!(map.get("code"), Some(&PhpValue::Int(500)));
            assert_eq!(map.get("meta"), Some(&PhpValue::string("user_123")));
            assert_eq!(map.get("caught"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("caught_as_throwable"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("file"), Some(&PhpValue::string("app/handler.php")));
            // Line 11 is where `new CustomAppException(...)` is instantiated
            assert_eq!(map.get("line"), Some(&PhpValue::Int(11)));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_custom_user_exception_without_custom_constructor() {
    let code = r#"
        class SimpleUserException extends Exception {}

        $e = new SimpleUserException("simple failure", 99);

        return [
            'message' => $e->getMessage(),
            'code' => $e->getCode(),
            'is_instance_of_exception' => $e instanceof Exception,
            'is_instance_of_throwable' => $e instanceof Throwable,
        ];
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("message"), Some(&PhpValue::string("simple failure")));
            assert_eq!(map.get("code"), Some(&PhpValue::Int(99)));
            assert_eq!(map.get("is_instance_of_exception"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("is_instance_of_throwable"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_tostring_and_string_cast_representation() {
    let code = r#"
        $e = new Exception("formatted output error", 123);
        $as_str = string($e);
        $to_str = $e->__toString();

        return [
            'matches' => $as_str === $to_str,
            'has_class' => str_contains($as_str, 'Exception: formatted output error'),
            'has_stack_trace' => str_contains($as_str, 'Stack trace:'),
            'has_main' => str_contains($as_str, '{main}'),
        ];
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("matches"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("has_class"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("has_stack_trace"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("has_main"), Some(&PhpValue::Bool(true)));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_source_root_cause_and_chain_primary_and_alias_methods() {
    let code = r#"
        $e1 = new Exception("root cause error", 1);
        $e2 = new RuntimeException("middle cause error", 2, $e1);
        $e3 = new Exception("top error", 3, $e2);

        // 1. source() is primary, getPrevious() is alias
        $src = $e3->source();
        $prev = $e3->getPrevious();
        $src_matches_prev = ($src->getMessage() === $prev->getMessage());

        // 2. rootCause() is primary, getRootCause() is alias
        $root = $e3->rootCause();
        $get_root = $e3->getRootCause();
        $root_matches_get_root = ($root->getMessage() === $get_root->getMessage());
        $root_is_e1 = ($root->getMessage() === "root cause error" && $root->getCode() === 1);

        // Standalone exception with no previous: rootCause() is itself
        $e1_root = $e1->rootCause();
        $e1_root_is_self = ($e1_root->getMessage() === "root cause error");

        // 3. chain() is primary, getChain() is alias
        $chain_list = $e3->chain();
        $get_chain_list = $e3->getChain();
        $chain_count = count($chain_list);
        $get_chain_count = count($get_chain_list);

        $messages = [];
        foreach ($chain_list as $err) {
            $messages[] = $err->getMessage();
        }

        return [
            'src_matches_prev' => $src_matches_prev,
            'src_msg' => $src->getMessage(),
            'root_matches_get_root' => $root_matches_get_root,
            'root_is_e1' => $root_is_e1,
            'e1_root_is_self' => $e1_root_is_self,
            'chain_count' => $chain_count,
            'get_chain_count' => $get_chain_count,
            'messages' => $messages,
        ];
    "#;

    let res = run_php(code, None);
    match res {
        PhpValue::Dict(d) => {
            let map = d.borrow();
            assert_eq!(map.get("src_matches_prev"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("src_msg"), Some(&PhpValue::string("middle cause error")));
            assert_eq!(map.get("root_matches_get_root"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("root_is_e1"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("e1_root_is_self"), Some(&PhpValue::Bool(true)));
            assert_eq!(map.get("chain_count"), Some(&PhpValue::Int(3)));
            assert_eq!(map.get("get_chain_count"), Some(&PhpValue::Int(3)));

            let msgs = match map.get("messages") {
                Some(PhpValue::List(l)) => l.borrow().clone(),
                Some(PhpValue::Array(a)) => a.borrow().clone(),
                other => panic!("expected List or Array, got {:?}", other),
            };
            assert_eq!(msgs.len(), 3);
            assert_eq!(msgs[0], PhpValue::string("top error"));
            assert_eq!(msgs[1], PhpValue::string("middle cause error"));
            assert_eq!(msgs[2], PhpValue::string("root cause error"));
        }
        other => panic!("expected Dict, got {:?}", other),
    }
}

#[test]
fn test_rust_side_throwable_traits_and_methods() {
    use rhp::vm::exception::{chain, root_cause, Throwable};
    use std::error::Error;

    let t1 = Throwable::new("root error", 100, "app/db.php", 10);
    let t2 = Throwable::new("middle error", 200, "app/repo.php", 20).with_previous(t1, None);
    let t3 = Throwable::new("top error", 300, "app/controller.php", 30).with_previous(t2, None);

    // Primary source() and get_previous() alias
    assert!(t3.source().is_some());
    assert_eq!(t3.source().unwrap().message, "middle error");
    assert_eq!(t3.get_previous().unwrap().message, "middle error");

    // Primary root_cause() and get_root_cause() alias
    assert_eq!(t3.root_cause().message, "root error");
    assert_eq!(t3.get_root_cause().message, "root error");
    assert_eq!(root_cause(&t3).message, "root error");

    // Primary chain() and get_chain() alias
    let chain_msgs: Vec<&str> = t3.chain().map(|t| t.message.as_str()).collect();
    assert_eq!(chain_msgs, vec!["top error", "middle error", "root error"]);

    let chain_fn_msgs: Vec<&str> = chain(&t3).map(|t| t.message.as_str()).collect();
    assert_eq!(chain_fn_msgs, vec!["top error", "middle error", "root error"]);

    let get_chain_msgs: Vec<&str> = t3.get_chain().map(|t| t.message.as_str()).collect();
    assert_eq!(get_chain_msgs, vec!["top error", "middle error", "root error"]);

    // std::error::Error trait implementation
    let err_trait: &dyn Error = &t3;
    let source_err = err_trait.source().expect("should have source");
    assert!(source_err.to_string().contains("middle error"));

    // From<std::io::Error>
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let t_io: Throwable = io_err.into();
    assert_eq!(t_io.message, "file not found");
    assert_eq!(t_io.file, "<native>");

    // From<&str> and From<String>
    let t_str: Throwable = "custom error message".into();
    assert_eq!(t_str.message, "custom error message");
}

