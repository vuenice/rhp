use std::fs;
use std::path::Path;

use rhp::{
    clear_rust_modules, clear_tier2_plugins, is_native_or_plugin_facade, is_static_facade,
    is_static_only, load_and_register_rust_file, run_source_with_vm, ParentProcess, PhpValue,
    WorkerRequest,
};

#[test]
fn test_import_rust_file_cache_facade() {
    clear_rust_modules();
    clear_tier2_plugins();

    // Create a temporary Cache.rs file
    let temp_dir = Path::new("target/tmp/test_cache_import");
    let _ = fs::create_dir_all(temp_dir);
    let cache_file = temp_dir.join("Cache.rs");
    fs::write(
        &cache_file,
        r#"
// High-performance Rust Cache helper for RHP
use std::collections::HashMap;

pub fn get(key: &str) -> Option<String> { None }
pub fn set(key: &str, val: &str) -> bool { true }
pub fn has(key: &str) -> bool { true }
pub fn delete(key: &str) -> bool { true }
"#,
    )
    .unwrap();

    let code = format!(
        r#"
        import "{path}";

        Cache::set("user_100", "Alice");
        $hasVal = Cache::has("user_100");
        $user = Cache::get("user_100");
        $fallback = Cache::get("unknown_key", "guest");

        Cache::increment("hits", 10);
        $hits = Cache::get("hits");

        Cache::delete("user_100");
        $afterDelete = Cache::has("user_100");
        "#,
        path = cache_file.to_string_lossy()
    );

    let (vm, _) = run_source_with_vm(&code).expect("Execution should succeed");

    assert_eq!(vm.get_main_variable("$hasVal"), Some(&PhpValue::Bool(true)));
    assert_eq!(
        vm.get_main_variable("$user"),
        Some(&PhpValue::string("Alice"))
    );
    assert_eq!(
        vm.get_main_variable("$fallback"),
        Some(&PhpValue::string("guest"))
    );
    assert_eq!(vm.get_main_variable("$hits"), Some(&PhpValue::Int(10)));
    assert_eq!(
        vm.get_main_variable("$afterDelete"),
        Some(&PhpValue::Bool(false))
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_use_rust_file_with_alias() {
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_alias_import");
    let _ = fs::create_dir_all(temp_dir);
    let cache_file = temp_dir.join("fast_cache.rs");
    fs::write(
        &cache_file,
        r#"
pub fn get(k: &str) -> Option<String> { None }
pub fn set(k: &str, v: &str) -> bool { true }
"#,
    )
    .unwrap();

    let code = format!(
        r#"
        use "{path}" as FastStore;

        FastStore::set("session_token", "tok_xyz_12345");
        $tok = FastStore::get("session_token");
        $hasTok = FastStore::has("session_token");
        "#,
        path = cache_file.to_string_lossy()
    );

    let (vm, _) = run_source_with_vm(&code).expect("Execution with alias should succeed");
    assert_eq!(
        vm.get_main_variable("$tok"),
        Some(&PhpValue::string("tok_xyz_12345"))
    );
    assert_eq!(vm.get_main_variable("$hasTok"), Some(&PhpValue::Bool(true)));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_import_rust_file_log_facade() {
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_log_import");
    let _ = fs::create_dir_all(temp_dir);
    let log_file = temp_dir.join("Log.rs");
    fs::write(
        &log_file,
        r#"
// High-performance Rust structured logging helper
pub fn info(msg: &str) {}
pub fn error(msg: &str) {}
pub fn warn(msg: &str) {}
"#,
    )
    .unwrap();

    let code = format!(
        r#"
        import "{path}";

        Log::info("Application booted successfully", ["env" => "production"]);
        Log::warn("High memory usage detected", ["usage_mb" => 450]);
        Log::error("Database connection refused", ["retry_in" => 3]);

        $history = Log::history();
        $count = Arr::list($history)->count();
        $firstLog = $history[0]["message"];
        $firstLevel = $history[0]["level"];
        "#,
        path = log_file.to_string_lossy()
    );

    let (vm, _) = run_source_with_vm(&code).expect("Log import execution should succeed");
    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(3)));
    assert_eq!(
        vm.get_main_variable("$firstLog"),
        Some(&PhpValue::string("Application booted successfully"))
    );
    assert_eq!(
        vm.get_main_variable("$firstLevel"),
        Some(&PhpValue::string("info"))
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_import_alias_from_syntax() {
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_import_from");
    let _ = fs::create_dir_all(temp_dir);
    let cache_file = temp_dir.join("memory.rs");
    fs::write(
        &cache_file,
        r#"
pub fn get(k: &str) -> Option<String> { None }
pub fn set(k: &str, v: &str) -> bool { true }
"#,
    )
    .unwrap();

    let code = format!(
        r#"
        import MemoryStore from "{path}";

        MemoryStore::set("user_count", 42);
        $count = MemoryStore::get("user_count");
        "#,
        path = cache_file.to_string_lossy()
    );

    let (vm, _) = run_source_with_vm(&code).expect("import ... from syntax should succeed");
    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(42)));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_imported_rust_class_is_static_only_facade() {
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_static_guard");
    let _ = fs::create_dir_all(temp_dir);
    let cache_file = temp_dir.join("Cache.rs");
    fs::write(&cache_file, "pub fn get() {}\n").unwrap();

    let _ = load_and_register_rust_file(&cache_file.to_string_lossy(), None, None);

    assert!(is_static_facade("Cache"));
    assert!(is_native_or_plugin_facade("Cache"));
    assert!(is_static_only("Cache"));

    // Instantiation must be rejected at compile-time/runtime
    let bad_code = format!(
        r#"
        import "{path}";
        $c = new Cache();
        "#,
        path = cache_file.to_string_lossy()
    );

    let res = run_source_with_vm(&bad_code);
    let err_str = match res {
        Err(e) => e.to_string(),
        Ok(_) => panic!("Expected error instantiating static facade"),
    };
    assert!(
        err_str.contains("Cannot instantiate static facade class 'Cache'"),
        "Unexpected error: {}",
        err_str
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_parent_process_boot_with_rust_import() {
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_parent_boot");
    let _ = fs::create_dir_all(temp_dir);
    let cache_file = temp_dir.join("Cache.rs");
    fs::write(
        &cache_file,
        r#"
pub fn get(k: &str) -> Option<String> { None }
pub fn set(k: &str, v: &str) -> bool { true }
"#,
    )
    .unwrap();

    let framework_code = format!(
        r#"
        import "{path}";
        Cache::set("app_name", "RHP Web Engine");
        "#,
        path = cache_file.to_string_lossy()
    );

    let state = ParentProcess::boot(&framework_code).expect("ParentProcess::boot should succeed");

    // Worker request accessing the shared Cache facade
    let req = WorkerRequest::new(
        r#"
        $appName = Cache::get("app_name");
        Cache::set("request_id", 101);
        $reqId = Cache::get("request_id");
        "#,
    );

    let resp = rhp::execute_isolated_request(&state, &req, 0);
    assert_eq!(resp.status, rhp::WorkerResponseStatus::Success);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_native_rustc_cdylib_compilation_and_execution() {
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_native_cdylib");
    let _ = fs::create_dir_all(temp_dir);
    let rust_file = temp_dir.join("NativeHasher.rs");
    fs::write(
        &rust_file,
        r#"
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn rhp_call(
    method: *const c_char,
    _args_json: *const c_char,
) -> *mut c_char {
    let m = unsafe {
        if method.is_null() { "" } else { CStr::from_ptr(method).to_str().unwrap_or("") }
    };
    let res = match m {
        "computeHash" => "\"0xdeadbeef\"",
        "fastSum" => "100",
        _ => "null",
    };
    CString::new(res).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn rhp_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)); }
    }
}
"#,
    )
    .unwrap();

    let code = format!(
        r#"
        import "{path}" as NativeHasher;

        $hash = NativeHasher::computeHash("hello");
        $sum = NativeHasher::fastSum(40, 60);
        "#,
        path = rust_file.to_string_lossy()
    );

    let (vm, _) = run_source_with_vm(&code).expect("Native cdylib execution should succeed");
    assert_eq!(
        vm.get_main_variable("$hash"),
        Some(&PhpValue::string("0xdeadbeef"))
    );
    assert_eq!(vm.get_main_variable("$sum"), Some(&PhpValue::Int(100)));

    let _ = fs::remove_dir_all(temp_dir);
}

