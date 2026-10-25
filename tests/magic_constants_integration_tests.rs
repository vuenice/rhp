use rhp::vm::{run_source, run_source_with_path, PhpValue};

#[test]
fn test_magic_line() {
    let code = r#"
$l1 = __LINE__;
$l2 = __LINE__;
$l3 = __line__;
return [$l1, $l2, $l3];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_i32().unwrap(), 2);
        assert_eq!(b[1].to_i32().unwrap(), 3);
        assert_eq!(b[2].to_i32().unwrap(), 4);
    } else {
        panic!("expected array return value, got {:?}", res);
    }
}

#[test]
fn test_magic_file_and_dir() {
    // 1. Without explicit path, defaults to empty strings
    let code_default = r#"
return [__FILE__, __DIR__, __file__, __dir__];
"#;
    let res_default = run_source(code_default).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_default {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "");
        assert_eq!(b[2].to_php_string(), "");
        assert_eq!(b[3].to_php_string(), "");
    } else {
        panic!("expected array, got {:?}", res_default);
    }

    // 2. With explicit path
    let code_with_path = r#"
return [__FILE__, __DIR__];
"#;
    let path = "/var/www/html/app/Http/Controllers/UserController.php";
    let res_path = run_source_with_path(code_with_path, path).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res_path {
        let b = arr.borrow();
        assert_eq!(
            b[0].to_php_string(),
            "/var/www/html/app/Http/Controllers/UserController.php"
        );
        assert_eq!(
            b[1].to_php_string(),
            "/var/www/html/app/Http/Controllers"
        );
    } else {
        panic!("expected array, got {:?}", res_path);
    }
}

#[test]
fn test_magic_class_and_enum() {
    let code = r#"
$global_class = __CLASS__;

class User {
    public function getMyClass(): string {
        return __CLASS__;
    }
    public function getCaseInsensitive(): string {
        return __class__;
    }
}

enum Status {
    case Pending;
    case Active;

    public function getEnumClass(): string {
        return __CLASS__;
    }
}

$u = new User();
$user_class = $u->getMyClass();
$user_ci = $u->getCaseInsensitive();

$s = Status::Active;
$enum_class = $s->getEnumClass();

return [$global_class, $user_class, $user_ci, $enum_class];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "User");
        assert_eq!(b[2].to_php_string(), "User");
        assert_eq!(b[3].to_php_string(), "Status");
    } else {
        panic!("expected array, got {:?}", res);
    }
}

#[test]
fn test_magic_function() {
    let code = r#"
$global_fn = __FUNCTION__;

function computeTotal(int $x, int $y): string {
    return __FUNCTION__;
}

class Invoice {
    public function generate(): string {
        return __FUNCTION__;
    }
}

$closure = function() {
    return __FUNCTION__;
};

$arrow = fn() => __FUNCTION__;

$inv = new Invoice();

return [
    $global_fn,
    computeTotal(1, 2),
    $inv->generate(),
    $closure(),
    $arrow(),
    __function__
];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "computeTotal");
        assert_eq!(b[2].to_php_string(), "generate");
        assert_eq!(b[3].to_php_string(), "{closure}");
        assert_eq!(b[4].to_php_string(), "{closure}");
        assert_eq!(b[5].to_php_string(), "");
    } else {
        panic!("expected array, got {:?}", res);
    }
}

#[test]
fn test_magic_method() {
    let code = r#"
$global_method = __METHOD__;

function standalone(): string {
    return __METHOD__;
}

class Service {
    public function handle(): string {
        return __METHOD__;
    }

    public function withClosure(): string {
        $cl = function() {
            return __METHOD__;
        };
        return $cl();
    }
}

$svc = new Service();

return [
    $global_method,
    standalone(),
    $svc->handle(),
    $svc->withClosure(),
    __method__
];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "standalone");
        assert_eq!(b[2].to_php_string(), "Service::handle");
        assert_eq!(b[3].to_php_string(), "Service::{closure}");
        assert_eq!(b[4].to_php_string(), "");
    } else {
        panic!("expected array, got {:?}", res);
    }
}

#[test]
fn test_magic_namespace_and_namespaced_fqn() {
    let code = r#"
$global_ns = __NAMESPACE__;

namespace App\Services;

$ns = __NAMESPACE__;
$ns_ci = __namespace__;

class Worker {
    public function work(): string {
        return __CLASS__ . " -> " . __METHOD__;
    }
}

function processTask(): string {
    return __METHOD__;
}

$w = new Worker();
return [$global_ns, $ns, $ns_ci, $w->work(), processTask()];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "App\\Services");
        assert_eq!(b[2].to_php_string(), "App\\Services");
        assert_eq!(
            b[3].to_php_string(),
            "App\\Services\\Worker -> App\\Services\\Worker::work"
        );
        assert_eq!(b[4].to_php_string(), "App\\Services\\processTask");
    } else {
        panic!("expected array, got {:?}", res);
    }
}

#[test]
fn test_magic_trait() {
    let code = r#"
$global_trait = __TRAIT__;

trait Loggable {
    public function getTrait(): string {
        return __TRAIT__;
    }
}

return [$global_trait, __trait__];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "");
    } else {
        panic!("expected array, got {:?}", res);
    }
}

#[test]
fn test_magic_constants_in_defaults_and_expressions() {
    let code = r#"
function logEvent(string $msg, int $line = __LINE__): string {
    return $msg . " at line " . $line;
}

class Record {
    public int $created_line = __LINE__;
}

$rec = new Record();
$ev = logEvent("Something happened");

return [$ev, $rec->created_line];
"#;
    let res = run_source(code).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert!(b[0].to_php_string().starts_with("Something happened at line "));
        assert_eq!(b[1].to_i32().unwrap(), 7);
    } else {
        panic!("expected array, got {:?}", res);
    }
}
