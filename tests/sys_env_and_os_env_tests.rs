use std::fs;
use std::sync::Mutex;
use rhp::PhpValue;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_sys_os_env_reads_system_environment() {
    let _guard = TEST_MUTEX.lock().unwrap();
    // 1. Set a unique OS environment variable
    unsafe {
        std::env::set_var("RHP_TEST_OS_KEY", "super_secret_system_value");
    }

    // 2. Sys::osEnv('RHP_TEST_OS_KEY') retrieves it
    let code = r#"
        $val = Sys::osEnv('RHP_TEST_OS_KEY');
        $fallback = Sys::osEnv('RHP_NON_EXISTENT_KEY_XYZ', 'default_fallback');
        $all = Sys::osEnv();
        return [$val, $fallback, $all['RHP_TEST_OS_KEY']];
    "#;

    let res = rhp::run_source(code).expect("Script execution failed");
    if let PhpValue::Array(list) | PhpValue::List(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "super_secret_system_value");
        assert_eq!(items[1].to_php_string(), "default_fallback");
        assert_eq!(items[2].to_php_string(), "super_secret_system_value");
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_getenv_and_os_env_intrinsics_lower_to_sys_os_env() {
    let _guard = TEST_MUTEX.lock().unwrap();
    unsafe {
        std::env::set_var("RHP_TEST_GETENV_KEY", "os_val_42");
    }

    let code = r#"
        $a = getenv('RHP_TEST_GETENV_KEY');
        $b = osEnv('RHP_TEST_GETENV_KEY');
        $c = os_env('RHP_TEST_GETENV_KEY');
        $d = osEnv('RHP_TEST_MISSING', 'my_default');
        return [$a, $b, $c, $d];
    "#;

    let res = rhp::run_source(code).expect("Script execution failed");
    if let PhpValue::Array(list) | PhpValue::List(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "os_val_42");
        assert_eq!(items[1].to_php_string(), "os_val_42");
        assert_eq!(items[2].to_php_string(), "os_val_42");
        assert_eq!(items[3].to_php_string(), "my_default");
    } else {
        panic!("Expected array result, got {:?}", res);
    }
}

#[test]
fn test_sys_env_reads_vite_env_file_hierarchy() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let temp_dir = std::env::temp_dir().join(format!("rhp_env_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&temp_dir).unwrap();

    // 1. Create base .env
    fs::write(
        temp_dir.join(".env"),
        r#"
APP_NAME="RHP Application"
PORT=3000
DATABASE_URL="postgres://localhost:5432/db"
SHARED_VAR="base"
"#,
    ).unwrap();

    // 2. Create .env.local (overrides base)
    fs::write(
        temp_dir.join(".env.local"),
        r#"
LOCAL_SECRET="local_secret_value"
SHARED_VAR="local_override"
"#,
    ).unwrap();

    // 3. Create .env.development (mode specific)
    fs::write(
        temp_dir.join(".env.development"),
        r#"
DEV_ONLY="enabled"
SHARED_VAR="dev_override"
PORT=5173
FULL_URL="http://localhost:${PORT}/api"
"#,
    ).unwrap();

    // 4. Create .env.development.local (highest priority in dev)
    fs::write(
        temp_dir.join(".env.development.local"),
        r#"
DEV_LOCAL_SECRET="dev_local_secret"
"#,
    ).unwrap();

    // Create index.rhp at that location
    let index_rhp = temp_dir.join("index.rhp");
    fs::write(&index_rhp, "return Sys::env();").unwrap();

    // Load env for development mode
    rhp::runtime::env_store::init_env(Some(&index_rhp), &temp_dir, "development");

    let code = r#"
        $appName = Sys::env('APP_NAME');
        $port = Sys::env('PORT');
        $shared = Sys::env('SHARED_VAR');
        $devOnly = Sys::env('DEV_ONLY');
        $devLocal = Sys::env('DEV_LOCAL_SECRET');
        $fullUrl = Sys::env('FULL_URL');
        $missing = Sys::env('NOT_FOUND', 'fallback_123');
        $all = Sys::env();

        return [
            $appName,
            $port,
            $shared,
            $devOnly,
            $devLocal,
            $fullUrl,
            $missing,
            $all['PORT']
        ];
    "#;

    let res = rhp::run_source(code).expect("Execution failed");
    if let PhpValue::Array(list) | PhpValue::List(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "RHP Application");
        assert_eq!(items[1].to_php_string(), "5173");
        assert_eq!(items[2].to_php_string(), "dev_override");
        assert_eq!(items[3].to_php_string(), "enabled");
        assert_eq!(items[4].to_php_string(), "dev_local_secret");
        assert_eq!(items[5].to_php_string(), "http://localhost:5173/api");
        assert_eq!(items[6].to_php_string(), "fallback_123");
        assert_eq!(items[7].to_php_string(), "5173");
    } else {
        panic!("Expected array result");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_sys_env_production_mode_does_not_load_development_env() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let temp_dir = std::env::temp_dir().join(format!("rhp_env_prod_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&temp_dir).unwrap();

    fs::write(
        temp_dir.join(".env"),
        "MODE_NAME=base\nPORT=80\n",
    ).unwrap();

    fs::write(
        temp_dir.join(".env.development"),
        "MODE_NAME=development\nPORT=3000\n",
    ).unwrap();

    fs::write(
        temp_dir.join(".env.production"),
        "MODE_NAME=production\nPORT=8080\n",
    ).unwrap();

    let index_file = temp_dir.join("index.rhp");
    fs::write(&index_file, "return Sys::env();").unwrap();

    // Production mode (build)
    rhp::runtime::env_store::init_env(Some(&index_file), &temp_dir, "production");

    let code = r#"
        $mode = env('MODE_NAME');
        $port = env('PORT');
        return [$mode, $port];
    "#;

    let res = rhp::run_source(code).expect("Execution failed");
    if let PhpValue::Array(list) | PhpValue::List(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "production");
        assert_eq!(items[1].to_php_string(), "8080");
    } else {
        panic!("Expected array result");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_env_file_parser_syntax_and_quoting() {
    let content = r#"
# Comment line
export PLAIN=hello_world
SINGLE_QUOTED='single\nquoted\traw'
DOUBLE_QUOTED="double\nescaped\tvalue"
WITH_COMMENT=value_without_comment # this is an inline comment
WITH_SPACES="   padded value   "
EXPAND_A="foo"
EXPAND_B="${EXPAND_A}_bar"
EXPAND_DEFAULT="${NON_EXISTENT_VAR:-default_val}"
MULTILINE="first line
second line
third line"
"#;

    let parsed = rhp::runtime::env_store::parse_env_content(content);

    assert_eq!(parsed.get("PLAIN").map(|s| s.as_str()), Some("hello_world"));
    assert_eq!(parsed.get("SINGLE_QUOTED").map(|s| s.as_str()), Some("single\\nquoted\\traw"));
    assert_eq!(parsed.get("DOUBLE_QUOTED").map(|s| s.as_str()), Some("double\nescaped\tvalue"));
    assert_eq!(parsed.get("WITH_COMMENT").map(|s| s.as_str()), Some("value_without_comment"));
    assert_eq!(parsed.get("WITH_SPACES").map(|s| s.as_str()), Some("   padded value   "));
    assert_eq!(parsed.get("EXPAND_A").map(|s| s.as_str()), Some("foo"));
    assert_eq!(parsed.get("EXPAND_B").map(|s| s.as_str()), Some("foo_bar"));
    assert_eq!(parsed.get("EXPAND_DEFAULT").map(|s| s.as_str()), Some("default_val"));
    assert_eq!(parsed.get("MULTILINE").map(|s| s.as_str()), Some("first line\nsecond line\nthird line"));
}
