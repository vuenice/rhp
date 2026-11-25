use rhp::parser::infer_namespace_from_path;
use rhp::vm::{run_source_with_path, PhpValue};
use std::fs;
use std::path::Path;

#[test]
fn test_infer_namespace_without_manifest_returns_none() {
    let rel_path = Path::new("some/random/dir/User.rhp");
    let ns = infer_namespace_from_path(rel_path);
    assert_eq!(ns, None);

    let root_file = Path::new("index.rhp");
    let ns_root = infer_namespace_from_path(root_file);
    assert_eq!(ns_root, None);
}

#[test]
fn test_infer_namespace_with_rhp_toml_direct_path() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src/Services/Auth")).unwrap();

    let rhp_toml = r#"[package]
name = "demo-pkg"
version = "0.1.0"
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let file_path = temp_dir.join("src/Services/Auth/User.rhp");
    fs::write(&file_path, "class User {}").unwrap();

    let ns = infer_namespace_from_path(&file_path);
    assert_eq!(ns, Some("Src\\Services\\Auth".to_string()));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_infer_namespace_with_rhp_toml_psr4_mapping() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_psr4_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src/Billing")).unwrap();

    let rhp_toml = r#"[package]
name = "acme-app"
version = "0.1.0"

[autoload]
psr-4 = { "Acme\\App\\" = "src/" }
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let file_path = temp_dir.join("src/Billing/Invoice.rhp");
    fs::write(&file_path, "class Invoice {}").unwrap();

    let ns = infer_namespace_from_path(&file_path);
    assert_eq!(ns, Some("Acme\\App\\Billing".to_string()));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_default_namespace_inferred_at_compile_and_runtime() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_exec_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src/Services/Auth")).unwrap();

    let rhp_toml = r#"[package]
name = "auth-demo"
version = "0.1.0"
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let code = r#"
class User {
    public function getRole(): string {
        return "admin";
    }
}

$u = new User();
$ns = __NAMESPACE__;
$cls = __CLASS__;
$fqn = User::class;
$role = $u->getRole();

return [$ns, $cls, $fqn, $role];
"#;

    let file_path = temp_dir.join("src/Services/Auth/User.rhp");
    fs::write(&file_path, code).unwrap();

    let res = run_source_with_path(code, file_path.to_str().unwrap()).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "Src\\Services\\Auth");
        assert_eq!(b[1].to_php_string(), "");
        assert_eq!(b[2].to_php_string(), "Src\\Services\\Auth\\User");
        assert_eq!(b[3].to_php_string(), "admin");
    } else {
        panic!("expected array return value, got {:?}", res);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_explicit_namespace_semicolon_forces_global_scope() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_global_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src/Services/Auth")).unwrap();

    let rhp_toml = r#"[package]
name = "auth-demo"
version = "0.1.0"
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let code = r#"
namespace;

class GlobalUser {
    public function getRole(): string {
        return "guest";
    }
}

$u = new GlobalUser();
$ns = __NAMESPACE__;
$fqn = GlobalUser::class;
$role = $u->getRole();

return [$ns, $fqn, $role];
"#;

    let file_path = temp_dir.join("src/Services/Auth/GlobalUser.rhp");
    fs::write(&file_path, code).unwrap();

    let res = run_source_with_path(code, file_path.to_str().unwrap()).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "GlobalUser");
        assert_eq!(b[2].to_php_string(), "guest");
    } else {
        panic!("expected array return value, got {:?}", res);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_explicit_namespace_tilde_forces_global_scope() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_tilde_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src/Services/Auth")).unwrap();

    let rhp_toml = r#"[package]
name = "auth-demo"
version = "0.1.0"
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let code = r#"
namespace ~;

class TildeUser {
    public function getRole(): string {
        return "superuser";
    }
}

$u = new TildeUser();
$ns = __NAMESPACE__;
$fqn = TildeUser::class;
$role = $u->getRole();

return [$ns, $fqn, $role];
"#;

    let file_path = temp_dir.join("src/Services/Auth/TildeUser.rhp");
    fs::write(&file_path, code).unwrap();

    let res = run_source_with_path(code, file_path.to_str().unwrap()).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "TildeUser");
        assert_eq!(b[2].to_php_string(), "superuser");
    } else {
        panic!("expected array return value, got {:?}", res);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_explicit_custom_namespace_overrides_path() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_override_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src/Services/Auth")).unwrap();

    let rhp_toml = r#"[package]
name = "auth-demo"
version = "0.1.0"
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let code = r#"
namespace Custom\Explicit\Auth;

class ExplicitUser {
    public function getRole(): string {
        return "explicit";
    }
}

$u = new ExplicitUser();
$ns = __NAMESPACE__;
$fqn = ExplicitUser::class;
$role = $u->getRole();

return [$ns, $fqn, $role];
"#;

    let file_path = temp_dir.join("src/Services/Auth/ExplicitUser.rhp");
    fs::write(&file_path, code).unwrap();

    let res = run_source_with_path(code, file_path.to_str().unwrap()).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "Custom\\Explicit\\Auth");
        assert_eq!(b[1].to_php_string(), "Custom\\Explicit\\Auth\\ExplicitUser");
        assert_eq!(b[2].to_php_string(), "explicit");
    } else {
        panic!("expected array return value, got {:?}", res);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_root_index_rhp_remains_global() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_ns_root_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let rhp_toml = r#"[package]
name = "root-demo"
version = "0.1.0"
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let code = r#"
class RootApp {
    public function name(): string {
        return "root";
    }
}

$app = new RootApp();
$ns = __NAMESPACE__;
$fqn = RootApp::class;

return [$ns, $fqn, $app->name()];
"#;

    let file_path = temp_dir.join("index.rhp");
    fs::write(&file_path, code).unwrap();

    let res = run_source_with_path(code, file_path.to_str().unwrap()).expect("execution should succeed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "");
        assert_eq!(b[1].to_php_string(), "RootApp");
        assert_eq!(b[2].to_php_string(), "root");
    } else {
        panic!("expected array return value, got {:?}", res);
    }

    let _ = fs::remove_dir_all(&temp_dir);
}
