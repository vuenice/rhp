use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rhp::{
    clear_rust_modules, clear_tier2_plugins, is_native_function_blocking,
    load_and_register_rust_file, register_in_process_native_function, register_tier2_plugin,
    run_source, run_source_in_fiber, FiberScheduler, FiberStepResult, FrozenValue, PhpValue,
    PluginFacade, RuntimeError, Span,
};

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_attribute_parsing_and_blocking_detection() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_attr_detection");
    let _ = fs::create_dir_all(temp_dir);
    let rust_file = temp_dir.join("MixedCrypto.rs");
    fs::write(
        &rust_file,
        r#"
// Native cryptographic helper with blocking and non-blocking exports
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[blocking]
pub fn argon2_hash(pwd: &str, salt: &str) -> String {
    format!("argon2id:${}:${}", salt, pwd)
}

pub fn quick_crc32(data: &str) -> u32 {
    data.len() as u32
}

#[rhp::blocking]
pub fn scrypt_hash(pwd: &str) -> String {
    format!("scrypt:{}", pwd)
}

#[no_mangle]
pub extern "C" fn rhp_call(
    method: *const c_char,
    _args: *const c_char,
) -> *mut c_char {
    let m = unsafe {
        if method.is_null() { "" } else { CStr::from_ptr(method).to_str().unwrap_or("") }
    };
    let res = match m {
        "argon2_hash" => "\"argon2id_result\"",
        "quick_crc32" => "42",
        "scrypt_hash" => "\"scrypt_result\"",
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

    let facade = load_and_register_rust_file(&rust_file.to_string_lossy(), None, None)
        .expect("MixedCrypto module should compile and load");

    assert_eq!(facade, "MixedCrypto");
    assert!(is_native_function_blocking("MixedCrypto", "argon2_hash"));
    assert!(is_native_function_blocking("MixedCrypto", "scrypt_hash"));
    assert!(!is_native_function_blocking("MixedCrypto", "quick_crc32"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_module_level_blocking_attribute() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_module_blocking");
    let _ = fs::create_dir_all(temp_dir);
    let rust_file = temp_dir.join("HeavyVips.rs");
    fs::write(
        &rust_file,
        r#"
#![blocking]
// All exports in this module are heavy image processing operations
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub fn resize(img: &str, w: i32, h: i32) -> String {
    format!("resized_{}_{}x{}", img, w, h)
}

pub fn compress(img: &str, quality: i32) -> String {
    format!("compressed_{}_q{}", img, quality)
}

#[no_mangle]
pub extern "C" fn rhp_call(
    method: *const c_char,
    _args: *const c_char,
) -> *mut c_char {
    let m = unsafe {
        if method.is_null() { "" } else { CStr::from_ptr(method).to_str().unwrap_or("") }
    };
    let res = match m {
        "resize" => "\"resized_ok\"",
        "compress" => "\"compressed_ok\"",
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

    let facade = load_and_register_rust_file(&rust_file.to_string_lossy(), None, None)
        .expect("HeavyVips module should compile and load");

    assert_eq!(facade, "HeavyVips");
    assert!(is_native_function_blocking("HeavyVips", "resize"));
    assert!(is_native_function_blocking("HeavyVips", "compress"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_compiled_cdylib_blocking_execution_fiber_suspension() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_rust_modules();
    clear_tier2_plugins();

    let temp_dir = Path::new("target/tmp/test_cdylib_blocking_exec");
    let _ = fs::create_dir_all(temp_dir);
    let rust_file = temp_dir.join("HeavyHash.rs");
    fs::write(
        &rust_file,
        r#"
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::thread;
use std::time::Duration;

#[blocking]
pub fn hash_heavy(data: &str) -> String {
    thread::sleep(Duration::from_millis(30));
    format!("hash_{}", data)
}

pub fn hash_fast(data: &str) -> String {
    format!("fast_{}", data)
}

#[no_mangle]
pub extern "C" fn rhp_call(
    method: *const c_char,
    _args: *const c_char,
) -> *mut c_char {
    let m = unsafe {
        if method.is_null() { "" } else { CStr::from_ptr(method).to_str().unwrap_or("") }
    };
    match m {
        "hash_heavy" => {
            // Emulate synchronous heavy processing
            thread::sleep(Duration::from_millis(30));
            CString::new("\"heavy_done\"").unwrap().into_raw()
        }
        "hash_fast" => {
            CString::new("\"fast_done\"").unwrap().into_raw()
        }
        _ => CString::new("null").unwrap().into_raw(),
    }
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
        import "{path}" as HeavyHash;

        // 1. Synchronous fast call (non-blocking)
        $fast = HeavyHash::hash_fast("quick_string");

        // 2. Synchronous heavy call (marked #[blocking] -> spawned on blocking thread pool)
        $heavy = HeavyHash::hash_heavy("secret_passphrase");

        return Arr::list([$fast, $heavy]);
        "#,
        path = rust_file.to_string_lossy()
    );

    let (res, stats) = run_source_in_fiber(&code, "blocking_fiber_test")
        .expect("Execution in fiber should succeed");

    match res {
        PhpValue::List(items) => {
            let b = items.borrow();
            assert_eq!(b[0], PhpValue::string("fast_done"));
            assert_eq!(b[1], PhpValue::string("heavy_done"));
        }
        other => panic!("Expected List, got {:?}", other),
    }

    // Verify fiber telemetry captured the #[blocking] native call as NativeBlocking I/O
    assert_eq!(stats.name, "blocking_fiber_test");
    assert!(stats.is_completed);
    assert_eq!(stats.io_breakdown.get("NativeBlocking"), Some(&1));
    assert!(stats.io_count >= 1);
    assert!(stats.io_wait_duration >= Duration::from_millis(20));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_in_process_native_function_blocking_dispatch() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_rust_modules();
    clear_tier2_plugins();

    // Register an in-process native extension function marked as blocking
    register_in_process_native_function(
        "Argon2Native",
        "hash",
        true, // is_blocking
        |args| {
            let pwd = match args.first() {
                Some(FrozenValue::String(s)) => s.clone(),
                _ => String::new(),
            };
            // Simulate synchronous heavy cryptographic computation
            thread::sleep(Duration::from_millis(25));
            Ok(FrozenValue::String(format!("argon2id_hash_of_{}", pwd)))
        },
    );

    // Register a non-blocking in-process native function
    register_in_process_native_function(
        "Argon2Native",
        "verify",
        false, // non-blocking
        |args| {
            let matches = args.len() >= 2;
            Ok(FrozenValue::Bool(matches))
        },
    );

    let code = r#"
        $h = Argon2Native::hash("super_secret_p@ss");
        $v = Argon2Native::verify("super_secret_p@ss", $h);
        return Arr::list([$h, $v]);
    "#;

    let (res, stats) = run_source_in_fiber(code, "in_process_blocking_fiber")
        .expect("Fiber execution should succeed");

    match res {
        PhpValue::List(items) => {
            let b = items.borrow();
            assert_eq!(b[0], PhpValue::string("argon2id_hash_of_super_secret_p@ss"));
            assert_eq!(b[1], PhpValue::Bool(true));
        }
        other => panic!("Expected List, got {:?}", other),
    }

    // Verify fiber telemetry
    assert_eq!(stats.io_breakdown.get("NativeBlocking"), Some(&1));
    assert!(stats.io_wait_duration >= Duration::from_millis(15));
}

#[test]
fn test_tier2_plugin_blocking_escape_hatch() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_rust_modules();
    clear_tier2_plugins();

    struct ImageMagickPlugin;

    impl PluginFacade for ImageMagickPlugin {
        fn name(&self) -> &str {
            "ImageMagick"
        }

        fn is_blocking(&self, method: &str) -> bool {
            method == "heavyFilter"
        }

        fn call(
            &self,
            method: &str,
            args: &[PhpValue],
            span: Span,
        ) -> Result<PhpValue, RuntimeError> {
            match method {
                "heavyFilter" => {
                    let img = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                    thread::sleep(Duration::from_millis(20));
                    Ok(PhpValue::string(format!("filtered_{}", img)))
                }
                "dimensions" => Ok(PhpValue::Int(1080)),
                _ => Err(RuntimeError::new(
                    rhp::RuntimeErrorKind::UndefinedFacadeMethod {
                        facade: self.name().to_string(),
                        method: method.to_string(),
                    },
                    span,
                )),
            }
        }
    }

    register_tier2_plugin(ImageMagickPlugin);

    let code = r#"
        $dim = ImageMagick::dimensions();
        $filtered = ImageMagick::heavyFilter("photo.jpg");
        return Arr::list([$dim, $filtered]);
    "#;

    let (res, stats) = run_source_in_fiber(code, "plugin_fiber_test")
        .expect("Plugin fiber execution should succeed");

    match res {
        PhpValue::List(items) => {
            let b = items.borrow();
            assert_eq!(b[0], PhpValue::Int(1080));
            assert_eq!(b[1], PhpValue::string("filtered_photo.jpg"));
        }
        other => panic!("Expected List, got {:?}", other),
    }

    assert_eq!(stats.io_breakdown.get("NativeBlocking"), Some(&1));
    assert!(stats.io_wait_duration >= Duration::from_millis(15));
}

#[test]
fn test_multi_fiber_concurrency_non_blocking_worker_thread() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_rust_modules();
    clear_tier2_plugins();

    let blocking_started = Arc::new(AtomicBool::new(false));
    let blocking_completed = Arc::new(AtomicBool::new(false));

    let started_clone = Arc::clone(&blocking_started);
    let completed_clone = Arc::clone(&blocking_completed);

    register_in_process_native_function(
        "HeavyWorker",
        "longTask",
        true, // is_blocking -> dispatched to spawn_blocking
        move |_args| {
            started_clone.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(60));
            completed_clone.store(true, Ordering::SeqCst);
            Ok(FrozenValue::String("task_done".to_string()))
        },
    );

    // Concurrently:
    // Fiber 1 calls HeavyWorker::longTask() and suspends via suspend_for_io(NativeBlocking).
    // While Fiber 1 is suspended, Fiber 2 can run freely on the thread without being blocked!
    let mut scheduler = FiberScheduler::new();

    let f1_id = scheduler.spawn("fiber_heavy", |_fiber| {
        let res = run_source("return HeavyWorker::longTask();");
        match res {
            Ok(v) => FiberStepResult::Done(v),
            Err(e) => FiberStepResult::Done(PhpValue::string(e.to_string())),
        }
    });

    let mut f2_counter = 0;
    let f2_id = scheduler.spawn("fiber_light", move |_fiber| {
        f2_counter += 1;
        if f2_counter < 3 {
            FiberStepResult::Yield
        } else {
            FiberStepResult::Done(PhpValue::Int(f2_counter))
        }
    });

    let results = scheduler.run_all();
    assert_eq!(results.len(), 2);

    assert_eq!(
        results.get(&f1_id).unwrap().as_ref().unwrap(),
        &PhpValue::string("task_done")
    );
    assert_eq!(
        results.get(&f2_id).unwrap().as_ref().unwrap(),
        &PhpValue::Int(3)
    );

    assert!(blocking_started.load(Ordering::SeqCst));
    assert!(blocking_completed.load(Ordering::SeqCst));
}
