//! OpenTelemetry Tracer, Active Span Stack & Span Recording Engine
//!
//! Provides thread-safe, fiber-aware distributed tracing:
//! - Thread-local active span stack supporting arbitrary nesting of parent and child spans
//! - Global in-memory completed spans registry for export and verification
//! - Service name, environment, and sampling configuration

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock, RwLock};

use indexmap::IndexMap;

use crate::telemetry::span::{AttributeValue, Span, SpanKind, SpanStatus};
use crate::telemetry::trace_context::{Baggage, SpanContext, SpanId, TraceId};

/// Global configuration for the OpenTelemetry distributed tracing engine.
#[derive(Debug, Clone)]
pub struct TracerConfig {
    pub service_name: String,
    pub service_version: String,
    pub environment: String,
    pub sample_rate: f64,
    pub max_completed_spans: usize,
}

impl Default for TracerConfig {
    fn default() -> Self {
        Self {
            service_name: "rhp-service".to_string(),
            service_version: "0.1.0".to_string(),
            environment: "production".to_string(),
            sample_rate: 1.0,
            max_completed_spans: 10_000,
        }
    }
}

static TRACER_CONFIG: OnceLock<RwLock<TracerConfig>> = OnceLock::new();

fn get_tracer_config() -> &'static RwLock<TracerConfig> {
    TRACER_CONFIG.get_or_init(|| RwLock::new(TracerConfig::default()))
}

/// Retrieve the active service name.
pub fn get_service_name() -> String {
    get_tracer_config()
        .read()
        .map(|c| c.service_name.clone())
        .unwrap_or_else(|_| "rhp-service".to_string())
}

/// Configure the active service name.
pub fn set_service_name(name: impl Into<String>) {
    if let Ok(mut c) = get_tracer_config().write() {
        c.service_name = name.into();
    }
}

/// Configure the sample rate (0.0 to 1.0).
pub fn set_sample_rate(rate: f64) {
    if let Ok(mut c) = get_tracer_config().write() {
        c.sample_rate = rate.clamp(0.0, 1.0);
    }
}

/// Global completed spans store for in-memory aggregation, APM export, and testing.
static COMPLETED_SPANS: OnceLock<Mutex<Vec<Span>>> = OnceLock::new();

fn get_completed_spans_store() -> &'static Mutex<Vec<Span>> {
    COMPLETED_SPANS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Append a completed span to the global store.
pub fn record_completed_span(span: Span) {
    if let Ok(mut store) = get_completed_spans_store().lock() {
        let max_spans = get_tracer_config()
            .read()
            .map(|c| c.max_completed_spans)
            .unwrap_or(10_000);
        if store.len() >= max_spans {
            // Keep buffer bounded
            store.remove(0);
        }
        store.push(span);
    }
}

/// Retrieve all completed spans currently recorded.
pub fn get_completed_spans() -> Vec<Span> {
    get_completed_spans_store()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// Clear all completed spans (useful between tests or after flushing).
pub fn clear_completed_spans() {
    if let Ok(mut store) = get_completed_spans_store().lock() {
        store.clear();
    }
}

thread_local! {
    /// Thread-local stack of active spans for the current thread/fiber.
    static ACTIVE_SPAN_STACK: RefCell<Vec<Span>> = const { RefCell::new(Vec::new()) };
    /// Active baggage for the current context.
    static ACTIVE_BAGGAGE: RefCell<Baggage> = RefCell::new(Baggage::new());
}

/// Start a new span.
///
/// If a span is already active on this thread/fiber, the new span automatically becomes
/// a child of the current active span (inheriting the active `trace_id` and setting `parent_span_id`).
/// If an explicit parent [`SpanContext`] is supplied, it is used instead.
pub fn start_span(
    name: impl Into<String>,
    kind: SpanKind,
    parent_ctx: Option<&SpanContext>,
    attributes: IndexMap<String, AttributeValue>,
) -> SpanContext {
    let mut span = ACTIVE_SPAN_STACK.with(|stack| {
        let stack = stack.borrow();
        if let Some(parent) = parent_ctx {
            // Explicit parent context provided (e.g. from incoming W3C traceparent header)
            let mut s = Span::new(name, kind, parent.trace_id, Some(parent.span_id));
            s.context.trace_flags = parent.trace_flags;
            s.context.trace_state = parent.trace_state.clone();
            s
        } else if let Some(current) = stack.last() {
            // Child span of currently active span
            Span::new(name, kind, current.trace_id(), Some(current.span_id()))
        } else {
            // Root span: generate brand new trace ID
            Span::new(name, kind, TraceId::generate(), None)
        }
    });

    span.set_attributes(attributes);
    let ctx = span.context.clone();

    ACTIVE_SPAN_STACK.with(|stack| {
        stack.borrow_mut().push(span);
    });

    ctx
}

/// End the currently active span (or the specified span), popping it from the active stack,
/// finalizing its timestamp, and recording it in the global completed spans store.
pub fn end_active_span(status: Option<SpanStatus>) -> Option<Span> {
    let mut finished = ACTIVE_SPAN_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        stack.pop()
    })?;

    if let Some(st) = status {
        finished.set_status(st);
    }
    finished.finish();
    record_completed_span(finished.clone());
    Some(finished)
}

/// Execute a closure with mutable access to the currently active span.
pub fn with_active_span<R>(f: impl FnOnce(&mut Span) -> R) -> Option<R> {
    ACTIVE_SPAN_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        stack.last_mut().map(f)
    })
}

/// Set an attribute on the currently active span.
pub fn set_active_span_attribute(key: impl Into<String>, val: AttributeValue) -> bool {
    with_active_span(|span| {
        span.set_attribute(key, val);
    })
    .is_some()
}

/// Bulk set attributes on the currently active span.
pub fn set_active_span_attributes(attrs: IndexMap<String, AttributeValue>) -> bool {
    with_active_span(|span| {
        span.set_attributes(attrs);
    })
    .is_some()
}

/// Add an event to the currently active span.
pub fn add_active_span_event(event: crate::telemetry::span::SpanEvent) -> bool {
    with_active_span(|span| {
        span.add_event(event);
    })
    .is_some()
}

/// Record an exception on the currently active span and set its status to Error.
pub fn record_active_span_error(message: &str) -> bool {
    with_active_span(|span| {
        span.record_exception("RuntimeError", message, None);
    })
    .is_some()
}

/// Retrieve the active [`TraceId`], if any.
pub fn current_trace_id() -> Option<TraceId> {
    ACTIVE_SPAN_STACK.with(|stack| stack.borrow().last().map(|s| s.trace_id()))
}

/// Retrieve the active [`SpanId`], if any.
pub fn current_span_id() -> Option<SpanId> {
    ACTIVE_SPAN_STACK.with(|stack| stack.borrow().last().map(|s| s.span_id()))
}

/// Retrieve the parent [`SpanId`] of the active span, if any.
pub fn current_parent_span_id() -> Option<SpanId> {
    ACTIVE_SPAN_STACK.with(|stack| stack.borrow().last().and_then(|s| s.parent_span_id))
}

/// Retrieve a clone of the currently active span.
pub fn current_span() -> Option<Span> {
    ACTIVE_SPAN_STACK.with(|stack| stack.borrow().last().cloned())
}

/// Get the active Baggage for the current thread/fiber.
pub fn get_active_baggage() -> Baggage {
    ACTIVE_BAGGAGE.with(|b| b.borrow().clone())
}

/// Set or update a baggage entry in the active context.
pub fn set_baggage_item(key: impl Into<String>, value: impl Into<String>) {
    ACTIVE_BAGGAGE.with(|b| b.borrow_mut().set(key, value));
}

/// Overwrite active baggage.
pub fn set_active_baggage(baggage: Baggage) {
    ACTIVE_BAGGAGE.with(|b| *b.borrow_mut() = baggage);
}

/// Clear active baggage.
pub fn clear_active_baggage() {
    ACTIVE_BAGGAGE.with(|b| *b.borrow_mut() = Baggage::new());
}
