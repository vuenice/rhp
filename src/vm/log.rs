//! # Centralized Structured Logging Subsystem
//!
//! Implements a trait-backed logging architecture for RHP:
//! - Standard call-site API (`Log::info()`, `Log::channel()`, `Log::isEnabled()`)
//! - Trait-backed swappable handlers (`LogHandler`)
//! - Automatic zero-cost runtime context enrichment: Fiber ID, Request ID, Trace/Span ID
//! - Lazy level evaluation before record/context allocation
//! - Production-ready zero-config stdout handler (Pretty in Dev, JSON in Prod)
//! - Test isolation via `Log::fake()` / `TestLogHandler` (zero unbounded memory growth in prod)

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use indexmap::IndexMap;

use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::parent_process::FrozenValue;
use crate::vm::value::{PhpValue, VmObject};

/// Log severity levels following the standard syslog / PSR-3 / RFC 5424 hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum LogLevel {
    #[default]
    Debug = 0,
    Info = 1,
    Notice = 2,
    Warn = 3,
    Error = 4,
    Critical = 5,
    Alert = 6,
    Emergency = 7,
}

impl LogLevel {
    /// Return the canonical lowercase string representation of the level.
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warn => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
            Self::Alert => "alert",
            Self::Emergency => "emergency",
        }
    }

    /// Return the uppercase string representation of the level for formatted output.
    #[inline]
    pub fn as_uppercase_str(&self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Notice => "NOTICE",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Critical => "CRITICAL",
            Self::Alert => "ALERT",
            Self::Emergency => "EMERGENCY",
        }
    }

    /// Parse a string into a [`LogLevel`], accepting aliases (e.g. `warn` for `warning`).
    pub fn parse(s: &str) -> Option<LogLevel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "notice" => Some(LogLevel::Notice),
            "warn" | "warning" => Some(LogLevel::Warn),
            "error" => Some(LogLevel::Error),
            "critical" | "crit" => Some(LogLevel::Critical),
            "alert" => Some(LogLevel::Alert),
            "emergency" | "emerg" => Some(LogLevel::Emergency),
            _ => None,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&str> for LogLevel {
    fn eq(&self, other: &&str) -> bool {
        self.as_str().eq_ignore_ascii_case(other)
            || (self == &LogLevel::Warn && other.eq_ignore_ascii_case("warn"))
    }
}

impl PartialEq<str> for LogLevel {
    fn eq(&self, other: &str) -> bool {
        self.as_str().eq_ignore_ascii_case(other)
            || (self == &LogLevel::Warn && other.eq_ignore_ascii_case("warn"))
    }
}

impl PartialEq<String> for LogLevel {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<LogLevel> for &str {
    fn eq(&self, other: &LogLevel) -> bool {
        other == self
    }
}

impl PartialEq<LogLevel> for String {
    fn eq(&self, other: &LogLevel) -> bool {
        other == self.as_str()
    }
}

/// Structured log record captured by the logging subsystem (`Send + Sync`).
#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    pub timestamp_millis: u64,
    pub timestamp_iso: String,
    pub level: LogLevel,
    pub message: String,
    pub channel: String,
    pub context: Option<FrozenValue>,
    pub fiber_id: Option<u64>,
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

impl LogRecord {
    /// Retrieve the context deserialized as a standard [`PhpValue`].
    pub fn context_php(&self) -> Option<PhpValue> {
        self.context.as_ref().map(|f| f.to_php_value())
    }

    /// Convert the log record into a standard RHP dictionary [`PhpValue::Dict`].
    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert("level".to_string(), PhpValue::string(self.level.as_str()));
        map.insert("message".to_string(), PhpValue::string(&self.message));
        map.insert("channel".to_string(), PhpValue::string(&self.channel));
        map.insert(
            "timestamp".to_string(),
            PhpValue::Int(self.timestamp_millis as i64),
        );
        map.insert(
            "timestamp_iso".to_string(),
            PhpValue::string(&self.timestamp_iso),
        );
        map.insert(
            "context".to_string(),
            self.context
                .as_ref()
                .map(|f| f.to_php_value())
                .unwrap_or(PhpValue::dict(Vec::new())),
        );
        map.insert(
            "fiber_id".to_string(),
            self.fiber_id
                .map(|id| PhpValue::Int(id as i64))
                .unwrap_or(PhpValue::Null),
        );
        map.insert(
            "request_id".to_string(),
            self.request_id
                .as_ref()
                .map(|s| PhpValue::string(s))
                .unwrap_or(PhpValue::Null),
        );
        map.insert(
            "trace_id".to_string(),
            self.trace_id
                .as_ref()
                .map(|s| PhpValue::string(s))
                .unwrap_or(PhpValue::Null),
        );
        map.insert(
            "span_id".to_string(),
            self.span_id
                .as_ref()
                .map(|s| PhpValue::string(s))
                .unwrap_or(PhpValue::Null),
        );
        PhpValue::dict_from_map(map)
    }

    /// Serialize the log record into a single compact JSON line (for production stdout/files).
    pub fn to_json_line(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert(
            "timestamp".to_string(),
            serde_json::Value::String(self.timestamp_iso.clone()),
        );
        map.insert(
            "level".to_string(),
            serde_json::Value::String(self.level.as_str().to_string()),
        );
        map.insert(
            "channel".to_string(),
            serde_json::Value::String(self.channel.clone()),
        );
        map.insert(
            "message".to_string(),
            serde_json::Value::String(self.message.clone()),
        );
        if let Some(fid) = self.fiber_id {
            map.insert(
                "fiber_id".to_string(),
                serde_json::Value::Number(serde_json::Number::from(fid)),
            );
        }
        if let Some(ref rid) = self.request_id {
            map.insert(
                "request_id".to_string(),
                serde_json::Value::String(rid.clone()),
            );
        }
        if let Some(ref tid) = self.trace_id {
            map.insert(
                "trace_id".to_string(),
                serde_json::Value::String(tid.clone()),
            );
        }
        if let Some(ref sid) = self.span_id {
            map.insert(
                "span_id".to_string(),
                serde_json::Value::String(sid.clone()),
            );
        }
        if let Some(ref ctx) = self.context {
            let php_val = ctx.to_php_value();
            let json_str = crate::vm::facade::php_value_to_json(&php_val);
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                map.insert("context".to_string(), parsed);
            }
        }
        serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
    }

    /// Format the log record into a human-readable string with ANSI colors and structured metadata.
    pub fn to_pretty_line(&self) -> String {
        let color_code = match self.level {
            LogLevel::Debug => "\x1b[36m",      // Cyan
            LogLevel::Info => "\x1b[32m",       // Green
            LogLevel::Notice => "\x1b[34m",     // Blue
            LogLevel::Warn => "\x1b[33m",       // Yellow
            LogLevel::Error => "\x1b[31m",      // Red
            LogLevel::Critical => "\x1b[1;31m", // Bold Red
            LogLevel::Alert => "\x1b[35m",      // Magenta
            LogLevel::Emergency => "\x1b[1;37;41m", // White on Red
        };
        let reset = "\x1b[0m";

        let mut meta = Vec::new();
        if let Some(fid) = self.fiber_id {
            meta.push(format!("fiber#{}", fid));
        }
        if let Some(ref rid) = self.request_id {
            meta.push(format!("req#{}", rid));
        }
        if let Some(ref tid) = self.trace_id {
            meta.push(format!("trace={}", tid));
        }
        if let Some(ref sid) = self.span_id {
            meta.push(format!("span={}", sid));
        }

        let meta_str = if meta.is_empty() {
            String::new()
        } else {
            format!(" ({})", meta.join(" "))
        };

        let ctx_str = match &self.context {
            Some(ctx) => {
                let php_val = ctx.to_php_value();
                let json = crate::vm::facade::php_value_to_json(&php_val);
                if json.is_empty() || json == "{}" || json == "[]" {
                    String::new()
                } else {
                    format!(" {}", json)
                }
            }
            None => String::new(),
        };

        let channel_tag = if self.channel == "default" {
            String::new()
        } else {
            format!(" [{}]", self.channel)
        };

        format!(
            "[{}] [{}{}{}] {}{}{}",
            self.timestamp_iso,
            color_code,
            self.level.as_uppercase_str(),
            reset,
            self.message,
            channel_tag,
            meta_str,
        ) + &ctx_str
    }
}

/// Swappable backend log handler interface (`Send + Sync`).
pub trait LogHandler: Send + Sync {
    /// Check whether the handler accepts logs for this level and channel.
    /// Crucial for lazy evaluation: if false, callers skip all string formatting
    /// and context allocations.
    fn enabled(&self, level: LogLevel, channel: &str) -> bool;

    /// Handle the processed log record.
    fn handle(&self, record: &LogRecord) -> Result<(), RuntimeError>;

    /// Optional: Flush any buffered log records.
    fn flush(&self) -> Result<(), RuntimeError> {
        Ok(())
    }
}

/// Zero-config default stdout logger (Pretty in Development, JSON in Production).
pub struct StdoutLogHandler {
    min_level: RwLock<LogLevel>,
    force_json: bool,
}

impl StdoutLogHandler {
    pub fn new() -> Self {
        let is_prod = std::env::var("APP_ENV")
            .map(|v| v.eq_ignore_ascii_case("production") || v.eq_ignore_ascii_case("prod"))
            .unwrap_or(false);
        let force_json = std::env::var("LOG_FORMAT")
            .map(|v| v.eq_ignore_ascii_case("json"))
            .unwrap_or(is_prod);
        let default_level = if is_prod {
            LogLevel::Info
        } else {
            LogLevel::Debug
        };
        let min_level = std::env::var("LOG_LEVEL")
            .ok()
            .and_then(|s| LogLevel::parse(&s))
            .unwrap_or(default_level);

        Self {
            min_level: RwLock::new(min_level),
            force_json,
        }
    }

    pub fn set_min_level(&self, level: LogLevel) {
        if let Ok(mut g) = self.min_level.write() {
            *g = level;
        }
    }

    pub fn min_level(&self) -> LogLevel {
        self.min_level.read().map(|g| *g).unwrap_or(LogLevel::Debug)
    }
}

impl Default for StdoutLogHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl LogHandler for StdoutLogHandler {
    fn enabled(&self, level: LogLevel, _channel: &str) -> bool {
        level >= self.min_level()
    }

    fn handle(&self, record: &LogRecord) -> Result<(), RuntimeError> {
        if self.force_json {
            println!("{}", record.to_json_line());
        } else {
            println!("{}", record.to_pretty_line());
        }
        Ok(())
    }
}

/// In-memory recording log handler for test isolation (`Log::fake()`).
#[derive(Debug, Default)]
pub struct TestLogHandler {
    records: RwLock<Vec<LogRecord>>,
    min_level: RwLock<LogLevel>,
}

impl TestLogHandler {
    pub fn new() -> Self {
        Self {
            records: RwLock::new(Vec::new()),
            min_level: RwLock::new(LogLevel::Debug),
        }
    }

    pub fn records(&self) -> Vec<LogRecord> {
        self.records.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn count(&self) -> usize {
        self.records.read().map(|g| g.len()).unwrap_or(0)
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.records.write() {
            g.clear();
        }
    }

    pub fn last(&self) -> Option<LogRecord> {
        self.records.read().ok().and_then(|g| g.last().cloned())
    }

    pub fn has(&self, level: Option<LogLevel>, msg_sub: &str) -> bool {
        if let Ok(g) = self.records.read() {
            g.iter().any(|r| {
                if let Some(lvl) = level {
                    if r.level != lvl {
                        return false;
                    }
                }
                if !msg_sub.is_empty() && !r.message.contains(msg_sub) {
                    return false;
                }
                true
            })
        } else {
            false
        }
    }

    pub fn set_min_level(&self, level: LogLevel) {
        if let Ok(mut g) = self.min_level.write() {
            *g = level;
        }
    }
}

impl LogHandler for TestLogHandler {
    fn enabled(&self, level: LogLevel, _channel: &str) -> bool {
        level >= self.min_level.read().map(|g| *g).unwrap_or(LogLevel::Debug)
    }

    fn handle(&self, record: &LogRecord) -> Result<(), RuntimeError> {
        if let Ok(mut g) = self.records.write() {
            g.push(record.clone());
        }
        Ok(())
    }
}

/// Global registry for the central logging subsystem.
pub struct LogRegistry {
    default_stdout: Arc<StdoutLogHandler>,
    default_handler: RwLock<Arc<dyn LogHandler>>,
    channel_handlers: RwLock<HashMap<String, Arc<dyn LogHandler>>>,
    test_handler: RwLock<Option<Arc<TestLogHandler>>>,
}

impl LogRegistry {
    pub fn new() -> Self {
        let stdout = Arc::new(StdoutLogHandler::new());
        Self {
            default_stdout: stdout.clone(),
            default_handler: RwLock::new(stdout),
            channel_handlers: RwLock::new(HashMap::new()),
            test_handler: RwLock::new(None),
        }
    }

    pub fn global() -> &'static LogRegistry {
        static REGISTRY: OnceLock<LogRegistry> = OnceLock::new();
        REGISTRY.get_or_init(LogRegistry::new)
    }

    pub fn set_min_level(&self, level: LogLevel) {
        self.default_stdout.set_min_level(level);
        if let Some(test) = self.get_test_handler() {
            test.set_min_level(level);
        }
    }

    pub fn is_enabled(&self, level: LogLevel, channel: &str) -> bool {
        if let Ok(guard) = self.test_handler.read() {
            if let Some(ref test) = *guard {
                return test.enabled(level, channel);
            }
        }
        if let Ok(guard) = self.channel_handlers.read() {
            if let Some(h) = guard.get(channel) {
                return h.enabled(level, channel);
            }
        }
        self.default_handler
            .read()
            .map(|h| h.enabled(level, channel))
            .unwrap_or(true)
    }

    pub fn handle(&self, record: &LogRecord) -> Result<(), RuntimeError> {
        if let Ok(guard) = self.test_handler.read() {
            if let Some(ref test) = *guard {
                return test.handle(record);
            }
        }
        if let Ok(guard) = self.channel_handlers.read() {
            if let Some(h) = guard.get(&record.channel) {
                return h.handle(record);
            }
        }
        if let Ok(h) = self.default_handler.read() {
            h.handle(record)?;
        }
        Ok(())
    }

    pub fn set_default_handler(&self, handler: Arc<dyn LogHandler>) {
        if let Ok(mut g) = self.default_handler.write() {
            *g = handler;
        }
    }

    pub fn register_channel(&self, channel: &str, handler: Arc<dyn LogHandler>) {
        if let Ok(mut g) = self.channel_handlers.write() {
            g.insert(channel.to_string(), handler);
        }
    }

    pub fn fake(&self) -> Arc<TestLogHandler> {
        let fake = Arc::new(TestLogHandler::new());
        if let Ok(mut g) = self.test_handler.write() {
            *g = Some(Arc::clone(&fake));
        }
        fake
    }

    pub fn restore(&self) {
        if let Ok(mut g) = self.test_handler.write() {
            *g = None;
        }
        self.default_stdout.set_min_level(LogLevel::Debug);
        if let Ok(mut g) = self.default_handler.write() {
            *g = self.default_stdout.clone();
        }
    }

    pub fn is_faking(&self) -> bool {
        self.test_handler.read().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn get_test_handler(&self) -> Option<Arc<TestLogHandler>> {
        self.test_handler.read().ok().and_then(|g| g.clone())
    }

    pub fn get_recorded_logs(&self) -> Vec<LogRecord> {
        if let Some(t) = self.get_test_handler() {
            t.records()
        } else {
            Vec::new()
        }
    }

    pub fn clear_log_history(&self) {
        if let Some(t) = self.get_test_handler() {
            t.clear();
        } else {
            // Auto-activate test recording when clear_log_history is invoked (e.g. at test boot)
            let _ = self.fake();
        }
    }
}

impl Default for LogRegistry {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// Thread-local storage for userland closure / object handlers.
    static USERLAND_DEFAULT_HANDLER: RefCell<Option<PhpValue>> = RefCell::new(None);
    static USERLAND_CHANNELS: RefCell<HashMap<String, PhpValue>> = RefCell::new(HashMap::new());
}

/// Check if a userland closure or object handler is registered for the given channel (or default).
pub fn get_userland_handler(channel: &str) -> Option<PhpValue> {
    USERLAND_CHANNELS.with(|cell| {
        if let Some(h) = cell.borrow().get(channel) {
            return Some(h.clone());
        }
        USERLAND_DEFAULT_HANDLER.with(|d_cell| d_cell.borrow().clone())
    })
}

/// Set a userland closure or object handler for default logging.
pub fn set_userland_default_handler(handler: PhpValue) {
    USERLAND_DEFAULT_HANDLER.with(|cell| {
        *cell.borrow_mut() = Some(handler);
    });
}

/// Register a userland closure or object handler for a specific channel.
pub fn set_userland_channel_handler(channel: &str, handler: PhpValue) {
    USERLAND_CHANNELS.with(|cell| {
        cell.borrow_mut().insert(channel.to_string(), handler);
    });
}

/// Clear userland handlers (for test isolation).
pub fn clear_userland_handlers() {
    USERLAND_DEFAULT_HANDLER.with(|cell| {
        *cell.borrow_mut() = None;
    });
    USERLAND_CHANNELS.with(|cell| {
        cell.borrow_mut().clear();
    });
}

/// Build a structured [`LogRecord`] capturing active fiber, request, and distributed trace context.
pub fn build_record(
    level: LogLevel,
    message: String,
    context: Option<PhpValue>,
    channel: String,
) -> LogRecord {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let millis = duration.as_millis() as u64;

    // ISO 8601 timestamp with millisecond precision
    let iso = chrono::DateTime::<chrono::Utc>::from(now)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // Automatic runtime context enrichment:
    // 1. Fiber ID
    let fiber_id = crate::vm::fiber::current_fiber_id().map(|f| f.as_u64());

    // 2. Request ID from fiber local or active request headers
    let request_id = crate::vm::facade::get_active_request_id();

    // 3. W3C Trace and Span IDs from active OpenTelemetry tracer stack
    let trace_id = crate::telemetry::current_trace_id().map(|t| t.to_hex());
    let span_id = crate::telemetry::current_span_id().map(|s| s.to_hex());

    let frozen_context = context.map(|c| FrozenValue::from_php_value(&c));

    LogRecord {
        timestamp_millis: millis,
        timestamp_iso: iso,
        level,
        message,
        channel,
        context: frozen_context,
        fiber_id,
        request_id,
        trace_id,
        span_id,
    }
}

/// Append a structured log entry into the logging subsystem.
pub fn append_log(level: &str, message: &str, context: Option<PhpValue>, channel: &str) {
    let lvl = LogLevel::parse(level).unwrap_or(LogLevel::Info);
    let registry = LogRegistry::global();
    if !registry.is_enabled(lvl, channel) {
        return;
    }
    let record = build_record(lvl, message.to_string(), context, channel.to_string());
    let _ = registry.handle(&record);
}

/// Retrieve all recorded log entries (when test fake handler is active).
pub fn get_recorded_logs() -> Vec<LogRecord> {
    LogRegistry::global().get_recorded_logs()
}

/// Clear structured log history (for test isolation).
pub fn clear_log_history() {
    LogRegistry::global().clear_log_history();
}

/// Dispatch a call on the static `Log` facade (`Log::info()`, `Log::channel()`, etc.).
pub fn dispatch_log(method: &str, args: &[PhpValue]) -> Result<PhpValue, RuntimeError> {
    let registry = LogRegistry::global();

    match method {
        "debug" | "info" | "notice" | "warn" | "warning" | "error" | "critical" | "alert"
        | "emergency" => {
            let level = LogLevel::parse(method).unwrap_or(LogLevel::Info);
            // LAZY EVALUATION: Check if level is enabled on default channel BEFORE allocating message or context!
            if !registry.is_enabled(level, "default") {
                return Ok(PhpValue::Null);
            }
            let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let ctx = args.get(1).cloned();
            let record = build_record(level, msg, ctx, "default".to_string());
            registry.handle(&record)?;
            Ok(PhpValue::Null)
        }
        "log" => {
            let lvl_str = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "info".to_string());
            let level = LogLevel::parse(&lvl_str).unwrap_or(LogLevel::Info);
            if !registry.is_enabled(level, "default") {
                return Ok(PhpValue::Null);
            }
            let msg = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let ctx = args.get(2).cloned();
            let record = build_record(level, msg, ctx, "default".to_string());
            registry.handle(&record)?;
            Ok(PhpValue::Null)
        }
        "channel" => {
            let ch = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "default".to_string());
            let mut obj = VmObject::new("LoggerChannel");
            obj.set_property("channel", PhpValue::string(ch));
            Ok(PhpValue::object(obj))
        }
        "isEnabled" | "is_enabled" => {
            let lvl_str = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            if let Some(level) = LogLevel::parse(&lvl_str) {
                Ok(PhpValue::Bool(registry.is_enabled(level, "default")))
            } else {
                Ok(PhpValue::Bool(false))
            }
        }
        "fake" => {
            registry.fake();
            let obj = VmObject::new("TestLogHandler");
            Ok(PhpValue::object(obj))
        }
        "restore" | "reset" => {
            registry.restore();
            clear_userland_handlers();
            Ok(PhpValue::Null)
        }
        "setHandler" | "set_handler" | "withHandler" | "with_handler" => {
            if let Some(handler) = args.first().cloned() {
                set_userland_default_handler(handler);
            }
            Ok(PhpValue::Null)
        }
        "registerChannel" | "register_channel" => {
            let ch = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            if let Some(handler) = args.get(1).cloned() {
                set_userland_channel_handler(&ch, handler);
            }
            Ok(PhpValue::Null)
        }
        "setLevel" | "set_level" => {
            if let Some(s) = args.first().map(|v| v.to_php_string()) {
                if let Some(lvl) = LogLevel::parse(&s) {
                    registry.set_min_level(lvl);
                }
            }
            Ok(PhpValue::Null)
        }
        "history" | "getHistory" => {
            let records = registry.get_recorded_logs();
            let mut list = Vec::with_capacity(records.len());
            for r in records {
                list.push(r.to_php_value());
            }
            Ok(PhpValue::list(list))
        }
        "clear" | "clearHistory" => {
            registry.clear_log_history();
            Ok(PhpValue::Null)
        }
        "last" | "lastLog" => {
            if let Some(last) = registry.get_recorded_logs().last() {
                Ok(last.to_php_value())
            } else {
                Ok(PhpValue::Null)
            }
        }
        "count" => Ok(PhpValue::Int(registry.get_recorded_logs().len() as i64)),
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Log".to_string(),
            method: method.to_string(),
        }
        .into()),
    }
}

/// Dispatch a method call on a [`LoggerChannel`] instance (`$logger->error()`, `$logger->isEnabled()`, etc.).
pub fn dispatch_channel_method(
    channel: &str,
    method: &str,
    args: &[PhpValue],
) -> Result<PhpValue, RuntimeError> {
    let registry = LogRegistry::global();

    match method {
        "debug" | "info" | "notice" | "warn" | "warning" | "error" | "critical" | "alert"
        | "emergency" => {
            let level = LogLevel::parse(method).unwrap_or(LogLevel::Info);
            // Lazy evaluation: skip allocations if level is disabled on this channel
            if !registry.is_enabled(level, channel) {
                return Ok(PhpValue::Null);
            }
            let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let ctx = args.get(1).cloned();
            let record = build_record(level, msg, ctx, channel.to_string());
            registry.handle(&record)?;
            Ok(PhpValue::Null)
        }
        "log" => {
            let lvl_str = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "info".to_string());
            let level = LogLevel::parse(&lvl_str).unwrap_or(LogLevel::Info);
            if !registry.is_enabled(level, channel) {
                return Ok(PhpValue::Null);
            }
            let msg = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let ctx = args.get(2).cloned();
            let record = build_record(level, msg, ctx, channel.to_string());
            registry.handle(&record)?;
            Ok(PhpValue::Null)
        }
        "isEnabled" | "is_enabled" => {
            let lvl_str = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            if let Some(level) = LogLevel::parse(&lvl_str) {
                Ok(PhpValue::Bool(registry.is_enabled(level, channel)))
            } else {
                Ok(PhpValue::Bool(false))
            }
        }
        "getChannel" | "channel" => Ok(PhpValue::string(channel)),
        _ => Err(RuntimeErrorKind::Custom {
            message: format!("Call to undefined method LoggerChannel::{}()", method),
        }
        .into()),
    }
}

/// Dispatch a method call on a [`TestLogHandler`] instance (`$fake->records()`, `$fake->assertLogged()`, etc.).
pub fn dispatch_test_handler_method(
    method: &str,
    args: &[PhpValue],
) -> Result<PhpValue, RuntimeError> {
    let registry = LogRegistry::global();

    match method {
        "records" | "history" | "getRecords" => {
            let records = registry.get_recorded_logs();
            let list = records.into_iter().map(|r| r.to_php_value()).collect();
            Ok(PhpValue::list(list))
        }
        "count" => Ok(PhpValue::Int(registry.get_recorded_logs().len() as i64)),
        "clear" => {
            registry.clear_log_history();
            Ok(PhpValue::Null)
        }
        "last" | "lastLog" => {
            if let Some(last) = registry.get_recorded_logs().last() {
                Ok(last.to_php_value())
            } else {
                Ok(PhpValue::Null)
            }
        }
        "has" => {
            let lvl_str = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let sub = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let level = LogLevel::parse(&lvl_str);
            let has = if let Some(t) = registry.get_test_handler() {
                t.has(level, &sub)
            } else {
                false
            };
            Ok(PhpValue::Bool(has))
        }
        "hasMessage" => {
            let sub = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let has = if let Some(t) = registry.get_test_handler() {
                t.has(None, &sub)
            } else {
                false
            };
            Ok(PhpValue::Bool(has))
        }
        "assertLogged" => {
            let lvl_str = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            let sub = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let level = LogLevel::parse(&lvl_str);
            let has = if let Some(t) = registry.get_test_handler() {
                t.has(level, &sub)
            } else {
                false
            };
            if !has {
                return Err(RuntimeErrorKind::Custom {
                    message: format!(
                        "Failed asserting that log with level '{}' containing '{}' was recorded",
                        lvl_str, sub
                    ),
                }
                .into());
            }
            Ok(PhpValue::Bool(true))
        }
        "assertCount" => {
            let expected = args
                .first()
                .and_then(|v| match v {
                    PhpValue::Int(i) => Some(*i as usize),
                    _ => None,
                })
                .unwrap_or(0);
            let actual = registry.get_recorded_logs().len();
            if actual != expected {
                return Err(RuntimeErrorKind::Custom {
                    message: format!(
                        "Failed asserting that log count is {} (actual: {})",
                        expected, actual
                    ),
                }
                .into());
            }
            Ok(PhpValue::Bool(true))
        }
        _ => Err(RuntimeErrorKind::Custom {
            message: format!("Call to undefined method TestLogHandler::{}()", method),
        }
        .into()),
    }
}
