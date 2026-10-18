//! OpenTelemetry Span Representation, Events, Links, and Status
//!
//! Follows OpenTelemetry Tracing API & SDK Specification v1.x:
//! - Spans with start/end Unix timestamps in nanoseconds
//! - Rich typed attributes (string, bool, int, float, array)
//! - Span events (with timestamps and attributes) including exception recording
//! - Span status (Unset, Ok, Error)
//! - Span kinds (Internal, Server, Client, Producer, Consumer)

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;

use crate::telemetry::trace_context::{SpanContext, SpanId, TraceFlags, TraceId};
use crate::vm::value::PhpValue;

/// Current wall-clock Unix timestamp in nanoseconds.
pub fn now_unix_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Span kind describing the relationship between the span, its parents, and its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanKind {
    /// Internal operation within the application.
    #[default]
    Internal = 1,
    /// Synchronous or asynchronous request handler (e.g. incoming HTTP request).
    Server = 2,
    /// Outgoing synchronous request (e.g. outgoing HTTP client call, database query).
    Client = 3,
    /// Producer of an asynchronous message/task (e.g. queue push).
    Producer = 4,
    /// Consumer of an asynchronous message/task (e.g. queue worker job).
    Consumer = 5,
}

impl SpanKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Internal => "INTERNAL",
            Self::Server => "SERVER",
            Self::Client => "CLIENT",
            Self::Producer => "PRODUCER",
            Self::Consumer => "CONSUMER",
        }
    }

    pub fn otlp_value(&self) -> u32 {
        *self as u32
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "INTERNAL" => Some(Self::Internal),
            "SERVER" => Some(Self::Server),
            "CLIENT" => Some(Self::Client),
            "PRODUCER" => Some(Self::Producer),
            "CONSUMER" => Some(Self::Consumer),
            _ => None,
        }
    }
}

/// Canonical status code of a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanStatusCode {
    /// Default status, meaning the operation has not set a status.
    #[default]
    Unset = 0,
    /// The operation completed successfully.
    Ok = 1,
    /// The operation contains an error.
    Error = 2,
}

impl SpanStatusCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unset => "UNSET",
            Self::Ok => "OK",
            Self::Error => "ERROR",
        }
    }

    pub fn otlp_value(&self) -> u32 {
        *self as u32
    }
}

/// Status of a span, containing the status code and an optional error description.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpanStatus {
    pub code: SpanStatusCode,
    pub description: String,
}

impl SpanStatus {
    pub fn unset() -> Self {
        Self {
            code: SpanStatusCode::Unset,
            description: String::new(),
        }
    }

    pub fn ok() -> Self {
        Self {
            code: SpanStatusCode::Ok,
            description: String::new(),
        }
    }

    pub fn error(description: impl Into<String>) -> Self {
        Self {
            code: SpanStatusCode::Error,
            description: description.into(),
        }
    }
}

/// OpenTelemetry attribute value.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    String(String),
    Bool(bool),
    Int(i64),
    Float(f64),
    Array(Vec<AttributeValue>),
}

impl AttributeValue {
    pub fn from_php_value(val: &PhpValue) -> Self {
        match val {
            PhpValue::String(s) => Self::String(s.to_string()),
            PhpValue::Bool(b) => Self::Bool(*b),
            PhpValue::Int(i) => Self::Int(*i as i64),
            PhpValue::Float(f) => Self::Float(*f as f64),
            PhpValue::RustInt { value, .. } => Self::Int(*value as i64),
            PhpValue::RustFloat { value, .. } => Self::Float(*value),
            PhpValue::List(l) => {
                let items: Vec<AttributeValue> =
                    l.borrow().iter().map(Self::from_php_value).collect();
                Self::Array(items)
            }
            PhpValue::Array(a) => {
                let items: Vec<AttributeValue> =
                    a.borrow().iter().map(Self::from_php_value).collect();
                Self::Array(items)
            }
            other => Self::String(other.to_php_string()),
        }
    }

    pub fn to_php_value(&self) -> PhpValue {
        match self {
            Self::String(s) => PhpValue::string(s.clone()),
            Self::Bool(b) => PhpValue::Bool(*b),
            Self::Int(i) => PhpValue::Int(*i as i32),
            Self::Float(f) => PhpValue::Float(*f as f32),
            Self::Array(arr) => {
                let list: Vec<PhpValue> = arr.iter().map(|item| item.to_php_value()).collect();
                PhpValue::list(list)
            }
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Array(arr) => format!(
                "[{}]",
                arr.iter()
                    .map(|a| a.as_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// An annotated event that occurred within the timeline of a span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_unix_nano: u64,
    pub attributes: IndexMap<String, AttributeValue>,
}

impl SpanEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            timestamp_unix_nano: now_unix_nanos(),
            attributes: IndexMap::new(),
        }
    }

    pub fn with_attribute(
        mut self,
        key: impl Into<String>,
        val: impl Into<AttributeValue>,
    ) -> Self {
        self.attributes.insert(key.into(), val.into());
        self
    }

    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert("name".to_string(), PhpValue::string(self.name.clone()));
        map.insert(
            "timestamp_unix_nano".to_string(),
            PhpValue::Int((self.timestamp_unix_nano / 1_000_000) as i32),
        );
        let mut attrs_map = IndexMap::new();
        for (k, v) in &self.attributes {
            attrs_map.insert(k.clone(), v.to_php_value());
        }
        map.insert(
            "attributes".to_string(),
            PhpValue::dict_from_map(attrs_map),
        );
        PhpValue::dict_from_map(map)
    }
}

/// A cross-trace link pointing to another span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanLink {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub attributes: IndexMap<String, AttributeValue>,
}

/// A contiguous segment of distributed execution.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub context: SpanContext,
    pub parent_span_id: Option<SpanId>,
    pub name: String,
    pub kind: SpanKind,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: Option<u64>,
    pub attributes: IndexMap<String, AttributeValue>,
    pub events: Vec<SpanEvent>,
    pub links: Vec<SpanLink>,
    pub status: SpanStatus,
}

impl Span {
    /// Create a new active span with current timestamp.
    pub fn new(
        name: impl Into<String>,
        kind: SpanKind,
        trace_id: TraceId,
        parent_span_id: Option<SpanId>,
    ) -> Self {
        let span_id = SpanId::generate();
        let context = SpanContext::new(trace_id, span_id, TraceFlags::SAMPLED);
        Self {
            context,
            parent_span_id,
            name: name.into(),
            kind,
            start_time_unix_nano: now_unix_nanos(),
            end_time_unix_nano: None,
            attributes: IndexMap::new(),
            events: Vec::new(),
            links: Vec::new(),
            status: SpanStatus::unset(),
        }
    }

    /// Retrieve the span's unique [`SpanId`].
    #[inline]
    pub fn span_id(&self) -> SpanId {
        self.context.span_id
    }

    /// Retrieve the span's [`TraceId`].
    #[inline]
    pub fn trace_id(&self) -> TraceId {
        self.context.trace_id
    }

    /// Attach a key-value attribute.
    pub fn set_attribute(&mut self, key: impl Into<String>, val: AttributeValue) {
        self.attributes.insert(key.into(), val);
    }

    /// Bulk-attach key-value attributes.
    pub fn set_attributes(&mut self, attrs: IndexMap<String, AttributeValue>) {
        for (k, v) in attrs {
            self.attributes.insert(k, v);
        }
    }

    /// Add an annotated event to the span.
    pub fn add_event(&mut self, event: SpanEvent) {
        self.events.push(event);
    }

    /// Record an exception event following OpenTelemetry semantic conventions:
    /// - `exception.type`
    /// - `exception.message`
    /// - `exception.stacktrace`
    pub fn record_exception(
        &mut self,
        exc_type: &str,
        message: &str,
        stacktrace: Option<&str>,
    ) {
        let mut event = SpanEvent::new("exception")
            .with_attribute("exception.type", AttributeValue::String(exc_type.to_string()))
            .with_attribute(
                "exception.message",
                AttributeValue::String(message.to_string()),
            );
        if let Some(st) = stacktrace {
            event = event.with_attribute(
                "exception.stacktrace",
                AttributeValue::String(st.to_string()),
            );
        }
        self.events.push(event);
        self.status = SpanStatus::error(message);
    }

    /// Set span status.
    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    /// Finish this span with current timestamp.
    pub fn finish(&mut self) {
        if self.end_time_unix_nano.is_none() {
            self.end_time_unix_nano = Some(now_unix_nanos());
        }
    }

    /// Check if the span is finished.
    pub fn is_finished(&self) -> bool {
        self.end_time_unix_nano.is_some()
    }

    /// Calculate elapsed duration of the span in nanoseconds.
    pub fn duration_nanos(&self) -> u64 {
        let end = self.end_time_unix_nano.unwrap_or_else(now_unix_nanos);
        end.saturating_sub(self.start_time_unix_nano)
    }

    /// Convert duration to a standard [`Duration`].
    pub fn duration(&self) -> Duration {
        Duration::from_nanos(self.duration_nanos())
    }

    /// Convert this span to a script-land dictionary (`Arr::dict`).
    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert(
            "trace_id".to_string(),
            PhpValue::string(self.context.trace_id.to_hex()),
        );
        map.insert(
            "span_id".to_string(),
            PhpValue::string(self.context.span_id.to_hex()),
        );
        map.insert(
            "parent_span_id".to_string(),
            self.parent_span_id
                .map(|p| PhpValue::string(p.to_hex()))
                .unwrap_or(PhpValue::Null),
        );
        map.insert("name".to_string(), PhpValue::string(self.name.clone()));
        map.insert(
            "kind".to_string(),
            PhpValue::string(self.kind.as_str().to_string()),
        );
        map.insert(
            "status".to_string(),
            PhpValue::string(self.status.code.as_str().to_string()),
        );
        if !self.status.description.is_empty() {
            map.insert(
                "status_description".to_string(),
                PhpValue::string(self.status.description.clone()),
            );
        }
        map.insert(
            "start_time_unix_nano".to_string(),
            PhpValue::Int((self.start_time_unix_nano / 1_000_000) as i32),
        );
        if let Some(end) = self.end_time_unix_nano {
            map.insert(
                "end_time_unix_nano".to_string(),
                PhpValue::Int((end / 1_000_000) as i32),
            );
        }
        map.insert(
            "duration_ms".to_string(),
            PhpValue::Float((self.duration_nanos() as f32) / 1_000_000.0),
        );

        let mut attrs_map = IndexMap::new();
        for (k, v) in &self.attributes {
            attrs_map.insert(k.clone(), v.to_php_value());
        }
        map.insert(
            "attributes".to_string(),
            PhpValue::dict_from_map(attrs_map),
        );

        let events_list: Vec<PhpValue> = self.events.iter().map(|e| e.to_php_value()).collect();
        map.insert("events".to_string(), PhpValue::list(events_list));

        PhpValue::dict_from_map(map)
    }
}
