//! Tier 1 Native Static Facade: `Trace` (and `Telemetry`)
//!
//! Exposes OpenTelemetry distributed tracing, metrics, context propagation,
//! and APM export directly to RHP scripts:
//! - `Trace::startSpan($name, $options)`
//! - `Trace::endSpan()`
//! - `Trace::currentSpan()`
//! - `Trace::traceId()`, `Trace::spanId()`
//! - `Trace::setAttribute($key, $val)`, `Trace::setAttributes($attrs)`
//! - `Trace::addEvent($name, $attrs)`
//! - `Trace::recordException($exc)`
//! - `Trace::injectContext($carrier)`, `Trace::extractContext($carrier)`
//! - `Trace::counter($name, $delta)`, `Trace::histogram($name, $val)`, `Trace::gauge($name, $val)`
//! - `Trace::metrics()`, `Trace::metricSummary($name)`
//! - `Trace::exportOtlp()`, `Trace::exportSentry()`, `Trace::exportDatadog()`

use indexmap::IndexMap;

use crate::telemetry::bridges::{
    capture_exception, clear_captured_crashes, export_datadog_traces, get_captured_crashes,
};
use crate::telemetry::metrics::{
    clear_metrics, counter, gauge, get_all_metrics, get_histogram_summary, histogram,
};
use crate::telemetry::otlp::{export_active_otlp_traces, export_otlp_metrics};
use crate::telemetry::span::{AttributeValue, SpanEvent, SpanKind, SpanStatus, SpanStatusCode};
use crate::telemetry::trace_context::{extract_trace_context, inject_trace_context};
use crate::telemetry::tracer::{
    add_active_span_event, clear_completed_spans, current_parent_span_id, current_span,
    current_span_id, current_trace_id, end_active_span, get_active_baggage, get_completed_spans,
    set_active_span_attribute, set_active_span_attributes, set_baggage_item, set_sample_rate,
    set_service_name, start_span,
};
use crate::vm::error::RuntimeErrorKind;
use crate::vm::facade::FacadeResult;
use crate::vm::value::PhpValue;

/// Dispatch calls on the `Trace` and `Telemetry` facades.
pub fn dispatch_trace(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "startSpan" => {
            let name = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "unnamed_span".to_string());

            let mut kind = SpanKind::Internal;
            let mut attrs = IndexMap::new();

            if let Some(opts) = args.get(1) {
                if let PhpValue::Dict(d) = opts {
                    let b = d.borrow();
                    if let Some(k_val) = b.get("kind") {
                        if let Some(parsed) = SpanKind::parse(&k_val.to_php_string()) {
                            kind = parsed;
                        }
                    }
                    if let Some(PhpValue::Dict(attr_dict)) = b.get("attributes") {
                        for (k, v) in attr_dict.borrow().iter() {
                            attrs.insert(k.clone(), AttributeValue::from_php_value(v));
                        }
                    }
                }
            }

            let ctx = start_span(&name, kind, None, attrs);
            let mut map = IndexMap::new();
            map.insert("trace_id".to_string(), PhpValue::string(ctx.trace_id.to_hex()));
            map.insert("span_id".to_string(), PhpValue::string(ctx.span_id.to_hex()));
            map.insert("name".to_string(), PhpValue::string(name));
            Ok(PhpValue::dict_from_map(map))
        }

        "endSpan" => {
            let status = if let Some(PhpValue::Dict(d)) = args.first() {
                let b = d.borrow();
                let code_str = b
                    .get("status")
                    .map(|v| v.to_php_string())
                    .unwrap_or_default();
                let desc = b
                    .get("description")
                    .map(|v| v.to_php_string())
                    .unwrap_or_default();
                match code_str.to_ascii_uppercase().as_str() {
                    "OK" => Some(SpanStatus::ok()),
                    "ERROR" => Some(SpanStatus::error(desc)),
                    _ => None,
                }
            } else {
                None
            };

            let finished = end_active_span(status);
            Ok(finished.map(|s| s.to_php_value()).unwrap_or(PhpValue::Null))
        }

        "currentSpan" => Ok(current_span()
            .map(|s| s.to_php_value())
            .unwrap_or(PhpValue::Null)),

        "traceId" => Ok(current_trace_id()
            .map(|t| PhpValue::string(t.to_hex()))
            .unwrap_or(PhpValue::Null)),

        "spanId" => Ok(current_span_id()
            .map(|s| PhpValue::string(s.to_hex()))
            .unwrap_or(PhpValue::Null)),

        "parentSpanId" => Ok(current_parent_span_id()
            .map(|s| PhpValue::string(s.to_hex()))
            .unwrap_or(PhpValue::Null)),

        "setAttribute" => {
            let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let val = args.get(1).map(AttributeValue::from_php_value).unwrap_or(AttributeValue::String(String::new()));
            set_active_span_attribute(key, val);
            Ok(PhpValue::Null)
        }

        "setAttributes" => {
            if let Some(PhpValue::Dict(d)) = args.first() {
                let mut map = IndexMap::new();
                for (k, v) in d.borrow().iter() {
                    map.insert(k.clone(), AttributeValue::from_php_value(v));
                }
                set_active_span_attributes(map);
            }
            Ok(PhpValue::Null)
        }

        "addEvent" => {
            let name = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let mut event = SpanEvent::new(name);
            if let Some(PhpValue::Dict(d)) = args.get(1) {
                for (k, v) in d.borrow().iter() {
                    event.attributes.insert(k.clone(), AttributeValue::from_php_value(v));
                }
            }
            add_active_span_event(event);
            Ok(PhpValue::Null)
        }

        "recordException" => {
            let exc_msg = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "Exception".to_string());
            let st = args.get(1).map(|v| v.to_php_string());
            crate::telemetry::tracer::with_active_span(|span| {
                span.record_exception("Exception", &exc_msg, st.as_deref());
            });
            capture_exception("Exception", &exc_msg, &[], None);
            Ok(PhpValue::Null)
        }

        "setStatus" => {
            let status_str = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let desc = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let code = match status_str.to_ascii_uppercase().as_str() {
                "OK" => SpanStatusCode::Ok,
                "ERROR" => SpanStatusCode::Error,
                _ => SpanStatusCode::Unset,
            };
            crate::telemetry::tracer::with_active_span(|span| {
                span.set_status(SpanStatus {
                    code,
                    description: desc,
                });
            });
            Ok(PhpValue::Null)
        }

        "injectContext" => {
            let mut headers = IndexMap::new();
            if let Some(PhpValue::Dict(d)) = args.first() {
                for (k, v) in d.borrow().iter() {
                    headers.insert(k.clone(), v.to_php_string());
                }
            }
            if let Some(span) = current_span() {
                let baggage = get_active_baggage();
                inject_trace_context(&mut headers, &span.context, Some(&baggage));
            }
            let mut map = IndexMap::new();
            for (k, v) in headers {
                map.insert(k, PhpValue::string(v));
            }
            Ok(PhpValue::dict_from_map(map))
        }

        "extractContext" => {
            let mut headers = IndexMap::new();
            if let Some(PhpValue::Dict(d)) = args.first() {
                for (k, v) in d.borrow().iter() {
                    headers.insert(k.clone(), v.to_php_string());
                }
            }
            let (span_ctx, baggage) = extract_trace_context(&headers);
            let mut map = IndexMap::new();
            if let Some(ctx) = span_ctx {
                map.insert("trace_id".to_string(), PhpValue::string(ctx.trace_id.to_hex()));
                map.insert("span_id".to_string(), PhpValue::string(ctx.span_id.to_hex()));
                map.insert("is_sampled".to_string(), PhpValue::Bool(ctx.trace_flags.is_sampled()));
            }
            if let Some(b) = baggage {
                map.insert("baggage".to_string(), b.to_php_value());
            }
            Ok(PhpValue::dict_from_map(map))
        }

        "spans" => {
            let list = get_completed_spans();
            let php_list: Vec<PhpValue> = list.iter().map(|s| s.to_php_value()).collect();
            Ok(PhpValue::list(php_list))
        }

        "clearSpans" => {
            clear_completed_spans();
            Ok(PhpValue::Null)
        }

        "counter" => {
            let name = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let val = args.get(1).and_then(|v| v.to_f32()).unwrap_or(1.0) as f64;
            counter(&name, val);
            Ok(PhpValue::Null)
        }

        "gauge" => {
            let name = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let val = args.get(1).and_then(|v| v.to_f32()).unwrap_or(0.0) as f64;
            gauge(&name, val);
            Ok(PhpValue::Null)
        }

        "histogram" => {
            let name = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let val = args.get(1).and_then(|v| v.to_f32()).unwrap_or(0.0) as f64;
            histogram(&name, val);
            Ok(PhpValue::Null)
        }

        "metrics" => Ok(get_all_metrics()),

        "metricSummary" => {
            let name = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            Ok(get_histogram_summary(&name)
                .map(|s| s.to_php_value())
                .unwrap_or(PhpValue::Null))
        }

        "clearMetrics" => {
            clear_metrics();
            Ok(PhpValue::Null)
        }

        "exportOtlp" => Ok(PhpValue::string(export_active_otlp_traces())),

        "exportOtlpMetrics" => Ok(PhpValue::string(export_otlp_metrics())),

        "exportSentry" => {
            let crashes = get_captured_crashes();
            if let Some(c) = crashes.last() {
                Ok(PhpValue::string(c.to_sentry_json()))
            } else {
                let report = capture_exception("Error", "Manual Sentry report", &[], None);
                Ok(PhpValue::string(report.to_sentry_json()))
            }
        }

        "exportDatadog" => {
            let spans = get_completed_spans();
            Ok(PhpValue::string(export_datadog_traces(&spans)))
        }

        "baggage" => {
            if let Some(key) = args.first() {
                let k_str = key.to_php_string();
                if let Some(val) = args.get(1) {
                    set_baggage_item(&k_str, val.to_php_string());
                    Ok(PhpValue::Null)
                } else {
                    let b = get_active_baggage();
                    Ok(b.get(&k_str)
                        .map(|v| PhpValue::string(v.to_string()))
                        .unwrap_or(PhpValue::Null))
                }
            } else {
                Ok(get_active_baggage().to_php_value())
            }
        }

        "configure" => {
            if let Some(PhpValue::Dict(d)) = args.first() {
                let b = d.borrow();
                if let Some(s) = b.get("service_name") {
                    set_service_name(s.to_php_string());
                }
                if let Some(r) = b.get("sample_rate").and_then(|v| v.to_f32()) {
                    set_sample_rate(r as f64);
                }
            }
            Ok(PhpValue::Null)
        }

        "crashes" => {
            let list = get_captured_crashes();
            let php_list: Vec<PhpValue> = list.iter().map(|c| c.to_php_value()).collect();
            Ok(PhpValue::list(php_list))
        }

        "clearCrashes" => {
            clear_captured_crashes();
            Ok(PhpValue::Null)
        }

        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Trace".to_string(),
            method: method.to_string(),
        }),
    }
}
