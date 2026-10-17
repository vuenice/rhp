use rhp::{
    clear_log_history, clear_runtime_closure_macros, clear_tier2_plugins, get_facades_by_tier,
    get_recorded_logs, is_native_or_plugin_facade, is_static_facade, is_static_only, parse,
    register_tier2_plugin, register_tier2_plugin_handler, run_source, run_source_with_vm,
    FacadeTier, ParseErrorKind, PhpValue, PluginFacade, RuntimeErrorKind, RustIntKind,
};

#[test]
fn test_tier1_native_facades_execution() {
    clear_log_history();

    // 1. Str Facade
    let src_str = r#"
        $slug = Str::slug("Hello World! 2026 Engine");
        $upper = Str::upper("rhp language");
        $len = Str::length("facade");
        $has = Str::contains("antigravity", "gravity");
    "#;
    let (vm, _) = run_source_with_vm(src_str).expect("Str facade execution should succeed");
    assert_eq!(vm.get_main_variable("$slug"), Some(&PhpValue::string("hello-world-2026-engine")));
    assert_eq!(vm.get_main_variable("$upper"), Some(&PhpValue::string("RHP LANGUAGE")));
    assert_eq!(vm.get_main_variable("$len"), Some(&PhpValue::Int(6)));
    assert_eq!(vm.get_main_variable("$has"), Some(&PhpValue::Bool(true)));

    // 2. Int & Float Facades
    let src_num = r#"
        $i64Val = Int::i64(9999);
        $minVal = Int::min(10, 50);
        $maxVal = Int::max(10, 50);
        $clamped = Int::clamp(150, 0, 100);
        $parsedInt = Int::parse("42");

        $rounded = Float::round(3.14159, 2);
        $floored = Float::floor(3.8);
        $ceiled = Float::ceil(3.2);
        $absVal = Float::abs(-42.5);
    "#;
    let (vm_num, _) = run_source_with_vm(src_num).expect("Int & Float facades execution should succeed");
    assert_eq!(
        vm_num.get_main_variable("$i64Val"),
        Some(&PhpValue::RustInt {
            kind: RustIntKind::I64,
            value: 9999,
        })
    );
    assert_eq!(vm_num.get_main_variable("$minVal"), Some(&PhpValue::Int(10)));
    assert_eq!(vm_num.get_main_variable("$maxVal"), Some(&PhpValue::Int(50)));
    assert_eq!(vm_num.get_main_variable("$clamped"), Some(&PhpValue::Int(100)));
    assert_eq!(vm_num.get_main_variable("$parsedInt"), Some(&PhpValue::Int(42)));

    assert_eq!(vm_num.get_main_variable("$rounded"), Some(&PhpValue::Float(3.14)));
    assert_eq!(vm_num.get_main_variable("$floored"), Some(&PhpValue::Float(3.0)));
    assert_eq!(vm_num.get_main_variable("$ceiled"), Some(&PhpValue::Float(4.0)));
    assert_eq!(vm_num.get_main_variable("$absVal"), Some(&PhpValue::Float(42.5)));

    // 3. Arr Facade
    let src_arr = r#"
        $l = Arr::list([10, 20, 30]);
        $d = Arr::dict(["key" => "value"]);
    "#;
    let (vm_arr, _) = run_source_with_vm(src_arr).expect("Arr facade execution should succeed");
    assert_eq!(
        vm_arr.get_main_variable("$l"),
        Some(&PhpValue::list(vec![
            PhpValue::Int(10),
            PhpValue::Int(20),
            PhpValue::Int(30),
        ]))
    );

    // 4. DB Facade Query Builder
    let src_db = r#"
        $users = DB::table("users")->where("active", 1)->orderBy("id", "desc")->get();
        $firstUser = DB::table("users")->first();
        $single = DB::table("posts")->find(42);
    "#;
    let (vm_db, _) = run_source_with_vm(src_db).expect("DB facade execution should succeed");
    assert!(vm_db.get_main_variable("$users").is_some());
    assert!(vm_db.get_main_variable("$firstUser").is_some());
    assert!(vm_db.get_main_variable("$single").is_some());

    // 5. Log Facade Channel & History
    let src_log = r#"
        Log::info("Application booted successfully", ["port" => 8080]);
        Log::channel("security")->error("Failed login attempt", ["ip" => "192.168.1.1"]);
    "#;
    let _ = run_source(src_log).expect("Log facade execution should succeed");
    let logs = get_recorded_logs();
    assert!(logs.len() >= 2);
    assert_eq!(logs[0].level, "info");
    assert_eq!(logs[0].message, "Application booted successfully");
    assert_eq!(logs[0].channel, "default");
    assert_eq!(logs[1].level, "error");
    assert_eq!(logs[1].message, "Failed login attempt");
    assert_eq!(logs[1].channel, "security");

    // 6. Future Concurrency Facade
    let src_fut = r#"
        $res = Future::run(fn() => 42 * 2)->wait();
    "#;
    let (vm_fut, _) = run_source_with_vm(src_fut).expect("Future facade execution should succeed");
    assert_eq!(vm_fut.get_main_variable("$res"), Some(&PhpValue::Int(84)));

    // 7. Memory Ownership & Safety Facade (Technical Point #20)
    let src_mem = r#"
        $isGcFree = Memory::isGcFree();
        $gcRuns = Memory::gcRuns();
        $gcPause = Memory::gcPauseMs();
        $safe = Memory::assertThreadSafe([1, 2, 3]);
        $stats = Memory::stats();
    "#;
    let (vm_mem, _) = run_source_with_vm(src_mem).expect("Memory facade execution should succeed");
    assert_eq!(vm_mem.get_main_variable("$isGcFree"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm_mem.get_main_variable("$gcRuns"), Some(&PhpValue::Int(0)));
    assert_eq!(vm_mem.get_main_variable("$gcPause"), Some(&PhpValue::Float(0.0)));
    assert_eq!(vm_mem.get_main_variable("$safe"), Some(&PhpValue::Bool(true)));
    assert!(vm_mem.get_main_variable("$stats").is_some());
}

// Custom Tier 2 Rust Plugin
struct CustomCryptoPlugin;

impl PluginFacade for CustomCryptoPlugin {
    fn name(&self) -> &'static str {
        "CustomCrypto"
    }

    fn description(&self) -> &'static str {
        "AOT Compiled Rust Crypto Plugin Facade"
    }

    fn call(&self, method: &str, args: &[PhpValue], span: rhp::Span) -> Result<PhpValue, rhp::RuntimeError> {
        match method {
            "sha256" => {
                let input = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                // Synthetic deterministic hash for test
                Ok(PhpValue::string(format!("hash({})", input)))
            }
            "version" => Ok(PhpValue::string("v2.1.0-rust")),
            _ => Err(rhp::RuntimeError::new(
                RuntimeErrorKind::UndefinedFacadeMethod {
                    facade: "CustomCrypto".to_string(),
                    method: method.to_string(),
                },
                span,
            )),
        }
    }
}

#[test]
fn test_tier2_custom_rust_plugin_registration_and_execution() {
    clear_tier2_plugins();

    // 1. Register struct-based plugin via trait
    register_tier2_plugin(CustomCryptoPlugin);

    // 2. Register closure-based plugin handler
    register_tier2_plugin_handler(
        "GeoIP",
        "GeoIP lookup plugin",
        true,
        std::sync::Arc::new(|method, args, span| match method {
            "lookup" => {
                let ip = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                Ok(PhpValue::string(format!("US-{}", ip)))
            }
            _ => Err(rhp::RuntimeError::new(
                RuntimeErrorKind::UndefinedFacadeMethod {
                    facade: "GeoIP".to_string(),
                    method: method.to_string(),
                },
                span,
            )),
        }),
    );

    // Verify registry metadata
    assert!(is_static_facade("CustomCrypto"));
    assert!(is_static_only("CustomCrypto"));
    assert!(is_native_or_plugin_facade("CustomCrypto"));

    assert!(is_static_facade("GeoIP"));
    assert!(is_static_only("GeoIP"));
    assert!(is_native_or_plugin_facade("GeoIP"));

    let tier2_facades = get_facades_by_tier(FacadeTier::Tier2RustPlugin);
    assert!(tier2_facades.iter().any(|f| f.name == "CustomCrypto"));
    assert!(tier2_facades.iter().any(|f| f.name == "GeoIP"));

    // 3. Execute Tier 2 plugin calls from RHP script
    let script = r#"
        $hash = CustomCrypto::sha256("mySecret");
        $ver = CustomCrypto::version();
        $location = GeoIP::lookup("127.0.0.1");
    "#;
    let (vm, _) = run_source_with_vm(script).expect("Tier 2 plugin execution should succeed");
    assert_eq!(vm.get_main_variable("$hash"), Some(&PhpValue::string("hash(mySecret)")));
    assert_eq!(vm.get_main_variable("$ver"), Some(&PhpValue::string("v2.1.0-rust")));
    assert_eq!(vm.get_main_variable("$location"), Some(&PhpValue::string("US-127.0.0.1")));
}

#[test]
fn test_tier3_script_land_static_facades() {
    // 1. Userland static class declared with `static class MathUtil`
    let script = r#"
        static class MathUtil {
            public static function square(int $x): int {
                return $x * $x;
            }
        }

        #[StaticOnly]
        class AppConfig {
            public static function getEnv(): string {
                return "production";
            }
        }

        #[Facade]
        class TextHelper {
            public static function shout(string $s): string {
                return Str::upper($s) . "!!!";
            }
        }

        $sq = MathUtil::square(9);
        $env = AppConfig::getEnv();
        $shout = TextHelper::shout("hello");
    "#;

    let (vm, _) = run_source_with_vm(script).expect("Tier 3 static facades should execute cleanly");
    assert_eq!(vm.get_main_variable("$sq"), Some(&PhpValue::Int(81)));
    assert_eq!(vm.get_main_variable("$env"), Some(&PhpValue::string("production")));
    assert_eq!(vm.get_main_variable("$shout"), Some(&PhpValue::string("HELLO!!!")));

    // Verify Tier 3 registration in registry
    assert!(is_static_facade("MathUtil"));
    assert!(is_static_only("MathUtil"));
    assert!(is_static_facade("AppConfig"));
    assert!(is_static_only("AppConfig"));
    assert!(is_static_facade("TextHelper"));
    assert!(is_static_only("TextHelper"));

    let tier3_facades = get_facades_by_tier(FacadeTier::Tier3Script);
    assert!(tier3_facades.iter().any(|f| f.name == "MathUtil"));
    assert!(tier3_facades.iter().any(|f| f.name == "AppConfig"));
    assert!(tier3_facades.iter().any(|f| f.name == "TextHelper"));
}

#[test]
fn test_instantiation_guard_static_compile_time() {
    register_tier2_plugin(CustomCryptoPlugin);

    // Register script static class
    let def_script = "static class MathUtil { public static function id(): int { return 1; } }";
    let _ = parse(def_script).expect("Static class should parse");

    // All static facades across Tier 1, Tier 2, and Tier 3 must halt instantiation attempts
    let static_classes = [
        "Int", "Float", "Arr", "Str", "Future", "HTTP", "File", "Log", "DB", "Sys",
        "Memory", "CustomCrypto", "MathUtil",
    ];

    for name in static_classes {
        let code = format!("$obj = new {}();", name);
        let result = parse(&code);
        assert!(result.is_err(), "Expected parse error instantiating static facade '{}'", name);

        let err = result.unwrap_err();
        match &err.kind {
            ParseErrorKind::StaticFacadeInstantiationForbidden { class_name } => {
                assert_eq!(class_name, name);
            }
            other => panic!("Expected StaticFacadeInstantiationForbidden for '{}', got: {:?}", name, other),
        }

        let err_msg = err.to_string();
        assert!(
            err_msg.contains(&format!(
                "Fatal Error: Cannot instantiate static facade class '{}'. Use static methods directly.",
                name
            )),
            "Unexpected error format: {}",
            err_msg
        );
    }
}

#[test]
fn test_instantiation_guard_dynamic_runtime() {
    register_tier2_plugin(CustomCryptoPlugin);

    let setup_script = "static class MathUtil { public static function foo(): int { return 1; } }";
    let _ = run_source(setup_script).expect("Static class setup should pass");

    let dynamic_cases = ["Int", "Arr", "Str", "DB", "Log", "Future", "Memory", "CustomCrypto", "MathUtil"];

    for name in dynamic_cases {
        let code = format!(
            r#"
            $cls = "{}";
            $obj = new $cls();
            "#,
            name
        );
        let result = run_source(&code);
        assert!(result.is_err(), "Expected runtime fatal error for dynamic instantiation of '{}'", name);

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains(&format!(
                "Fatal Error: Cannot instantiate static facade class '{}'. Use static methods directly.",
                name
            )),
            "Expected runtime fatal instantiation error for '{}', got: {}",
            name,
            err_msg
        );
    }
}

#[test]
fn test_inheritance_guard_across_all_tiers() {
    register_tier2_plugin(CustomCryptoPlugin);

    let def_script = "static class MathUtil { public static function square(int $x): int { return $x * $x; } }";
    let _ = parse(def_script).expect("Static class setup should pass");

    let static_classes = ["Int", "Str", "Arr", "DB", "Log", "Memory", "CustomCrypto", "MathUtil"];

    for name in static_classes {
        let code = format!("class SubClass extends {} {{}}", name);
        let result = parse(&code);
        assert!(result.is_err(), "Expected parse error extending static facade '{}'", name);

        let err = result.unwrap_err();
        match &err.kind {
            ParseErrorKind::StaticFacadeInheritanceForbidden { class_name } => {
                assert_eq!(class_name, name);
            }
            other => panic!("Expected StaticFacadeInheritanceForbidden for '{}', got: {:?}", name, other),
        }

        let err_msg = err.to_string();
        assert!(
            err_msg.contains(&format!(
                "Fatal Error: Cannot extend static facade class '{0}'. Use '{0}::macro()' to register custom methods dynamically.",
                name
            )),
            "Unexpected inheritance error message: {}",
            err_msg
        );
    }
}

#[test]
fn test_facade_macro_registration_and_execution_across_tiers() {
    clear_runtime_closure_macros();
    register_tier2_plugin(CustomCryptoPlugin);

    let script = r#"
        static class MathUtil {
            public static function square(int $x): int {
                return $x * $x;
            }
        }

        // Tier 1 dynamic macro
        Str::macro("shout", fn(string $s) => Str::upper($s) . "!!!");
        $tier1_res = Str::shout("hello world");

        // Tier 2 dynamic macro
        CustomCrypto::macro("salt", fn(string $s) => $s . "_salt_123");
        $tier2_res = CustomCrypto::salt("pass");

        // Tier 3 dynamic macro
        MathUtil::macro("cube", fn(int $x) => $x * $x * $x);
        $tier3_res = MathUtil::cube(4);
    "#;

    let (vm, _) = run_source_with_vm(script).expect("Macro registration and execution should pass");
    assert_eq!(vm.get_main_variable("$tier1_res"), Some(&PhpValue::string("HELLO WORLD!!!")));
    assert_eq!(vm.get_main_variable("$tier2_res"), Some(&PhpValue::string("pass_salt_123")));
    assert_eq!(vm.get_main_variable("$tier3_res"), Some(&PhpValue::Int(64)));
}
