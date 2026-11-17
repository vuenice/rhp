//! Comprehensive test suite for RHP's centralized structured logging subsystem.

use rhp::{
    clear_tier2_plugins, end_active_span, run_source, run_source_in_fiber, run_source_with_vm,
    set_active_request_id, start_span, FrozenValue, LogHandler, LogLevel, LogRecord, LogRegistry,
    PhpValue, SpanKind, StdoutLogHandler,
};
use std::sync::{Arc, Mutex};

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_standard_call_site_api_and_levels() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();
    let fake = LogRegistry::global().fake();

    let code = r#"
        Log::debug("Debug event", ["debug_code" => 101]);
        Log::info("Service started", ["host" => "127.0.0.1", "port" => 8080]);
        Log::notice("Config reloaded");
        Log::warn("Slow query detected", ["duration_ms" => 150]);
        Log::error("Failed to connect to redis", ["retry" => 3]);
        Log::critical("Payment gateway down", ["gateway" => "stripe"]);
        Log::alert("Disk 95% full", ["path" => "/data"]);
        Log::emergency("System out of memory", ["available_mb" => 0]);
        Log::log("info", "Dynamic log level", ["status" => "ok"]);
    "#;

    let _ = run_source(code).expect("Execution should succeed");

    assert_eq!(fake.count(), 9);

    let records = fake.records();
    assert_eq!(records[0].level, LogLevel::Debug);
    assert_eq!(records[0].message, "Debug event");

    assert_eq!(records[1].level, LogLevel::Info);
    assert_eq!(records[1].message, "Service started");

    assert_eq!(records[2].level, LogLevel::Notice);
    assert_eq!(records[2].message, "Config reloaded");

    assert_eq!(records[3].level, LogLevel::Warn);
    assert_eq!(records[3].message, "Slow query detected");

    assert_eq!(records[4].level, LogLevel::Error);
    assert_eq!(records[4].message, "Failed to connect to redis");

    assert_eq!(records[5].level, LogLevel::Critical);
    assert_eq!(records[5].message, "Payment gateway down");

    assert_eq!(records[6].level, LogLevel::Alert);
    assert_eq!(records[6].message, "Disk 95% full");

    assert_eq!(records[7].level, LogLevel::Emergency);
    assert_eq!(records[7].message, "System out of memory");

    assert_eq!(records[8].level, LogLevel::Info);
    assert_eq!(records[8].message, "Dynamic log level");

    LogRegistry::global().restore();
}

#[test]
fn test_log_fake_in_userland_script() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();
    LogRegistry::global().restore();

    let code = r#"
        $fake = Log::fake();

        Log::info("Booting kernel");
        Log::channel("auth")->warn("Suspicious login attempt", ["ip" => "10.0.0.1"]);
        Log::error("Authentication failed");

        $count = $fake->count();
        $hasAuth = $fake->has("warn", "Suspicious login");
        $hasMissing = $fake->has("debug", "Nonexistent");
        $hasMessage = $fake->hasMessage("failed");

        $fake->assertLogged("info", "Booting kernel");
        $fake->assertLogged("warn", "Suspicious login");
        $fake->assertCount(3);

        $records = $fake->records();
        $firstMsg = $records[0]["message"];
        $secondChannel = $records[1]["channel"];

        $fake->clear();
        $countAfterClear = $fake->count();

        Log::restore();
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Log::fake execution should succeed");

    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(3)));
    assert_eq!(vm.get_main_variable("$hasAuth"), Some(&PhpValue::Bool(true)));
    assert_eq!(
        vm.get_main_variable("$hasMissing"),
        Some(&PhpValue::Bool(false))
    );
    assert_eq!(
        vm.get_main_variable("$hasMessage"),
        Some(&PhpValue::Bool(true))
    );
    assert_eq!(
        vm.get_main_variable("$firstMsg"),
        Some(&PhpValue::string("Booting kernel"))
    );
    assert_eq!(
        vm.get_main_variable("$secondChannel"),
        Some(&PhpValue::string("auth"))
    );
    assert_eq!(
        vm.get_main_variable("$countAfterClear"),
        Some(&PhpValue::Int(0))
    );
}

#[test]
fn test_automatic_fiber_and_trace_and_request_context_propagation() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();
    let fake = LogRegistry::global().fake();

    // Start an OpenTelemetry span
    start_span(
        "http_request_handler".to_string(),
        SpanKind::Server,
        None,
        indexmap::IndexMap::new(),
    );

    // Set active request ID
    set_active_request_id("req_abc_789");

    let code = r#"
        Log::info("Inside fiber execution", ["work" => "compute"]);
    "#;

    let _ = run_source_in_fiber(code, "worker-1")
        .expect("Execution with fiber and trace context should succeed");

    assert_eq!(fake.count(), 1);
    let record = fake.last().expect("Record should exist");

    // Verify automatic Fiber ID capture
    assert!(
        record.fiber_id.is_some(),
        "Fiber ID should be automatically captured"
    );

    // Verify automatic Request ID capture
    assert_eq!(record.request_id.as_deref(), Some("req_abc_789"));

    // Verify automatic OpenTelemetry trace and span ID capture
    assert!(
        record.trace_id.is_some(),
        "Trace ID should be automatically captured"
    );
    assert!(
        record.span_id.is_some(),
        "Span ID should be automatically captured"
    );

    // Verify serialized RHP dictionary has all context fields
    let dict = record.to_php_value();
    if let PhpValue::Dict(d) = dict {
        let map = d.borrow();
        assert!(map.contains_key("fiber_id"));
        assert_eq!(
            map.get("request_id"),
            Some(&PhpValue::string("req_abc_789"))
        );
        assert!(map.contains_key("trace_id"));
        assert!(map.contains_key("span_id"));
    } else {
        panic!("Expected Dict PhpValue");
    }

    end_active_span(None);
    LogRegistry::global().restore();
}

#[test]
fn test_swappable_channel_architecture() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();
    let fake = LogRegistry::global().fake();

    let code = r#"
        $sec = Log::channel("security");
        $sec->info("User authenticated", ["user_id" => 42]);
        $sec->error("Invalid 2FA token", ["attempts" => 3]);

        $audit = Log::channel("audit");
        $audit->notice("Table created", ["table" => "orders"]);

        $secChannelName = $sec->getChannel();
        $auditChannelName = $audit->getChannel();
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Channel logging should succeed");
    assert_eq!(
        vm.get_main_variable("$secChannelName"),
        Some(&PhpValue::string("security"))
    );
    assert_eq!(
        vm.get_main_variable("$auditChannelName"),
        Some(&PhpValue::string("audit"))
    );

    let records = fake.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].channel, "security");
    assert_eq!(records[0].message, "User authenticated");
    assert_eq!(records[1].channel, "security");
    assert_eq!(records[1].message, "Invalid 2FA token");
    assert_eq!(records[2].channel, "audit");
    assert_eq!(records[2].message, "Table created");

    LogRegistry::global().restore();
}

#[test]
fn test_userland_custom_handler_class() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();
    LogRegistry::global().restore();

    let code = r#"
        class MemoryLogger {
            public list $captured = [];
            public function handle(dict $record): void {
                $this->captured = Arr::list([...$this->captured, $record["message"]]);
            }
        }

        $logger = new MemoryLogger();
        Log::setHandler($logger);

        Log::info("Transaction completed");
        Log::error("Service unreachable");

        $count = Arr::list($logger->captured)->count();
        $first = $logger->captured[0];
        $second = $logger->captured[1];

        Log::restore();
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Userland handler execution should succeed");
    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(2)));
    assert_eq!(
        vm.get_main_variable("$first"),
        Some(&PhpValue::string("Transaction completed"))
    );
    assert_eq!(
        vm.get_main_variable("$second"),
        Some(&PhpValue::string("Service unreachable"))
    );
}

#[test]
fn test_lazy_evaluation_level_filtering() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();

    let code = r#"
        $fake = Log::fake();
        Log::setLevel("warn");
        $debugEnabled = Log::isEnabled("debug");
        $infoEnabled = Log::isEnabled("info");
        $warnEnabled = Log::isEnabled("warn");
        $errorEnabled = Log::isEnabled("error");

        // These should be skipped at zero cost
        Log::debug("Should be skipped");
        Log::info("Should also be skipped");

        // These should be captured
        Log::warn("Warning logged");
        Log::error("Error logged");

        $count = $fake->count();
    "#;

    let (vm, _) = run_source_with_vm(code).expect("Lazy evaluation test should succeed");
    assert_eq!(
        vm.get_main_variable("$debugEnabled"),
        Some(&PhpValue::Bool(false))
    );
    assert_eq!(
        vm.get_main_variable("$infoEnabled"),
        Some(&PhpValue::Bool(false))
    );
    assert_eq!(
        vm.get_main_variable("$warnEnabled"),
        Some(&PhpValue::Bool(true))
    );
    assert_eq!(
        vm.get_main_variable("$errorEnabled"),
        Some(&PhpValue::Bool(true))
    );
    assert_eq!(vm.get_main_variable("$count"), Some(&PhpValue::Int(2)));

    let records = LogRegistry::global().get_recorded_logs();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].level, LogLevel::Warn);
    assert_eq!(records[0].message, "Warning logged");
    assert_eq!(records[1].level, LogLevel::Error);
    assert_eq!(records[1].message, "Error logged");

    LogRegistry::global().restore();
}

#[test]
fn test_json_and_pretty_formatting() {
    let record = LogRecord {
        timestamp_millis: 1700000000000,
        timestamp_iso: "2026-11-17T18:00:00.000Z".to_string(),
        level: LogLevel::Info,
        message: "Order placed".to_string(),
        channel: "sales".to_string(),
        context: Some(FrozenValue::Dict(vec![(
            "order_id".to_string(),
            FrozenValue::Int(9999),
        )])),
        fiber_id: Some(5),
        request_id: Some("req_xyz".to_string()),
        trace_id: Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string()),
        span_id: Some("00f067aa0ba902b7".to_string()),
    };

    // Test JSON line formatting (production mode)
    let json = record.to_json_line();
    assert!(json.contains("\"level\":\"info\""));
    assert!(json.contains("\"message\":\"Order placed\""));
    assert!(json.contains("\"channel\":\"sales\""));
    assert!(json.contains("\"fiber_id\":5"));
    assert!(json.contains("\"request_id\":\"req_xyz\""));
    assert!(json.contains("\"trace_id\":\"4bf92f3577b34da6a3ce929d0e0e4736\""));
    assert!(json.contains("\"span_id\":\"00f067aa0ba902b7\""));
    assert!(json.contains("\"order_id\":9999"));

    // Verify it is valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("Should be valid JSON");
    assert_eq!(parsed["level"], "info");
    assert_eq!(parsed["context"]["order_id"], 9999);

    // Test Pretty formatting (development mode)
    let pretty = record.to_pretty_line();
    assert!(pretty.contains("INFO"));
    assert!(pretty.contains("Order placed"));
    assert!(pretty.contains("[sales]"));
    assert!(pretty.contains("fiber#5"));
    assert!(pretty.contains("req#req_xyz"));
    assert!(pretty.contains("trace=4bf92f3577b34da6a3ce929d0e0e4736"));
}

#[test]
fn test_swappable_rust_handler_implementation() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();

    struct MemoryLogCollector {
        captured: Mutex<Vec<String>>,
    }

    impl LogHandler for MemoryLogCollector {
        fn enabled(&self, level: LogLevel, _channel: &str) -> bool {
            level >= LogLevel::Info
        }

        fn handle(&self, record: &LogRecord) -> Result<(), rhp::RuntimeError> {
            self.captured
                .lock()
                .unwrap()
                .push(format!("{}: {}", record.level, record.message));
            Ok(())
        }
    }

    let collector = Arc::new(MemoryLogCollector {
        captured: Mutex::new(Vec::new()),
    });

    LogRegistry::global().set_default_handler(collector.clone());

    let code = r#"
        Log::debug("Should be ignored by custom collector");
        Log::info("Collector received this");
        Log::error("Collector received error");
    "#;

    let _ = run_source(code).expect("Execution should succeed");

    let messages = collector.captured.lock().unwrap().clone();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], "info: Collector received this");
    assert_eq!(messages[1], "error: Collector received error");

    // Restore standard handler
    LogRegistry::global().set_default_handler(Arc::new(StdoutLogHandler::new()));
}

#[test]
fn test_disabled_log_level_near_zero_overhead() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tier2_plugins();
    LogRegistry::global().restore();
    LogRegistry::global().set_min_level(LogLevel::Warn);

    let code = r#"
        for ($i = 0; $i < 10000; $i++) {
            Log::debug("Expensive message format that should be completely skipped");
            Log::info("Another info skipped message");
        }
    "#;

    let start = std::time::Instant::now();
    let _ = run_source(code).expect("Disabled logging loop should succeed");
    let elapsed = start.elapsed();

    // 20,000 skipped log facade calls should execute in mere milliseconds (sub-microsecond per call)
    assert!(
        elapsed.as_millis() < 100,
        "20,000 skipped log calls took too long: {:?}",
        elapsed
    );

    LogRegistry::global().restore();
}
