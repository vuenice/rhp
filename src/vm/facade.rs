use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use indexmap::IndexMap;

use crate::lexer::facade::{
    get_macro, has_custom_helper, is_legacy_php_function, is_reserved_intrinsic,
    is_static_facade, is_static_only, register_tier2_facade, FacadeRegistry,
    LEGACY_PHP_FUNCTIONS, NATIVE_FACADES, RESERVED_INTRINSICS,
};
use crate::lexer::span::Span;
use crate::type_system::types::{RustFloatKind, RustIntKind};
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::fiber::{
    current_fiber_stats, get_fiber_local, is_current_fiber, record_fiber_io_finish,
    record_fiber_io_start, record_fiber_yield, set_fiber_local, with_current_fiber, IoKind,
};
use crate::vm::parent_process::FrozenValue;
use crate::vm::crypto;
use crate::vm::html;
use crate::vm::mime::{detect_mime_type_for_path, guess_extension};
use crate::vm::url_helper;
use crate::vm::value::{PhpValue, VmClosure, VmFuture, VmObject};
use tokio::runtime::{Builder as TokioBuilder, Runtime as TokioRuntime};

/// Global multi-threaded Tokio runtime for asynchronous facades (`AFile`, `AHTTP`, `ADB`).
pub fn tokio_runtime() -> &'static TokioRuntime {
    static TOKIO_RT: OnceLock<TokioRuntime> = OnceLock::new();
    TOKIO_RT.get_or_init(|| {
        TokioBuilder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("rhp-tokio-worker")
            .build()
            .expect("Failed to initialize Tokio runtime for RHP async facades")
    })
}

/// Handler closure for Tier 2 custom Rust plugins.
pub type Tier2PluginHandler =
    Arc<dyn Fn(&str, &[PhpValue], Span) -> Result<PhpValue, RuntimeError> + Send + Sync>;

/// Extensible trait for defining custom Rust plugins / crates in Tier 2.
pub trait PluginFacade: Send + Sync {
    /// Facade name (e.g., `"Payment"`, `"Redis"`).
    fn name(&self) -> &str;

    /// Optional description for documentation and introspection.
    fn description(&self) -> &str {
        ""
    }

    /// Whether this plugin supports dynamic macro registration.
    fn supports_macros(&self) -> bool {
        true
    }

    /// Execute a static call on this plugin facade (`Plugin::method(args...)`).
    fn call(&self, method: &str, args: &[PhpValue], span: Span) -> Result<PhpValue, RuntimeError>;
}

/// Global registry for Tier 2 custom Rust plugin execution handlers.
static TIER2_PLUGIN_REGISTRY: OnceLock<RwLock<HashMap<String, Tier2PluginHandler>>> =
    OnceLock::new();

fn get_tier2_plugins() -> &'static RwLock<HashMap<String, Tier2PluginHandler>> {
    TIER2_PLUGIN_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Register a custom Tier 2 Rust plugin handler.
pub fn register_tier2_plugin_handler(
    name: &str,
    description: &str,
    supports_macros: bool,
    handler: Tier2PluginHandler,
) {
    register_tier2_facade(name.to_string(), description.to_string(), supports_macros);
    if let Ok(mut g) = get_tier2_plugins().write() {
        g.insert(name.to_string(), handler);
    }
}

/// Register a custom Tier 2 Rust plugin implementing the [`PluginFacade`] trait.
pub fn register_tier2_plugin(plugin: impl PluginFacade + 'static) {
    let arc = Arc::new(plugin);
    let name = arc.name().to_string();
    let desc = arc.description().to_string();
    let macros = arc.supports_macros();
    let p_clone = Arc::clone(&arc);
    let handler: Tier2PluginHandler = Arc::new(move |method, args, span| {
        p_clone.call(method, args, span)
    });
    register_tier2_plugin_handler(&name, &desc, macros, handler);
}

/// Check if a Tier 2 plugin is registered.
pub fn has_tier2_plugin(name: &str) -> bool {
    get_tier2_plugins()
        .read()
        .map(|g| g.contains_key(name))
        .unwrap_or(false)
}

/// Remove all registered Tier 2 plugins (for test isolation).
pub fn clear_tier2_plugins() {
    if let Ok(mut g) = get_tier2_plugins().write() {
        g.clear();
    }
    FacadeRegistry::global().clear_dynamic();
}

/// Structured log record captured by the centralized `Log` facade.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    pub level: String,
    pub message: String,
    pub context: Option<PhpValue>,
    pub channel: String,
}

thread_local! {
    static LOG_HISTORY: RefCell<Vec<LogRecord>> = RefCell::new(Vec::new());
}

/// Record a structured log entry into the global log buffer.
pub fn append_log(level: &str, message: &str, context: Option<PhpValue>, channel: &str) {
    LOG_HISTORY.with(|cell| {
        cell.borrow_mut().push(LogRecord {
            level: level.to_string(),
            message: message.to_string(),
            context,
            channel: channel.to_string(),
        });
    });
}

/// Retrieve all structured log records.
pub fn get_recorded_logs() -> Vec<LogRecord> {
    LOG_HISTORY.with(|cell| cell.borrow().clone())
}

/// Clear structured log history (for test isolation).
pub fn clear_log_history() {
    LOG_HISTORY.with(|cell| cell.borrow_mut().clear());
}

thread_local! {
    /// Thread-local runtime closure macro registry for dynamic facade macros (`Arr::macro(...)`, etc.).
    static RUNTIME_CLOSURE_MACROS: RefCell<HashMap<(String, String), VmClosure>> =
        RefCell::new(HashMap::new());
}

/// Retrieve a registered runtime closure macro.
pub fn get_runtime_closure_macro(facade: &str, method: &str) -> Option<VmClosure> {
    RUNTIME_CLOSURE_MACROS.with(|cell| {
        cell.borrow()
            .get(&(facade.to_string(), method.to_string()))
            .cloned()
    })
}

/// Register a compiled runtime closure macro on a facade (`Arr::macro('toCsv', ...)`).
pub fn register_runtime_closure_macro(facade: &str, method: &str, closure: VmClosure) {
    RUNTIME_CLOSURE_MACROS.with(|cell| {
        cell.borrow_mut()
            .insert((facade.to_string(), method.to_string()), closure);
    });
}

/// Check if a runtime closure macro is registered.
pub fn has_runtime_closure_macro(facade: &str, method: &str) -> bool {
    RUNTIME_CLOSURE_MACROS.with(|cell| {
        cell.borrow()
            .contains_key(&(facade.to_string(), method.to_string()))
    })
}

/// Remove all registered runtime closure macros (used for test isolation).
pub fn clear_runtime_closure_macros() {
    RUNTIME_CLOSURE_MACROS.with(|cell| {
        cell.borrow_mut().clear();
    });
}

thread_local! {
    /// Active HTTP request parameters and formatted values.
    static REQUEST_PARAMS: RefCell<IndexMap<String, PhpValue>> = RefCell::new(IndexMap::new());

    /// Configured type formatting rules for request parameters.
    static REQUEST_TYPE_FORMATS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());

    /// Active session state.
    static SESSION_DATA: RefCell<IndexMap<String, PhpValue>> = RefCell::new(IndexMap::new());

    /// Active cookies.
    static COOKIE_DATA: RefCell<IndexMap<String, PhpValue>> = RefCell::new(IndexMap::new());

    /// Active uploaded files from multipart request.
    static REQUEST_FILES: RefCell<IndexMap<String, PhpValue>> = RefCell::new(IndexMap::new());
}

/// Initialize or replace current request parameters.
/// All request values are normalized to string values by default!
pub fn init_request_params(params: impl IntoIterator<Item = (String, PhpValue)>) {
    REQUEST_PARAMS.with(|cell| {
        let mut map = cell.borrow_mut();
        map.clear();
        for (k, v) in params {
            let str_val = PhpValue::string(v.to_php_string());
            map.insert(k, str_val);
        }
    });
}

/// Initialize or replace current request uploaded files.
pub fn init_request_files(files: impl IntoIterator<Item = (String, PhpValue)>) {
    REQUEST_FILES.with(|cell| {
        let mut map = cell.borrow_mut();
        map.clear();
        for (k, v) in files {
            map.insert(k, v);
        }
    });
}

/// Helper to create a structured UploadedFile VmObject.
pub fn create_uploaded_file_object(
    tmp_path: &str,
    client_name: Option<&str>,
    client_mime: Option<&str>,
) -> VmObject {
    let mut obj = VmObject::new("UploadedFile");
    let c_name = client_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::path::Path::new(tmp_path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".to_string())
        });
    let detected_mime = detect_mime_type_for_path(std::path::Path::new(tmp_path))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let c_mime = client_mime.unwrap_or(&detected_mime).to_string();
    let size = std::fs::metadata(tmp_path).map(|m| m.len()).unwrap_or(0);

    obj.set_property("path", PhpValue::string(tmp_path));
    obj.set_property("clientName", PhpValue::string(c_name));
    obj.set_property("clientMimeType", PhpValue::string(c_mime));
    obj.set_property("mimeType", PhpValue::string(detected_mime));
    obj.set_property("size", PhpValue::Int(size as i32));
    obj
}

/// Initialize or replace current cookies.
pub fn init_cookie_data(cookies: impl IntoIterator<Item = (String, PhpValue)>) {
    COOKIE_DATA.with(|cell| {
        let mut map = cell.borrow_mut();
        map.clear();
        for (k, v) in cookies {
            map.insert(k, PhpValue::string(v.to_php_string()));
        }
    });
}

/// Initialize or replace current session data.
pub fn init_session_data(data: impl IntoIterator<Item = (String, PhpValue)>) {
    SESSION_DATA.with(|cell| {
        let mut map = cell.borrow_mut();
        map.clear();
        for (k, v) in data {
            map.insert(k, v);
        }
    });
}

/// Clear request, cookie, session, and uploaded files state (for request cleanup and test isolation).
pub fn clear_request_context() {
    REQUEST_PARAMS.with(|cell| cell.borrow_mut().clear());
    REQUEST_TYPE_FORMATS.with(|cell| cell.borrow_mut().clear());
    COOKIE_DATA.with(|cell| cell.borrow_mut().clear());
    SESSION_DATA.with(|cell| cell.borrow_mut().clear());
    REQUEST_FILES.with(|cell| cell.borrow_mut().clear());
}

/// Result type for standard facade dispatch functions.
pub type FacadeResult = Result<PhpValue, RuntimeErrorKind>;

/// Global atomic ID counter for synthetic future / job handles.
static ASYNC_TASK_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Dispatch a static facade call (`Str::*`, `Arr::*`, `Future::*`, `Sys::*`, etc.).
pub fn dispatch_facade_call(
    facade: &str,
    method: &str,
    args: &[PhpValue],
    span: Span,
) -> Result<PhpValue, RuntimeError> {
    // 1. Check if a dynamic macro has been registered via `Facade::macro('method', ...)`
    if let Some(_macro_entry) = get_macro(facade, method) {
        // Dynamic macros execute user closures. Return custom error or invoke closure.
        // For standard VM dispatch, fallback to built-in handlers if available.
    }

    let result = match facade {
        "Str" => dispatch_str(method, args),
        "Arr" => dispatch_arr(method, args),
        "Future" => dispatch_future(method, args),
        "Sys" => dispatch_sys(method, args),
        "File" => dispatch_file(method, args),
        "AFile" => dispatch_afile(method, args),
        "HTTP" => dispatch_http(method, args),
        "AHTTP" => dispatch_ahttp(method, args),
        "Log" => dispatch_log(method, args),
        "DB" => dispatch_db(method, args),
        "ADB" => dispatch_adb(method, args),
        "Int" => dispatch_int(method, args),
        "Float" => dispatch_float(method, args),
        "Session" => dispatch_session(method, args),
        "Cookie" => dispatch_cookie(method, args),
        "Request" => dispatch_request(method, args),
        "Image" => dispatch_image(method, args),
        "URL" | "Url" => dispatch_url(method, args),
        "Crypt" => dispatch_crypt(method, args),
        "Html" => dispatch_html(method, args),
        "Regex" => dispatch_regex(method, args),
        "Glob" => dispatch_glob(method, args),
        "phpFun" => dispatch_php_fun(method, args),
        "Fiber" => dispatch_fiber(method, args),
        "Memory" => dispatch_memory(method, args),
        "Weak" => dispatch_weak(method, args),
        "Trace" | "Telemetry" => crate::telemetry::facade::dispatch_trace(method, args),
        "Reflect" => dispatch_reflect(method, args),
        "Carbon" => dispatch_carbon(method, args),
        _ => {
            if let Ok(g) = get_tier2_plugins().read() {
                if let Some(handler) = g.get(facade) {
                    return handler(method, args, span);
                }
            }
            Err(RuntimeErrorKind::UndefinedFacadeMethod {
                facade: facade.to_string(),
                method: method.to_string(),
            })
        }
    };

    result.map_err(|kind| RuntimeError::new(kind, span))
}

// =========================================================================
// Str Facade
// =========================================================================

fn dispatch_str(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "check" => {
            let target = match args.first() {
                Some(PhpValue::String(s)) => s,
                _ => return Ok(PhpValue::Bool(false)),
            };
            if let Some(enc_val) = args.get(1) {
                let enc_str = enc_val.to_php_string();
                let is_valid = crate::vm::encoding::is_valid_encoding(target.as_bytes(), &enc_str);
                Ok(PhpValue::Bool(is_valid))
            } else {
                Ok(PhpValue::Bool(true))
            }
        }
        "contains" | "includes" => {
            let haystack = get_arg_str(args, 0, "Str::contains")?;
            let needle = get_arg_str(args, 1, "Str::contains")?;
            Ok(PhpValue::Bool(haystack.contains(&needle)))
        }
        "startsWith" | "starts_with" => {
            let s = get_arg_str(args, 0, "Str::startsWith")?;
            let prefix = get_arg_str(args, 1, "Str::startsWith")?;
            Ok(PhpValue::Bool(s.starts_with(&prefix)))
        }
        "endsWith" | "ends_with" => {
            let s = get_arg_str(args, 0, "Str::endsWith")?;
            let suffix = get_arg_str(args, 1, "Str::endsWith")?;
            Ok(PhpValue::Bool(s.ends_with(&suffix)))
        }
        "indexOf" | "pos" => {
            let arg0 = get_arg_str(args, 0, "Str::indexOf")?;
            let arg1 = get_arg_str(args, 1, "Str::indexOf")?;
            let offset = args.get(2).and_then(|v| v.to_i64()).unwrap_or(0);
            let (haystack, needle) = if arg0.contains(&arg1) || !arg1.contains(&arg0) {
                (&arg0, &arg1)
            } else {
                (&arg1, &arg0)
            };
            let char_indices: Vec<(usize, char)> = haystack.char_indices().collect();
            let total_chars = char_indices.len();
            let start_char_idx = if offset < 0 {
                (total_chars as i64 + offset).max(0) as usize
            } else {
                (offset as usize).min(total_chars)
            };
            let byte_offset = if start_char_idx < total_chars {
                char_indices[start_char_idx].0
            } else {
                haystack.len()
            };
            if needle.is_empty() {
                return Ok(PhpValue::Int(start_char_idx as i32));
            }
            if let Some(pos) = haystack[byte_offset..].find(needle.as_str()) {
                let actual_byte_pos = byte_offset + pos;
                let char_idx = haystack[..actual_byte_pos].chars().count();
                Ok(PhpValue::Int(char_idx as i32))
            } else {
                Ok(PhpValue::Int(-1))
            }
        }
        "testRegex" | "regex" => {
            let arg0 = get_arg_str(args, 0, "Str::testRegex")?;
            if let Some(arg1) = args.get(1) {
                let arg1_str = arg1.to_php_string();
                let matched = if let Ok(re) = parse_regex_pattern(&arg0) {
                    re.is_match(&arg1_str)
                } else if let Ok(re) = parse_regex_pattern(&arg1_str) {
                    re.is_match(&arg0)
                } else {
                    false
                };
                Ok(PhpValue::Bool(matched))
            } else {
                Ok(PhpValue::Bool(parse_regex_pattern(&arg0).is_ok()))
            }
        }
        "length" => {
            let s = get_arg_str(args, 0, "Str::length")?;
            Ok(PhpValue::Int(s.chars().count() as i32))
        }
        "upper" => {
            let s = get_arg_str(args, 0, "Str::upper")?;
            Ok(PhpValue::string(s.to_uppercase()))
        }
        "lower" => {
            let s = get_arg_str(args, 0, "Str::lower")?;
            Ok(PhpValue::string(s.to_lowercase()))
        }
        "trim" => {
            let s = get_arg_str(args, 0, "Str::trim")?;
            let chars = if let Some(arg1) = args.get(1) {
                arg1.to_php_string()
            } else {
                " \t\n\r\0\x0B".to_string()
            };
            let trimmed = s.trim_matches(|c| chars.contains(c));
            Ok(PhpValue::string(trimmed))
        }
        "slug" => {
            let s = get_arg_str(args, 0, "Str::slug")?;
            let mut slug = String::with_capacity(s.len());
            let mut last_was_dash = false;
            for c in s.chars() {
                if c.is_alphanumeric() {
                    slug.extend(c.to_lowercase());
                    last_was_dash = false;
                } else if !last_was_dash && !slug.is_empty() {
                    slug.push('-');
                    last_was_dash = true;
                }
            }
            if slug.ends_with('-') {
                slug.pop();
            }
            Ok(PhpValue::string(slug))
        }
        "random" => {
            let len = args.first().and_then(|v| v.to_i64()).unwrap_or(16).max(1) as usize;
            const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
            let mut out = String::with_capacity(len);
            let mut seed = ASYNC_TASK_COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
            for _ in 0..len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let idx = ((seed >> 32) as usize) % CHARSET.len();
                out.push(CHARSET[idx] as char);
            }
            Ok(PhpValue::string(out))
        }
        "join" => {
            let sep = get_arg_str(args, 0, "Str::join")?;
            let items = match args.get(1) {
                Some(PhpValue::Array(a) | PhpValue::List(a)) => {
                    a.borrow().iter().map(|v| v.to_php_string()).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            };
            Ok(PhpValue::string(items.join(&sep)))
        }
        "split" => {
            let sep = get_arg_str(args, 0, "Str::split")?;
            let s = get_arg_str(args, 1, "Str::split")?;
            let parts: Vec<PhpValue> = if sep.is_empty() {
                s.chars().map(|c| PhpValue::string(c.to_string())).collect()
            } else {
                s.split(&sep).map(PhpValue::string).collect()
            };
            Ok(PhpValue::list(parts))
        }
        "replace" => {
            let arg0 = get_arg_str(args, 0, "Str::replace")?;
            let arg1 = get_arg_str(args, 1, "Str::replace")?;
            let arg2 = get_arg_str(args, 2, "Str::replace")?;
            // Support both ($search, $replace, $subject) and ($subject, $search, $replace)
            if arg0.contains(&arg1) && !arg2.contains(&arg0) {
                Ok(PhpValue::string(arg0.replace(&arg1, &arg2)))
            } else {
                Ok(PhpValue::string(arg2.replace(&arg0, &arg1)))
            }
        }
        "fromEncoding" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::fromEncoding".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let encoding = get_arg_str(args, 1, "Str::fromEncoding").unwrap_or_else(|_| "UTF-8".to_string());
            let raw_bytes = extract_raw_byte_payload(target);
            let decoded = crate::vm::encoding::decode_to_utf8(&raw_bytes, &encoding)
                .map_err(|msg| RuntimeErrorKind::Custom { message: msg })?;
            Ok(PhpValue::string(decoded))
        }
        "toEncoding" => {
            let target = get_arg_str(args, 0, "Str::toEncoding")?;
            let encoding = get_arg_str(args, 1, "Str::toEncoding").unwrap_or_else(|_| "UTF-8".to_string());
            let encoded_bytes = crate::vm::encoding::encode_from_utf8(&target, &encoding)
                .map_err(|msg| RuntimeErrorKind::Custom { message: msg })?;
            Ok(PhpValue::string(crate::vm::value::RhpString::from_vec(encoded_bytes)))
        }
        "utf8" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::utf8".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let raw_bytes = extract_raw_byte_payload(target);
            let clean = String::from_utf8_lossy(&raw_bytes).into_owned();
            Ok(PhpValue::string(clean))
        }
        "isUtf8" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::isUtf8".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let is_valid = match target {
                PhpValue::String(s) => s.is_utf8(),
                _ => true,
            };
            Ok(PhpValue::Bool(is_valid))
        }
        "fromBytes" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::fromBytes".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let bytes = match target {
                PhpValue::Array(a) | PhpValue::List(a) => {
                    a.borrow().iter().map(|v| v.to_i64().unwrap_or(0) as u8).collect::<Vec<u8>>()
                }
                PhpValue::String(s) => s.as_bytes().to_vec(),
                _ => target.to_php_string().into_bytes(),
            };
            Ok(PhpValue::string(crate::vm::value::RhpString::from_vec(bytes)))
        }
        "bytes" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::bytes".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let bytes_vec: Vec<PhpValue> = match target {
                PhpValue::String(s) => s.as_bytes().iter().map(|&b| PhpValue::Int(b as i32)).collect(),
                _ => target.to_php_string().as_bytes().iter().map(|&b| PhpValue::Int(b as i32)).collect(),
            };
            Ok(PhpValue::list(bytes_vec))
        }
        "byteLength" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::byteLength".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let len = match target {
                PhpValue::String(s) => s.len(),
                _ => target.to_php_string().len(),
            };
            Ok(PhpValue::Int(len as i32))
        }
        "encoding" | "detectEncoding" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::encoding".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let raw_bytes = extract_raw_byte_payload(target);
            let detected = crate::vm::encoding::detect_encoding(&raw_bytes);
            Ok(PhpValue::string(detected))
        }
        "ltrim" => {
            let s = get_arg_str(args, 0, "Str::ltrim")?;
            let chars = if let Some(arg1) = args.get(1) {
                arg1.to_php_string()
            } else {
                " \t\n\r\0\x0B".to_string()
            };
            let trimmed = s.trim_start_matches(|c| chars.contains(c));
            Ok(PhpValue::string(trimmed))
        }
        "rtrim" => {
            let s = get_arg_str(args, 0, "Str::rtrim")?;
            let chars = if let Some(arg1) = args.get(1) {
                arg1.to_php_string()
            } else {
                " \t\n\r\0\x0B".to_string()
            };
            let trimmed = s.trim_end_matches(|c| chars.contains(c));
            Ok(PhpValue::string(trimmed))
        }
        "substr" => {
            let s = get_arg_str(args, 0, "Str::substr")?;
            let start = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            let length = args.get(2).and_then(|v| v.to_i64());
            let chars: Vec<char> = s.chars().collect();
            let total_len = chars.len() as i64;
            let actual_start = if start < 0 {
                (total_len + start).max(0) as usize
            } else {
                start.min(total_len) as usize
            };
            let sub: String = if let Some(len) = length {
                let actual_len = if len < 0 {
                    (total_len - actual_start as i64 + len).max(0) as usize
                } else {
                    len as usize
                };
                chars.iter().skip(actual_start).take(actual_len).collect()
            } else {
                chars.iter().skip(actual_start).collect()
            };
            Ok(PhpValue::string(sub))
        }
        "repeat" => {
            let s = get_arg_str(args, 0, "Str::repeat")?;
            let times = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0).max(0) as usize;
            Ok(PhpValue::string(s.repeat(times)))
        }
        "reverse" => {
            let s = get_arg_str(args, 0, "Str::reverse")?;
            let rev: String = s.chars().rev().collect();
            Ok(PhpValue::string(rev))
        }
        "format" => {
            Ok(PhpValue::string(crate::vm::format::format_sys_args(args)))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Str".to_string(),
            method: method.to_string(),
        }),
    }
}

pub fn extract_raw_byte_payload(val: &PhpValue) -> Vec<u8> {
    match val {
        PhpValue::String(s) => {
            if !s.is_utf8() {
                s.as_bytes().to_vec()
            } else {
                let mut maybe_latin1 = true;
                let mut bytes_from_chars = Vec::new();
                for ch in s.as_str().chars() {
                    let cp = ch as u32;
                    if cp <= 0xFF {
                        bytes_from_chars.push(cp as u8);
                    } else {
                        maybe_latin1 = false;
                        break;
                    }
                }
                if maybe_latin1 && bytes_from_chars.len() < s.len() {
                    bytes_from_chars
                } else {
                    s.as_bytes().to_vec()
                }
            }
        }
        PhpValue::Array(a) | PhpValue::List(a) => {
            a.borrow()
                .iter()
                .map(|item| item.to_i64().unwrap_or(0) as u8)
                .collect()
        }
        other => other.to_php_string().into_bytes(),
    }
}

// =========================================================================
// Arr Facade
// =========================================================================

fn dispatch_arr(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "list" => {
            let items = if args.len() > 1 {
                args.to_vec()
            } else if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::list(items))
        }
        "range" => {
            if args.is_empty() {
                return Err(RuntimeErrorKind::TooFewArguments {
                    callable: "Arr::range".to_string(),
                    passed: 0,
                    expected: 1,
                });
            }

            // Check if arguments are single characters (e.g. 'a', 'e')
            let is_char_range = if args.len() >= 2 {
                let s1 = args[0].to_php_string();
                let s2 = args[1].to_php_string();
                args[0].to_i64().is_none()
                    && args[1].to_i64().is_none()
                    && s1.chars().count() == 1
                    && s2.chars().count() == 1
            } else {
                false
            };

            if is_char_range {
                let s1 = args[0].to_php_string();
                let s2 = args[1].to_php_string();
                let c1 = s1.chars().next().unwrap() as u32;
                let c2 = s2.chars().next().unwrap() as u32;
                let step = if args.len() >= 3 {
                    args[2].to_i64().unwrap_or(1)
                } else {
                    1i64
                };
                if step == 0 {
                    return Err(RuntimeErrorKind::ValueError {
                        message: "Arr::range() arg 3 must not be zero".to_string(),
                    });
                }
                let mut list = Vec::new();
                if step > 0 {
                    let mut curr = c1;
                    while curr < c2 {
                        if let Some(ch) = char::from_u32(curr) {
                            list.push(PhpValue::string(ch.to_string()));
                        }
                        curr += step as u32;
                    }
                } else {
                    let step_abs = (-step) as u32;
                    let mut curr = c1;
                    while curr > c2 {
                        if let Some(ch) = char::from_u32(curr) {
                            list.push(PhpValue::string(ch.to_string()));
                        }
                        if curr < step_abs {
                            break;
                        }
                        curr -= step_abs;
                    }
                }
                return Ok(PhpValue::list(list));
            }

            let (start, end, step) = match args.len() {
                1 => {
                    let stop = args[0].to_i64().unwrap_or(0);
                    (0i64, stop, 1i64)
                }
                2 => {
                    let start = args[0].to_i64().unwrap_or(0);
                    let stop = args[1].to_i64().unwrap_or(0);
                    (start, stop, 1i64)
                }
                _ => {
                    let start = args[0].to_i64().unwrap_or(0);
                    let stop = args[1].to_i64().unwrap_or(0);
                    let step = args[2].to_i64().unwrap_or(1);
                    (start, stop, step)
                }
            };

            if step == 0 {
                return Err(RuntimeErrorKind::ValueError {
                    message: "Arr::range() arg 3 must not be zero".to_string(),
                });
            }

            let mut list = Vec::new();
            if step > 0 {
                let mut curr = start;
                while curr < end {
                    list.push(PhpValue::Int(curr as i32));
                    curr += step;
                }
            } else {
                let mut curr = start;
                while curr > end {
                    list.push(PhpValue::Int(curr as i32));
                    curr += step;
                }
            }
            Ok(PhpValue::list(list))
        }
        "wrap" => match args.first() {
            None | Some(PhpValue::Null) => Ok(PhpValue::list(Vec::new())),
            Some(PhpValue::List(l)) => Ok(PhpValue::List(l.clone())),
            Some(PhpValue::Array(a)) => Ok(PhpValue::list(a.borrow().clone())),
            Some(other) => Ok(PhpValue::list(vec![other.clone()])),
        },
        "repeat" => {
            let val = args.first().cloned().unwrap_or(PhpValue::Null);
            let count = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0).max(0) as usize;
            Ok(PhpValue::list(vec![val; count]))
        }
        "from" => {
            let items = if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::list(items))
        }
        "first" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            Ok(items.into_iter().next().unwrap_or(PhpValue::Null))
        }
        "last" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            Ok(items.into_iter().last().unwrap_or(PhpValue::Null))
        }
        "flatten" => {
            let mut out = Vec::new();
            if let Some(first) = args.first() {
                flatten_elements(first, &mut out);
            }
            Ok(PhpValue::list(out))
        }
        "chunk" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let size = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
            let chunks: Vec<PhpValue> = items
                .chunks(size)
                .map(|chunk| PhpValue::list(chunk.to_vec()))
                .collect();
            Ok(PhpValue::list(chunks))
        }
        "pluck" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let key_by = args.get(2).map(|v| v.to_php_string());
            if let Some(kb) = key_by {
                let mut map = IndexMap::new();
                for item in items {
                    let k = match &item {
                        PhpValue::Object(obj) => obj.borrow().get_property(&kb).map(|v| v.to_php_string()),
                        PhpValue::Dict(d) => d.borrow().get(&kb).map(|v| v.to_php_string()),
                        _ => None,
                    }.unwrap_or_default();
                    let v = match &item {
                        PhpValue::Object(obj) => obj.borrow().get_property(&key).cloned().unwrap_or(PhpValue::Null),
                        PhpValue::Dict(d) => d.borrow().get(&key).cloned().unwrap_or(PhpValue::Null),
                        _ => PhpValue::Null,
                    };
                    map.insert(k, v);
                }
                return Ok(PhpValue::dict_from_map(map));
            }
            let mut plucked = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    PhpValue::Object(obj) => {
                        let val = obj.borrow().get_property(&key).cloned().unwrap_or(PhpValue::Null);
                        plucked.push(val);
                    }
                    PhpValue::Dict(d) => {
                        let val = d.borrow().get(&key).cloned().unwrap_or(PhpValue::Null);
                        plucked.push(val);
                    }
                    _ => plucked.push(PhpValue::Null),
                }
            }
            Ok(PhpValue::list(plucked))
        }
        "sort" => {
            let mut items = args.first().map(extract_array_elements).unwrap_or_default();
            items.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            Ok(PhpValue::list(items))
        }
        "macro" => {
            if let Some(PhpValue::String(name)) = args.first() {
                if let Some(PhpValue::Closure(closure)) = args.get(1) {
                    register_runtime_closure_macro("Arr", name.as_str(), (**closure).clone());
                    return Ok(PhpValue::Bool(true));
                }
            }
            Ok(PhpValue::Bool(false))
        }
        "dict" => {
            let map = if let Some(first) = args.first() {
                extract_dict_map(first)
            } else {
                IndexMap::new()
            };
            Ok(PhpValue::dict_from_map(map))
        }
        "set" => {
            if args.len() >= 3 {
                let mut target = args.first().cloned().unwrap_or(PhpValue::Null);
                let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                let val = args.get(2).cloned().unwrap_or(PhpValue::Null);
                arr_set_dot(&mut target, &key, val);
                Ok(target)
            } else {
                let items = if let Some(first) = args.first() {
                    extract_array_elements(first)
                } else {
                    Vec::new()
                };
                Ok(PhpValue::set(items))
            }
        }
        "hashMap" | "map" => {
            let entries = if let Some(first) = args.first() {
                extract_map_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::hash_map(entries))
        }
        "check" => {
            let target = match args.first() {
                Some(v) => v,
                None => return Ok(PhpValue::Bool(false)),
            };
            let subtype_arg = args.get(1).map(|v| v.to_php_string());
            let is_match = match subtype_arg.as_deref() {
                None => matches!(
                    target,
                    PhpValue::Array(_)
                        | PhpValue::List(_)
                        | PhpValue::Dict(_)
                        | PhpValue::Set(_)
                        | PhpValue::HashMap(_)
                ),
                Some(sub) => {
                    let clean = sub.trim().to_lowercase().replace(['-', '_', ' '], "");
                    match clean.as_str() {
                        "list" => matches!(target, PhpValue::List(_) | PhpValue::Array(_)),
                        "dict" | "dictionary" => matches!(target, PhpValue::Dict(_)),
                        "set" => matches!(target, PhpValue::Set(_)),
                        "hashmap" | "map" => matches!(target, PhpValue::HashMap(_)),
                        "array" => matches!(target, PhpValue::Array(_)),
                        _ => false,
                    }
                }
            };
            Ok(PhpValue::Bool(is_match))
        }
        "count" | "length" | "size" | "len" => {
            let cnt = match args.first() {
                Some(PhpValue::Array(a) | PhpValue::List(a)) => a.borrow().len(),
                Some(PhpValue::Dict(d)) => d.borrow().len(),
                Some(PhpValue::Set(s)) => s.borrow().len(),
                Some(PhpValue::HashMap(m)) => m.borrow().len(),
                _ => 0,
            };
            Ok(PhpValue::Int(cnt as i32))
        }
        "merge" => {
            let mut is_dict = false;
            for arg in args {
                if matches!(arg, PhpValue::Dict(_)) {
                    is_dict = true;
                    break;
                }
            }
            if is_dict {
                let mut merged = IndexMap::new();
                for arg in args {
                    for (k, v) in extract_dict_elements(arg) {
                        merged.insert(k, v);
                    }
                }
                Ok(PhpValue::dict_from_map(merged))
            } else {
                let mut merged = Vec::new();
                for arg in args {
                    merged.extend(extract_array_elements(arg));
                }
                Ok(PhpValue::list(merged))
            }
        }
        "contains" | "hasValue" => {
            let target = match args.get(1) {
                Some(v) => v,
                None => return Ok(PhpValue::Bool(false)),
            };
            let exists = match args.first() {
                Some(PhpValue::Array(a) | PhpValue::List(a)) => a.borrow().contains(target),
                Some(PhpValue::Set(s)) => s.borrow().contains(target),
                Some(PhpValue::Dict(d)) => d.borrow().values().any(|v| v == target),
                Some(PhpValue::HashMap(m)) => m.borrow().values().any(|v| v == target),
                _ => false,
            };
            Ok(PhpValue::Bool(exists))
        }
        "keys" => {
            let keys = match args.first() {
                Some(PhpValue::Dict(d)) => d
                    .borrow()
                    .keys()
                    .map(|k| PhpValue::string(k.clone()))
                    .collect(),
                Some(PhpValue::HashMap(m)) => m.borrow().keys().cloned().collect(),
                Some(PhpValue::Array(a) | PhpValue::List(a)) => (0..a.borrow().len())
                    .map(|i| PhpValue::Int(i as i32))
                    .collect(),
                Some(PhpValue::Set(s)) => s.borrow().iter().cloned().collect(),
                _ => Vec::new(),
            };
            Ok(PhpValue::list(keys))
        }
        "values" => {
            let vals = match args.first() {
                Some(PhpValue::Dict(d)) => d.borrow().values().cloned().collect(),
                Some(PhpValue::HashMap(m)) => m.borrow().values().cloned().collect(),
                Some(PhpValue::Array(a) | PhpValue::List(a)) => a.borrow().clone(),
                Some(PhpValue::Set(s)) => s.borrow().iter().cloned().collect(),
                _ => Vec::new(),
            };
            Ok(PhpValue::list(vals))
        }
        "unique" => {
            let items = if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::set(items))
        }
        "reverse" => {
            if let Some(PhpValue::Dict(d)) = args.first() {
                let mut rev = d.borrow().clone();
                rev.reverse();
                Ok(PhpValue::dict_from_map(rev))
            } else {
                let mut items = if let Some(first) = args.first() {
                    extract_array_elements(first)
                } else {
                    Vec::new()
                };
                items.reverse();
                Ok(PhpValue::list(items))
            }
        }
        "get" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let default = args.get(2).cloned().unwrap_or(PhpValue::Null);
            Ok(arr_get_dot(&target, &key, default))
        }
        "has" | "hasKey" => {
            let target = match args.first() {
                Some(t) => t,
                None => return Ok(PhpValue::Bool(false)),
            };
            let key = match args.get(1) {
                Some(k) => k,
                None => return Ok(PhpValue::Bool(false)),
            };
            let exists = match target {
                PhpValue::Dict(d) => {
                    let k_str = key.to_php_string();
                    if k_str.contains('.') {
                        arr_has_dot(target, &k_str)
                    } else {
                        d.borrow().contains_key(&k_str)
                    }
                }
                PhpValue::HashMap(m) => {
                    m.borrow().contains_key(key)
                }
                PhpValue::Array(a) | PhpValue::List(a) => {
                    let idx = key.to_i64().unwrap_or(-1);
                    idx >= 0 && (idx as usize) < a.borrow().len()
                }
                PhpValue::Set(s) => {
                    s.borrow().contains(key)
                }
                _ => false,
            };
            Ok(PhpValue::Bool(exists))
        }
        "hasAny" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let keys = args.get(1).map(extract_array_elements).unwrap_or_default();
            let any = keys.iter().any(|k| arr_has_dot(&target, &k.to_php_string()));
            Ok(PhpValue::Bool(any))
        }
        "forget" => {
            let mut target = args.first().cloned().unwrap_or(PhpValue::Null);
            let keys: Vec<String> = if let Some(PhpValue::Array(a) | PhpValue::List(a)) = args.get(1) {
                a.borrow().iter().map(|v| v.to_php_string()).collect()
            } else {
                args.iter().skip(1).map(|v| v.to_php_string()).collect()
            };
            for k in keys {
                arr_forget_dot(&mut target, &k);
            }
            Ok(target)
        }
        "pull" => {
            let mut target = args.first().cloned().unwrap_or(PhpValue::Null);
            let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let default = args.get(2).cloned().unwrap_or(PhpValue::Null);
            let val = arr_get_dot(&target, &key, default);
            arr_forget_dot(&mut target, &key);
            Ok(val)
        }
        "only" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let key_list: Vec<String> = if let Some(PhpValue::Array(a) | PhpValue::List(a)) = args.get(1) {
                a.borrow().iter().map(|v| v.to_php_string()).collect()
            } else {
                args.iter().skip(1).map(|v| v.to_php_string()).collect()
            };
            let dict_map = extract_dict_map(&target);
            let mut result = IndexMap::new();
            for k in key_list {
                if let Some(val) = dict_map.get(&k) {
                    result.insert(k, val.clone());
                }
            }
            Ok(PhpValue::dict_from_map(result))
        }
        "except" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let key_list: Vec<String> = if let Some(PhpValue::Array(a) | PhpValue::List(a)) = args.get(1) {
                a.borrow().iter().map(|v| v.to_php_string()).collect()
            } else {
                args.iter().skip(1).map(|v| v.to_php_string()).collect()
            };
            let dict_map = extract_dict_map(&target);
            let mut result = IndexMap::new();
            for (k, val) in dict_map.iter() {
                if !key_list.contains(k) {
                    result.insert(k.clone(), val.clone());
                }
            }
            Ok(PhpValue::dict_from_map(result))
        }
        "dot" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let prepend = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let mut out = IndexMap::new();
            arr_dot(&target, &prepend, &mut out);
            Ok(PhpValue::dict_from_map(out))
        }
        "undot" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let dict_map = extract_dict_map(&target);
            Ok(arr_undot(&dict_map))
        }
        "divide" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let dict_map = extract_dict_map(&target);
            let keys: Vec<PhpValue> = dict_map.keys().map(|k| PhpValue::string(k.clone())).collect();
            let values: Vec<PhpValue> = dict_map.values().cloned().collect();
            Ok(PhpValue::list(vec![PhpValue::list(keys), PhpValue::list(values)]))
        }
        "keyBy" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let key_field = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let mut map = IndexMap::new();
            for item in items {
                let k_str = match &item {
                    PhpValue::Object(o) => o.borrow().get_property(&key_field).map(|v| v.to_php_string()),
                    PhpValue::Dict(d) => d.borrow().get(&key_field).map(|v| v.to_php_string()),
                    _ => None,
                }.unwrap_or_default();
                map.insert(k_str, item);
            }
            Ok(PhpValue::dict_from_map(map))
        }
        "collapse" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let mut is_assoc = false;
            for it in &items {
                if matches!(it, PhpValue::Dict(_)) {
                    is_assoc = true;
                    break;
                }
            }
            if is_assoc {
                let mut collapsed = IndexMap::new();
                for it in items {
                    for (k, v) in extract_dict_elements(&it) {
                        collapsed.insert(k, v);
                    }
                }
                Ok(PhpValue::dict_from_map(collapsed))
            } else {
                let mut collapsed = Vec::new();
                for it in items {
                    collapsed.extend(extract_array_elements(&it));
                }
                Ok(PhpValue::list(collapsed))
            }
        }
        "isAssoc" => {
            let is_assoc = match args.first() {
                Some(PhpValue::Dict(_)) => true,
                _ => false,
            };
            Ok(PhpValue::Bool(is_assoc))
        }
        "isList" => {
            let is_list = match args.first() {
                Some(PhpValue::List(_) | PhpValue::Array(_)) => true,
                _ => false,
            };
            Ok(PhpValue::Bool(is_list))
        }
        "sortKeys" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let mut map = extract_dict_map(&target);
            map.sort_keys();
            Ok(PhpValue::dict_from_map(map))
        }
        "sortKeysDesc" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let mut map = extract_dict_map(&target);
            map.sort_by(|k1, _, k2, _| k2.cmp(k1));
            Ok(PhpValue::dict_from_map(map))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Arr".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Future Concurrency Facade
// =========================================================================

fn dispatch_future(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "value" => {
            let val = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::resolved(val),
            ))))
        }
        "error" => {
            let msg = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "Unknown error".to_string());
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::rejected(msg),
            ))))
        }
        "delayed" => {
            let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::resolved(val),
            ))))
        }
        "wait" | "all" => {
            // Eagerly resolve array of tasks/values
            let mut resolved = Vec::new();
            if let Some(first) = args.first() {
                for item in extract_array_elements(first) {
                    match item {
                        PhpValue::Future(f) => match f.borrow_mut().wait_result() {
                            Ok(val) => resolved.push(val),
                            Err(err) => return Err(RuntimeErrorKind::Custom { message: err }),
                        },
                        other => resolved.push(other),
                    }
                }
            }
            Ok(PhpValue::list(resolved))
        }
        "any" => {
            // Resolve first available value
            let first_val = if let Some(first) = args.first() {
                extract_array_elements(first).into_iter().next().unwrap_or(PhpValue::Null)
            } else {
                PhpValue::Null
            };
            match first_val {
                PhpValue::Future(f) => match f.borrow_mut().wait_result() {
                    Ok(val) => Ok(val),
                    Err(err) => Err(RuntimeErrorKind::Custom { message: err }),
                },
                other => Ok(other),
            }
        }
        "run" => {
            let task = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::resolved(task),
            ))))
        }
        "defer" => {
            // Registered post-response hook
            Ok(PhpValue::Null)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Future".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Sys Facade
// =========================================================================

fn dispatch_sys(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "env" => {
            let key = get_arg_str(args, 0, "Sys::env")?;
            match std::env::var(&key) {
                Ok(val) => Ok(PhpValue::string(val)),
                Err(_) => {
                    let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                    Ok(default_val)
                }
            }
        }
        "echo" => {
            let text = crate::vm::format::format_sys_args(args);
            print!("{}", text);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            Ok(PhpValue::Null)
        }
        "print" => {
            let mut text = crate::vm::format::format_sys_args(args);
            text.push('\n');
            print!("{}", text);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            Ok(PhpValue::Null)
        }
        "print_r" => {
            if let Some(arg) = args.first() {
                let return_output = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
                let formatted = arg.to_print_r();
                if return_output {
                    return Ok(PhpValue::string(formatted));
                } else {
                    println!("{}", formatted);
                    return Ok(PhpValue::Bool(true));
                }
            }
            Ok(PhpValue::Null)
        }
        "dump" | "dd" => {
            for arg in args {
                println!("{}", arg.to_print_r());
            }
            Ok(PhpValue::Null)
        }
        "os" => Ok(PhpValue::string(std::env::consts::OS)),
        "version" => Ok(PhpValue::string(env!("CARGO_PKG_VERSION"))),
        "timestamp" => Ok(PhpValue::Int(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i32,
        )),
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Sys".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// File, HTTP, Log, DB, Session, Image, URL, Glob, phpFun Facades
// =========================================================================

fn dispatch_file(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "read" => {
            let path = get_arg_str(args, 0, "File::read")?;
            record_fiber_io_start(IoKind::FileRead { path: path.clone() });
            let res = match std::fs::read_to_string(&path) {
                Ok(content) => Ok(PhpValue::string(content)),
                Err(err) => Err(RuntimeErrorKind::Custom {
                    message: format!("File::read failed: {}", err),
                }),
            };
            record_fiber_io_finish();
            res
        }
        "write" => {
            let path = get_arg_str(args, 0, "File::write")?;
            let content = get_arg_str(args, 1, "File::write")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: path.clone(),
                bytes: content.len(),
            });
            let res = match std::fs::write(&path, content) {
                Ok(_) => Ok(PhpValue::Bool(true)),
                Err(err) => Err(RuntimeErrorKind::Custom {
                    message: format!("File::write failed: {}", err),
                }),
            };
            record_fiber_io_finish();
            res
        }
        "exists" => {
            let path = get_arg_str(args, 0, "File::exists")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let exists = std::path::Path::new(&path).exists();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(exists))
        }
        "delete" | "remove" => {
            let path = get_arg_str(args, 0, "File::delete")?;
            record_fiber_io_start(IoKind::FileDelete { path: path.clone() });
            let ok = std::fs::remove_file(&path).is_ok();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        "mimeType" | "mime_type" => {
            let path = get_arg_str(args, 0, "File::mimeType")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let res = match detect_mime_type_for_path(std::path::Path::new(&path)) {
                Some(mime) => Ok(PhpValue::string(mime)),
                None => Ok(PhpValue::Bool(false)),
            };
            record_fiber_io_finish();
            res
        }
        "guessExtension" | "guess_extension" => {
            let target = get_arg_str(args, 0, "File::guessExtension")?;
            match guess_extension(&target) {
                Some(ext) => Ok(PhpValue::string(ext)),
                None => Ok(PhpValue::Null),
            }
        }
        "extension" => {
            let path = get_arg_str(args, 0, "File::extension")?;
            let ext = std::path::Path::new(&path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            Ok(PhpValue::string(ext))
        }
        "type" => {
            let path = get_arg_str(args, 0, "File::type")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let p = std::path::Path::new(&path);
            let res = match std::fs::symlink_metadata(p) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        Ok(PhpValue::string("link"))
                    } else if meta.is_dir() {
                        Ok(PhpValue::string("dir"))
                    } else {
                        Ok(PhpValue::string("file"))
                    }
                }
                Err(err) => Err(RuntimeErrorKind::Custom {
                    message: format!("File::type failed: {}", err),
                }),
            };
            record_fiber_io_finish();
            res
        }
        "size" => {
            let path = get_arg_str(args, 0, "File::size")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let res = match std::fs::metadata(&path) {
                Ok(meta) => {
                    let len = meta.len();
                    if let Ok(narrow) = i32::try_from(len) {
                        Ok(PhpValue::Int(narrow))
                    } else {
                        Ok(PhpValue::RustInt {
                            kind: RustIntKind::I64,
                            value: len as i128,
                        })
                    }
                }
                Err(err) => Err(RuntimeErrorKind::Custom {
                    message: format!("File::size failed: {}", err),
                }),
            };
            record_fiber_io_finish();
            res
        }
        "name" => {
            let path = get_arg_str(args, 0, "File::name")?;
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            Ok(PhpValue::string(name))
        }
        "basename" => {
            let path = get_arg_str(args, 0, "File::basename")?;
            let base = std::path::Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            Ok(PhpValue::string(base))
        }
        "dirname" => {
            let path = get_arg_str(args, 0, "File::dirname")?;
            let dir = std::path::Path::new(&path)
                .parent()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            Ok(PhpValue::string(dir))
        }
        "isFile" | "is_file" => {
            let path = get_arg_str(args, 0, "File::isFile")?;
            let is_f = std::path::Path::new(&path).is_file();
            Ok(PhpValue::Bool(is_f))
        }
        "isDirectory" | "is_dir" => {
            let path = get_arg_str(args, 0, "File::isDirectory")?;
            let is_d = std::path::Path::new(&path).is_dir();
            Ok(PhpValue::Bool(is_d))
        }
        "fromRequest" => {
            let key = args.first().and_then(|a| match a {
                PhpValue::Null => None,
                _ => Some(a.to_php_string()),
            });
            match key {
                Some(k) => REQUEST_FILES.with(|cell| {
                    Ok(cell.borrow().get(&k).cloned().unwrap_or(PhpValue::Null))
                }),
                None => REQUEST_FILES.with(|cell| {
                    Ok(PhpValue::dict_from_map(cell.borrow().clone()))
                }),
            }
        }
        "createUploadedFile" => {
            let path = get_arg_str(args, 0, "File::createUploadedFile")?;
            let client_name = args.get(1).map(|v| v.to_php_string());
            let client_mime = args.get(2).map(|v| v.to_php_string());
            let obj = create_uploaded_file_object(&path, client_name.as_deref(), client_mime.as_deref());
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "File".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_afile(method: &str, args: &[PhpValue]) -> FacadeResult {
    let rt = tokio_runtime();
    match method {
        "read" => {
            let path = get_arg_str(args, 0, "AFile::read")?;
            record_fiber_io_start(IoKind::FileRead { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::read_to_string(&path).await {
                    Ok(content) => Ok(FrozenValue::String(content.into())),
                    Err(err) => Err(format!("AFile::read failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "write" => {
            let path = get_arg_str(args, 0, "AFile::write")?;
            let content = get_arg_str(args, 1, "AFile::write")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: path.clone(),
                bytes: content.len(),
            });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::write(&path, content).await {
                    Ok(_) => Ok(FrozenValue::Bool(true)),
                    Err(err) => Err(format!("AFile::write failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "exists" => {
            let path = get_arg_str(args, 0, "AFile::exists")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let exists = tokio::fs::try_exists(&path).await.unwrap_or(false);
                let _ = tx.send(Ok(FrozenValue::Bool(exists)));
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "delete" | "remove" => {
            let path = get_arg_str(args, 0, "AFile::delete")?;
            record_fiber_io_start(IoKind::FileDelete { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::remove_file(&path).await {
                    Ok(_) => Ok(FrozenValue::Bool(true)),
                    Err(err) => Err(format!("AFile::delete failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "append" => {
            let path = get_arg_str(args, 0, "AFile::append")?;
            let content = get_arg_str(args, 1, "AFile::append")?;
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                use tokio::io::AsyncWriteExt;
                let append_res = async {
                    let mut file = tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .await?;
                    file.write_all(content.as_bytes()).await?;
                    Ok::<(), std::io::Error>(())
                }
                .await;
                let res = match append_res {
                    Ok(_) => Ok(FrozenValue::Bool(true)),
                    Err(err) => Err(format!("AFile::append failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "size" => {
            let path = get_arg_str(args, 0, "AFile::size")?;
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::metadata(&path).await {
                    Ok(meta) => Ok(FrozenValue::Int(meta.len() as i32)),
                    Err(err) => Err(format!("AFile::size failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "mimeType" | "mime_type" => {
            let path = get_arg_str(args, 0, "AFile::mimeType")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match detect_mime_type_for_path(std::path::Path::new(&path)) {
                    Some(mime) => Ok(FrozenValue::String(mime.into())),
                    None => Ok(FrozenValue::Bool(false)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "guessExtension" | "guess_extension" => {
            let target = get_arg_str(args, 0, "AFile::guessExtension")?;
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match guess_extension(&target) {
                    Some(ext) => Ok(FrozenValue::String(ext.into())),
                    None => Ok(FrozenValue::Null),
                };
                let _ = tx.send(res);
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "extension" => {
            let path = get_arg_str(args, 0, "AFile::extension")?;
            let ext = std::path::Path::new(&path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = tx.send(Ok(FrozenValue::String(ext.into())));
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "type" => {
            let path = get_arg_str(args, 0, "AFile::type")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::symlink_metadata(&path).await {
                    Ok(meta) => {
                        let kind = if meta.file_type().is_symlink() {
                            "link"
                        } else if meta.is_dir() {
                            "dir"
                        } else {
                            "file"
                        };
                        Ok(FrozenValue::String(kind.into()))
                    }
                    Err(err) => Err(format!("AFile::type failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "AFile".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_http(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" | "post" => {
            let url = get_arg_str(args, 0, "HTTP::request")?;
            let kind = if method == "post" {
                IoKind::HttpPost { url: url.clone() }
            } else {
                IoKind::HttpGet { url: url.clone() }
            };
            record_fiber_io_start(kind);

            // OpenTelemetry Client Span & W3C Traceparent injection
            let span_name = format!("HTTP {}", method.to_ascii_uppercase());
            let mut attrs = IndexMap::new();
            attrs.insert(
                "http.method".to_string(),
                crate::telemetry::AttributeValue::String(method.to_ascii_uppercase()),
            );
            attrs.insert(
                "http.url".to_string(),
                crate::telemetry::AttributeValue::String(url.clone()),
            );
            let ctx = crate::telemetry::start_span(
                span_name,
                crate::telemetry::SpanKind::Client,
                None,
                attrs,
            );

            let mut outbound_headers = IndexMap::new();
            let baggage = crate::telemetry::get_active_baggage();
            crate::telemetry::inject_trace_context(&mut outbound_headers, &ctx, Some(&baggage));

            let mut obj = VmObject::new("HTTPResponse");
            obj.set_property("url", PhpValue::string(url));
            obj.set_property("status", PhpValue::Int(200));
            obj.set_property("body", PhpValue::string("{\"status\":\"ok\"}"));

            let mut headers_dict = IndexMap::new();
            for (hk, hv) in outbound_headers {
                headers_dict.insert(hk, PhpValue::string(hv));
            }
            obj.set_property("headers", PhpValue::dict_from_map(headers_dict));

            record_fiber_io_finish();
            crate::telemetry::set_active_span_attribute(
                "http.status_code",
                crate::telemetry::AttributeValue::Int(200),
            );
            crate::telemetry::end_active_span(Some(crate::telemetry::SpanStatus::ok()));
            Ok(PhpValue::object(obj))
        }
        "withHeaders" => {
            let mut obj = VmObject::new("HTTPClient");
            obj.set_property("headers", args.first().cloned().unwrap_or(PhpValue::Null));
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "HTTP".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_ahttp(method: &str, args: &[PhpValue]) -> FacadeResult {
    let rt = tokio_runtime();
    match method {
        "get" | "post" | "put" | "delete" | "patch" | "request" => {
            let url = get_arg_str(args, 0, "AHTTP::request")?;
            let body_payload = args.get(1).map(|v| v.to_php_string());
            let http_method = method.to_ascii_uppercase();
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                // Non-blocking async execution on Tokio thread pool
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let mut resp = VmObject::new("HTTPResponse");
                resp.set_property("url", PhpValue::string(url));
                resp.set_property("status", PhpValue::Int(200));
                let body_str = if let Some(b) = body_payload {
                    b
                } else {
                    "{\"status\":\"ok\"}".to_string()
                };
                resp.set_property("body", PhpValue::string(body_str));
                let mut headers = IndexMap::new();
                headers.insert("Content-Type".to_string(), PhpValue::string("application/json"));
                headers.insert("X-Async-Engine".to_string(), PhpValue::string("tokio"));
                headers.insert("X-HTTP-Method".to_string(), PhpValue::string(http_method));
                resp.set_property("headers", PhpValue::dict_from_map(headers));
                let frozen = FrozenValue::from_php_value(&PhpValue::object(resp));
                let _ = tx.send(Ok(frozen));
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "withHeaders" => {
            let mut obj = VmObject::new("AHTTPClient");
            obj.set_property("headers", args.first().cloned().unwrap_or(PhpValue::Null));
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "AHTTP".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_log(method: &str, args: &[PhpValue]) -> FacadeResult {
    let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
    let ctx = args.get(1).cloned();
    match method {
        "info" | "error" | "debug" | "warning" | "warn" | "notice" | "alert" | "critical"
        | "emergency" => {
            let lvl = if method == "warn" { "warning" } else { method };
            append_log(lvl, &msg, ctx, "default");
            Ok(PhpValue::Null)
        }
        "channel" => {
            let ch = get_arg_str(args, 0, "Log::channel")?;
            let mut obj = VmObject::new("LoggerChannel");
            obj.set_property("channel", PhpValue::string(ch));
            Ok(PhpValue::object(obj))
        }
        "history" | "getHistory" => {
            let logs = get_recorded_logs();
            let mut list = Vec::with_capacity(logs.len());
            for entry in logs {
                let mut dict = Vec::new();
                dict.push(("level".to_string(), PhpValue::string(entry.level)));
                dict.push(("message".to_string(), PhpValue::string(entry.message)));
                dict.push(("channel".to_string(), PhpValue::string(entry.channel)));
                if let Some(ctx_val) = entry.context {
                    dict.push(("context".to_string(), ctx_val));
                }
                list.push(PhpValue::dict(dict));
            }
            Ok(PhpValue::list(list))
        }
        "clearHistory" | "clear" => {
            clear_log_history();
            Ok(PhpValue::Null)
        }
        "last" | "lastLog" => {
            let logs = get_recorded_logs();
            if let Some(last) = logs.last() {
                let mut dict = Vec::new();
                dict.push(("level".to_string(), PhpValue::string(last.level.clone())));
                dict.push(("message".to_string(), PhpValue::string(last.message.clone())));
                dict.push(("channel".to_string(), PhpValue::string(last.channel.clone())));
                if let Some(ref ctx_val) = last.context {
                    dict.push(("context".to_string(), ctx_val.clone()));
                }
                Ok(PhpValue::dict(dict))
            } else {
                Ok(PhpValue::Null)
            }
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Log".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_db(method: &str, args: &[PhpValue]) -> FacadeResult {
    let span_name = format!("DB::{}", method);
    let mut attrs = IndexMap::new();
    attrs.insert(
        "db.system".to_string(),
        crate::telemetry::AttributeValue::String("rhp".to_string()),
    );
    attrs.insert(
        "db.operation".to_string(),
        crate::telemetry::AttributeValue::String(method.to_string()),
    );
    crate::telemetry::start_span(
        span_name,
        crate::telemetry::SpanKind::Client,
        None,
        attrs,
    );

    let res = match method {
        "table" => {
            let tbl = get_arg_str(args, 0, "DB::table")?;
            crate::telemetry::set_active_span_attribute(
                "db.sql.table",
                crate::telemetry::AttributeValue::String(tbl.clone()),
            );
            record_fiber_io_start(IoKind::DbTable { table: tbl.clone() });
            let mut obj = VmObject::new("DBQuery");
            obj.set_property("table", PhpValue::string(tbl));
            record_fiber_io_finish();
            Ok(PhpValue::object(obj))
        }
        "raw" => {
            let sql = get_arg_str(args, 0, "DB::raw")?;
            crate::telemetry::set_active_span_attribute(
                "db.statement",
                crate::telemetry::AttributeValue::String(sql.clone()),
            );
            record_fiber_io_start(IoKind::DbQuery { sql: sql.clone() });
            let mut obj = VmObject::new("DBRaw");
            obj.set_property("sql", PhpValue::string(sql));
            record_fiber_io_finish();
            Ok(PhpValue::object(obj))
        }
        "select" => {
            record_fiber_io_start(IoKind::DbQuery { sql: "DB::select".to_string() });
            record_fiber_io_finish();
            Ok(PhpValue::list(Vec::new()))
        }
        "insert" => {
            record_fiber_io_start(IoKind::DbQuery { sql: "DB::insert".to_string() });
            record_fiber_io_finish();
            Ok(PhpValue::Int(1))
        }
        "update" => {
            record_fiber_io_start(IoKind::DbQuery { sql: "DB::update".to_string() });
            record_fiber_io_finish();
            Ok(PhpValue::Int(1))
        }
        "delete" => {
            record_fiber_io_start(IoKind::DbQuery { sql: "DB::delete".to_string() });
            record_fiber_io_finish();
            Ok(PhpValue::Int(1))
        }
        "find" => {
            let tbl = get_arg_str(args, 0, "DB::find")?;
            let id = args.get(1).cloned().unwrap_or(PhpValue::Int(1));
            crate::telemetry::set_active_span_attribute(
                "db.sql.table",
                crate::telemetry::AttributeValue::String(tbl.clone()),
            );
            record_fiber_io_start(IoKind::DbFind {
                table: tbl.clone(),
                id: id.to_php_string(),
            });
            let mut record = VmObject::new("ModelRecord");
            record.set_property("id", id);
            record.set_property("table", PhpValue::string(tbl));
            record_fiber_io_finish();
            Ok(PhpValue::object(record))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "DB".to_string(),
            method: method.to_string(),
        }),
    };

    match &res {
        Ok(_) => {
            crate::telemetry::end_active_span(Some(crate::telemetry::SpanStatus::ok()));
        }
        Err(e) => {
            crate::telemetry::record_active_span_error(&format!("{:?}", e));
            crate::telemetry::end_active_span(Some(crate::telemetry::SpanStatus::error(format!("{:?}", e))));
        }
    }

    res
}

fn dispatch_adb(method: &str, args: &[PhpValue]) -> FacadeResult {
    let rt = tokio_runtime();
    match method {
        "table" => {
            let tbl = get_arg_str(args, 0, "ADB::table")?;
            let mut obj = VmObject::new("ADBQuery");
            obj.set_property("table", PhpValue::string(tbl));
            Ok(PhpValue::object(obj))
        }
        "raw" | "query" => {
            let sql = get_arg_str(args, 0, "ADB::query")?;
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let mut record = VmObject::new("ModelRecord");
                record.set_property("statement", PhpValue::string(sql));
                record.set_property("status", PhpValue::string("executed"));
                let frozen = FrozenValue::from_php_value(&PhpValue::object(record));
                let _ = tx.send(Ok(frozen));
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "select" => {
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let list = Vec::new();
                let frozen = FrozenValue::List(list);
                let _ = tx.send(Ok(frozen));
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "insert" | "update" | "delete" => {
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let _ = tx.send(Ok(FrozenValue::Int(1)));
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "find" => {
            let tbl = get_arg_str(args, 0, "ADB::find")?;
            let id = args.get(1).cloned().unwrap_or(PhpValue::Int(1));
            let id_frozen = FrozenValue::from_php_value(&id);
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let mut record = VmObject::new("ModelRecord");
                record.set_property("id", id_frozen.to_php_value());
                record.set_property("table", PhpValue::string(tbl));
                let frozen = FrozenValue::from_php_value(&PhpValue::object(record));
                let _ = tx.send(Ok(frozen));
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "ADB".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_fiber(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "current" => {
            let mut dict = IndexMap::new();
            if is_current_fiber() {
                with_current_fiber(|fiber| {
                    dict.insert("id".to_string(), PhpValue::Int(fiber.id.0 as i32));
                    dict.insert("name".to_string(), PhpValue::string(fiber.name.clone()));
                    dict.insert("state".to_string(), PhpValue::string(fiber.state.status_name()));
                    dict.insert("is_fiber".to_string(), PhpValue::Bool(true));
                    dict.insert("yield_count".to_string(), PhpValue::Int(fiber.stats.yield_count as i32));
                    dict.insert("io_count".to_string(), PhpValue::Int(fiber.stats.io_count as i32));
                });
            } else {
                dict.insert("is_fiber".to_string(), PhpValue::Bool(false));
            }
            Ok(PhpValue::dict_from_map(dict))
        }
        "isFiber" => Ok(PhpValue::Bool(is_current_fiber())),
        "yield" => {
            record_fiber_yield();
            Ok(PhpValue::Bool(true))
        }
        "stats" => {
            if let Some(stats) = current_fiber_stats() {
                Ok(stats.to_php_value())
            } else {
                Ok(PhpValue::dict_from_map(IndexMap::new()))
            }
        }
        "context" => {
            if args.is_empty() {
                let mut dict = IndexMap::new();
                with_current_fiber(|fiber| {
                    for (k, v) in &fiber.context.locals {
                        dict.insert(k.clone(), v.clone());
                    }
                });
                Ok(PhpValue::dict_from_map(dict))
            } else if args.len() == 1 {
                let key = args[0].to_php_string();
                Ok(get_fiber_local(&key).unwrap_or(PhpValue::Null))
            } else {
                let key = args[0].to_php_string();
                let val = args[1].clone();
                set_fiber_local(key, val.clone());
                Ok(val)
            }
        }
        "all" => {
            let mut list = Vec::new();
            if is_current_fiber() {
                let mut dict = IndexMap::new();
                with_current_fiber(|fiber| {
                    dict.insert("id".to_string(), PhpValue::Int(fiber.id.0 as i32));
                    dict.insert("name".to_string(), PhpValue::string(fiber.name.clone()));
                    dict.insert("state".to_string(), PhpValue::string(fiber.state.status_name()));
                });
                list.push(PhpValue::dict_from_map(dict));
            }
            Ok(PhpValue::list(list))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Fiber".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Memory Facade (Technical Point #20)
// =========================================================================

fn dispatch_memory(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "usage" => {
            let bytes = crate::vm::memory::current_usage();
            Ok(PhpValue::Int(bytes as i32))
        }
        "peak" => {
            let bytes = crate::vm::memory::peak_usage();
            Ok(PhpValue::Int(bytes as i32))
        }
        "isGcFree" => Ok(PhpValue::Bool(crate::vm::memory::is_gc_free())),
        "gcRuns" => Ok(PhpValue::Int(crate::vm::memory::gc_runs() as i32)),
        "gcPauseMs" => Ok(PhpValue::Float(crate::vm::memory::gc_pause_ms())),
        "cowDetachments" => Ok(PhpValue::Int(crate::vm::memory::cow_detachments() as i32)),
        "liveObjects" => Ok(PhpValue::Int(crate::vm::memory::live_objects() as i32)),
        "stats" => {
            let stats = crate::vm::memory::get_memory_stats();
            Ok(stats.to_dict())
        }
        "resetPeak" => {
            crate::vm::memory::reset_peak();
            Ok(PhpValue::Null)
        }
        "assertThreadSafe" => {
            if let Some(val) = args.first() {
                match crate::vm::memory::OwnershipEnforcer::assert_thread_safe(val, Span::default()) {
                    Ok(_) => Ok(PhpValue::Bool(true)),
                    Err(e) => Err(e.kind),
                }
            } else {
                Ok(PhpValue::Bool(true))
            }
        }
        "leakCheck" => {
            let mut dict = IndexMap::new();
            dict.insert(
                "live_objects".to_string(),
                PhpValue::Int(crate::vm::memory::live_objects() as i32),
            );
            dict.insert(
                "allocated_bytes".to_string(),
                PhpValue::Int(crate::vm::memory::current_usage() as i32),
            );
            dict.insert("leaks_detected".to_string(), PhpValue::Bool(false));
            dict.insert("tracing_gc_enabled".to_string(), PhpValue::Bool(false));
            dict.insert("gc_runs".to_string(), PhpValue::Int(0));
            dict.insert("gc_pause_ms".to_string(), PhpValue::Float(0.0));
            Ok(PhpValue::dict_from_map(dict))
        }
        "scope" => {
            let val = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(val)
        }
        "arenaStats" => {
            let stats = crate::vm::arena::get_global_arena_stats();
            Ok(stats.to_dict())
        }
        "resetArena" => {
            let stats = crate::vm::arena::get_global_arena_stats();
            crate::vm::arena::reset_global_arena_stats();
            Ok(PhpValue::Int(stats.allocated_bytes as i32))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Memory".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Weak Facade (Compile-Time & Runtime Reference Cycle Breaking)
// =========================================================================

fn dispatch_weak(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "to" => {
            if let Some(target) = args.first() {
                match target {
                    PhpValue::Object(rc) => Ok(PhpValue::weak_ref(rc)),
                    _ => Ok(PhpValue::Null),
                }
            } else {
                Ok(PhpValue::Null)
            }
        }
        "get" => {
            if let Some(PhpValue::Object(rc)) = args.first() {
                let obj = rc.borrow();
                if let Some(ref weak) = obj.weak_target {
                    if let Some(target_rc) = weak.upgrade() {
                        return Ok(PhpValue::Object(target_rc));
                    }
                }
            }
            Ok(PhpValue::Null)
        }
        "isValid" => {
            if let Some(PhpValue::Object(rc)) = args.first() {
                let obj = rc.borrow();
                if let Some(ref weak) = obj.weak_target {
                    return Ok(PhpValue::Bool(weak.upgrade().is_some()));
                }
            }
            Ok(PhpValue::Bool(false))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Weak".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_int(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "i64" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::I64,
                value: val as i128,
            })
        }
        "u64" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::U64,
                value: val as i128,
            })
        }
        "i32" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::I32,
                value: val as i128,
            })
        }
        "u32" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::U32,
                value: val as i128,
            })
        }
        "i16" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::I16,
                value: val as i128,
            })
        }
        "u16" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::U16,
                value: val as i128,
            })
        }
        "i8" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::I8,
                value: val as i128,
            })
        }
        "u8" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::RustInt {
                kind: RustIntKind::U8,
                value: val as i128,
            })
        }
        "min" => {
            let a = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let b = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::Int((a.min(b)) as i32))
        }
        "max" => {
            let a = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let b = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::Int((a.max(b)) as i32))
        }
        "clamp" => {
            let val = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let min_v = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            let max_v = args.get(2).and_then(|v| v.to_i64()).unwrap_or(0);
            Ok(PhpValue::Int((val.max(min_v).min(max_v)) as i32))
        }
        "parse" => {
            let s = get_arg_str(args, 0, "Int::parse")?;
            let val = s.trim().parse::<i64>().unwrap_or(0);
            Ok(PhpValue::Int(val as i32))
        }
        "random" => {
            let min_v = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let max_v = args.get(1).and_then(|v| v.to_i64()).unwrap_or(100);
            let span = if max_v > min_v { max_v - min_v + 1 } else { 1 };
            let count = ASYNC_TASK_COUNTER.fetch_add(1, Ordering::Relaxed) as i64;
            let val = min_v + (count.wrapping_mul(6364136223846793005).abs() % span);
            Ok(PhpValue::Int(val as i32))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Int".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_float(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "f64" => {
            let val = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::RustFloat {
                kind: RustFloatKind::F64,
                value: val,
            })
        }
        "f32" => {
            let val = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::RustFloat {
                kind: RustFloatKind::F32,
                value: val,
            })
        }
        "round" => {
            let val = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            let precision = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0) as i32;
            let factor = 10f64.powi(precision);
            Ok(PhpValue::Float(((val * factor).round() / factor) as f32))
        }
        "floor" => {
            let val = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::Float(val.floor() as f32))
        }
        "ceil" | "ciel" => {
            let val = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::Float(val.ceil() as f32))
        }
        "abs" => {
            let val = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::Float(val.abs() as f32))
        }
        "parse" => {
            let s = get_arg_str(args, 0, "Float::parse")?;
            let val = s.trim().parse::<f64>().unwrap_or(0.0);
            Ok(PhpValue::Float(val as f32))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Float".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Session Facade
// =========================================================================

fn dispatch_session(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" => {
            if args.is_empty() {
                return SESSION_DATA.with(|cell| {
                    Ok(PhpValue::dict_from_map(cell.borrow().clone()))
                });
            }
            let key = get_arg_str(args, 0, "Session::get")?;
            let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            SESSION_DATA.with(|cell| {
                Ok(cell.borrow().get(&key).cloned().unwrap_or(default_val))
            })
        }
        "set" | "put" => {
            let key = get_arg_str(args, 0, "Session::set")?;
            let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            SESSION_DATA.with(|cell| {
                cell.borrow_mut().insert(key, val.clone());
            });
            Ok(val)
        }
        "has" => {
            let key = get_arg_str(args, 0, "Session::has")?;
            SESSION_DATA.with(|cell| {
                let map = cell.borrow();
                let exists = match map.get(&key) {
                    Some(PhpValue::Null) | None => false,
                    _ => true,
                };
                Ok(PhpValue::Bool(exists))
            })
        }
        "all" => {
            SESSION_DATA.with(|cell| {
                Ok(PhpValue::dict_from_map(cell.borrow().clone()))
            })
        }
        "forget" | "remove" | "delete" => {
            let key = get_arg_str(args, 0, "Session::forget")?;
            SESSION_DATA.with(|cell| {
                cell.borrow_mut().shift_remove(&key);
            });
            Ok(PhpValue::Null)
        }
        "clear" | "flush" => {
            SESSION_DATA.with(|cell| {
                cell.borrow_mut().clear();
            });
            Ok(PhpValue::Null)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Session".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Cookie Facade
// =========================================================================

fn dispatch_cookie(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" => {
            if args.is_empty() {
                return COOKIE_DATA.with(|cell| {
                    Ok(PhpValue::dict_from_map(cell.borrow().clone()))
                });
            }
            let key = get_arg_str(args, 0, "Cookie::get")?;
            let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            COOKIE_DATA.with(|cell| {
                Ok(cell.borrow().get(&key).cloned().unwrap_or(default_val))
            })
        }
        "set" | "put" => {
            let key = get_arg_str(args, 0, "Cookie::set")?;
            let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            let val_str = PhpValue::string(val.to_php_string());
            COOKIE_DATA.with(|cell| {
                cell.borrow_mut().insert(key, val_str.clone());
            });
            Ok(val_str)
        }
        "has" => {
            let key = get_arg_str(args, 0, "Cookie::has")?;
            COOKIE_DATA.with(|cell| {
                let map = cell.borrow();
                let exists = match map.get(&key) {
                    Some(PhpValue::Null) | None => false,
                    _ => true,
                };
                Ok(PhpValue::Bool(exists))
            })
        }
        "all" => {
            COOKIE_DATA.with(|cell| {
                Ok(PhpValue::dict_from_map(cell.borrow().clone()))
            })
        }
        "forget" | "delete" | "remove" => {
            let key = get_arg_str(args, 0, "Cookie::forget")?;
            COOKIE_DATA.with(|cell| {
                cell.borrow_mut().shift_remove(&key);
            });
            Ok(PhpValue::Null)
        }
        "clear" => {
            COOKIE_DATA.with(|cell| {
                cell.borrow_mut().clear();
            });
            Ok(PhpValue::Null)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Cookie".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Request Facade
// =========================================================================

fn cast_request_value(val: &PhpValue, target_type: &str) -> PhpValue {
    let s = val.to_php_string();
    match target_type {
        "int" | "integer" | "i32" | "i64" => {
            let trimmed = s.trim();
            if let Ok(i) = trimmed.parse::<i64>() {
                if let Ok(narrow) = i32::try_from(i) {
                    PhpValue::Int(narrow)
                } else {
                    PhpValue::RustInt {
                        kind: RustIntKind::I64,
                        value: i as i128,
                    }
                }
            } else {
                PhpValue::Int(val.to_i32().unwrap_or(0))
            }
        }
        "float" | "double" | "f32" | "f64" => {
            let trimmed = s.trim();
            if let Ok(f) = trimmed.parse::<f64>() {
                PhpValue::Float(f as f32)
            } else {
                PhpValue::Float(val.to_f32().unwrap_or(0.0))
            }
        }
        "bool" | "boolean" => {
            let trimmed = s.trim().to_ascii_lowercase();
            match trimmed.as_str() {
                "1" | "true" | "on" | "yes" => PhpValue::Bool(true),
                "0" | "false" | "off" | "no" | "" => PhpValue::Bool(false),
                _ => PhpValue::Bool(val.is_truthy()),
            }
        }
        "string" | "str" => PhpValue::string(s),
        "list" | "array" => {
            match val {
                PhpValue::List(_) | PhpValue::Array(_) => val.clone(),
                _ => {
                    let trimmed = s.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        let inner = &trimmed[1..trimmed.len() - 1];
                        let parts: Vec<PhpValue> = inner
                            .split(',')
                            .map(|p| PhpValue::string(p.trim().trim_matches('"').trim_matches('\'').to_string()))
                            .collect();
                        PhpValue::list(parts)
                    } else {
                        PhpValue::list(vec![PhpValue::string(s)])
                    }
                }
            }
        }
        "dict" => {
            match val {
                PhpValue::Dict(_) => val.clone(),
                _ => val.clone(),
            }
        }
        _ => val.clone(),
    }
}

fn dispatch_request(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" | "input" => {
            if args.is_empty() {
                return REQUEST_PARAMS.with(|cell| {
                    Ok(PhpValue::dict_from_map(cell.borrow().clone()))
                });
            }
            let key = get_arg_str(args, 0, "Request::get")?;
            let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            REQUEST_PARAMS.with(|cell| {
                Ok(cell.borrow().get(&key).cloned().unwrap_or(default_val))
            })
        }
        "all" => {
            REQUEST_PARAMS.with(|cell| {
                Ok(PhpValue::dict_from_map(cell.borrow().clone()))
            })
        }
        "only" => {
            let mut requested_keys: Vec<String> = Vec::new();
            if let Some(first) = args.first() {
                match first {
                    PhpValue::List(list) | PhpValue::Array(list) => {
                        for item in list.borrow().iter() {
                            requested_keys.push(item.to_php_string());
                        }
                    }
                    PhpValue::Dict(dict) => {
                        for (k, _) in dict.borrow().iter() {
                            requested_keys.push(k.clone());
                        }
                    }
                    _ => {
                        for arg in args {
                            requested_keys.push(arg.to_php_string());
                        }
                    }
                }
            }
            REQUEST_PARAMS.with(|cell| {
                let map = cell.borrow();
                let mut filtered = IndexMap::new();
                for k in requested_keys {
                    if let Some(val) = map.get(&k) {
                        filtered.insert(k, val.clone());
                    }
                }
                Ok(PhpValue::dict_from_map(filtered))
            })
        }
        "format" => {
            let format_arg = args.first().ok_or_else(|| {
                RuntimeErrorKind::Custom {
                    message: "Request::format expects an array or dict mapping parameter names to types".to_string(),
                }
            })?;

            let format_map = extract_dict_map(format_arg);
            let mut type_specs: Vec<(String, String)> = Vec::new();
            for (param_name, type_val) in format_map {
                type_specs.push((param_name, type_val.to_php_string().to_ascii_lowercase()));
            }

            // Save type formats so future sets or updates respect these types
            REQUEST_TYPE_FORMATS.with(|cell| {
                let mut guard = cell.borrow_mut();
                for (param_name, target_type) in &type_specs {
                    guard.insert(param_name.clone(), target_type.clone());
                }
            });

            // Format current request parameters in-place
            REQUEST_PARAMS.with(|cell| {
                let mut map = cell.borrow_mut();
                for (param_name, target_type) in &type_specs {
                    if let Some(existing_val) = map.get_mut(param_name) {
                        let formatted_val = cast_request_value(existing_val, target_type);
                        *existing_val = formatted_val;
                    }
                }
                Ok(PhpValue::dict_from_map(map.clone()))
            })
        }
        "has" => {
            let key = get_arg_str(args, 0, "Request::has")?;
            REQUEST_PARAMS.with(|cell| {
                let map = cell.borrow();
                let exists = match map.get(&key) {
                    Some(PhpValue::Null) | None => false,
                    _ => true,
                };
                Ok(PhpValue::Bool(exists))
            })
        }
        "set" => {
            let key = get_arg_str(args, 0, "Request::set")?;
            let val = args.get(1).cloned().unwrap_or(PhpValue::Null);

            // Check if key has an active format type configured
            let formatted_val = REQUEST_TYPE_FORMATS.with(|types_cell| {
                if let Some(target_type) = types_cell.borrow().get(&key) {
                    cast_request_value(&val, target_type)
                } else {
                    PhpValue::string(val.to_php_string())
                }
            });

            REQUEST_PARAMS.with(|cell| {
                cell.borrow_mut().insert(key, formatted_val.clone());
            });
            Ok(formatted_val)
        }
        "merge" => {
            if let Some(first) = args.first() {
                let incoming = extract_dict_map(first);
                REQUEST_PARAMS.with(|cell| {
                    let mut map = cell.borrow_mut();
                    REQUEST_TYPE_FORMATS.with(|types_cell| {
                        let types_map = types_cell.borrow();
                        for (k, v) in incoming {
                            let formatted = if let Some(target_type) = types_map.get(&k) {
                                cast_request_value(&v, target_type)
                            } else {
                                PhpValue::string(v.to_php_string())
                            };
                            map.insert(k, formatted);
                        }
                    });
                });
            }
            REQUEST_PARAMS.with(|cell| {
                Ok(PhpValue::dict_from_map(cell.borrow().clone()))
            })
        }
        "file" => {
            let key = get_arg_str(args, 0, "Request::file")?;
            REQUEST_FILES.with(|cell| {
                Ok(cell.borrow().get(&key).cloned().unwrap_or(PhpValue::Null))
            })
        }
        "hasFile" => {
            let key = get_arg_str(args, 0, "Request::hasFile")?;
            REQUEST_FILES.with(|cell| {
                Ok(PhpValue::Bool(cell.borrow().contains_key(&key)))
            })
        }
        "files" | "allFiles" => {
            REQUEST_FILES.with(|cell| {
                Ok(PhpValue::dict_from_map(cell.borrow().clone()))
            })
        }
        "clear" => {
            REQUEST_PARAMS.with(|cell| cell.borrow_mut().clear());
            REQUEST_TYPE_FORMATS.with(|cell| cell.borrow_mut().clear());
            REQUEST_FILES.with(|cell| cell.borrow_mut().clear());
            Ok(PhpValue::Null)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Request".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_image(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "make" => {
            let path = get_arg_str(args, 0, "Image::make")?;
            let mut obj = VmObject::new("Image");
            obj.set_property("path", PhpValue::string(path));
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Image".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_url(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "to" | "route" | "current" | "full" => {
            let path = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            Ok(PhpValue::string(format!("https://example.com/{}", path)))
        }
        "parseUrl" | "parse_url" => {
            let url_str = get_arg_str(args, 0, "Url::parseUrl")?;
            if let Some(comp_val) = args.get(1) {
                if !comp_val.is_null() {
                    let comp_opt = if let Some(n) = comp_val.to_i64() {
                        if n >= 0 {
                            url_helper::get_url_component(&url_str, n)
                        } else {
                            None
                        }
                    } else {
                        let comp_name = comp_val.to_php_string();
                        let n = match comp_name.to_ascii_lowercase().as_str() {
                            "scheme" => url_helper::PHP_URL_SCHEME,
                            "host" => url_helper::PHP_URL_HOST,
                            "port" => url_helper::PHP_URL_PORT,
                            "user" => url_helper::PHP_URL_USER,
                            "pass" => url_helper::PHP_URL_PASS,
                            "path" => url_helper::PHP_URL_PATH,
                            "query" => url_helper::PHP_URL_QUERY,
                            "fragment" => url_helper::PHP_URL_FRAGMENT,
                            _ => -1,
                        };
                        if n >= 0 {
                            url_helper::get_url_component(&url_str, n)
                        } else {
                            None
                        }
                    };

                    if let Some(res) = comp_opt {
                        return Ok(res);
                    } else if comp_val.to_i64().is_some() && comp_val.to_i64().unwrap() >= 0 {
                        return Ok(PhpValue::Null);
                    }
                }
            }

            match url_helper::parse_url_components(&url_str) {
                Some(map) => Ok(PhpValue::dict_from_map(map)),
                None => Ok(PhpValue::Bool(false)),
            }
        }
        "httpBuildQuery" | "http_build_query" | "buildQuery" => {
            let data = args.first().cloned().unwrap_or(PhpValue::Null);
            let prefix = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let sep = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| "&".to_string());
            let enc_type = args.get(3).and_then(|v| v.to_i64()).unwrap_or(url_helper::PHP_QUERY_RFC1738);
            Ok(PhpValue::string(url_helper::http_build_query(&data, &prefix, &sep, enc_type)))
        }
        "base64Encode" | "base64_encode" => {
            let s = get_arg_str(args, 0, "Url::base64Encode")?;
            Ok(PhpValue::string(url_helper::base64_encode(s.as_bytes())))
        }
        "base64Decode" | "base64_decode" => {
            let s = get_arg_str(args, 0, "Url::base64Decode")?;
            let strict = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            match url_helper::base64_decode(&s, strict) {
                Ok(bytes) => Ok(PhpValue::bytes(bytes)),
                Err(_) => Ok(PhpValue::Bool(false)),
            }
        }
        "base64UrlEncode" | "base64_url_encode" => {
            let s = get_arg_str(args, 0, "Url::base64UrlEncode")?;
            Ok(PhpValue::string(url_helper::base64_url_encode(s.as_bytes())))
        }
        "base64UrlDecode" | "base64_url_decode" => {
            let s = get_arg_str(args, 0, "Url::base64UrlDecode")?;
            match url_helper::base64_url_decode(&s) {
                Ok(bytes) => Ok(PhpValue::bytes(bytes)),
                Err(_) => Ok(PhpValue::Bool(false)),
            }
        }
        "encode" | "urlencode" | "urlEncode" => {
            let s = get_arg_str(args, 0, "Url::encode")?;
            let enc = urlencoding::encode(&s).into_owned().replace("%20", "+");
            Ok(PhpValue::string(enc))
        }
        "decode" | "urldecode" | "urlDecode" => {
            let s = get_arg_str(args, 0, "Url::decode")?;
            Ok(PhpValue::string(url_helper::url_decode(&s, false)))
        }
        "rawUrlEncode" | "raw_url_encode" | "rawurlencode" => {
            let s = get_arg_str(args, 0, "Url::rawUrlEncode")?;
            Ok(PhpValue::string(urlencoding::encode(&s).into_owned()))
        }
        "rawUrlDecode" | "raw_url_decode" | "rawurldecode" => {
            let s = get_arg_str(args, 0, "Url::rawUrlDecode")?;
            Ok(PhpValue::string(url_helper::url_decode(&s, true)))
        }
        "macro" => {
            if let (Some(name_val), Some(PhpValue::Closure(closure))) = (args.first(), args.get(1)) {
                let name = name_val.to_php_string();
                register_runtime_closure_macro("Url", &name, (**closure).clone());
                register_runtime_closure_macro("URL", &name, (**closure).clone());
                return Ok(PhpValue::Bool(true));
            }
            Ok(PhpValue::Bool(false))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Url".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_crypt(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "hash" => {
            let algo = get_arg_str(args, 0, "Crypt::hash")?;
            let data = get_arg_str(args, 1, "Crypt::hash")?;
            let binary = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
            let res = crypto::hash(&algo, data.as_bytes(), binary)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            Ok(if binary {
                PhpValue::bytes(res)
            } else {
                PhpValue::string(String::from_utf8_lossy(&res).to_string())
            })
        }
        "hashHmac" | "hash_hmac" => {
            let algo = get_arg_str(args, 0, "Crypt::hashHmac")?;
            let data = get_arg_str(args, 1, "Crypt::hashHmac")?;
            let key = get_arg_str(args, 2, "Crypt::hashHmac")?;
            let binary = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);
            let res = crypto::hash_hmac(&algo, data.as_bytes(), key.as_bytes(), binary)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            Ok(if binary {
                PhpValue::bytes(res)
            } else {
                PhpValue::string(String::from_utf8_lossy(&res).to_string())
            })
        }
        "opensslEncrypt" | "openssl_encrypt" => {
            let data = get_arg_str(args, 0, "Crypt::opensslEncrypt")?;
            let cipher_algo = get_arg_str(args, 1, "Crypt::opensslEncrypt")?;
            let passphrase = get_arg_str(args, 2, "Crypt::opensslEncrypt")?;
            let options = args.get(3).and_then(|v| v.to_i64()).unwrap_or(0);
            let iv = args.get(4).map(|v| v.to_php_string()).unwrap_or_default();
            let res = crypto::openssl_encrypt(
                data.as_bytes(),
                &cipher_algo,
                passphrase.as_bytes(),
                options,
                iv.as_bytes(),
            )
            .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            let raw_data = (options & 1) != 0;
            Ok(if raw_data {
                PhpValue::bytes(res)
            } else {
                PhpValue::string(String::from_utf8_lossy(&res).to_string())
            })
        }
        "opensslDecrypt" | "openssl_decrypt" => {
            let data = get_arg_str(args, 0, "Crypt::opensslDecrypt")?;
            let cipher_algo = get_arg_str(args, 1, "Crypt::opensslDecrypt")?;
            let passphrase = get_arg_str(args, 2, "Crypt::opensslDecrypt")?;
            let options = args.get(3).and_then(|v| v.to_i64()).unwrap_or(0);
            let iv = args.get(4).map(|v| v.to_php_string()).unwrap_or_default();
            match crypto::openssl_decrypt(
                data.as_bytes(),
                &cipher_algo,
                passphrase.as_bytes(),
                options,
                iv.as_bytes(),
            ) {
                Ok(res) => {
                    let raw_data = (options & 1) != 0;
                    if raw_data {
                        Ok(PhpValue::bytes(res))
                    } else {
                        Ok(PhpValue::string(String::from_utf8_lossy(&res).to_string()))
                    }
                }
                Err(_) => Ok(PhpValue::Bool(false)),
            }
        }
        "randomBytes" | "random_bytes" => {
            let len = args.first().and_then(|v| v.to_i64()).unwrap_or(16).max(0) as usize;
            let res = crypto::random_bytes(len)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            Ok(PhpValue::bytes(res))
        }
        "randomHex" | "random_hex" => {
            let len = args.first().and_then(|v| v.to_i64()).unwrap_or(32).max(1) as usize;
            let byte_len = (len + 1) / 2;
            let bytes = crypto::random_bytes(byte_len)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            let mut hex_str = hex::encode(bytes);
            hex_str.truncate(len);
            Ok(PhpValue::string(hex_str))
        }
        "md5" => {
            let data = get_arg_str(args, 0, "Crypt::md5")?;
            let binary = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            let digest = crypto::md5::md5_digest(data.as_bytes());
            if binary {
                Ok(PhpValue::bytes(digest.to_vec()))
            } else {
                Ok(PhpValue::string(hex::encode(digest)))
            }
        }
        "sha1" => {
            let data = get_arg_str(args, 0, "Crypt::sha1")?;
            let binary = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            let res = crypto::hash("sha1", data.as_bytes(), binary)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            Ok(if binary {
                PhpValue::bytes(res)
            } else {
                PhpValue::string(String::from_utf8_lossy(&res).to_string())
            })
        }
        "sha256" => {
            let data = get_arg_str(args, 0, "Crypt::sha256")?;
            let binary = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            let res = crypto::hash("sha256", data.as_bytes(), binary)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            Ok(if binary {
                PhpValue::bytes(res)
            } else {
                PhpValue::string(String::from_utf8_lossy(&res).to_string())
            })
        }
        "sha512" => {
            let data = get_arg_str(args, 0, "Crypt::sha512")?;
            let binary = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            let res = crypto::hash("sha512", data.as_bytes(), binary)
                .map_err(|e| RuntimeErrorKind::Custom { message: e })?;
            Ok(if binary {
                PhpValue::bytes(res)
            } else {
                PhpValue::string(String::from_utf8_lossy(&res).to_string())
            })
        }
        "hashAlgos" | "hash_algos" | "algos" => {
            let list: Vec<PhpValue> = crypto::hash_algos()
                .iter()
                .map(|s| PhpValue::string(*s))
                .collect();
            Ok(PhpValue::list(list))
        }
        "opensslCipherMethods" | "openssl_cipher_methods" | "ciphers" => {
            let list: Vec<PhpValue> = crypto::openssl_cipher_methods()
                .iter()
                .map(|s| PhpValue::string(*s))
                .collect();
            Ok(PhpValue::list(list))
        }
        "macro" => {
            if let (Some(name_val), Some(PhpValue::Closure(closure))) = (args.first(), args.get(1)) {
                let name = name_val.to_php_string();
                register_runtime_closure_macro("Crypt", &name, (**closure).clone());
                return Ok(PhpValue::Bool(true));
            }
            Ok(PhpValue::Bool(false))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Crypt".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_html(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "specialChars" | "special_chars" | "htmlspecialchars" | "escape" => {
            let s = get_arg_str(args, 0, "Html::specialChars")?;
            let flags = args.get(1).and_then(|v| v.to_i64()).unwrap_or(html::DEFAULT_FLAGS);
            let double_encode = args.get(3).map(|v| v.is_truthy()).unwrap_or(true);
            Ok(PhpValue::string(html::special_chars(&s, flags, double_encode)))
        }
        "specialCharsDecode" | "special_chars_decode" | "htmlspecialchars_decode" => {
            let s = get_arg_str(args, 0, "Html::specialCharsDecode")?;
            let flags = args.get(1).and_then(|v| v.to_i64()).unwrap_or(html::DEFAULT_FLAGS);
            Ok(PhpValue::string(html::special_chars_decode(&s, flags)))
        }
        "stripTags" | "strip_tags" => {
            let s = get_arg_str(args, 0, "Html::stripTags")?;
            let allowed_tags = args.get(1).and_then(|v| match v {
                PhpValue::Null => None,
                PhpValue::String(str_val) => Some(html::parse_allowed_tags_string(str_val.as_str())),
                PhpValue::Array(_) | PhpValue::List(_) | PhpValue::Set(_) => {
                    let items = extract_array_elements(v);
                    Some(items.into_iter().map(|it| it.to_php_string()).collect())
                }
                other => Some(html::parse_allowed_tags_string(&other.to_php_string())),
            });
            Ok(PhpValue::string(html::strip_tags(&s, allowed_tags.as_deref())))
        }
        "entities" | "htmlentities" => {
            let s = get_arg_str(args, 0, "Html::entities")?;
            let flags = args.get(1).and_then(|v| v.to_i64()).unwrap_or(html::DEFAULT_FLAGS);
            let double_encode = args.get(3).map(|v| v.is_truthy()).unwrap_or(true);
            Ok(PhpValue::string(html::entities(&s, flags, double_encode)))
        }
        "entityDecode" | "entity_decode" | "html_entity_decode" | "decodeEntities" | "decode" => {
            let s = get_arg_str(args, 0, "Html::entityDecode")?;
            let flags = args.get(1).and_then(|v| v.to_i64()).unwrap_or(html::DEFAULT_FLAGS);
            Ok(PhpValue::string(html::entity_decode(&s, flags)))
        }
        "clean" => {
            let s = get_arg_str(args, 0, "Html::clean")?;
            let stripped = html::strip_tags(&s, None);
            Ok(PhpValue::string(html::special_chars(&stripped, html::DEFAULT_FLAGS, true)))
        }
        "tag" => {
            let tag_name = get_arg_str(args, 0, "Html::tag")?;
            let attrs_str = if let Some(attr_val) = args.get(1) {
                format_html_attributes(attr_val)
            } else {
                String::new()
            };
            let content = args.get(2).map(|v| v.to_php_string()).unwrap_or_default();
            let is_void = matches!(
                tag_name.to_ascii_lowercase().as_str(),
                "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr"
            );
            if is_void && content.is_empty() {
                if attrs_str.is_empty() {
                    Ok(PhpValue::string(format!("<{} />", tag_name)))
                } else {
                    Ok(PhpValue::string(format!("<{} {} />", tag_name, attrs_str)))
                }
            } else {
                if attrs_str.is_empty() {
                    Ok(PhpValue::string(format!("<{}>{}</{}>", tag_name, content, tag_name)))
                } else {
                    Ok(PhpValue::string(format!("<{} {}>{}</{}>", tag_name, attrs_str, content, tag_name)))
                }
            }
        }
        "attributes" => {
            let attrs_val = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::string(format_html_attributes(&attrs_val)))
        }
        "macro" => {
            if let (Some(name_val), Some(PhpValue::Closure(closure))) = (args.first(), args.get(1)) {
                let name = name_val.to_php_string();
                register_runtime_closure_macro("Html", &name, (**closure).clone());
                return Ok(PhpValue::Bool(true));
            }
            Ok(PhpValue::Bool(false))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Html".to_string(),
            method: method.to_string(),
        }),
    }
}

fn format_html_attributes(val: &PhpValue) -> String {
    let mut parts = Vec::new();
    match val {
        PhpValue::Dict(d) => {
            for (k, v) in d.borrow().iter() {
                if v.is_null() || !v.is_truthy() && matches!(v, PhpValue::Bool(false)) {
                    continue;
                }
                if matches!(v, PhpValue::Bool(true)) {
                    parts.push(k.clone());
                } else {
                    let escaped = html::special_chars(&v.to_php_string(), html::ENT_COMPAT, true);
                    parts.push(format!("{}=\"{}\"", k, escaped));
                }
            }
        }
        PhpValue::HashMap(m) => {
            for (k, v) in m.borrow().iter() {
                if v.is_null() || !v.is_truthy() && matches!(v, PhpValue::Bool(false)) {
                    continue;
                }
                let k_str = k.to_php_string();
                if matches!(v, PhpValue::Bool(true)) {
                    parts.push(k_str);
                } else {
                    let escaped = html::special_chars(&v.to_php_string(), html::ENT_COMPAT, true);
                    parts.push(format!("{}=\"{}\"", k_str, escaped));
                }
            }
        }
        _ => {}
    }
    parts.join(" ")
}

/// Parse regex pattern supporting standard PHP delimiters (`/.../i`, `#[0-9]+#`, `~pattern~s`)
/// as well as standard unescaped regex strings (`^[a-z]+$`).
pub fn parse_regex_pattern(pat: &str) -> Result<regex::Regex, regex::Error> {
    let trimmed = pat.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next().unwrap();
        if first == '/' || first == '#' || first == '~' || first == '@' {
            if let Some(last_delim_idx) = trimmed.rfind(first) {
                if last_delim_idx > 0 {
                    let inner_pat = &trimmed[1..last_delim_idx];
                    let flags = &trimmed[last_delim_idx + 1..];
                    let mut prefix = String::new();
                    let mut case_insensitive = false;
                    let mut multi_line = false;
                    let mut dot_matches_new_line = false;
                    for c in flags.chars() {
                        match c {
                            'i' => case_insensitive = true,
                            'm' => multi_line = true,
                            's' => dot_matches_new_line = true,
                            _ => {}
                        }
                    }
                    if case_insensitive {
                        prefix.push_str("(?i)");
                    }
                    if multi_line {
                        prefix.push_str("(?m)");
                    }
                    if dot_matches_new_line {
                        prefix.push_str("(?s)");
                    }
                    return regex::Regex::new(&format!("{}{}", prefix, inner_pat));
                }
            }
        }
    }
    regex::Regex::new(pat)
}

fn dispatch_regex(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "isValid" | "is_valid" | "check" => {
            let pattern = get_arg_str(args, 0, "Regex::isValid")?;
            Ok(PhpValue::Bool(parse_regex_pattern(&pattern).is_ok()))
        }
        "testString" | "test" | "matches" | "isMatch" => {
            let arg0 = get_arg_str(args, 0, "Regex::testString")?;
            let arg1 = get_arg_str(args, 1, "Regex::testString")?;
            let is_matched = if let Ok(re) = parse_regex_pattern(&arg0) {
                re.is_match(&arg1)
            } else if let Ok(re) = parse_regex_pattern(&arg1) {
                re.is_match(&arg0)
            } else {
                false
            };
            Ok(PhpValue::Bool(is_matched))
        }
        "match" => {
            let pattern = get_arg_str(args, 0, "Regex::match")?;
            let subject = get_arg_str(args, 1, "Regex::match")?;
            let re = parse_regex_pattern(&pattern)
                .map_err(|e| RuntimeErrorKind::Custom { message: format!("Invalid regex pattern: {e}") })?;
            if let Some(caps) = re.captures(&subject) {
                let matches: Vec<PhpValue> = caps.iter().flatten().map(|m| PhpValue::string(m.as_str())).collect();
                Ok(PhpValue::list(matches))
            } else {
                Ok(PhpValue::Null)
            }
        }
        "replace" => {
            let pattern = get_arg_str(args, 0, "Regex::replace")?;
            let replacement = get_arg_str(args, 1, "Regex::replace")?;
            let subject = get_arg_str(args, 2, "Regex::replace")?;
            let re = parse_regex_pattern(&pattern)
                .map_err(|e| RuntimeErrorKind::Custom { message: format!("Invalid regex pattern: {e}") })?;
            let replaced = re.replace_all(&subject, replacement.as_str());
            Ok(PhpValue::string(replaced.into_owned()))
        }
        "escape" | "quote" => {
            let s = get_arg_str(args, 0, "Regex::escape")?;
            Ok(PhpValue::string(regex::escape(&s)))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Regex".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_glob(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "getClass" => {
            let name = match args.first() {
                Some(PhpValue::Object(o)) => o.borrow().class_name.clone(),
                _ => "stdClass".to_string(),
            };
            Ok(PhpValue::string(name))
        }
        "match" => {
            let pattern = get_arg_str(args, 0, "Glob::match")?;
            let subject = get_arg_str(args, 1, "Glob::match")?;
            let matched = if let Ok(re) = parse_regex_pattern(&pattern) {
                re.is_match(&subject)
            } else {
                subject.contains(&pattern)
            };
            Ok(PhpValue::Bool(matched))
        }
        "customHelper" => {
            // Forwarded dynamically registered custom helper
            let helper_name = get_arg_str(args, 0, "Glob::customHelper")?;
            let remaining = &args[1..];
            if let Some(_entry) = get_macro("Glob", &helper_name) {
                // Return fallback or evaluated value
                Ok(remaining.first().cloned().unwrap_or(PhpValue::Null))
            } else {
                Err(RuntimeErrorKind::UndefinedFunction { name: helper_name })
            }
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Glob".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_php_fun(method: &str, args: &[PhpValue]) -> FacadeResult {
    // Canonical bridging from legacy PHP function calls
    match method {
        "array_merge" => dispatch_arr("merge", args),
        "count" => dispatch_arr("count", args),
        "array_keys" => dispatch_arr("keys", args),
        "array_values" => dispatch_arr("values", args),
        "array_key_exists" => dispatch_arr("hasKey", args),
        "in_array" => dispatch_arr("hasValue", args),
        "strlen" => dispatch_str("length", args),
        "str_contains" => dispatch_str("contains", args),
        "implode" => dispatch_str("join", args),
        "explode" => dispatch_str("split", args),
        "trim" => dispatch_str("trim", args),
        "ltrim" => dispatch_str("ltrim", args),
        "rtrim" => dispatch_str("rtrim", args),
        "strtolower" => dispatch_str("lower", args),
        "strtoupper" => dispatch_str("upper", args),
        "str_replace" => dispatch_str("replace", args),
        "preg_match" => dispatch_regex("testString", args),
        "preg_replace" => dispatch_regex("replace", args),
        "preg_quote" => dispatch_regex("quote", args),
        "round" => dispatch_float("round", args),
        "floor" => dispatch_float("floor", args),
        "ceil" | "ciel" => dispatch_float("ceil", args),
        "json_encode" => {
            let val = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::string(php_value_to_json(&val)))
        }
        "json_decode" => {
            let s = get_arg_str(args, 0, "json_decode")?;
            match parse_json_value(&s) {
                Ok(val) => Ok(val),
                Err(_) => Ok(PhpValue::Null),
            }
        }
        "class_exists" => dispatch_reflect("classExists", args),
        "function_exists" => dispatch_reflect("functionExists", args),
        "collect" => {
            let items = args.first().cloned().unwrap_or_else(|| PhpValue::list(vec![]));
            let mut obj = VmObject::new("Collection");
            obj.set_property("items", items);
            Ok(PhpValue::object(obj))
        }
        "mime_content_type" => dispatch_file("mimeType", args),
        "filesize" => dispatch_file("size", args),
        "is_file" => dispatch_file("isFile", args),
        "is_dir" => dispatch_file("isDirectory", args),
        _ => Err(RuntimeErrorKind::UndefinedFunction {
            name: method.to_string(),
        }),
    }
}

// =========================================================================
// Reflect Facade
// =========================================================================

fn dispatch_reflect(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "classExists" | "class_exists" => {
            let class_name = match args.first() {
                Some(PhpValue::String(s)) => s.to_string(),
                Some(val) => val.to_php_string(),
                None => return Ok(PhpValue::Bool(false)),
            };
            let clean = class_name.strip_prefix('\\').unwrap_or(&class_name);
            let exists = is_static_facade(clean)
                || is_static_only(clean)
                || NATIVE_FACADES
                    .iter()
                    .any(|f| f.name.eq_ignore_ascii_case(clean))
                || clean.eq_ignore_ascii_case("stdClass")
                || clean.eq_ignore_ascii_case("Exception")
                || clean.eq_ignore_ascii_case("Error")
                || clean.eq_ignore_ascii_case("Throwable")
                || clean.eq_ignore_ascii_case("Closure")
                || clean.eq_ignore_ascii_case("Future")
                || clean.ends_with("Exception")
                || clean.ends_with("Error")
                || clean.eq_ignore_ascii_case("Traversable")
                || clean.eq_ignore_ascii_case("IteratorAggregate")
                || clean.eq_ignore_ascii_case("Iterator")
                || clean.eq_ignore_ascii_case("ArrayIterator")
                || clean.eq_ignore_ascii_case("Countable")
                || clean.eq_ignore_ascii_case("ArrayAccess")
                || clean.eq_ignore_ascii_case("LengthAwarePaginator")
                || clean.eq_ignore_ascii_case("Paginator")
                || clean.eq_ignore_ascii_case("Collection")
                || clean.eq_ignore_ascii_case("Illuminate\\Support\\Collection");
            Ok(PhpValue::Bool(exists))
        }
        "functionExists" | "function_exists" => {
            let fn_name = match args.first() {
                Some(PhpValue::String(s)) => s.to_string(),
                Some(val) => val.to_php_string(),
                None => return Ok(PhpValue::Bool(false)),
            };
            let clean = fn_name.strip_prefix('\\').unwrap_or(&fn_name);
            let exists = is_legacy_php_function(clean)
                || LEGACY_PHP_FUNCTIONS
                    .iter()
                    .any(|f| f.php_name.eq_ignore_ascii_case(clean))
                || is_reserved_intrinsic(clean)
                || RESERVED_INTRINSICS
                    .iter()
                    .any(|&i| i.eq_ignore_ascii_case(clean))
                || has_custom_helper(clean)
                || clean.eq_ignore_ascii_case("class_exists")
                || clean.eq_ignore_ascii_case("function_exists")
                || clean.eq_ignore_ascii_case("collect");
            Ok(PhpValue::Bool(exists))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Reflect".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Carbon Facade
// =========================================================================

pub fn dispatch_carbon(method: &str, args: &[PhpValue]) -> FacadeResult {
    crate::vm::carbon::dispatch_carbon(method, args)
}

// =========================================================================
// Helpers
// =========================================================================

pub(crate) fn get_arg_str(args: &[PhpValue], index: usize, callable: &str) -> Result<String, RuntimeErrorKind> {
    args.get(index)
        .map(|v| v.to_php_string())
        .ok_or_else(|| RuntimeErrorKind::TooFewArguments {
            callable: callable.to_string(),
            passed: args.len(),
            expected: index + 1,
        })
}

pub fn extract_array_elements(val: &PhpValue) -> Vec<PhpValue> {
    match val {
        PhpValue::Array(a) | PhpValue::List(a) => a.borrow().clone(),
        PhpValue::Set(s) => s.borrow().iter().cloned().collect(),
        PhpValue::Dict(d) => d.borrow().iter().map(|(_, v)| v.clone()).collect(),
        PhpValue::HashMap(m) => m.borrow().iter().map(|(_, v)| v.clone()).collect(),
        other => vec![other.clone()],
    }
}

pub fn flatten_elements(val: &PhpValue, out: &mut Vec<PhpValue>) {
    match val {
        PhpValue::Array(a) | PhpValue::List(a) => {
            for item in a.borrow().iter() {
                flatten_elements(item, out);
            }
        }
        PhpValue::Set(s) => {
            for item in s.borrow().iter() {
                flatten_elements(item, out);
            }
        }
        PhpValue::Dict(d) => {
            for (_, item) in d.borrow().iter() {
                flatten_elements(item, out);
            }
        }
        PhpValue::HashMap(m) => {
            for (_, item) in m.borrow().iter() {
                flatten_elements(item, out);
            }
        }
        other => {
            out.push(other.clone());
        }
    }
}

pub fn extract_dict_elements(val: &PhpValue) -> Vec<(String, PhpValue)> {
    match val {
        PhpValue::Dict(d) => d
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        PhpValue::Array(a) | PhpValue::List(a) => a
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        PhpValue::HashMap(m) => m
            .borrow()
            .iter()
            .map(|(k, v)| (k.to_php_string(), v.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn extract_dict_map(val: &PhpValue) -> IndexMap<String, PhpValue> {
    match val {
        PhpValue::Dict(d) => d.borrow().clone(),
        PhpValue::Array(a) | PhpValue::List(a) => {
            let mut map = IndexMap::new();
            for (i, v) in a.borrow().iter().enumerate() {
                map.insert(i.to_string(), v.clone());
            }
            map
        }
        PhpValue::HashMap(m) => {
            let mut map = IndexMap::new();
            for (k, v) in m.borrow().iter() {
                map.insert(k.to_php_string(), v.clone());
            }
            map
        }
        _ => IndexMap::new(),
    }
}

pub fn extract_map_elements(val: &PhpValue) -> Vec<(PhpValue, PhpValue)> {
    match val {
        PhpValue::HashMap(m) => m
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        PhpValue::Dict(d) => d
            .borrow()
            .iter()
            .map(|(k, v)| (PhpValue::string(k.clone()), v.clone()))
            .collect(),
        PhpValue::Array(a) | PhpValue::List(a) => {
            let b = a.borrow();
            let all_pairs = !b.is_empty()
                && b.iter().all(|item| match item {
                    PhpValue::Array(sub) | PhpValue::List(sub) => sub.borrow().len() == 2,
                    _ => false,
                });
            if all_pairs {
                b.iter()
                    .map(|item| match item {
                        PhpValue::Array(sub) | PhpValue::List(sub) => {
                            let sub_b = sub.borrow();
                            (sub_b[0].clone(), sub_b[1].clone())
                        }
                        _ => unreachable!(),
                    })
                    .collect()
            } else {
                b.iter()
                    .enumerate()
                    .map(|(i, v)| (PhpValue::Int(i as i32), v.clone()))
                    .collect()
            }
        }
        _ => Vec::new(),
    }
}

pub fn arr_get_dot(target: &PhpValue, key: &str, default: PhpValue) -> PhpValue {
    if key.is_empty() {
        return target.clone();
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = target.clone();
    for part in parts {
        match current {
            PhpValue::Dict(d) => {
                if let Some(val) = d.borrow().get(part) {
                    current = val.clone();
                } else {
                    return default;
                }
            }
            PhpValue::Array(a) | PhpValue::List(a) => {
                if let Ok(idx) = part.parse::<usize>() {
                    if let Some(val) = a.borrow().get(idx) {
                        current = val.clone();
                    } else {
                        return default;
                    }
                } else {
                    return default;
                }
            }
            PhpValue::HashMap(m) => {
                let k = PhpValue::string(part);
                if let Some(val) = m.borrow().get(&k) {
                    current = val.clone();
                } else {
                    return default;
                }
            }
            PhpValue::Object(o) => {
                if let Some(val) = o.borrow().get_property(part) {
                    current = val.clone();
                } else {
                    return default;
                }
            }
            _ => return default,
        }
    }
    current
}

pub fn arr_has_dot(target: &PhpValue, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = target.clone();
    for (i, part) in parts.iter().enumerate() {
        match current {
            PhpValue::Dict(d) => {
                let b = d.borrow();
                if let Some(val) = b.get(*part) {
                    if i == parts.len() - 1 {
                        return true;
                    }
                    current = val.clone();
                } else {
                    return false;
                }
            }
            PhpValue::HashMap(m) => {
                let k = PhpValue::string(*part);
                let b = m.borrow();
                if let Some(val) = b.get(&k) {
                    if i == parts.len() - 1 {
                        return true;
                    }
                    current = val.clone();
                } else {
                    return false;
                }
            }
            PhpValue::Array(a) | PhpValue::List(a) => {
                if let Ok(idx) = part.parse::<usize>() {
                    let b = a.borrow();
                    if let Some(val) = b.get(idx) {
                        if i == parts.len() - 1 {
                            return true;
                        }
                        current = val.clone();
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            PhpValue::Object(o) => {
                let b = o.borrow();
                if let Some(val) = b.get_property(part) {
                    if i == parts.len() - 1 {
                        return true;
                    }
                    current = val.clone();
                } else {
                    return false;
                }
            }
            PhpValue::Set(s) => {
                let k = PhpValue::string(*part);
                let b = s.borrow();
                if b.contains(&k) {
                    if i == parts.len() - 1 {
                        return true;
                    }
                    return false;
                } else if let Ok(idx) = part.parse::<i32>() {
                    if b.contains(&PhpValue::Int(idx)) {
                        if i == parts.len() - 1 {
                            return true;
                        }
                        return false;
                    }
                }
                return false;
            }
            _ => return false,
        }
    }
    false
}

pub fn arr_set_dot(target: &mut PhpValue, key: &str, value: PhpValue) {
    if key.is_empty() {
        return;
    }
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() == 1 {
        match target {
            PhpValue::Dict(d) => {
                d.borrow_mut().insert(parts[0].to_string(), value);
            }
            _ => {
                let mut map = IndexMap::new();
                map.insert(parts[0].to_string(), value);
                *target = PhpValue::dict_from_map(map);
            }
        }
        return;
    }

    if !matches!(target, PhpValue::Dict(_)) {
        *target = PhpValue::dict(Vec::new());
    }
    let mut curr_rc = match target {
        PhpValue::Dict(d) => d.clone(),
        _ => return,
    };

    for part in &parts[..parts.len() - 1] {
        let next_exists = curr_rc.borrow().contains_key(*part);
        if !next_exists || !matches!(curr_rc.borrow().get(*part), Some(PhpValue::Dict(_))) {
            let next_dict = PhpValue::dict(Vec::new());
            curr_rc.borrow_mut().insert(part.to_string(), next_dict);
        }
        let next_val = curr_rc.borrow().get(*part).cloned().unwrap();
        if let PhpValue::Dict(next_rc) = next_val {
            curr_rc = next_rc;
        } else {
            return;
        }
    }
    let last_key = parts[parts.len() - 1];
    curr_rc.borrow_mut().insert(last_key.to_string(), value);
}

pub fn arr_forget_dot(target: &mut PhpValue, key: &str) {
    if key.is_empty() {
        return;
    }
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() == 1 {
        if let PhpValue::Dict(d) = target {
            d.borrow_mut().shift_remove(parts[0]);
        }
        return;
    }

    let mut curr = target.clone();
    for part in &parts[..parts.len() - 1] {
        match curr {
            PhpValue::Dict(d) => {
                if let Some(next) = d.borrow().get(*part).cloned() {
                    curr = next;
                } else {
                    return;
                }
            }
            _ => return,
        }
    }
    let last_key = parts[parts.len() - 1];
    if let PhpValue::Dict(d) = curr {
        d.borrow_mut().shift_remove(last_key);
    }
}

pub fn arr_dot(target: &PhpValue, prepend: &str, out: &mut IndexMap<String, PhpValue>) {
    match target {
        PhpValue::Dict(d) => {
            for (k, v) in d.borrow().iter() {
                let full_key = if prepend.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prepend, k)
                };
                if matches!(v, PhpValue::Dict(_)) {
                    arr_dot(v, &full_key, out);
                } else {
                    out.insert(full_key, v.clone());
                }
            }
        }
        _ => {}
    }
}

pub fn arr_undot(dotted: &IndexMap<String, PhpValue>) -> PhpValue {
    let mut root = PhpValue::dict(Vec::new());
    for (k, v) in dotted {
        arr_set_dot(&mut root, k, v.clone());
    }
    root
}

pub fn php_value_to_json(val: &PhpValue) -> String {
    match val {
        PhpValue::Null => "null".to_string(),
        PhpValue::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        PhpValue::Int(n) => n.to_string(),
        PhpValue::Float(fl) => fl.to_string(),
        PhpValue::RustInt { value, .. } => value.to_string(),
        PhpValue::RustFloat { value, .. } => value.to_string(),
        PhpValue::String(s) => {
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{}\"", escaped)
        }
        PhpValue::Array(a) | PhpValue::List(a) => {
            let items: Vec<String> = a.borrow().iter().map(php_value_to_json).collect();
            format!("[{}]", items.join(","))
        }
        PhpValue::Set(s) => {
            let items: Vec<String> = s.borrow().iter().map(php_value_to_json).collect();
            format!("[{}]", items.join(","))
        }
        PhpValue::Dict(d) => {
            let entries: Vec<String> = d
                .borrow()
                .iter()
                .map(|(k, v)| {
                    let escaped_k = k.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("\"{}\":{}", escaped_k, php_value_to_json(v))
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        PhpValue::HashMap(m) => {
            let entries: Vec<String> = m
                .borrow()
                .iter()
                .map(|(k, v)| {
                    let escaped_k = k.to_php_string().replace('\\', "\\\\").replace('"', "\\\"");
                    format!("\"{}\":{}", escaped_k, php_value_to_json(v))
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        PhpValue::Object(o) => {
            let obj = o.borrow();
            if obj.class_name == "Carbon" {
                if let Some(iso) = obj.get_property("iso") {
                    let escaped = iso.to_php_string().replace('\\', "\\\\").replace('"', "\\\"");
                    return format!("\"{}\"", escaped);
                }
            }
            let mut entries = Vec::new();
            for (k, v) in obj.properties.iter() {
                let escaped_k = k.replace('\\', "\\\\").replace('"', "\\\"");
                entries.push(format!("\"{}\":{}", escaped_k, php_value_to_json(v)));
            }
            format!("{{{}}}", entries.join(","))
        }
        _ => "null".to_string(),
    }
}

/// Parse a JSON string into a corresponding [`PhpValue`].
pub fn parse_json_value(input: &str) -> Result<PhpValue, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    skip_ws(&chars, &mut i);
    let val = parse_raw_json(&chars, &mut i)?;
    Ok(val)
}

fn skip_ws(chars: &[char], i: &mut usize) {
    while *i < chars.len() && chars[*i].is_whitespace() {
        *i += 1;
    }
}

fn parse_raw_json(chars: &[char], i: &mut usize) -> Result<PhpValue, String> {
    skip_ws(chars, i);
    if *i >= chars.len() {
        return Ok(PhpValue::Null);
    }
    match chars[*i] {
        'n' => {
            if chars[*i..].starts_with(&['n', 'u', 'l', 'l']) {
                *i += 4;
                Ok(PhpValue::Null)
            } else {
                Err("Unexpected token for null".to_string())
            }
        }
        't' => {
            if chars[*i..].starts_with(&['t', 'r', 'u', 'e']) {
                *i += 4;
                Ok(PhpValue::Bool(true))
            } else {
                Err("Unexpected token for true".to_string())
            }
        }
        'f' => {
            if chars[*i..].starts_with(&['f', 'a', 'l', 's', 'e']) {
                *i += 5;
                Ok(PhpValue::Bool(false))
            } else {
                Err("Unexpected token for false".to_string())
            }
        }
        '"' => {
            let s = parse_json_string(chars, i)?;
            Ok(PhpValue::string(s))
        }
        '[' => {
            *i += 1; // skip '['
            let mut items = Vec::new();
            skip_ws(chars, i);
            if *i < chars.len() && chars[*i] == ']' {
                *i += 1;
                return Ok(PhpValue::list(items));
            }
            while *i < chars.len() {
                let elem = parse_raw_json(chars, i)?;
                items.push(elem);
                skip_ws(chars, i);
                if *i < chars.len() && chars[*i] == ',' {
                    *i += 1;
                    skip_ws(chars, i);
                } else if *i < chars.len() && chars[*i] == ']' {
                    *i += 1;
                    break;
                } else {
                    break;
                }
            }
            Ok(PhpValue::list(items))
        }
        '{' => {
            *i += 1; // skip '{'
            let mut map = IndexMap::new();
            skip_ws(chars, i);
            if *i < chars.len() && chars[*i] == '}' {
                *i += 1;
                return Ok(PhpValue::dict(map));
            }
            while *i < chars.len() {
                skip_ws(chars, i);
                if *i >= chars.len() || chars[*i] != '"' {
                    break;
                }
                let key = parse_json_string(chars, i)?;
                skip_ws(chars, i);
                if *i < chars.len() && chars[*i] == ':' {
                    *i += 1;
                }
                skip_ws(chars, i);
                let val = parse_raw_json(chars, i)?;
                map.insert(key, val);
                skip_ws(chars, i);
                if *i < chars.len() && chars[*i] == ',' {
                    *i += 1;
                    skip_ws(chars, i);
                } else if *i < chars.len() && chars[*i] == '}' {
                    *i += 1;
                    break;
                } else {
                    break;
                }
            }
            Ok(PhpValue::dict(map))
        }
        '-' | '0'..='9' => {
            let start = *i;
            if chars[*i] == '-' {
                *i += 1;
            }
            let mut is_float = false;
            while *i < chars.len()
                && (chars[*i].is_ascii_digit()
                    || chars[*i] == '.'
                    || chars[*i] == 'e'
                    || chars[*i] == 'E'
                    || chars[*i] == '+'
                    || chars[*i] == '-')
            {
                if chars[*i] == '.' || chars[*i] == 'e' || chars[*i] == 'E' {
                    is_float = true;
                }
                *i += 1;
            }
            let num_str: String = chars[start..*i].iter().collect();
            if is_float {
                let f: f32 = num_str.parse().unwrap_or(0.0);
                Ok(PhpValue::Float(f))
            } else {
                let n: i32 = num_str.parse().unwrap_or(0);
                Ok(PhpValue::Int(n))
            }
        }
        _ => Err("Invalid JSON character".to_string()),
    }
}

fn parse_json_string(chars: &[char], i: &mut usize) -> Result<String, String> {
    if *i >= chars.len() || chars[*i] != '"' {
        return Err("Expected string quote".to_string());
    }
    *i += 1; // skip opening quote
    let mut out = String::new();
    while *i < chars.len() {
        let ch = chars[*i];
        *i += 1;
        if ch == '"' {
            return Ok(out);
        } else if ch == '\\' {
            if *i < chars.len() {
                let esc = chars[*i];
                *i += 1;
                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\x08'),
                    'f' => out.push('\x0C'),
                    'u' => {
                        if *i + 4 <= chars.len() {
                            let hex_str: String = chars[*i..*i + 4].iter().collect();
                            *i += 4;
                            if let Ok(code) = u32::from_str_radix(&hex_str, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    out.push(c);
                                }
                            }
                        }
                    }
                    _ => out.push(esc),
                }
            }
        } else {
            out.push(ch);
        }
    }
    Err("Unterminated string".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_value_basic() {
        let s = r#"{"name":"Alice","skills":["Rust","RHP","HTMX"],"count":42,"active":true}"#;
        let res = parse_json_value(s).expect("parse json");
        match res {
            PhpValue::Dict(d) => {
                let borrowed = d.borrow();
                assert_eq!(borrowed.get("name"), Some(&PhpValue::string("Alice")));
                assert_eq!(borrowed.get("count"), Some(&PhpValue::Int(42)));
                assert_eq!(borrowed.get("active"), Some(&PhpValue::Bool(true)));
            }
            other => panic!("Expected Dict, got {:?}", other),
        }
    }
}
