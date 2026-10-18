//! Comprehensive OpenTelemetry Integration Tests
//!
//! Tests OpenTelemetry Distributed Tracing, Metrics, OTLP Export,
//! Context Propagation, Sentry/Datadog Bridges, and Worker Auto-Instrumentation.

use indexmap::IndexMap;
use rhp::telemetry::{
    add_active_span_event, capture_exception, clear_captured_crashes, clear_completed_spans,
    clear_metrics, counter, current_parent_span_id, current_span_id, current_trace_id,
    end_active_span, export_active_otlp_traces, export_datadog_traces, export_otlp_metrics,
    extract_trace_context, gauge, get_captured_crashes, get_completed_spans,
    get_histogram_summary, histogram, inject_trace_context, set_active_span_attribute,
    set_service_name, start_span, with_active_span, AttributeValue, Baggage, SpanContext,
    SpanEvent, SpanId, SpanKind, SpanStatus, SpanStatusCode, TraceFlags, TraceId,
};
use rhp::vm::{
    execute_isolated_request, run_source, FrozenValue, ParentProcess, PhpValue, WorkerRequest,
    WorkerResponseStatus,
};

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// =========================================================================
// 1. W3C Trace Context & Baggage Distributed Propagation
// =========================================================================

#[test]
fn test_w3c_traceparent_parsing_and_serialization() {
    let raw_header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let ctx = SpanContext::parse_traceparent(raw_header).expect("Must parse valid traceparent");

    assert_eq!(ctx.trace_id.to_hex(), "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(ctx.span_id.to_hex(), "00f067aa0ba902b7");
    assert!(ctx.trace_flags.is_sampled());
    assert!(ctx.is_remote);
    assert_eq!(ctx.format_traceparent(), raw_header);

    // Reject all-zero TraceId as per W3C specification
    let zero_trace = "00-00000000000000000000000000000000-00f067aa0ba902b7-01";
    assert!(SpanContext::parse_traceparent(zero_trace).is_none());

    // Reject all-zero SpanId as per W3C specification
    let zero_span = "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01";
    assert!(SpanContext::parse_traceparent(zero_span).is_none());

    // Reject invalid delimiter counts
    let invalid_parts = "00-4bf92f3577b34da6a3ce929d0e0e4736-01";
    assert!(SpanContext::parse_traceparent(invalid_parts).is_none());
}

#[test]
fn test_w3c_baggage_propagation() {
    let header = "userId=alice,serverNode=df28;region=us-east-1,tenantId=cust_99";
    let baggage = Baggage::parse(header);

    assert_eq!(baggage.get("userId"), Some("alice"));
    assert_eq!(baggage.get("serverNode"), Some("df28"));
    assert_eq!(baggage.get("tenantId"), Some("cust_99"));
    assert_eq!(baggage.get("nonexistent"), None);

    let mut carrier = IndexMap::new();
    let ctx = SpanContext::new(TraceId::generate(), SpanId::generate(), TraceFlags::SAMPLED);
    inject_trace_context(&mut carrier, &ctx, Some(&baggage));

    assert!(carrier.contains_key("traceparent"));
    assert!(carrier.contains_key("baggage"));

    let (extracted_ctx, extracted_baggage) = extract_trace_context(&carrier);
    assert!(extracted_ctx.is_some());
    assert_eq!(
        extracted_ctx.unwrap().trace_id.to_hex(),
        ctx.trace_id.to_hex()
    );

    let eb = extracted_baggage.expect("Baggage must be extracted");
    assert_eq!(eb.get("userId"), Some("alice"));
    assert_eq!(eb.get("serverNode"), Some("df28"));
    assert_eq!(eb.get("tenantId"), Some("cust_99"));
}

// =========================================================================
// 2. Span Hierarchy, Attributes, Events, and Exceptions
// =========================================================================

#[test]
fn test_span_nesting_and_lifecycle() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();

    let root_ctx = start_span(
        "root_operation",
        SpanKind::Server,
        None,
        IndexMap::new(),
    );
    assert_eq!(current_trace_id(), Some(root_ctx.trace_id));
    assert_eq!(current_span_id(), Some(root_ctx.span_id));
    assert_eq!(current_parent_span_id(), None);

    set_active_span_attribute(
        "http.route",
        AttributeValue::String("/api/orders".to_string()),
    );

    // Child span inherits root trace ID and sets parent_span_id to root span ID
    let child_ctx = start_span(
        "child_database_query",
        SpanKind::Client,
        None,
        IndexMap::new(),
    );
    assert_eq!(current_trace_id(), Some(root_ctx.trace_id));
    assert_eq!(current_span_id(), Some(child_ctx.span_id));
    assert_eq!(current_parent_span_id(), Some(root_ctx.span_id));

    set_active_span_attribute(
        "db.statement",
        AttributeValue::String("SELECT * FROM orders WHERE id = 1".to_string()),
    );

    let finished_child = end_active_span(Some(SpanStatus::ok())).expect("Child span must finish");
    assert_eq!(finished_child.name, "child_database_query");
    assert_eq!(finished_child.parent_span_id, Some(root_ctx.span_id));
    assert_eq!(finished_child.status.code, SpanStatusCode::Ok);

    // After child finishes, active span returns to root span
    assert_eq!(current_span_id(), Some(root_ctx.span_id));
    assert_eq!(current_parent_span_id(), None);

    let finished_root = end_active_span(Some(SpanStatus::ok())).expect("Root span must finish");
    assert_eq!(finished_root.name, "root_operation");
    assert_eq!(finished_root.parent_span_id, None);

    // Both spans recorded in completed spans store
    let spans = get_completed_spans();
    assert!(spans.iter().any(|s| s.name == "child_database_query"));
    assert!(spans.iter().any(|s| s.name == "root_operation"));
}

#[test]
fn test_span_events_and_exception_recording() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();

    let _ctx = start_span("risky_task", SpanKind::Internal, None, IndexMap::new());

    let event = SpanEvent::new("cache_miss")
        .with_attribute("key", AttributeValue::String("user_42".to_string()));
    add_active_span_event(event);

    with_active_span(|span| {
        span.record_exception(
            "ConnectionTimeout",
            "Failed to connect to primary DB replica",
            Some("at OrderService::find (orders.rhp:42)"),
        );
    });

    let finished = end_active_span(None).expect("Span must finish");
    assert_eq!(finished.status.code, SpanStatusCode::Error);
    assert_eq!(
        finished.status.description,
        "Failed to connect to primary DB replica"
    );

    // Events should include both custom event and exception event
    assert_eq!(finished.events.len(), 2);
    assert_eq!(finished.events[0].name, "cache_miss");
    assert_eq!(finished.events[1].name, "exception");
    assert_eq!(
        finished.events[1].attributes.get("exception.type"),
        Some(&AttributeValue::String("ConnectionTimeout".to_string()))
    );
}

// =========================================================================
// 3. High-Precision Metrics & Histograms
// =========================================================================

#[test]
fn test_metrics_counter_gauge_and_histogram_percentiles() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_metrics();

    counter("requests.total", 10.0);
    counter("requests.total", 5.0);
    gauge("memory.allocated_mb", 128.5);

    // Record latency samples: 10, 20, 30, 40, 50, 60, 70, 80, 90, 100 ms
    for i in 1..=10 {
        histogram("http.latency_ms", (i * 10) as f64);
    }

    let summary = get_histogram_summary("http.latency_ms").expect("Summary must exist");
    assert_eq!(summary.count, 10);
    assert_eq!(summary.min, 10.0);
    assert_eq!(summary.max, 100.0);
    assert_eq!(summary.sum, 550.0);
    assert_eq!(summary.avg, 55.0);

    // Verify percentiles (p50, p90, p95, p99)
    assert!(summary.p50 >= 50.0 && summary.p50 <= 60.0);
    assert!(summary.p90 >= 90.0 && summary.p90 <= 95.0);
    assert!(summary.p95 >= 95.0 && summary.p95 <= 100.0);
    assert!(summary.p99 >= 99.0 && summary.p99 <= 100.0);
}

// =========================================================================
// 4. Standard OTLP / Jaeger / Datadog / Sentry JSON Exporters
// =========================================================================

#[test]
fn test_otlp_traces_and_metrics_export_json() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();
    clear_metrics();
    set_service_name("billing-microservice");

    let _ctx = start_span(
        "process_payment",
        SpanKind::Server,
        None,
        IndexMap::new(),
    );
    set_active_span_attribute("payment.amount", AttributeValue::Float(99.99));
    end_active_span(Some(SpanStatus::ok()));

    histogram("rhp.request.duration_ms", 12.5);
    counter("payments.processed", 1.0);

    let otlp_traces_json = export_active_otlp_traces();
    assert!(otlp_traces_json.contains("\"billing-microservice\""));
    assert!(otlp_traces_json.contains("\"process_payment\""));
    assert!(otlp_traces_json.contains("\"resourceSpans\""));
    assert!(otlp_traces_json.contains("\"scopeSpans\""));

    let otlp_metrics_json = export_otlp_metrics();
    assert!(otlp_metrics_json.contains("\"resourceMetrics\""));
    assert!(otlp_metrics_json.contains("\"rhp.request.duration_ms\""));

    let datadog_json = export_datadog_traces(&get_completed_spans());
    assert!(datadog_json.contains("\"billing-microservice\""));
    assert!(datadog_json.contains("\"process_payment\""));
}

#[test]
fn test_sentry_crash_reporting_bridge() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_captured_crashes();
    set_service_name("checkout-service");

    let report = capture_exception(
        "OutOfBoundsException",
        "Array index 99 out of bounds",
        &[],
        None,
    );

    let sentry_json = report.to_sentry_json();
    assert!(sentry_json.contains("\"checkout-service\""));
    assert!(sentry_json.contains("\"OutOfBoundsException\""));
    assert!(sentry_json.contains("\"Array index 99 out of bounds\""));
    assert!(sentry_json.contains("\"platform\":\"rhp\""));
    assert!(sentry_json.contains("\"level\":\"error\""));
}

// =========================================================================
// 5. Tier 1 Native Static Facade `Trace` in RHP Script-land
// =========================================================================

#[test]
fn test_trace_facade_basic_spans_in_script() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();

    let source = r#"
        Trace::startSpan('manual_user_sync', Arr::dict(['kind' => 'SERVER']));
        Trace::setAttribute('sync.entity', 'users');
        $traceId = Trace::traceId();
        $spanId = Trace::spanId();
        Trace::addEvent('db_connected', Arr::dict(['pool_size' => 10]));
        Trace::endSpan(Arr::dict(['status' => 'OK']));
        return Arr::dict([
            'trace_id' => $traceId,
            'span_id' => $spanId,
        ]);
    "#;

    let res = run_source(source).expect("Script execution must succeed");
    if let PhpValue::Dict(d) = res {
        let map = d.borrow();
        assert!(map.get("trace_id").is_some());
        assert!(map.get("span_id").is_some());
    } else {
        panic!("Expected dict result");
    }

    let completed = get_completed_spans();
    assert!(completed.iter().any(|s| s.name == "manual_user_sync"));
}

#[test]
fn test_trace_facade_closure_span_helper() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();

    let source = r#"
        $result = Trace::span('calculate_tax', function() {
            $sub = 100;
            $tax = 15;
            Trace::setAttribute('tax.rate', '15%');
            return $sub + $tax;
        }, Arr::dict(['category' => 'finance']));

        return $result;
    "#;

    let res = run_source(source).expect("Script execution must succeed");
    assert_eq!(res.to_i32(), Some(115));

    let completed = get_completed_spans();
    let span = completed
        .iter()
        .find(|s| s.name == "calculate_tax")
        .expect("calculate_tax span must be recorded");
    assert_eq!(span.status.code, SpanStatusCode::Ok);
    assert_eq!(
        span.attributes.get("category"),
        Some(&AttributeValue::String("finance".to_string()))
    );
    assert_eq!(
        span.attributes.get("tax.rate"),
        Some(&AttributeValue::String("15%".to_string()))
    );
}

#[test]
fn test_trace_facade_metrics_and_otlp_export_in_script() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_metrics();
    clear_completed_spans();

    let source = r#"
        Trace::counter('orders.created', 5);
        Trace::gauge('queue.depth', 42);
        Trace::histogram('order.value', 199);
        Trace::histogram('order.value', 299);

        $summary = Trace::metricSummary('order.value');
        $otlp = Trace::exportOtlp();
        $metricsOtlp = Trace::exportOtlpMetrics();

        return Arr::dict([
            'count' => $summary['count'],
            'avg' => $summary['avg'],
            'otlp_len' => Str::length($otlp),
            'metrics_otlp_len' => Str::length($metricsOtlp),
        ]);
    "#;

    let res = run_source(source).expect("Script execution must succeed");
    if let PhpValue::Dict(d) = res {
        let map = d.borrow();
        assert_eq!(map.get("count").and_then(|v| v.to_i32()), Some(2));
        assert_eq!(map.get("avg").and_then(|v| v.to_f32()), Some(249.0));
        assert!(map.get("otlp_len").and_then(|v| v.to_i32()).unwrap_or(0) > 0);
        assert!(map.get("metrics_otlp_len").and_then(|v| v.to_i32()).unwrap_or(0) > 0);
    } else {
        panic!("Expected dict result");
    }
}

// =========================================================================
// 6. Worker Request & Facade Auto-Instrumentation
// =========================================================================

#[test]
fn test_worker_incoming_request_w3c_context_extraction_and_root_span() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();

    let state = ParentProcess::boot("").expect("ParentProcess boot");
    let mut req = WorkerRequest::new("return 100 * 2;");
    req.headers.insert(
        "traceparent".to_string(),
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string(),
    );
    req.headers.insert("baggage".to_string(), "userId=alice123".to_string());

    let resp = execute_isolated_request(&state, &req, 1);
    assert_eq!(resp.status, WorkerResponseStatus::Success);
    assert_eq!(resp.value, FrozenValue::Int(200));

    // Response contains distributed tracing IDs
    assert_eq!(resp.trace_id(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
    assert!(resp.span_id().is_some());

    // Completed root server span inherits the incoming W3C trace ID
    let spans = get_completed_spans();
    let root = spans
        .iter()
        .find(|s| s.context.trace_id.to_hex() == "4bf92f3577b34da6a3ce929d0e0e4736")
        .expect("Root server span must exist");
    assert_eq!(root.kind, SpanKind::Server);
    assert_eq!(root.parent_span_id.unwrap().to_hex(), "00f067aa0ba902b7");
    assert_eq!(root.status.code, SpanStatusCode::Ok);
}

#[test]
fn test_auto_instrumentation_http_and_db_facades() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();

    let source = r#"
        // 1. Outgoing HTTP client call auto-instruments client span & injects traceparent
        $response = HTTP::get('https://api.example.com/status');
        $headers = $response->headers;

        // 2. Database query auto-instruments database client span
        $table = DB::table('users');
        $row = DB::find('users', 42);

        return $headers['traceparent'] ?? '';
    "#;

    let res = run_source(source).expect("Script execution must succeed");
    let traceparent_header = res.to_php_string();
    assert!(traceparent_header.starts_with("00-"));

    let spans = get_completed_spans();
    // Verify HTTP client span was created
    assert!(spans.iter().any(|s| s.name == "HTTP GET" && s.kind == SpanKind::Client));
    // Verify DB client spans were created
    assert!(spans.iter().any(|s| s.name == "DB::table" && s.kind == SpanKind::Client));
    assert!(spans.iter().any(|s| s.name == "DB::find" && s.kind == SpanKind::Client));
}

#[test]
fn test_runtime_error_and_panic_auto_capture_in_spans() {
    let _guard = TEST_LOCK.lock().unwrap();
    clear_completed_spans();
    clear_captured_crashes();

    let state = ParentProcess::boot("").expect("ParentProcess boot");
    let req = WorkerRequest::new("return 10 / 0;"); // DivisionByZero
    let resp = execute_isolated_request(&state, &req, 1);

    assert_eq!(resp.status, WorkerResponseStatus::RuntimeError);
    assert!(resp.error.is_some());

    let spans = get_completed_spans();
    let err_span = spans.last().expect("Error span must be recorded");
    assert_eq!(err_span.status.code, SpanStatusCode::Error);

    let crashes = get_captured_crashes();
    assert!(!crashes.is_empty());
    assert_eq!(crashes[0].exception_class, "RuntimeError");
}
