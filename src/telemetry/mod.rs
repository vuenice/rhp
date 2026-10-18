//! # Native OpenTelemetry Distributed Tracing & Metrics Engine
//!
//! Implements Technical Point #16 of the RHP Architecture Specification:
//! - **OpenTelemetry Distributed Tracing**:
//!   - W3C Distributed Trace Context propagation (`traceparent` and `baggage`)
//!   - Spans with high-resolution nanosecond timestamps, attributes, events, and links
//!   - Automatic root and child span hierarchy with context inheritance
//! - **High-Precision Runtime Metrics**:
//!   - Request latency percentiles ($p50$, $p90$, $p95$, $p99$), fiber queue depths,
//!     active connections, JIT compile times, and memory allocations
//! - **Standard APM Exporters**:
//!   - Real-time OTLP/HTTP JSON export for OpenTelemetry Collector, Jaeger, Datadog, Honeycomb, New Relic
//!   - Native error tracking & crash reporting bridges for Sentry and Datadog
//! - **Tier 1 Native Static Facade**:
//!   - `Trace` and `Telemetry` static facades for script-land instrumentation

pub mod bridges;
pub mod facade;
pub mod metrics;
pub mod otlp;
pub mod span;
pub mod trace_context;
pub mod tracer;

pub use bridges::{
    capture_exception, clear_captured_crashes, export_datadog_traces, get_captured_crashes,
    CrashReport,
};
pub use facade::dispatch_trace;
pub use metrics::{
    clear_metrics, counter, gauge, get_all_metrics, get_histogram_summary, histogram,
    HistogramSummary, MetricsRegistry, DEFAULT_HISTOGRAM_BOUNDS,
};
pub use otlp::{export_active_otlp_traces, export_otlp_metrics, export_otlp_traces};
pub use span::{
    now_unix_nanos, AttributeValue, Span, SpanEvent, SpanKind, SpanLink, SpanStatus,
    SpanStatusCode,
};
pub use trace_context::{
    extract_trace_context, inject_trace_context, Baggage, SpanContext, SpanId, TraceFlags,
    TraceId, TraceState,
};
pub use tracer::{
    add_active_span_event, clear_active_baggage, clear_completed_spans, current_parent_span_id,
    current_span, current_span_id, current_trace_id, end_active_span, get_active_baggage,
    get_completed_spans, get_service_name, record_active_span_error, record_completed_span,
    set_active_baggage, set_active_span_attribute, set_active_span_attributes, set_baggage_item,
    set_sample_rate, set_service_name, start_span, with_active_span, TracerConfig,
};
