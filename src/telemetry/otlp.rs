//! OpenTelemetry Protocol (OTLP) Exporter Engine
//!
//! Generates standard OTLP/HTTP JSON export payloads compatible with:
//! - OpenTelemetry Collector (`/v1/traces`, `/v1/metrics`)
//! - Jaeger Tracing (OTLP Ingestion endpoint)
//! - Honeycomb OTLP endpoint
//! - Datadog Agent OTLP ingest
//! - New Relic OTLP ingest

use std::fmt::Write;

use crate::telemetry::metrics::get_metrics_registry;
use crate::telemetry::span::{AttributeValue, Span, SpanStatusCode};
use crate::telemetry::tracer::{get_completed_spans, get_service_name};

/// Escape a string for valid JSON output.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if c.is_control() => {
                let _ = write!(&mut out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Convert an [`AttributeValue`] to an OTLP JSON value object:
/// `{ "stringValue": ... }`, `{ "boolValue": ... }`, `{ "intValue": ... }`, or `{ "doubleValue": ... }`
fn format_otlp_attribute_value(val: &AttributeValue) -> String {
    match val {
        AttributeValue::String(s) => format!("{{\"stringValue\":\"{}\"}}", escape_json(s)),
        AttributeValue::Bool(b) => format!("{{\"boolValue\":{}}}", b),
        AttributeValue::Int(i) => format!("{{\"intValue\":\"{}\"}}", i),
        AttributeValue::Float(f) => format!("{{\"doubleValue\":{}}}", f),
        AttributeValue::Array(arr) => {
            let values: Vec<String> = arr.iter().map(format_otlp_attribute_value).collect();
            format!("{{\"arrayValue\":{{\"values\":[{}]}}}}", values.join(","))
        }
    }
}

/// Format a single span into OTLP Span JSON.
fn format_otlp_span(span: &Span) -> String {
    let mut attrs_json = Vec::new();
    for (k, v) in &span.attributes {
        attrs_json.push(format!(
            "{{\"key\":\"{}\",\"value\":{}}}",
            escape_json(k),
            format_otlp_attribute_value(v)
        ));
    }

    let mut events_json = Vec::new();
    for ev in &span.events {
        let mut ev_attrs = Vec::new();
        for (k, v) in &ev.attributes {
            ev_attrs.push(format!(
                "{{\"key\":\"{}\",\"value\":{}}}",
                escape_json(k),
                format_otlp_attribute_value(v)
            ));
        }
        events_json.push(format!(
            "{{\"timeUnixNano\":\"{}\",\"name\":\"{}\",\"attributes\":[{}]}}",
            ev.timestamp_unix_nano,
            escape_json(&ev.name),
            ev_attrs.join(",")
        ));
    }

    let parent_str = span
        .parent_span_id
        .map(|p| format!(",\"parentSpanId\":\"{}\"", p.to_hex()))
        .unwrap_or_default();

    let end_time = span
        .end_time_unix_nano
        .unwrap_or(span.start_time_unix_nano);

    let status_code = match span.status.code {
        SpanStatusCode::Unset => 0,
        SpanStatusCode::Ok => 1,
        SpanStatusCode::Error => 2,
    };

    let status_desc = if span.status.description.is_empty() {
        String::new()
    } else {
        format!(",\"message\":\"{}\"", escape_json(&span.status.description))
    };

    format!(
        "{{\"traceId\":\"{}\",\"spanId\":\"{}\"{},\"name\":\"{}\",\"kind\":{},\"startTimeUnixNano\":\"{}\",\"endTimeUnixNano\":\"{}\",\"attributes\":[{}],\"events\":[{}],\"status\":{{\"code\":{}{}}}}}",
        span.context.trace_id.to_hex(),
        span.context.span_id.to_hex(),
        parent_str,
        escape_json(&span.name),
        span.kind.otlp_value(),
        span.start_time_unix_nano,
        end_time,
        attrs_json.join(","),
        events_json.join(","),
        status_code,
        status_desc
    )
}

/// Export spans in canonical OTLP/HTTP JSON format (`/v1/traces`).
pub fn export_otlp_traces(spans: &[Span]) -> String {
    let service_name = get_service_name();
    let spans_json: Vec<String> = spans.iter().map(format_otlp_span).collect();

    format!(
        "{{\"resourceSpans\":[{{\"resource\":{{\"attributes\":[{{\"key\":\"service.name\",\"value\":{{\"stringValue\":\"{}\"}}}},{{\"key\":\"telemetry.sdk.language\",\"value\":{{\"stringValue\":\"rhp\"}}}},{{\"key\":\"telemetry.sdk.name\",\"value\":{{\"stringValue\":\"rhp-opentelemetry\"}}}},{{\"key\":\"telemetry.sdk.version\",\"value\":{{\"stringValue\":\"0.1.0\"}}}}]}},\"scopeSpans\":[{{\"scope\":{{\"name\":\"rhp.engine\",\"version\":\"0.1.0\"}},\"spans\":[{}]}}]}}]}}",
        escape_json(&service_name),
        spans_json.join(",")
    )
}

/// Export all currently completed spans from the global buffer into OTLP JSON.
pub fn export_active_otlp_traces() -> String {
    let spans = get_completed_spans();
    export_otlp_traces(&spans)
}

/// Export recorded metrics in canonical OTLP/HTTP JSON format (`/v1/metrics`).
pub fn export_otlp_metrics() -> String {
    let service_name = get_service_name();
    let reg_guard = get_metrics_registry().lock().ok();

    let mut metrics_json = Vec::new();

    if let Some(reg) = reg_guard {
        // 1. Export Histograms
        let php_metrics = reg.to_php_value();
        if let crate::vm::value::PhpValue::Dict(d) = php_metrics {
            if let Some(crate::vm::value::PhpValue::Dict(hist_dict)) = d.borrow().get("histograms")
            {
                for (name, h_val) in hist_dict.borrow().iter() {
                    if let crate::vm::value::PhpValue::Dict(hd) = h_val {
                        let count = hd.borrow().get("count").and_then(|v| v.to_i32()).unwrap_or(0);
                        let sum = hd.borrow().get("sum").and_then(|v| v.to_f32()).unwrap_or(0.0);
                        metrics_json.push(format!(
                            "{{\"name\":\"{}\",\"histogram\":{{\"dataPoints\":[{{\"count\":\"{}\",\"sum\":{}}}]}}}}",
                            escape_json(name),
                            count,
                            sum
                        ));
                    }
                }
            }
            if let Some(crate::vm::value::PhpValue::Dict(counter_dict)) =
                d.borrow().get("counters")
            {
                for (name, c_val) in counter_dict.borrow().iter() {
                    let val = c_val.to_f32().unwrap_or(0.0);
                    metrics_json.push(format!(
                        "{{\"name\":\"{}\",\"sum\":{{\"dataPoints\":[{{\"asDouble\":{}}}]}}}}",
                        escape_json(name),
                        val
                    ));
                }
            }
        }
    }

    format!(
        "{{\"resourceMetrics\":[{{\"resource\":{{\"attributes\":[{{\"key\":\"service.name\",\"value\":{{\"stringValue\":\"{}\"}}}}]}},\"scopeMetrics\":[{{\"scope\":{{\"name\":\"rhp.engine\",\"version\":\"0.1.0\"}},\"metrics\":[{}]}}]}}]}}",
        escape_json(&service_name),
        metrics_json.join(",")
    )
}
