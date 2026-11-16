use std::fs;
use std::path::Path;

use rhp::{
    clear_rust_modules, clear_tier2_plugins, is_native_or_plugin_facade, is_static_facade,
    is_static_only, load_and_register_package_plugin,
    register_in_process_native_function, run_source, run_source_with_vm, FrozenValue,
    ParentProcess, PhpValue, WorkerRequest,
};

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_direct_rust_file_import_is_forbidden() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    // 1. import "./Cache.rs" is rejected at parse/compile time
    let bad_code1 = r#"import "./Cache.rs";"#;
    let err1 = run_source(bad_code1).unwrap_err();
    assert!(
        err1.to_string().contains("Direct importing of Rust source files ('.rs') is prohibited"),
        "Unexpected error: {}",
        err1
    );

    // 2. use "./Cache.rs" as Cache is rejected at parse/compile time
    let bad_code2 = r#"use "./Cache.rs" as Cache;"#;
    let err2 = run_source(bad_code2).unwrap_err();
    assert!(
        err2.to_string().contains("Direct importing of Rust source files ('.rs') is prohibited"),
        "Unexpected error: {}",
        err2
    );

    // 3. import Cache from "./Cache.rs" is rejected at parse/compile time
    let bad_code3 = r#"import Cache from "./Cache.rs";"#;
    let err3 = run_source(bad_code3).unwrap_err();
    assert!(
        err3.to_string().contains("Direct importing of Rust source files ('.rs') is prohibited"),
        "Unexpected error: {}",
        err3
    );

    // 4. API call rejects .rs path
    let api_err = load_and_register_package_plugin("Cache.rs", None, None).unwrap_err();
    assert!(
        api_err.to_string().contains("Direct importing of Rust source files ('.rs') is prohibited"),
        "Unexpected error: {}",
        api_err
    );
}

#[test]
fn test_import_package_cache_facade() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let code = r#"
        import "cache";

        Cache::set("user_100", "Alice");
        $hasVal = Cache::has("user_100");
        $user = Cache::get("user_100");
        $fallback = Cache::get("unknown_key", "guest");

        Cache::increment("hits", 10);
        $hits = Cache::get("hits");

        Cache::delete("user_100");
        $afterDelete = Cache::has("user_100");
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Package import execution should succeed");

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
}

#[test]
fn test_use_package_with_alias() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let code = r#"
        use "cache" as FastStore;

        FastStore::set("session_token", "tok_xyz_12345");
        $tok = FastStore::get("session_token");
        $hasTok = FastStore::has("session_token");
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Execution with alias should succeed");
    assert_eq!(
        vm.get_main_variable("$tok"),
        Some(&PhpValue::string("tok_xyz_12345"))
    );
    assert_eq!(vm.get_main_variable("$hasTok"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_import_package_log_facade() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let code = r#"
        import "log";

        Log::info("Application booted successfully", ["env" => "production"]);
        Log::warn("High memory usage detected", ["usage_mb" => 450]);
        Log::error("Database connection refused", ["retry_in" => 3]);

        $history = Log::history();
        $count = Arr::list($history)->count();
        $firstLog = $history[0]["message"];
        $firstLevel = $history[0]["level"];
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Log package import execution should succeed");
    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(3)));
    assert_eq!(
        vm.get_main_variable("$firstLog"),
        Some(&PhpValue::string("Application booted successfully"))
    );
    assert_eq!(
        vm.get_main_variable("$firstLevel"),
        Some(&PhpValue::string("info"))
    );
}

#[test]
fn test_import_alias_from_syntax() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let code = r#"
        import MemoryStore from "cache";

        MemoryStore::set("user_count", 42);
        $count = MemoryStore::get("user_count");
    "#;

    let (vm, _) = run_source_with_vm(code).expect("import ... from syntax should succeed");
    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(42)));
}

#[test]
fn test_imported_package_class_is_static_only_facade() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let _ = load_and_register_package_plugin("cache", None, None);

    assert!(is_static_facade("Cache"));
    assert!(is_native_or_plugin_facade("Cache"));
    assert!(is_static_only("Cache"));

    // Instantiation must be rejected at compile-time/runtime
    let bad_code = r#"
        import "cache";
        $c = new Cache();
    "#;

    let res = run_source(bad_code);
    let err_str = match res {
        Err(e) => e.to_string(),
        Ok(_) => panic!("Expected error instantiating static facade"),
    };
    assert!(
        err_str.contains("Cannot instantiate static facade class 'Cache'"),
        "Unexpected error: {}",
        err_str
    );
}

#[test]
fn test_rust_package_exporting_struct_or_trait_is_rejected() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_struct_trait_rejected");
    let _ = fs::create_dir_all(temp_dir);

    // Package with illegal struct and trait exports
    let rhp_toml = r#"[package]
name = "acme/illegal-rust"
version = "0.1.0"
type = "native-rust"
safety = "safe"

[exports]
facades = ["AcmeHelper"]
structs = ["AcmeClient"]
traits = ["AcmeSerializable"]
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    // 1. Audit reports violations
    let audit_res = rhp::pkg::audit_package(temp_dir).expect("Audit should complete");
    assert!(!audit_res.is_clean());
    let struct_violation = audit_res.violations.iter().any(|v| {
        v.reason.contains("Rust packages cannot export structs to RHP")
    });
    let trait_violation = audit_res.violations.iter().any(|v| {
        v.reason.contains("Rust packages cannot export traits to RHP")
    });
    assert!(struct_violation, "Expected struct violation in audit");
    assert!(trait_violation, "Expected trait violation in audit");

    // 2. Package plugin loader rejects package exporting structs/traits
    let load_res = load_and_register_package_plugin(
        &temp_dir.to_string_lossy(),
        None,
        None,
    );
    assert!(load_res.is_err());
    let err = load_res.unwrap_err();
    assert!(
        err.to_string().contains("cannot export structs to RHP")
            || err.to_string().contains("cannot export traits to RHP"),
        "Unexpected error: {}",
        err
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_parent_process_boot_with_package_import() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    let framework_code = r#"
        import "cache";
        Cache::set("app_name", "RHP Web Engine");
    "#;

    let state = ParentProcess::boot(framework_code).expect("ParentProcess::boot should succeed");

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
}

#[test]
fn test_package_plugin_helper_facade_and_functions() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_rust_modules();
    clear_tier2_plugins();

    // Register a native helper facade with standalone functions via plugin system
    register_in_process_native_function("NativeHasher", "computeHash", false, |args| {
        let input = match args.first() {
            Some(FrozenValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        Ok(FrozenValue::String(format!("hash_of_{}", input)))
    });

    register_in_process_native_function("NativeHasher", "fastSum", false, |args| {
        let a = match args.first() {
            Some(FrozenValue::Int(n)) => *n,
            _ => 0,
        };
        let b = match args.get(1) {
            Some(FrozenValue::Int(n)) => *n,
            _ => 0,
        };
        Ok(FrozenValue::Int(a + b))
    });

    let code = r#"
        use "native-hasher" as NativeHasher;

        $hash = NativeHasher::computeHash("hello");
        $sum = NativeHasher::fastSum(40, 60);
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Package plugin facade call should succeed");
    assert_eq!(
        vm.get_main_variable("$hash"),
        Some(&PhpValue::string("hash_of_hello"))
    );
    assert_eq!(vm.get_main_variable("$sum"), Some(&PhpValue::Int(100)));
}
