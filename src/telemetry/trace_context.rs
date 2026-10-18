//! W3C Trace Context & Baggage Distributed Context Propagation
//!
//! Implements W3C Trace Context Level 1 and W3C Baggage specifications:
//! - `traceparent`: `00-{trace_id_32_hex}-{parent_id_16_hex}-{trace_flags_2_hex}`
//! - `tracestate`: vendor-specific comma-delimited key-value list
//! - `baggage`: comma-separated key=value entries propagated across microservices

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;

use crate::vm::value::PhpValue;

static ID_STATE: AtomicU64 = AtomicU64::new(0x8844a7139c2e0b51);

/// Thread-safe pseudo-random 64-bit generator with system time salt.
pub fn next_random_u64() -> u64 {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = ID_STATE.load(Ordering::Relaxed) ^ now_nanos;
    loop {
        // xorshift64*
        let mut x = state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let next = x.wrapping_mul(0x2545F4914F6CDD1D);
        match ID_STATE.compare_exchange_weak(state, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return if next == 0 { 0x517cc1b727220a95 } else { next },
            Err(curr) => state = curr,
        }
    }
}

/// 128-bit OpenTelemetry Trace Identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraceId(pub [u8; 16]);

impl TraceId {
    /// Generate a new globally unique, non-zero 128-bit [`TraceId`].
    pub fn generate() -> Self {
        let hi = next_random_u64();
        let lo = next_random_u64();
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&hi.to_be_bytes());
        bytes[8..16].copy_from_slice(&lo.to_be_bytes());
        if bytes.iter().all(|&b| b == 0) {
            bytes[15] = 1;
        }
        Self(bytes)
    }

    /// Construct from raw 16 bytes.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Parse a 32-character hexadecimal string into a [`TraceId`].
    pub fn from_hex(hex_str: &str) -> Option<Self> {
        let s = hex_str.trim();
        if s.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            let chunk = &s[i * 2..i * 2 + 2];
            bytes[i] = u8::from_str_radix(chunk, 16).ok()?;
        }
        if bytes.iter().all(|&b| b == 0) {
            // W3C Trace Context explicitly forbids all-zero Trace ID
            return None;
        }
        Some(Self(bytes))
    }

    /// Format the [`TraceId`] as a 32-character lowercase hexadecimal string.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(32);
        for byte in &self.0 {
            use std::fmt::Write;
            let _ = write!(&mut s, "{:02x}", byte);
        }
        s
    }

    /// Check whether this Trace ID is valid (non-zero).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.0.iter().any(|&b| b != 0)
    }

    /// Raw byte representation.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraceId({})", self.to_hex())
    }
}

/// 64-bit OpenTelemetry Span Identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpanId(pub [u8; 8]);

impl SpanId {
    /// Generate a new non-zero 64-bit [`SpanId`].
    pub fn generate() -> Self {
        let r = next_random_u64();
        let bytes = r.to_be_bytes();
        let mut res = Self(bytes);
        if res.0.iter().all(|&b| b == 0) {
            res.0[7] = 1;
        }
        res
    }

    /// Construct from raw 8 bytes.
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    /// Parse a 16-character hexadecimal string into a [`SpanId`].
    pub fn from_hex(hex_str: &str) -> Option<Self> {
        let s = hex_str.trim();
        if s.len() != 16 {
            return None;
        }
        let mut bytes = [0u8; 8];
        for i in 0..8 {
            let chunk = &s[i * 2..i * 2 + 2];
            bytes[i] = u8::from_str_radix(chunk, 16).ok()?;
        }
        if bytes.iter().all(|&b| b == 0) {
            // W3C Trace Context explicitly forbids all-zero Span ID
            return None;
        }
        Some(Self(bytes))
    }

    /// Format the [`SpanId`] as a 16-character lowercase hexadecimal string.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(16);
        for byte in &self.0 {
            use std::fmt::Write;
            let _ = write!(&mut s, "{:02x}", byte);
        }
        s
    }

    /// Check whether this Span ID is valid (non-zero).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.0.iter().any(|&b| b != 0)
    }

    /// Raw byte representation.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SpanId({})", self.to_hex())
    }
}

/// OpenTelemetry 8-bit trace flags.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraceFlags(pub u8);

impl TraceFlags {
    /// Flag indicating that the span is sampled (recorded and exported).
    pub const SAMPLED: Self = Self(0x01);
    /// Flag indicating no options set.
    pub const NONE: Self = Self(0x00);

    /// Whether the sampled bit (0x01) is active.
    #[inline]
    pub fn is_sampled(&self) -> bool {
        (self.0 & 0x01) != 0
    }

    /// Format as 2-character hexadecimal string.
    pub fn to_hex(&self) -> String {
        format!("{:02x}", self.0)
    }

    /// Parse from 2-character hexadecimal string.
    pub fn from_hex(hex_str: &str) -> Option<Self> {
        let s = hex_str.trim();
        if s.len() != 2 {
            return None;
        }
        u8::from_str_radix(s, 16).ok().map(Self)
    }
}

impl Default for TraceFlags {
    fn default() -> Self {
        Self::SAMPLED
    }
}

impl fmt::Debug for TraceFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraceFlags({:02x})", self.0)
    }
}

/// Vendor-specific tracestate list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TraceState {
    entries: Vec<(String, String)>,
}

impl TraceState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Parse a W3C `tracestate` header value.
    pub fn parse(header: &str) -> Self {
        let mut entries = Vec::new();
        for item in header.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some((k, v)) = item.split_once('=') {
                entries.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        Self { entries }
    }

    /// Serialize to W3C `tracestate` header format.
    pub fn header_value(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Retrieve an entry by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Insert or update an entry.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let k = key.into();
        let v = value.into();
        if let Some(pos) = self.entries.iter().position(|(ek, _)| ek == &k) {
            self.entries[pos].1 = v;
        } else {
            self.entries.push((k, v));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// W3C Baggage key-value context propagated across microservices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Baggage {
    entries: IndexMap<String, String>,
}

impl Baggage {
    pub fn new() -> Self {
        Self {
            entries: IndexMap::new(),
        }
    }

    /// Parse a W3C `baggage` header.
    pub fn parse(header: &str) -> Self {
        let mut entries = IndexMap::new();
        for item in header.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            // Strip any properties (;prop=val)
            let pair = item.split(';').next().unwrap_or(item).trim();
            if let Some((k, v)) = pair.split_once('=') {
                let key = k.trim().to_string();
                let val = v.trim().to_string();
                if !key.is_empty() {
                    entries.insert(key, val);
                }
            }
        }
        Self { entries }
    }

    /// Format to W3C `baggage` header.
    pub fn header_value(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|s| s.as_str())
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.shift_remove(key)
    }

    pub fn entries(&self) -> &IndexMap<String, String> {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Convert to script-land dictionary (`Arr::dict`).
    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        for (k, v) in &self.entries {
            map.insert(k.clone(), PhpValue::string(v.clone()));
        }
        PhpValue::dict_from_map(map)
    }
}

/// Immutable snapshot of a span's identity within a distributed trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanContext {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub trace_flags: TraceFlags,
    pub trace_state: TraceState,
    pub is_remote: bool,
}

impl SpanContext {
    /// Create a new local [`SpanContext`].
    pub fn new(trace_id: TraceId, span_id: SpanId, trace_flags: TraceFlags) -> Self {
        Self {
            trace_id,
            span_id,
            trace_flags,
            trace_state: TraceState::new(),
            is_remote: false,
        }
    }

    /// Parse a W3C `traceparent` header string:
    /// `00-{trace_id}-{parent_id}-{trace_flags}`
    pub fn parse_traceparent(header: &str) -> Option<Self> {
        let s = header.trim();
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = parts[0];
        let trace_id_str = parts[1];
        let parent_id_str = parts[2];
        let flags_str = parts[3];

        // Level 1 supports version "00"
        if version != "00" && version.len() != 2 {
            return None;
        }

        let trace_id = TraceId::from_hex(trace_id_str)?;
        let span_id = SpanId::from_hex(parent_id_str)?;
        let trace_flags = TraceFlags::from_hex(flags_str)?;

        Some(Self {
            trace_id,
            span_id,
            trace_flags,
            trace_state: TraceState::new(),
            is_remote: true,
        })
    }

    /// Serialize into canonical W3C `traceparent` header value.
    pub fn format_traceparent(&self) -> String {
        format!(
            "00-{}-{}-{}",
            self.trace_id.to_hex(),
            self.span_id.to_hex(),
            self.trace_flags.to_hex()
        )
    }

    /// Check if this context is valid.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.trace_id.is_valid() && self.span_id.is_valid()
    }
}

/// Extract W3C distributed trace context and baggage from an HTTP headers carrier.
///
/// Looks up case-insensitive header names: `traceparent`, `tracestate`, `baggage`.
pub fn extract_trace_context(
    headers: &IndexMap<String, String>,
) -> (Option<SpanContext>, Option<Baggage>) {
    let mut traceparent_val: Option<&str> = None;
    let mut tracestate_val: Option<&str> = None;
    let mut baggage_val: Option<&str> = None;

    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if lower == "traceparent" || lower == "http_traceparent" {
            traceparent_val = Some(v.as_str());
        } else if lower == "tracestate" || lower == "http_tracestate" {
            tracestate_val = Some(v.as_str());
        } else if lower == "baggage" || lower == "http_baggage" {
            baggage_val = Some(v.as_str());
        }
    }

    let span_ctx = traceparent_val.and_then(SpanContext::parse_traceparent).map(|mut ctx| {
        if let Some(ts) = tracestate_val {
            ctx.trace_state = TraceState::parse(ts);
        }
        ctx
    });

    let baggage = baggage_val.map(Baggage::parse);

    (span_ctx, baggage)
}

/// Inject W3C distributed trace context and baggage into an outbound HTTP headers carrier.
pub fn inject_trace_context(
    headers: &mut IndexMap<String, String>,
    ctx: &SpanContext,
    baggage: Option<&Baggage>,
) {
    headers.insert("traceparent".to_string(), ctx.format_traceparent());
    if !ctx.trace_state.is_empty() {
        headers.insert("tracestate".to_string(), ctx.trace_state.header_value());
    }
    if let Some(b) = baggage {
        if !b.is_empty() {
            headers.insert("baggage".to_string(), b.header_value());
        }
    }
}
