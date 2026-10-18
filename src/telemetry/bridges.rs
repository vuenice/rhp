//! Native Error Tracking & APM Bridges (Sentry & Datadog)
//!
//! Provides native in-memory exception capture pipelines and crash reporting
//! formatters without requiring legacy Zend C extensions:
//! - Sentry-compatible event payloads with stack traces, breadcrumbs, tags, and trace context
//! - Datadog APM trace formatting with service tags and resource names

use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;

use crate::telemetry::span::Span;
use crate::telemetry::trace_context::TraceId;
use crate::telemetry::tracer::{current_span_id, current_trace_id, get_service_name};
use crate::vm::error::StackTraceEntry;
use crate::vm::value::PhpValue;

/// An in-memory crash report captured by the native exception tracking pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct CrashReport {
    pub id: String,
    pub exception_class: String,
    pub message: String,
    pub stack_trace: Vec<StackTraceEntry>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp_unix_nano: u64,
    pub tags: IndexMap<String, String>,
}

impl CrashReport {
    /// Format this crash report into a standard Sentry event payload.
    pub fn to_sentry_json(&self) -> String {
        let service_name = get_service_name();
        let sec = (self.timestamp_unix_nano / 1_000_000_000) as f64;

        let frames_json: Vec<String> = self
            .stack_trace
            .iter()
            .map(|frame| {
                format!(
                    "{{\"filename\":\"file_{}\",\"lineno\":{},\"function\":\"{}\"}}",
                    frame.span.file_id, frame.span.line, frame.function_name
                )
            })
            .collect();

        let trace_context = match (&self.trace_id, &self.span_id) {
            (Some(tid), Some(sid)) => format!(
                ",\"contexts\":{{\"trace\":{{\"trace_id\":\"{}\",\"span_id\":\"{}\",\"op\":\"rhp.error\"}}}}",
                tid, sid
            ),
            _ => String::new(),
        };

        let mut tags_vec = Vec::new();
        for (k, v) in &self.tags {
            tags_vec.push(format!("\"{}\":\"{}\"", k, v));
        }
        let tags_str = if tags_vec.is_empty() {
            String::new()
        } else {
            format!(",\"tags\":{{{}}}", tags_vec.join(","))
        };

        format!(
            "{{\"event_id\":\"{}\",\"timestamp\":{:.3},\"platform\":\"rhp\",\"level\":\"error\",\"server_name\":\"{}\",\"exception\":{{\"values\":[{{\"type\":\"{}\",\"value\":\"{}\",\"stacktrace\":{{\"frames\":[{}]}}}}]}}{}{}}}",
            self.id,
            sec,
            service_name,
            self.exception_class,
            self.message.replace('"', "\\\""),
            frames_json.join(","),
            trace_context,
            tags_str
        )
    }

    /// Convert to script-land dictionary (`Arr::dict`).
    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert("id".to_string(), PhpValue::string(self.id.clone()));
        map.insert(
            "exception_class".to_string(),
            PhpValue::string(self.exception_class.clone()),
        );
        map.insert(
            "message".to_string(),
            PhpValue::string(self.message.clone()),
        );
        map.insert(
            "trace_id".to_string(),
            self.trace_id
                .as_ref()
                .map(|t| PhpValue::string(t.clone()))
                .unwrap_or(PhpValue::Null),
        );
        map.insert(
            "span_id".to_string(),
            self.span_id
                .as_ref()
                .map(|s| PhpValue::string(s.clone()))
                .unwrap_or(PhpValue::Null),
        );
        map.insert(
            "sentry_payload".to_string(),
            PhpValue::string(self.to_sentry_json()),
        );
        PhpValue::dict_from_map(map)
    }
}

static CAPTURED_CRASHES: OnceLock<Mutex<Vec<CrashReport>>> = OnceLock::new();

fn get_crashes_store() -> &'static Mutex<Vec<CrashReport>> {
    CAPTURED_CRASHES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Capture an unhandled exception or crash report into the native pipeline.
pub fn capture_exception(
    exception_class: &str,
    message: &str,
    stack_trace: &[StackTraceEntry],
    tags: Option<IndexMap<String, String>>,
) -> CrashReport {
    let id = TraceId::generate().to_hex();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let trace_id = current_trace_id().map(|t| t.to_hex());
    let span_id = current_span_id().map(|s| s.to_hex());

    let report = CrashReport {
        id,
        exception_class: exception_class.to_string(),
        message: message.to_string(),
        stack_trace: stack_trace.to_vec(),
        trace_id,
        span_id,
        timestamp_unix_nano: now,
        tags: tags.unwrap_or_default(),
    };

    if let Ok(mut store) = get_crashes_store().lock() {
        store.push(report.clone());
    }

    report
}

/// Retrieve all captured crash reports.
pub fn get_captured_crashes() -> Vec<CrashReport> {
    get_crashes_store()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// Clear captured crash reports.
pub fn clear_captured_crashes() {
    if let Ok(mut store) = get_crashes_store().lock() {
        store.clear();
    }
}

/// Format completed spans into Datadog APM trace format.
///
/// Datadog expects a list of traces: `[[ { "trace_id": uint64, "span_id": uint64, ... } ]]`
pub fn export_datadog_traces(spans: &[Span]) -> String {
    let service_name = get_service_name();
    let mut span_json_list = Vec::new();

    for s in spans {
        // Datadog uses 64-bit unsigned integers for trace_id and span_id
        let trace_id_bytes = &s.context.trace_id.0[8..16];
        let trace_id_u64 = u64::from_be_bytes(trace_id_bytes.try_into().unwrap_or([0; 8]));
        let span_id_u64 = u64::from_be_bytes(s.context.span_id.0);

        let parent_str = s
            .parent_span_id
            .map(|p| {
                let pu64 = u64::from_be_bytes(p.0);
                format!(",\"parent_id\":{}", pu64)
            })
            .unwrap_or_default();

        let error_val = if s.status.code == crate::telemetry::span::SpanStatusCode::Error {
            1
        } else {
            0
        };

        let duration_ns = s.duration_nanos();

        span_json_list.push(format!(
            "{{\"trace_id\":{},\"span_id\":{}{},\"name\":\"{}\",\"resource\":\"{}\",\"service\":\"{}\",\"type\":\"web\",\"error\":{},\"start\":{},\"duration\":{}}}",
            trace_id_u64,
            span_id_u64,
            parent_str,
            s.name.replace('"', "\\\""),
            s.name.replace('"', "\\\""),
            service_name,
            error_val,
            s.start_time_unix_nano,
            duration_ns
        ));
    }

    format!("[[{}]]", span_json_list.join(","))
}
