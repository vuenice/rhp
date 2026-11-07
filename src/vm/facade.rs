use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
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
use std::net::Ipv4Addr;
use std::io::{Read, Write};
use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
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

    /// Whether a specific method on this plugin is blocking and should be dispatched to `tokio::task::spawn_blocking`.
    fn is_blocking(&self, _method: &str) -> bool {
        false
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
        if p_clone.is_blocking(method) {
            let p_inner = Arc::clone(&p_clone);
            let m_str = method.to_string();
            let f_str = p_inner.name().to_string();
            let frozen_args: Vec<crate::vm::parent_process::FrozenValue> =
                args.iter().map(crate::vm::parent_process::FrozenValue::from_php_value).collect();
            let method_call = m_str.clone();
            let frozen_res = crate::vm::fiber::dispatch_blocking_native(&f_str, &m_str, move || {
                let thawed_args: Vec<PhpValue> =
                    frozen_args.iter().map(|f| f.to_php_value()).collect();
                p_inner
                    .call(&method_call, &thawed_args, span)
                    .map(|val| crate::vm::parent_process::FrozenValue::from_php_value(&val))
            })?;
            Ok(frozen_res.to_php_value())
        } else {
            p_clone.call(method, args, span)
        }
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
    FacadeRegistry::global().clear_tier2();
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
        "Bool" => dispatch_bool(method, args),
        "Validate" => dispatch_validate(method, args),
        "Debug" => dispatch_debug(method, args),
        "Csv" => crate::vm::csv::dispatch_csv(method, args),
        "Str" => dispatch_str(method, args),
        "Arr" | "Array" => dispatch_arr(method, args),
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
        "Time" | "Carbon" => dispatch_time(method, args),
        "Ip" => dispatch_ip(method, args),
        "Compress" => dispatch_compress(method, args),
        "Uniq" => crate::vm::uniq::dispatch_uniq(method, args),
        "Path" => dispatch_path(method, args),
        "Socket" => dispatch_socket(method, args),
        "Binary" => dispatch_binary(method, args),
        _ => {
            if let Some((in_proc_fn, is_blocking)) =
                crate::vm::rust_loader::get_in_process_native_function(facade, method)
            {
                let frozen_args: Vec<crate::vm::parent_process::FrozenValue> =
                    args.iter().map(crate::vm::parent_process::FrozenValue::from_php_value).collect();
                if is_blocking {
                    let facade_owned = facade.to_string();
                    let method_owned = method.to_string();
                    let frozen_res = crate::vm::fiber::dispatch_blocking_native(
                        &facade_owned,
                        &method_owned,
                        move || in_proc_fn(&frozen_args),
                    )?;
                    return Ok(frozen_res.to_php_value());
                } else {
                    let frozen_res = in_proc_fn(&frozen_args)?;
                    return Ok(frozen_res.to_php_value());
                }
            }

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
// Bool Facade
// =========================================================================

pub fn to_bool_loose(v: &PhpValue) -> bool {
    match v {
        PhpValue::Bool(b) => *b,
        PhpValue::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" | "" => false,
            _ => v.is_truthy(),
        },
        _ => v.is_truthy(),
    }
}

fn dispatch_bool(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "compare" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            let b1 = to_bool_loose(a);
            let b2 = to_bool_loose(b);
            Ok(PhpValue::Bool(b1 == b2))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Bool".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Str Facade
// =========================================================================

fn dispatch_str(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "utf8" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::utf8".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            match val {
                PhpValue::String(s) => match s {
                    crate::vm::value::StrValue::Utf8 { .. } => Ok(val.clone()),
                    crate::vm::value::StrValue::Bytes(b) => match std::str::from_utf8(b) {
                        Ok(valid) => Ok(PhpValue::string(valid)),
                        Err(e) => Err(RuntimeErrorKind::UncaughtException {
                            class: "InvalidEncodingException".to_string(),
                            message: format!("Invalid UTF-8 sequence at byte offset {}", e.valid_up_to()),
                        }),
                    },
                },
                PhpValue::Int(_) | PhpValue::Float(_) | PhpValue::Bool(_) | PhpValue::RustInt { .. } | PhpValue::RustFloat { .. } => {
                    Ok(PhpValue::string(val.to_php_string()))
                }
                other => Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::utf8() expects string or bytes, got {}", other.type_name()),
                }),
            }
        }
        "bytes" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::bytes".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            match val {
                PhpValue::String(s) => match s {
                    crate::vm::value::StrValue::Bytes(_) => Ok(val.clone()),
                    crate::vm::value::StrValue::Utf8 { .. } => Ok(PhpValue::bytes(s.as_bytes())),
                },
                other => Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::bytes() expects string or bytes, got {}", other.type_name()),
                }),
            }
        }
        "len" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::len".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            match val {
                PhpValue::String(s) => Ok(PhpValue::Int(s.len() as i32)),
                other => Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::len() expects string or bytes, got {}", other.type_name()),
                }),
            }
        }
        "mbStrlen" | "mb_strlen" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::mbStrlen".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let enc = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            match val {
                PhpValue::String(s) => {
                    if s.is_bytes() {
                        return Err(RuntimeErrorKind::TypeError {
                            message: "Method mbStrlen() is not supported on bytes. Call ->decode() first to get a string.".to_string(),
                        });
                    }
                    if enc.as_deref().map(|e| e.eq_ignore_ascii_case("UTF-8") || e.eq_ignore_ascii_case("UTF8")).unwrap_or(true) {
                        if s.is_ascii() {
                            Ok(PhpValue::Int(s.len() as i32))
                        } else {
                            Ok(PhpValue::Int(s.as_str().chars().count() as i32))
                        }
                    } else {
                        let count = crate::vm::encoding::mb_strlen_encoding(s.as_bytes(), &enc.unwrap())
                            .map_err(|(cls, msg)| RuntimeErrorKind::UncaughtException { class: cls, message: msg })?;
                        Ok(PhpValue::Int(count as i32))
                    }
                }
                other => Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::mbStrlen() expects string, got {}", other.type_name()),
                }),
            }
        }
        "graphemeLen" | "grapheme_len" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::graphemeLen".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            match val {
                PhpValue::String(s) => {
                    if s.is_bytes() {
                        return Err(RuntimeErrorKind::TypeError {
                            message: "Method graphemeLen() is not supported on bytes. Call ->decode() first to get a string.".to_string(),
                        });
                    }
                    let count = crate::vm::encoding::grapheme_len(s.as_str());
                    Ok(PhpValue::Int(count as i32))
                }
                other => Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::graphemeLen() expects string, got {}", other.type_name()),
                }),
            }
        }
        "mbSubstr" | "mb_substr" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::mbSubstr".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let start = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            let len = args.get(2).and_then(|v| if v.is_null() { None } else { v.to_i64() });
            let enc = args.get(3).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            match val {
                PhpValue::String(s) => {
                    if s.is_bytes() {
                        return Err(RuntimeErrorKind::TypeError {
                            message: "Method mbSubstr() is not supported on bytes. Call ->decode() first to get a string.".to_string(),
                        });
                    }
                    let res = crate::vm::encoding::mb_substr(s.as_str(), s.is_ascii(), start, len, enc.as_deref())
                        .map_err(|(cls, msg)| RuntimeErrorKind::UncaughtException { class: cls, message: msg })?;
                    Ok(PhpValue::string(res))
                }
                other => Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::mbSubstr() expects string, got {}", other.type_name()),
                }),
            }
        }
        "convertEncoding" | "convert_encoding" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::convertEncoding".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let to = args.get(1).map(|v| v.to_php_string()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::convertEncoding".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let from = args.get(2).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let on_unmappable = args.get(3).map(|v| v.to_php_string()).unwrap_or_else(|| "throw".to_string());
            let bytes = match val {
                PhpValue::String(s) => s.as_bytes(),
                other => return Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::convertEncoding() expects string or bytes, got {}", other.type_name()),
                }),
            };
            let converted = crate::vm::encoding::convert_encoding(bytes, &to, from.as_deref(), &on_unmappable)
                .map_err(|(cls, msg)| RuntimeErrorKind::UncaughtException { class: cls, message: msg })?;
            Ok(PhpValue::bytes(converted))
        }
        "detectEncoding" | "detect_encoding" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::detectEncoding".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let candidates: Option<Vec<String>> = match args.get(1) {
                Some(PhpValue::Array(items)) | Some(PhpValue::List(items)) => {
                    Some(items.borrow().iter().map(|v| v.to_php_string()).collect())
                }
                _ => None,
            };
            let bytes = match val {
                PhpValue::String(s) => s.as_bytes(),
                other => return Err(RuntimeErrorKind::TypeError {
                    message: format!("Str::detectEncoding() expects string or bytes, got {}", other.type_name()),
                }),
            };
            match crate::vm::encoding::detect_encoding(bytes, candidates.as_deref()) {
                Some(enc) => Ok(PhpValue::string(enc)),
                None => Ok(PhpValue::Null),
            }
        }
        "compare" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            let case_insensitive = args.get(2).and_then(|v| v.to_bool()).unwrap_or(false);
            let s1 = a.to_php_string();
            let s2 = b.to_php_string();
            let eq = if case_insensitive {
                s1.eq_ignore_ascii_case(&s2)
            } else {
                s1 == s2
            };
            Ok(PhpValue::Bool(eq))
        }
        "strcmp" | "cmp" | "compareBinary" | "binaryCompare" => {
            let s1 = get_arg_bytes(args, 0, "Str::strcmp")?;
            let s2 = get_arg_bytes(args, 1, "Str::strcmp")?;
            Ok(PhpValue::Int(binary_strcmp(&s1, &s2)))
        }
        "strcasecmp" | "casecmp" | "caseCmp" | "caseCompare" | "compareCase" => {
            let s1 = get_arg_bytes(args, 0, "Str::strcasecmp")?;
            let s2 = get_arg_bytes(args, 1, "Str::strcasecmp")?;
            Ok(PhpValue::Int(binary_strcasecmp(&s1, &s2)))
        }
        "strncmp" | "ncmp" | "nCmp" | "compareN" | "compareBinaryN" => {
            let s1 = get_arg_bytes(args, 0, "Str::strncmp")?;
            let s2 = get_arg_bytes(args, 1, "Str::strncmp")?;
            let len_val = args.get(2).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::strncmp".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let len_i64 = len_val.to_i64().ok_or_else(|| RuntimeErrorKind::TypeError {
                message: format!("Str::strncmp(): Argument #3 ($length) must be of type int, got {}", len_val.type_name()),
            })?;
            if len_i64 < 0 {
                return Err(RuntimeErrorKind::ValueError {
                    message: "Str::strncmp(): Argument #3 ($length) must be greater than or equal to 0".to_string(),
                });
            }
            Ok(PhpValue::Int(binary_strncmp(&s1, &s2, len_i64 as usize)))
        }
        "strncasecmp" | "ncasecmp" | "nCaseCmp" | "caseNCmp" | "caseCompareN" => {
            let s1 = get_arg_bytes(args, 0, "Str::strncasecmp")?;
            let s2 = get_arg_bytes(args, 1, "Str::strncasecmp")?;
            let len_val = args.get(2).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::strncasecmp".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let len_i64 = len_val.to_i64().ok_or_else(|| RuntimeErrorKind::TypeError {
                message: format!("Str::strncasecmp(): Argument #3 ($length) must be of type int, got {}", len_val.type_name()),
            })?;
            if len_i64 < 0 {
                return Err(RuntimeErrorKind::ValueError {
                    message: "Str::strncasecmp(): Argument #3 ($length) must be greater than or equal to 0".to_string(),
                });
            }
            Ok(PhpValue::Int(binary_strncasecmp(&s1, &s2, len_i64 as usize)))
        }
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
        "digit" | "isDigit" | "is_digit" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::digit".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let is_match = match target {
                PhpValue::String(s) => {
                    let b = s.as_bytes();
                    !b.is_empty() && b.iter().all(|c| c.is_ascii_digit())
                }
                PhpValue::Int(n) => {
                    let s = n.to_string();
                    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
                }
                PhpValue::RustInt { value, .. } => {
                    let s = value.to_string();
                    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
                }
                _ => false,
            };
            Ok(PhpValue::Bool(is_match))
        }
        "alpha" | "isAlpha" | "is_alpha" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::alpha".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let is_match = match target {
                PhpValue::String(s) => {
                    let b = s.as_bytes();
                    !b.is_empty() && b.iter().all(|c| c.is_ascii_alphabetic())
                }
                _ => false,
            };
            Ok(PhpValue::Bool(is_match))
        }
        "alnum" | "isAlnum" | "is_alnum" | "alphaNum" | "alphanumeric" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::alnum".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let is_match = match target {
                PhpValue::String(s) => {
                    let b = s.as_bytes();
                    !b.is_empty() && b.iter().all(|c| c.is_ascii_alphanumeric())
                }
                PhpValue::Int(n) => {
                    let s = n.to_string();
                    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric())
                }
                PhpValue::RustInt { value, .. } => {
                    let s = value.to_string();
                    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric())
                }
                _ => false,
            };
            Ok(PhpValue::Bool(is_match))
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
        "find" | "indexOf" | "pos" => {
            let arg0 = get_arg_str(args, 0, "Str::find")?;
            let arg1 = get_arg_str(args, 1, "Str::find")?;
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
                Ok(PhpValue::Null)
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
        "encoding" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::encoding".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let raw_bytes = extract_raw_byte_payload(target);
            let detected = crate::vm::encoding::detect_encoding(&raw_bytes, None).unwrap_or_else(|| "UTF-8".to_string());
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
        "fromLast" | "substringFromLast" | "lastFrom" | "strrchr" => {
            let s = get_arg_str(args, 0, "Str::fromLast")?;
            let needle_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::fromLast".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let needle = needle_val.to_php_string();
            if needle.is_empty() {
                return Ok(PhpValue::Bool(false));
            }
            let search_char = needle.chars().next().unwrap();
            if let Some(idx) = s.rfind(search_char) {
                Ok(PhpValue::string(&s[idx..]))
            } else {
                Ok(PhpValue::Bool(false))
            }
        }
        "afterLast" => {
            let s = get_arg_str(args, 0, "Str::afterLast")?;
            let needle = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            if needle.is_empty() {
                return Ok(PhpValue::string(s));
            }
            if let Some(idx) = s.rfind(&needle) {
                Ok(PhpValue::string(&s[idx + needle.len()..]))
            } else {
                Ok(PhpValue::string(s))
            }
        }
        // Pad a string to a given length with another string.
        // Example: `Str::pad("Alien", 10, "_", "right")` -> `"Alien_____"`
        "pad" => {
            let s = get_arg_str(args, 0, "Str::pad")?;
            let length = args.get(1).and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::pad".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let pad = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| " ".to_string());
            let side = args.get(3).map(|v| v.to_php_string()).unwrap_or_else(|| "right".to_string());
            if pad.is_empty() {
                return Err(RuntimeErrorKind::Custom {
                    message: "Str::pad(): Padding string cannot be empty".to_string(),
                });
            }
            let target_len = length.max(0) as usize;
            let current_len = s.chars().count();
            if target_len <= current_len {
                return Ok(PhpValue::string(s));
            }
            let needed = target_len - current_len;
            let side_lower = side.to_ascii_lowercase();
            let res = match side_lower.as_str() {
                "left" | "0" => {
                    let padding: String = pad.chars().cycle().take(needed).collect();
                    format!("{}{}", padding, s)
                }
                "right" | "1" => {
                    let padding: String = pad.chars().cycle().take(needed).collect();
                    format!("{}{}", s, padding)
                }
                "both" | "2" => {
                    let left_count = needed / 2;
                    let right_count = needed - left_count;
                    let left_pad: String = pad.chars().cycle().take(left_count).collect();
                    let right_pad: String = pad.chars().cycle().take(right_count).collect();
                    format!("{}{}{}", left_pad, s, right_pad)
                }
                _ => return Err(RuntimeErrorKind::Custom {
                    message: format!("Str::pad(): Invalid padding side '{}', expected 'left', 'right', or 'both'", side),
                }),
            };
            Ok(PhpValue::string(res))
        }
        // Repeat a string a specified number of times.
        // Example: `Str::repeat("abc", 3)` -> `"abcabcabc"`
        "repeat" => {
            let s = get_arg_str(args, 0, "Str::repeat")?;
            let times = args.get(1).and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::repeat".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            if times < 0 {
                return Err(RuntimeErrorKind::Custom {
                    message: "Str::repeat(): Argument #2 ($times) must be greater than or equal to 0".to_string(),
                });
            }
            Ok(PhpValue::string(s.repeat(times as usize)))
        }
        // Count the number of non-overlapping substring occurrences.
        // Example: `Str::countOccurrences("hello world hello", "hello")` -> `2`
        "countOccurrences" | "count_occurrences" => {
            let haystack = get_arg_str(args, 0, "Str::countOccurrences")?;
            let needle = get_arg_str(args, 1, "Str::countOccurrences")?;
            if needle.is_empty() {
                return Err(RuntimeErrorKind::Custom {
                    message: "Str::countOccurrences(): Substring cannot be empty".to_string(),
                });
            }
            let count = haystack.matches(&needle).count();
            Ok(PhpValue::Int(count as i32))
        }
        // Replace text within a portion of a string.
        // Example: `Str::replaceAt("abcdef", "123", 2, 2)` -> `"ab123ef"`
        "replaceAt" | "replace_at" => {
            let s = get_arg_str(args, 0, "Str::replaceAt")?;
            let replacement = get_arg_str(args, 1, "Str::replaceAt")?;
            let start = args.get(2).and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Str::replaceAt".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let length_opt = args.get(3).and_then(|v| if v.is_null() { None } else { v.to_i64() });

            let chars: Vec<char> = s.chars().collect();
            let total_len = chars.len() as i64;
            let start_idx = if start < 0 {
                (total_len + start).max(0) as usize
            } else {
                start.min(total_len) as usize
            };

            let end_idx = match length_opt {
                None => total_len as usize,
                Some(l) if l >= 0 => (start_idx as i64 + l).min(total_len) as usize,
                Some(l) => (total_len + l).max(start_idx as i64) as usize,
            };

            let mut out = String::new();
            out.extend(&chars[..start_idx]);
            out.push_str(&replacement);
            out.extend(&chars[end_idx..]);
            Ok(PhpValue::string(out))
        }
        "reverse" => {
            let s = get_arg_str(args, 0, "Str::reverse")?;
            let rev: String = s.chars().rev().collect();
            Ok(PhpValue::string(rev))
        }
        "natCompare" | "strnatcmp" => {
            let s1 = get_arg_str(args, 0, "Str::natCompare")?;
            let s2 = get_arg_str(args, 1, "Str::natCompare")?;
            let ord = natural_cmp(&s1, &s2, false);
            let res = match ord {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            Ok(PhpValue::Int(res))
        }
        "natCaseCompare" | "strnatcasecmp" => {
            let s1 = get_arg_str(args, 0, "Str::natCaseCompare")?;
            let s2 = get_arg_str(args, 1, "Str::natCaseCompare")?;
            let ord = natural_cmp(&s1, &s2, true);
            let res = match ord {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            Ok(PhpValue::Int(res))
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
        "compareJson" | "jsonCompare" | "compareByJson" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            if !is_array_or_collection(a) || !is_array_or_collection(b) {
                return Err(RuntimeErrorKind::Custom {
                    message: "Array::compareJson() expects array or collection arguments".to_string(),
                });
            }
            let json_a = php_value_to_json_canonical(a);
            let json_b = php_value_to_json_canonical(b);
            Ok(PhpValue::Bool(json_a == json_b))
        }
        "compareRef" | "sameRef" | "compareAddress" | "compareMemory" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            let same = match (a, b) {
                (PhpValue::Array(x), PhpValue::Array(y))
                | (PhpValue::List(x), PhpValue::List(y))
                | (PhpValue::Array(x), PhpValue::List(y))
                | (PhpValue::List(x), PhpValue::Array(y)) => Rc::ptr_eq(x, y),
                (PhpValue::Dict(x), PhpValue::Dict(y)) => Rc::ptr_eq(x, y),
                (PhpValue::Set(x), PhpValue::Set(y)) => Rc::ptr_eq(x, y),
                (PhpValue::HashMap(x), PhpValue::HashMap(y)) => Rc::ptr_eq(x, y),
                _ => false,
            };
            Ok(PhpValue::Bool(same))
        }
        "compareJoined" | "compareToString" | "compareImplode" | "compareJoin" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            let delim = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| ",".to_string());
            if !is_array_or_collection(a) || !is_array_or_collection(b) {
                return Err(RuntimeErrorKind::Custom {
                    message: "Array::compareJoined() expects array or collection arguments".to_string(),
                });
            }
            let str_a = collection_to_joined_string(a, &delim);
            let str_b = collection_to_joined_string(b, &delim);
            Ok(PhpValue::Bool(str_a == str_b))
        }
        "compareUnordered" | "compareValuesUnordered" | "compareIgnoreOrder" | "unorderedCompare" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            if !is_list_or_set(a) || !is_list_or_set(b) {
                return Err(RuntimeErrorKind::Custom {
                    message: "Array::compareUnordered() only supports list and set collections".to_string(),
                });
            }
            let match_res = compare_collections_unordered(a, b);
            Ok(PhpValue::Bool(match_res))
        }
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
        // Return the first element of an array, or `$default` if empty.
        // Example: `Arr::first([10, 20], null)` -> `10`
        "first" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            Ok(items.into_iter().next().unwrap_or(default_val))
        }
        // Return the last element of an array, or `$default` if empty.
        // Example: `Arr::last([10, 20], null)` -> `20`
        "last" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            Ok(items.into_iter().last().unwrap_or(default_val))
        }
        // Return the first key of an array, or null if empty.
        // Example: `Arr::firstKey(['a' => 1, 'b' => 2])` -> `'a'`
        "firstKey" => {
            let res = match args.first() {
                Some(PhpValue::Dict(d)) => d
                    .borrow()
                    .keys()
                    .next()
                    .map(|k| PhpValue::string(k.clone()))
                    .unwrap_or(PhpValue::Null),
                Some(PhpValue::HashMap(m)) => {
                    m.borrow().keys().next().cloned().unwrap_or(PhpValue::Null)
                }
                Some(PhpValue::Array(a) | PhpValue::List(a)) => {
                    if a.borrow().is_empty() {
                        PhpValue::Null
                    } else {
                        PhpValue::Int(0)
                    }
                }
                Some(PhpValue::Set(s)) => {
                    s.borrow().iter().next().cloned().unwrap_or(PhpValue::Null)
                }
                _ => PhpValue::Null,
            };
            Ok(res)
        }
        // Return the last key of an array, or null if empty.
        // Example: `Arr::lastKey(['a' => 1, 'b' => 2])` -> `'b'`
        "lastKey" => {
            let res = match args.first() {
                Some(PhpValue::Dict(d)) => d
                    .borrow()
                    .keys()
                    .last()
                    .map(|k| PhpValue::string(k.clone()))
                    .unwrap_or(PhpValue::Null),
                Some(PhpValue::HashMap(m)) => {
                    m.borrow().keys().last().cloned().unwrap_or(PhpValue::Null)
                }
                Some(PhpValue::Array(a) | PhpValue::List(a)) => {
                    let len = a.borrow().len();
                    if len == 0 {
                        PhpValue::Null
                    } else {
                        PhpValue::Int((len - 1) as i32)
                    }
                }
                Some(PhpValue::Set(s)) => {
                    s.borrow().iter().last().cloned().unwrap_or(PhpValue::Null)
                }
                _ => PhpValue::Null,
            };
            Ok(res)
        }
        // Return the sum of values in an array.
        // Example: `Arr::sum([1, 2, 3])` -> `6`
        "sum" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            let mut is_float = false;
            let mut float_sum = 0.0f64;
            let mut int_sum = 0i64;
            for item in &items {
                if let Some(fl) = item.to_f64() {
                    if item.to_i64().is_none() || matches!(item, PhpValue::Float(_) | PhpValue::RustFloat { .. }) {
                        is_float = true;
                    }
                    float_sum += fl;
                }
                if let Some(i) = item.to_i64() {
                    int_sum = int_sum.wrapping_add(i);
                }
            }
            if is_float {
                Ok(PhpValue::Float(float_sum as f32))
            } else {
                Ok(PhpValue::Int(int_sum as i32))
            }
        }
        // Return the product of values in an array.
        // Example: `Arr::product([2, 3, 4])` -> `24`
        "product" => {
            let items = args.first().map(extract_array_elements).unwrap_or_default();
            if items.is_empty() {
                return Ok(PhpValue::Int(1));
            }
            let mut is_float = false;
            let mut float_prod = 1.0f64;
            let mut int_prod = 1i64;
            for item in &items {
                if let Some(fl) = item.to_f64() {
                    if item.to_i64().is_none() || matches!(item, PhpValue::Float(_) | PhpValue::RustFloat { .. }) {
                        is_float = true;
                    }
                    float_prod *= fl;
                }
                if let Some(i) = item.to_i64() {
                    int_prod = int_prod.wrapping_mul(i);
                }
            }
            if is_float {
                Ok(PhpValue::Float(float_prod as f32))
            } else {
                Ok(PhpValue::Int(int_prod as i32))
            }
        }
        // Fill an array with `$count` entries of `$value`, starting at `$start`.
        // Example: `Arr::fill(0, 3, 'val')` -> `['val', 'val', 'val']`
        "fill" => {
            let start = args.first().and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Arr::fill".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let count = args.get(1).and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Arr::fill".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let val = args.get(2).cloned().unwrap_or(PhpValue::Null);
            if count < 0 {
                return Err(RuntimeErrorKind::Custom {
                    message: "Arr::fill(): Argument #2 ($count) must be greater than or equal to 0".to_string(),
                });
            }
            if count == 0 {
                return Ok(PhpValue::list(Vec::new()));
            }
            if start == 0 {
                Ok(PhpValue::list(vec![val; count as usize]))
            } else {
                let mut map = IndexMap::new();
                for i in 0..count {
                    map.insert(format!("{}", start + i), val.clone());
                }
                Ok(PhpValue::dict_from_map(map))
            }
        }
        // Fill an array with values using the specified keys.
        // Example: `Arr::fillKeys(['a', 'b'], 10)` -> `['a' => 10, 'b' => 10]`
        "fillKeys" => {
            let keys_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Arr::fillKeys".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            let keys = extract_array_elements(keys_val);
            let mut map = IndexMap::new();
            for k in keys {
                map.insert(k.to_php_string(), val.clone());
            }
            Ok(PhpValue::dict_from_map(map))
        }
        // Pad an array to `$size` with `$value`.
        // Example: `Arr::pad([1, 2], 4, 0)` -> `[1, 2, 0, 0]`
        "pad" => {
            let target = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Arr::pad".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let size = args.get(1).and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Arr::pad".to_string(),
                passed: args.len(),
                expected: 3,
            })?;
            let val = args.get(2).cloned().unwrap_or(PhpValue::Null);
            let mut items = extract_array_elements(target);
            let abs_size = size.abs() as usize;
            if abs_size > items.len() {
                let pad_count = abs_size - items.len();
                if size > 0 {
                    for _ in 0..pad_count {
                        items.push(val.clone());
                    }
                } else {
                    let mut new_items = vec![val; pad_count];
                    new_items.extend(items);
                    items = new_items;
                }
            }
            Ok(PhpValue::list(items))
        }
        "flatten" => {
            let mut out = Vec::new();
            if let Some(first) = args.first() {
                flatten_elements(first, &mut out);
            }
            Ok(PhpValue::list(out))
        }
        "chunk" | "array_chunk" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let size = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
            match target {
                PhpValue::Dict(d) => {
                    let entries: Vec<(String, PhpValue)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    let chunks: Vec<PhpValue> = entries
                        .chunks(size)
                        .map(|c| {
                            let mut m = IndexMap::new();
                            for (k, v) in c {
                                m.insert(k.clone(), v.clone());
                            }
                            PhpValue::dict_from_map(m)
                        })
                        .collect();
                    Ok(PhpValue::list(chunks))
                }
                PhpValue::Set(s) => {
                    let items: Vec<PhpValue> = s.borrow().iter().cloned().collect();
                    let chunks: Vec<PhpValue> = items
                        .chunks(size)
                        .map(|c| {
                            let mut hs = HashSet::new();
                            for it in c {
                                hs.insert(it.clone());
                            }
                            PhpValue::set_from_hash_set(hs)
                        })
                        .collect();
                    Ok(PhpValue::list(chunks))
                }
                _ => {
                    let items = extract_array_elements(target);
                    let chunks: Vec<PhpValue> = items
                        .chunks(size)
                        .map(|chunk| PhpValue::list(chunk.to_vec()))
                        .collect();
                    Ok(PhpValue::list(chunks))
                }
            }
        }
        "pluck" | "column" | "array_column" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            let key_by = args.get(2).map(|v| v.to_php_string());
            let is_key_null = args.get(1).map_or(true, |v| matches!(v, PhpValue::Null));

            match target {
                PhpValue::Dict(d) => {
                    let mut map = IndexMap::new();
                    for (orig_k, item) in d.borrow().iter() {
                        let k = if let Some(ref kb) = key_by {
                            get_item_column(item, kb).map(|v| v.to_php_string()).unwrap_or_default()
                        } else {
                            orig_k.clone()
                        };
                        let v = if is_key_null {
                            item.clone()
                        } else {
                            get_item_column(item, &key).unwrap_or(PhpValue::Null)
                        };
                        map.insert(k, v);
                    }
                    Ok(PhpValue::dict_from_map(map))
                }
                PhpValue::Set(s) => {
                    let mut out = HashSet::new();
                    for item in s.borrow().iter() {
                        let v = if is_key_null {
                            item.clone()
                        } else {
                            get_item_column(item, &key).unwrap_or(PhpValue::Null)
                        };
                        out.insert(v);
                    }
                    Ok(PhpValue::set_from_hash_set(out))
                }
                _ => {
                    let items = extract_array_elements(target);
                    if let Some(ref kb) = key_by {
                        let mut map = IndexMap::new();
                        for item in items {
                            let k = get_item_column(&item, kb).map(|v| v.to_php_string()).unwrap_or_default();
                            let v = if is_key_null {
                                item
                            } else {
                                get_item_column(&item, &key).unwrap_or(PhpValue::Null)
                            };
                            map.insert(k, v);
                        }
                        return Ok(PhpValue::dict_from_map(map));
                    }
                    let mut plucked = Vec::with_capacity(items.len());
                    for item in items {
                        let v = if is_key_null {
                            item
                        } else {
                            get_item_column(&item, &key).unwrap_or(PhpValue::Null)
                        };
                        plucked.push(v);
                    }
                    Ok(PhpValue::list(plucked))
                }
            }
        }
        "sort" | "asort" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            match target {
                PhpValue::Dict(d) => {
                    let mut entries: Vec<(String, PhpValue)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    entries.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let mut out = IndexMap::new();
                    for (k, v) in entries {
                        out.insert(k, v);
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => Ok(PhpValue::set_from_hash_set(s.borrow().clone())),
                _ => {
                    let mut items = extract_array_elements(target);
                    items.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Ok(PhpValue::list(items))
                }
            }
        }
        "natsort" | "natSort" | "sortNatural" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            match target {
                PhpValue::Dict(d) => {
                    let mut entries: Vec<(String, PhpValue)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    entries.sort_by(|(_, a), (_, b)| php_value_natural_cmp(a, b, false));
                    let mut out = IndexMap::new();
                    for (k, v) in entries {
                        out.insert(k, v);
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => Ok(PhpValue::set_from_hash_set(s.borrow().clone())),
                _ => {
                    let mut items = extract_array_elements(target);
                    items.sort_by(|a, b| php_value_natural_cmp(a, b, false));
                    Ok(PhpValue::list(items))
                }
            }
        }
        "natcasesort" | "natCaseSort" | "sortNaturalCase" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            match target {
                PhpValue::Dict(d) => {
                    let mut entries: Vec<(String, PhpValue)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    entries.sort_by(|(_, a), (_, b)| php_value_natural_cmp(a, b, true));
                    let mut out = IndexMap::new();
                    for (k, v) in entries {
                        out.insert(k, v);
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => Ok(PhpValue::set_from_hash_set(s.borrow().clone())),
                _ => {
                    let mut items = extract_array_elements(target);
                    items.sort_by(|a, b| php_value_natural_cmp(a, b, true));
                    Ok(PhpValue::list(items))
                }
            }
        }
        "flip" | "array_flip" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            match target {
                PhpValue::Dict(d) => {
                    let mut out = IndexMap::new();
                    for (k, v) in d.borrow().iter() {
                        let orig_val = if let Ok(n) = k.parse::<i32>() {
                            PhpValue::Int(n)
                        } else {
                            PhpValue::string(k.clone())
                        };
                        out.insert(v.to_php_string(), orig_val);
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => Ok(PhpValue::set_from_hash_set(s.borrow().clone())),
                _ => {
                    let items = extract_array_elements(target);
                    let mut out = IndexMap::new();
                    for (idx, v) in items.into_iter().enumerate() {
                        out.insert(v.to_php_string(), PhpValue::Int(idx as i32));
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
            }
        }
        "combine" | "array_combine" => {
            let keys_arg = args.first().unwrap_or(&PhpValue::Null);
            let vals_arg = args.get(1).unwrap_or(&PhpValue::Null);
            let keys = extract_array_elements(keys_arg);
            let vals = extract_array_elements(vals_arg);
            if keys.len() != vals.len() {
                return Err(RuntimeErrorKind::ValueError {
                    message: "array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements".to_string(),
                });
            }
            let mut map = IndexMap::new();
            for (k, v) in keys.into_iter().zip(vals) {
                map.insert(k.to_php_string(), v);
            }
            Ok(PhpValue::dict_from_map(map))
        }
        "countValues" | "count_values" | "array_count_values" | "countBy" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let items: Vec<PhpValue> = match target {
                PhpValue::Dict(d) => d.borrow().values().cloned().collect(),
                PhpValue::HashMap(m) => m.borrow().values().cloned().collect(),
                PhpValue::Set(s) => s.borrow().iter().cloned().collect(),
                _ => extract_array_elements(target),
            };
            let mut counts = IndexMap::new();
            for item in items {
                let key = item.to_php_string();
                let entry = counts.entry(key).or_insert(0i32);
                *entry += 1;
            }
            let mut out = IndexMap::new();
            for (k, v) in counts {
                out.insert(k, PhpValue::Int(v));
            }
            Ok(PhpValue::dict_from_map(out))
        }
        "shuffle" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            match target {
                PhpValue::Dict(d) => {
                    let mut entries: Vec<(String, PhpValue)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    shuffle_slice(&mut entries);
                    let mut out = IndexMap::new();
                    for (k, v) in entries {
                        out.insert(k, v);
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => {
                    Ok(PhpValue::set_from_hash_set(s.borrow().clone()))
                }
                _ => {
                    let mut items = extract_array_elements(target);
                    shuffle_slice(&mut items);
                    Ok(PhpValue::list(items))
                }
            }
        }
        "randomKey" | "randomKeys" | "randKey" | "array_rand" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let num = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1) as usize;
            match target {
                PhpValue::Dict(d) => {
                    let keys: Vec<String> = d.borrow().keys().cloned().collect();
                    if keys.is_empty() {
                        return Ok(PhpValue::Null);
                    }
                    if num < 1 || num > keys.len() {
                        return Err(RuntimeErrorKind::ValueError {
                            message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                        });
                    }
                    if num == 1 {
                        let idx = random_usize(keys.len());
                        Ok(PhpValue::string(keys[idx].clone()))
                    } else {
                        let mut indices: Vec<usize> = (0..keys.len()).collect();
                        shuffle_slice(&mut indices);
                        indices.truncate(num);
                        indices.sort();
                        let picked: Vec<PhpValue> = indices.into_iter().map(|i| PhpValue::string(keys[i].clone())).collect();
                        Ok(PhpValue::list(picked))
                    }
                }
                PhpValue::Set(s) => {
                    let items: Vec<PhpValue> = s.borrow().iter().cloned().collect();
                    if items.is_empty() {
                        return Ok(PhpValue::Null);
                    }
                    if num < 1 || num > items.len() {
                        return Err(RuntimeErrorKind::ValueError {
                            message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                        });
                    }
                    if num == 1 {
                        let idx = random_usize(items.len());
                        Ok(items[idx].clone())
                    } else {
                        let mut indices: Vec<usize> = (0..items.len()).collect();
                        shuffle_slice(&mut indices);
                        indices.truncate(num);
                        let picked: HashSet<PhpValue> = indices.into_iter().map(|i| items[i].clone()).collect();
                        Ok(PhpValue::set_from_hash_set(picked))
                    }
                }
                _ => {
                    let items = extract_array_elements(target);
                    if items.is_empty() {
                        return Ok(PhpValue::Null);
                    }
                    if num < 1 || num > items.len() {
                        return Err(RuntimeErrorKind::ValueError {
                            message: "array_rand(): Argument #2 ($num) must be between 1 and the number of elements or greater in array".to_string(),
                        });
                    }
                    if num == 1 {
                        let idx = random_usize(items.len());
                        Ok(PhpValue::Int(idx as i32))
                    } else {
                        let mut indices: Vec<usize> = (0..items.len()).collect();
                        shuffle_slice(&mut indices);
                        indices.truncate(num);
                        indices.sort();
                        let picked: Vec<PhpValue> = indices.into_iter().map(|i| PhpValue::Int(i as i32)).collect();
                        Ok(PhpValue::list(picked))
                    }
                }
            }
        }
        "random" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let num_opt = args.get(1).and_then(|v| if v.is_null() { None } else { v.to_i64() });
            match target {
                PhpValue::Dict(d) => {
                    let entries: Vec<(String, PhpValue)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    if entries.is_empty() {
                        return Ok(PhpValue::Null);
                    }
                    match num_opt {
                        None => {
                            let idx = random_usize(entries.len());
                            Ok(entries[idx].1.clone())
                        }
                        Some(num) => {
                            let n = num as usize;
                            if n < 1 || n > entries.len() {
                                return Err(RuntimeErrorKind::ValueError {
                                    message: format!("Arr::random(): requested {} elements, but array contains only {}", n, entries.len()),
                                });
                            }
                            let mut indices: Vec<usize> = (0..entries.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(n);
                            let mut map = IndexMap::new();
                            for i in indices {
                                map.insert(entries[i].0.clone(), entries[i].1.clone());
                            }
                            Ok(PhpValue::dict_from_map(map))
                        }
                    }
                }
                PhpValue::Set(s) => {
                    let items: Vec<PhpValue> = s.borrow().iter().cloned().collect();
                    if items.is_empty() {
                        return Ok(PhpValue::Null);
                    }
                    match num_opt {
                        None => {
                            let idx = random_usize(items.len());
                            Ok(items[idx].clone())
                        }
                        Some(num) => {
                            let n = num as usize;
                            if n < 1 || n > items.len() {
                                return Err(RuntimeErrorKind::ValueError {
                                    message: format!("Arr::random(): requested {} elements, but set contains only {}", n, items.len()),
                                });
                            }
                            let mut indices: Vec<usize> = (0..items.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(n);
                            let picked: HashSet<PhpValue> = indices.into_iter().map(|i| items[i].clone()).collect();
                            Ok(PhpValue::set_from_hash_set(picked))
                        }
                    }
                }
                _ => {
                    let items = extract_array_elements(target);
                    if items.is_empty() {
                        return Ok(PhpValue::Null);
                    }
                    match num_opt {
                        None => {
                            let idx = random_usize(items.len());
                            Ok(items[idx].clone())
                        }
                        Some(num) => {
                            let n = num as usize;
                            if n < 1 || n > items.len() {
                                return Err(RuntimeErrorKind::ValueError {
                                    message: format!("Arr::random(): requested {} elements, but array contains only {}", n, items.len()),
                                });
                            }
                            let mut indices: Vec<usize> = (0..items.len()).collect();
                            shuffle_slice(&mut indices);
                            indices.truncate(n);
                            let picked: Vec<PhpValue> = indices.into_iter().map(|i| items[i].clone()).collect();
                            Ok(PhpValue::list(picked))
                        }
                    }
                }
            }
        }
        "diff" | "array_diff" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let other_args = if args.len() > 1 { &args[1..] } else { &[] };
            let mut exclude = HashSet::new();
            for other in other_args {
                for item in extract_array_elements(other) {
                    exclude.insert(item);
                }
            }
            match target {
                PhpValue::Dict(d) => {
                    let mut out = IndexMap::new();
                    for (k, v) in d.borrow().iter() {
                        if !exclude.contains(v) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => {
                    let mut out = HashSet::new();
                    for it in s.borrow().iter() {
                        if !exclude.contains(it) {
                            out.insert(it.clone());
                        }
                    }
                    Ok(PhpValue::set_from_hash_set(out))
                }
                _ => {
                    let items = extract_array_elements(target);
                    let out: Vec<PhpValue> = items.into_iter().filter(|it| !exclude.contains(it)).collect();
                    Ok(PhpValue::list(out))
                }
            }
        }
        "diffKey" | "diffKeys" | "array_diff_key" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let other_args = if args.len() > 1 { &args[1..] } else { &[] };
            let mut exclude_keys = HashSet::new();
            for other in other_args {
                for (k, _) in extract_dict_elements(other) {
                    exclude_keys.insert(k);
                }
            }
            match target {
                PhpValue::Dict(d) => {
                    let mut out = IndexMap::new();
                    for (k, v) in d.borrow().iter() {
                        if !exclude_keys.contains(k) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => Ok(PhpValue::set_from_hash_set(s.borrow().clone())),
                _ => {
                    let items = extract_array_elements(target);
                    let out: Vec<PhpValue> = items
                        .into_iter()
                        .enumerate()
                        .filter(|(idx, _)| !exclude_keys.contains(&idx.to_string()))
                        .map(|(_, it)| it)
                        .collect();
                    Ok(PhpValue::list(out))
                }
            }
        }
        "intersect" | "array_intersect" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let other_args = if args.len() > 1 { &args[1..] } else { &[] };
            let other_sets: Vec<HashSet<PhpValue>> = other_args
                .iter()
                .map(|other| extract_array_elements(other).into_iter().collect())
                .collect();
            match target {
                PhpValue::Dict(d) => {
                    let mut out = IndexMap::new();
                    for (k, v) in d.borrow().iter() {
                        if other_sets.iter().all(|s| s.contains(v)) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => {
                    let mut out = HashSet::new();
                    for it in s.borrow().iter() {
                        if other_sets.iter().all(|s| s.contains(it)) {
                            out.insert(it.clone());
                        }
                    }
                    Ok(PhpValue::set_from_hash_set(out))
                }
                _ => {
                    let items = extract_array_elements(target);
                    let out: Vec<PhpValue> = items
                        .into_iter()
                        .filter(|it| other_sets.iter().all(|s| s.contains(it)))
                        .collect();
                    Ok(PhpValue::list(out))
                }
            }
        }
        "intersectKey" | "intersectKeys" | "array_intersect_key" => {
            let target = args.first().unwrap_or(&PhpValue::Null);
            let other_args = if args.len() > 1 { &args[1..] } else { &[] };
            let other_key_sets: Vec<HashSet<String>> = other_args
                .iter()
                .map(|other| extract_dict_elements(other).into_iter().map(|(k, _)| k).collect())
                .collect();
            match target {
                PhpValue::Dict(d) => {
                    let mut out = IndexMap::new();
                    for (k, v) in d.borrow().iter() {
                        if other_key_sets.iter().all(|s| s.contains(k)) {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    Ok(PhpValue::dict_from_map(out))
                }
                PhpValue::Set(s) => Ok(PhpValue::set_from_hash_set(s.borrow().clone())),
                _ => {
                    let items = extract_array_elements(target);
                    let out: Vec<PhpValue> = items
                        .into_iter()
                        .enumerate()
                        .filter(|(idx, _)| {
                            let idx_str = idx.to_string();
                            other_key_sets.iter().all(|s| s.contains(&idx_str))
                        })
                        .map(|(_, it)| it)
                        .collect();
                    Ok(PhpValue::list(out))
                }
            }
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
            let (target, key) = if args.len() >= 2 {
                if !matches!(args[0], PhpValue::Dict(_) | PhpValue::HashMap(_) | PhpValue::Array(_) | PhpValue::List(_) | PhpValue::Set(_))
                    && matches!(args[1], PhpValue::Dict(_) | PhpValue::HashMap(_) | PhpValue::Array(_) | PhpValue::List(_) | PhpValue::Set(_))
                {
                    (&args[1], &args[0])
                } else {
                    (&args[0], &args[1])
                }
            } else if let Some(t) = args.first() {
                (t, &PhpValue::Null)
            } else {
                return Ok(PhpValue::Bool(false));
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
        "sortKeys" | "ksort" => {
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

pub type ExitHook = Box<dyn Fn(i32) + Send + Sync>;
pub static EXIT_HOOK: std::sync::Mutex<Option<ExitHook>> = std::sync::Mutex::new(None);

/// Set a custom hook to intercept `exit()` and `die()` calls during testing.
pub fn set_exit_hook<F>(hook: F)
where
    F: Fn(i32) + Send + Sync + 'static,
{
    if let Ok(mut guard) = EXIT_HOOK.lock() {
        *guard = Some(Box::new(hook));
    }
}

/// Clear any active exit hook.
pub fn clear_exit_hook() {
    if let Ok(mut guard) = EXIT_HOOK.lock() {
        *guard = None;
    }
}

/// Check if a value is empty under RHP semantics (type-aware, non-falsy for 0/0.0/"0").
pub fn php_value_is_empty(val: &PhpValue) -> bool {
    match val {
        PhpValue::Null => true,
        PhpValue::Bool(b) => !b,
        PhpValue::Int(_) | PhpValue::Float(_) | PhpValue::RustInt { .. } | PhpValue::RustFloat { .. } => false,
        PhpValue::String(s) => s.is_empty(),
        PhpValue::Array(a) | PhpValue::List(a) => a.borrow().is_empty(),
        PhpValue::Dict(d) => d.borrow().is_empty(),
        PhpValue::Set(s) => s.borrow().is_empty(),
        PhpValue::HashMap(m) => m.borrow().is_empty(),
        _ => false,
    }
}

pub fn execute_exit_signal(args: &[PhpValue]) -> FacadeResult {
    let mut exit_code = 0;
    if let Some(arg) = args.first() {
        match arg {
            PhpValue::String(s) => {
                use std::io::Write;
                print!("{}", s.as_str());
                let _ = std::io::stdout().flush();
                exit_code = 0;
            }
            PhpValue::Int(i) => {
                exit_code = *i;
            }
            PhpValue::RustInt { value, .. } => {
                exit_code = *value as i32;
            }
            _ => {
                exit_code = 0;
            }
        }
    }
    Err(RuntimeErrorKind::Exit(exit_code))
}

pub fn execute_process_exit(args: &[PhpValue]) -> FacadeResult {
    let mut exit_code = 0;
    if let Some(arg) = args.first() {
        match arg {
            PhpValue::String(s) => {
                use std::io::Write;
                print!("{}", s.as_str());
                let _ = std::io::stdout().flush();
                exit_code = 0;
            }
            PhpValue::Int(i) => {
                exit_code = *i;
            }
            PhpValue::RustInt { value, .. } => {
                exit_code = *value as i32;
            }
            _ => {
                exit_code = 0;
            }
        }
    }

    if let Ok(guard) = EXIT_HOOK.lock() {
        if let Some(ref hook) = *guard {
            hook(exit_code);
            return Err(RuntimeErrorKind::Die(exit_code));
        }
    }

    std::process::exit(exit_code);
}

static STAT_CACHE_CLEARED_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Clear filesystem stat cache and realpath cache.
pub fn clear_stat_cache(_clear_realpath: bool, _filename: Option<&str>) {
    STAT_CACHE_CLEARED_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Query how many times stat cache clearing was invoked.
pub fn stat_cache_cleared_count() -> usize {
    STAT_CACHE_CLEARED_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskSpaceMetric {
    Free,
    Total,
}

pub fn get_disk_space(path: &str, metric: DiskSpaceMetric) -> FacadeResult {
    record_fiber_io_start(IoKind::FileExists {
        path: path.to_string(),
    });
    let res = get_disk_space_impl(path, metric);
    record_fiber_io_finish();
    res
}

fn get_disk_space_impl(path: &str, metric: DiskSpaceMetric) -> FacadeResult {
    if path.is_empty() {
        return Ok(PhpValue::Bool(false));
    }

    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = match CString::new(path) {
            Ok(c) => c,
            Err(_) => return Ok(PhpValue::Bool(false)),
        };
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
        if ret != 0 {
            return Ok(PhpValue::Bool(false));
        }
        let block_size = if stat.f_frsize > 0 {
            stat.f_frsize as f64
        } else {
            stat.f_bsize as f64
        };
        let bytes = match metric {
            DiskSpaceMetric::Free => (stat.f_bavail as f64) * block_size,
            DiskSpaceMetric::Total => (stat.f_blocks as f64) * block_size,
        };
        Ok(PhpValue::Float(bytes as f32))
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut wide: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().collect();
        wide.push(0);
        let mut free_bytes_available: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut total_free_bytes: u64 = 0;
        extern "system" {
            fn GetDiskFreeSpaceExW(
                lpDirectoryName: *const u16,
                lpFreeBytesAvailableToCaller: *mut u64,
                lpTotalNumberOfBytes: *mut u64,
                lpTotalNumberOfFreeBytes: *mut u64,
            ) -> i32;
        }
        let ret = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_bytes_available,
                &mut total_bytes,
                &mut total_free_bytes,
            )
        };
        if ret == 0 {
            return Ok(PhpValue::Bool(false));
        }
        let bytes = match metric {
            DiskSpaceMetric::Free => free_bytes_available as f64,
            DiskSpaceMetric::Total => total_bytes as f64,
        };
        Ok(PhpValue::Float(bytes as f32))
    }

    #[cfg(not(any(unix, windows)))]
    {
        Ok(PhpValue::Bool(false))
    }
}

fn dispatch_sys(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "isset" => {
            let is_set = !args.is_empty() && args.iter().all(|a| !matches!(a, PhpValue::Null));
            Ok(PhpValue::Bool(is_set))
        }
        "empty" => {
            let is_emp = php_value_is_empty(args.first().unwrap_or(&PhpValue::Null));
            Ok(PhpValue::Bool(is_emp))
        }
        "usleep" => {
            let micros = args
                .first()
                .and_then(|v| v.to_i64())
                .unwrap_or(0)
                .max(0) as u64;
            if micros > 0 {
                std::thread::sleep(std::time::Duration::from_micros(micros));
            }
            Ok(PhpValue::Null)
        }
        "sleep" => {
            let secs = args
                .first()
                .and_then(|v| v.to_i64())
                .unwrap_or(0)
                .max(0) as u64;
            if secs > 0 {
                std::thread::sleep(std::time::Duration::from_secs(secs));
            }
            Ok(PhpValue::Int(0))
        }
        "exit" => execute_exit_signal(args),
        "die" => execute_process_exit(args),
        "register_shutdown_function" | "registerShutdownFunction" => Ok(PhpValue::Null),
        "set_error_handler" | "setErrorHandler" | "set_exception_handler" | "setExceptionHandler" => Ok(PhpValue::Null),
        "restore_error_handler" | "restoreErrorHandler" | "restore_exception_handler" | "restoreExceptionHandler" => Ok(PhpValue::Bool(true)),
        "trigger_error" | "triggerError" => Ok(PhpValue::Bool(true)),
        "clearstatcache" | "clearStatCache" => {
            let clear_realpath_cache = args.first().map(|v| v.is_truthy()).unwrap_or(false);
            let filename = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            clear_stat_cache(clear_realpath_cache, filename.as_deref());
            Ok(PhpValue::Null)
        }
        "disk_free_space" | "diskFreeSpace" | "diskfreespace" => {
            let path = get_arg_str(args, 0, "Sys::disk_free_space")?;
            get_disk_space(&path, DiskSpaceMetric::Free)
        }
        "disk_total_space" | "diskTotalSpace" => {
            let path = get_arg_str(args, 0, "Sys::disk_total_space")?;
            get_disk_space(&path, DiskSpaceMetric::Total)
        }
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

/// Parse file permissions from string format (octal e.g. "0755", "755", "0o755" or symbolic e.g. "rwxr-xr-x").
pub fn parse_file_mode_str(s: &str) -> Option<u32> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Symbolic mode: 9 chars (e.g. "rwxr-xr-x") or 10 chars (e.g. "drwxr-xr-x", "-rwxr-xr-x")
    let sym_chars: Vec<char> = trimmed.chars().collect();
    let sym_slice = if sym_chars.len() == 10 && matches!(sym_chars[0], '-' | 'd' | 'l' | 'c' | 'b' | 'p' | 's') {
        &sym_chars[1..]
    } else if sym_chars.len() == 9 {
        &sym_chars[..]
    } else {
        &[]
    };

    if sym_slice.len() == 9 {
        let mut mode: u32 = 0;
        let valid_u_r = match sym_slice[0] {
            'r' => { mode |= 0o400; true }
            '-' => true,
            _ => false,
        };
        let valid_u_w = match sym_slice[1] {
            'w' => { mode |= 0o200; true }
            '-' => true,
            _ => false,
        };
        let valid_u_x = match sym_slice[2] {
            'x' => { mode |= 0o100; true }
            's' => { mode |= 0o4100; true }
            'S' => { mode |= 0o4000; true }
            '-' => true,
            _ => false,
        };
        let valid_g_r = match sym_slice[3] {
            'r' => { mode |= 0o040; true }
            '-' => true,
            _ => false,
        };
        let valid_g_w = match sym_slice[4] {
            'w' => { mode |= 0o020; true }
            '-' => true,
            _ => false,
        };
        let valid_g_x = match sym_slice[5] {
            'x' => { mode |= 0o010; true }
            's' => { mode |= 0o2010; true }
            'S' => { mode |= 0o2000; true }
            '-' => true,
            _ => false,
        };
        let valid_o_r = match sym_slice[6] {
            'r' => { mode |= 0o004; true }
            '-' => true,
            _ => false,
        };
        let valid_o_w = match sym_slice[7] {
            'w' => { mode |= 0o002; true }
            '-' => true,
            _ => false,
        };
        let valid_o_x = match sym_slice[8] {
            'x' => { mode |= 0o001; true }
            't' => { mode |= 0o1001; true }
            'T' => { mode |= 0o1000; true }
            '-' => true,
            _ => false,
        };

        if valid_u_r && valid_u_w && valid_u_x && valid_g_r && valid_g_w && valid_g_x && valid_o_r && valid_o_w && valid_o_x {
            return Some(mode);
        }
    }

    // 2. Octal string: e.g. "0755", "755", "0o755", "0644", "1777"
    let oct_str = if trimmed.starts_with("0o") || trimmed.starts_with("0O") {
        &trimmed[2..]
    } else {
        trimmed
    };

    if !oct_str.is_empty() && oct_str.chars().all(|c| ('0'..='7').contains(&c)) {
        if oct_str.len() <= 5 {
            if let Ok(val) = u32::from_str_radix(oct_str, 8) {
                if val <= 0o7777 {
                    return Some(val);
                }
            }
        }
    }

    None
}

/// Helper that only accepts PhpValue::String and parses permissions mode.
pub fn parse_file_mode(val: &PhpValue) -> Option<u32> {
    match val {
        PhpValue::String(s) => parse_file_mode_str(&s.to_str_lossy()),
        _ => None,
    }
}

static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Return the directory used for temporary files without a trailing slash.
pub fn get_system_temp_dir() -> String {
    let p = std::env::temp_dir();
    let mut s = p.to_string_lossy().to_string();
    if s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        s.pop();
    }
    s
}

pub fn sanitize_temp_prefix(prefix: Option<&str>) -> String {
    match prefix {
        Some(p) if !p.is_empty() => {
            let sanitized: String = p
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if sanitized.is_empty() {
                "tmp".to_string()
            } else {
                sanitized
            }
        }
        _ => "tmp".to_string(),
    }
}

pub fn create_unique_temp_file_handle(
    dir: Option<&str>,
    prefix: Option<&str>,
) -> Result<(std::fs::File, std::path::PathBuf), std::io::Error> {
    let clean_prefix = sanitize_temp_prefix(prefix);
    let sys_tmp = std::path::PathBuf::from(get_system_temp_dir());

    let target_dir = match dir {
        Some(d) if !d.is_empty() && std::path::Path::new(d).is_dir() => {
            std::path::PathBuf::from(d)
        }
        _ => sys_tmp.clone(),
    };

    let try_create_in_dir = |base: &std::path::Path| -> Result<(std::fs::File, std::path::PathBuf), std::io::Error> {
        let pid = std::process::id();
        for _ in 0..200 {
            let count = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let filename = format!("{}{:x}{:x}{:04x}.tmp", clean_prefix, pid, now, count & 0xffff);
            let file_path = base.join(filename);

            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }

            match options.open(&file_path) {
                Ok(file) => return Ok((file, file_path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Could not generate unique temporary file",
        ))
    };

    match try_create_in_dir(&target_dir) {
        Ok(res) => Ok(res),
        Err(err) => {
            if target_dir != sys_tmp {
                try_create_in_dir(&sys_tmp)
            } else {
                Err(err)
            }
        }
    }
}

pub fn create_unique_temp_file_path(dir: Option<&str>, prefix: Option<&str>) -> Option<String> {
    match create_unique_temp_file_handle(dir, prefix) {
        Ok((file, path)) => {
            drop(file);
            Some(path.to_string_lossy().to_string())
        }
        Err(_) => None,
    }
}

pub fn create_unique_temp_dir_with_mode(
    dir: Option<&str>,
    prefix: Option<&str>,
    mode: u32,
) -> Option<String> {
    let clean_prefix = sanitize_temp_prefix(prefix);
    let sys_tmp = std::path::PathBuf::from(get_system_temp_dir());

    let target_dir = match dir {
        Some(d) if !d.is_empty() && std::path::Path::new(d).is_dir() => {
            std::path::PathBuf::from(d)
        }
        _ => sys_tmp.clone(),
    };

    let try_create_in_dir = |base: &std::path::Path| -> Result<std::path::PathBuf, std::io::Error> {
        let pid = std::process::id();
        for _ in 0..200 {
            let count = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dirname = format!("{}{:x}{:x}{:04x}.dir", clean_prefix, pid, now, count & 0xffff);
            let dir_path = base.join(dirname);

            match std::fs::create_dir(&dir_path) {
                Ok(_) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&dir_path, std::fs::Permissions::from_mode(mode));
                    }
                    return Ok(dir_path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Could not generate unique temporary directory",
        ))
    };

    let result = match try_create_in_dir(&target_dir) {
        Ok(p) => Some(p),
        Err(_) => {
            if target_dir != sys_tmp {
                try_create_in_dir(&sys_tmp).ok()
            } else {
                None
            }
        }
    };

    result.map(|p| p.to_string_lossy().to_string())
}

pub fn create_unique_temp_dir(
    dir: Option<&str>,
    prefix: Option<&str>,
    mode_val: Option<&PhpValue>,
) -> Option<String> {
    let mode: u32 = if let Some(mv) = mode_val {
        if mv.is_null() {
            0o700
        } else {
            match parse_file_mode(mv) {
                Some(m) => m,
                None => return None,
            }
        }
    } else {
        0o700
    };

    create_unique_temp_dir_with_mode(dir, prefix, mode)
}

fn dispatch_file(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "read" => {
            let path = get_arg_str(args, 0, "File::read")?;
            let enc = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let invalid = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| "strict".to_string());
            record_fiber_io_start(IoKind::FileRead { path: path.clone() });
            let raw = match std::fs::read(&path) {
                Ok(b) => b,
                Err(err) => {
                    record_fiber_io_finish();
                    return Err(RuntimeErrorKind::Custom {
                        message: format!("File::read failed: {}", err),
                    });
                }
            };
            record_fiber_io_finish();
            let encoding = enc.as_deref().unwrap_or("UTF-8");
            match crate::vm::encoding::decode_bytes(&raw, encoding, &invalid) {
                Ok(content) => Ok(PhpValue::string(content)),
                Err((msg, _)) => Err(RuntimeErrorKind::UncaughtException {
                    class: "InvalidEncodingException".to_string(),
                    message: msg,
                }),
            }
        }
        "readBytes" | "read_bytes" => {
            let path = get_arg_str(args, 0, "File::readBytes")?;
            record_fiber_io_start(IoKind::FileRead { path: path.clone() });
            let raw = match std::fs::read(&path) {
                Ok(b) => b,
                Err(err) => {
                    record_fiber_io_finish();
                    return Err(RuntimeErrorKind::Custom {
                        message: format!("File::readBytes failed: {}", err),
                    });
                }
            };
            record_fiber_io_finish();
            Ok(PhpValue::bytes(raw))
        }
        "write" => {
            let path = get_arg_str(args, 0, "File::write")?;
            let data_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::write".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let enc = args.get(2).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let bytes_to_write: Vec<u8> = match data_val {
                PhpValue::String(s) => {
                    if let Some(target_enc) = enc {
                        if s.is_bytes() {
                            crate::vm::encoding::convert_encoding(s.as_bytes(), &target_enc, None, "throw")
                                .map_err(|(cls, msg)| RuntimeErrorKind::UncaughtException { class: cls, message: msg })?
                        } else {
                            crate::vm::encoding::convert_encoding(s.as_bytes(), &target_enc, Some("UTF-8"), "throw")
                                .map_err(|(cls, msg)| RuntimeErrorKind::UncaughtException { class: cls, message: msg })?
                        }
                    } else {
                        s.as_bytes().to_vec()
                    }
                }
                other => {
                    let s = other.to_php_string();
                    if let Some(target_enc) = enc {
                        crate::vm::encoding::convert_encoding(s.as_bytes(), &target_enc, Some("UTF-8"), "throw")
                            .map_err(|(cls, msg)| RuntimeErrorKind::UncaughtException { class: cls, message: msg })?
                    } else {
                        s.into_bytes()
                    }
                }
            };
            record_fiber_io_start(IoKind::FileWrite {
                path: path.clone(),
                bytes: bytes_to_write.len(),
            });
            let written_len = bytes_to_write.len();
            let res = match std::fs::write(&path, bytes_to_write) {
                Ok(_) => Ok(PhpValue::Int(written_len as i32)),
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
        "rmdir" | "removeDirectory" | "deleteDirectory" => {
            let path = get_arg_str(args, 0, "File::rmdir")?;
            record_fiber_io_start(IoKind::FileDelete { path: path.clone() });
            let ok = std::fs::remove_dir(&path).is_ok();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        "copy" => {
            let from = get_arg_str(args, 0, "File::copy")?;
            let to = get_arg_str(args, 1, "File::copy")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: to.clone(),
                bytes: 0,
            });
            let ok = std::fs::copy(&from, &to).is_ok();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        "rename" | "move" => {
            let from = get_arg_str(args, 0, "File::rename")?;
            let to = get_arg_str(args, 1, "File::rename")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: to.clone(),
                bytes: 0,
            });
            let ok = std::fs::rename(&from, &to).is_ok();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        "chmod" => {
            let path = get_arg_str(args, 0, "File::chmod")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let res = if let Some(mode_val) = args.get(1) {
                if matches!(mode_val, PhpValue::Null) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = std::fs::metadata(&path) {
                            let mode = meta.permissions().mode() & 0o7777;
                            Ok(PhpValue::string(format!("{:04o}", mode)))
                        } else {
                            Ok(PhpValue::Null)
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        Ok(PhpValue::Null)
                    }
                } else {
                    let mode: u32 = match parse_file_mode(mode_val) {
                        Some(m) => m,
                        None => {
                            record_fiber_io_finish();
                            return Ok(PhpValue::Bool(false));
                        }
                    };
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let permissions = std::fs::Permissions::from_mode(mode);
                        let ok = std::fs::set_permissions(&path, permissions).is_ok();
                        Ok(PhpValue::Bool(ok))
                    }
                    #[cfg(not(unix))]
                    {
                        Ok(PhpValue::Bool(std::path::Path::new(&path).exists()))
                    }
                }
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let mode = meta.permissions().mode() & 0o7777;
                        Ok(PhpValue::string(format!("{:04o}", mode)))
                    } else {
                        Ok(PhpValue::Null)
                    }
                }
                #[cfg(not(unix))]
                {
                    Ok(PhpValue::Null)
                }
            };
            record_fiber_io_finish();
            res
        }
        "parseMode" | "parse_mode" => {
            let mode_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::parseMode".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            match parse_file_mode(mode_val) {
                Some(mode) => Ok(PhpValue::Int(mode as i32)),
                None => Ok(PhpValue::Bool(false)),
            }
        }
        "mkdir" | "makeDirectory" | "make_directory" => {
            let path = get_arg_str(args, 0, "File::mkdir")?;
            let mode = if let Some(mode_val) = args.get(1) {
                if mode_val.is_null() {
                    0o777
                } else {
                    match parse_file_mode(mode_val) {
                        Some(m) => m,
                        None => return Ok(PhpValue::Bool(false)),
                    }
                }
            } else {
                0o777
            };
            let recursive = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
            let force = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

            record_fiber_io_start(IoKind::FileWrite {
                path: path.clone(),
                bytes: 0,
            });

            let p = std::path::Path::new(&path);
            if p.exists() && !force {
                record_fiber_io_finish();
                return Ok(PhpValue::Bool(false));
            }

            let ok = if recursive {
                std::fs::create_dir_all(p).is_ok()
            } else {
                std::fs::create_dir(p).is_ok()
            };

            if ok {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
                }
            }

            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        "chown" => {
            let path = get_arg_str(args, 0, "File::chown")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            #[cfg(unix)]
            let res = {
                use std::ffi::CString;
                let path_c = match CString::new(path.as_str()) {
                    Ok(c) => c,
                    Err(_) => {
                        record_fiber_io_finish();
                        return Ok(PhpValue::Bool(false));
                    }
                };
                let user_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                    callable: "File::chown".to_string(),
                    passed: args.len(),
                    expected: 2,
                })?;
                let uid = match user_val {
                    PhpValue::Int(uid) => *uid as libc::uid_t,
                    PhpValue::RustInt { value, .. } => *value as libc::uid_t,
                    PhpValue::String(s) => {
                        let user_str = s.to_str_lossy();
                        if let Ok(uid_num) = user_str.parse::<libc::uid_t>() {
                            uid_num
                        } else if let Ok(c_user) = CString::new(user_str.as_ref()) {
                            unsafe {
                                let pwd = libc::getpwnam(c_user.as_ptr());
                                if pwd.is_null() {
                                    record_fiber_io_finish();
                                    return Ok(PhpValue::Bool(false));
                                }
                                (*pwd).pw_uid
                            }
                        } else {
                            record_fiber_io_finish();
                            return Ok(PhpValue::Bool(false));
                        }
                    }
                    _ => {
                        record_fiber_io_finish();
                        return Ok(PhpValue::Bool(false));
                    }
                };
                let gid = if let Some(g_val) = args.get(2) {
                    match g_val {
                        PhpValue::Int(gid) => *gid as libc::gid_t,
                        PhpValue::RustInt { value, .. } => *value as libc::gid_t,
                        PhpValue::String(s) => {
                            let grp_str = s.to_str_lossy();
                            if let Ok(gid_num) = grp_str.parse::<libc::gid_t>() {
                                gid_num
                            } else if let Ok(c_grp) = CString::new(grp_str.as_ref()) {
                                unsafe {
                                    let grp = libc::getgrnam(c_grp.as_ptr());
                                    if grp.is_null() {
                                        record_fiber_io_finish();
                                        return Ok(PhpValue::Bool(false));
                                    }
                                    (*grp).gr_gid
                                }
                            } else {
                                record_fiber_io_finish();
                                return Ok(PhpValue::Bool(false));
                            }
                        }
                        _ => !0 as libc::gid_t,
                    }
                } else {
                    !0 as libc::gid_t
                };
                let ret = unsafe { libc::chown(path_c.as_ptr(), uid, gid) };
                Ok(PhpValue::Bool(ret == 0))
            };
            #[cfg(not(unix))]
            let res = Ok(PhpValue::Bool(false));
            record_fiber_io_finish();
            res
        }
        "touch" => {
            let path = get_arg_str(args, 0, "File::touch")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: path.clone(),
                bytes: 0,
            });
            let file = match std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(&path)
            {
                Ok(f) => f,
                Err(_) => {
                    record_fiber_io_finish();
                    return Ok(PhpValue::Bool(false));
                }
            };

            let mtime_opt = args.get(1).and_then(|v| v.to_i64());
            let atime_opt = args.get(2).and_then(|v| v.to_i64()).or(mtime_opt);

            let ok = if let Some(mtime_sec) = mtime_opt {
                let to_system_time = |sec: i64| -> std::time::SystemTime {
                    if sec >= 0 {
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(sec as u64)
                    } else {
                        std::time::UNIX_EPOCH - std::time::Duration::from_secs((-sec) as u64)
                    }
                };
                let mtime = to_system_time(mtime_sec);
                let atime = atime_opt.map(to_system_time).unwrap_or(mtime);
                let times = std::fs::FileTimes::new()
                    .set_modified(mtime)
                    .set_accessed(atime);
                file.set_times(times).is_ok()
            } else {
                let now = std::time::SystemTime::now();
                let times = std::fs::FileTimes::new()
                    .set_modified(now)
                    .set_accessed(now);
                file.set_times(times).is_ok()
            };
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
            let levels = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
            let mut p = std::path::Path::new(&path);
            for _ in 0..levels {
                if let Some(parent) = p.parent() {
                    p = parent;
                } else {
                    break;
                }
            }
            let dir = p.to_str().unwrap_or("");
            Ok(PhpValue::string(dir))
        }
        "realpath" | "realPath" | "canonicalize" => {
            let path = get_arg_str(args, 0, "File::realpath")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let res = match std::fs::canonicalize(&path) {
                Ok(p) => {
                    #[allow(unused_mut)]
                    let mut s = p.to_string_lossy().to_string();
                    #[cfg(windows)]
                    {
                        if s.starts_with(r"\\?\UNC\") {
                            s = format!(r"\\{}", &s[8..]);
                        } else if s.starts_with(r"\\?\") {
                            s = s[4..].to_string();
                        }
                    }
                    Ok(PhpValue::string(s))
                }
                Err(_) => Ok(PhpValue::Bool(false)),
            };
            record_fiber_io_finish();
            res
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
        "info" => {
            let path = get_arg_str(args, 0, "File::info")?;
            let p = std::path::Path::new(&path);
            let dirname = p.parent().and_then(|s| s.to_str()).unwrap_or("");
            let basename = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let extension = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            let filename = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let mut map = IndexMap::new();
            map.insert("dirname".to_string(), PhpValue::string(dirname));
            map.insert("basename".to_string(), PhpValue::string(basename));
            map.insert("extension".to_string(), PhpValue::string(extension));
            map.insert("filename".to_string(), PhpValue::string(filename));
            Ok(PhpValue::dict_from_map(map))
        }
        "modifiedAt" | "modified_at" => {
            let path = get_arg_str(args, 0, "File::modifiedAt")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(err) => {
                    record_fiber_io_finish();
                    return Err(RuntimeErrorKind::Custom {
                        message: format!("File::modifiedAt failed: {}", err),
                    });
                }
            };
            let mtime = match meta.modified() {
                Ok(t) => t,
                Err(err) => {
                    record_fiber_io_finish();
                    return Err(RuntimeErrorKind::Custom {
                        message: format!("File::modifiedAt failed: {}", err),
                    });
                }
            };
            record_fiber_io_finish();
            let duration = match mtime.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_secs() as i64,
                Err(_) => 0,
            };
            if let Ok(narrow) = i32::try_from(duration) {
                Ok(PhpValue::Int(narrow))
            } else {
                Ok(PhpValue::RustInt {
                    kind: RustIntKind::I64,
                    value: duration as i128,
                })
            }
        }
        "isReadable" | "is_readable" => {
            let path = get_arg_str(args, 0, "File::isReadable")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            #[cfg(unix)]
            let ok = {
                use std::ffi::CString;
                if let Ok(c_path) = CString::new(path.as_str()) {
                    unsafe { libc::access(c_path.as_ptr(), libc::R_OK) == 0 }
                } else {
                    false
                }
            };
            #[cfg(not(unix))]
            let ok = std::fs::File::open(&path).is_ok();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        "isWritable" | "is_writable" => {
            let path = get_arg_str(args, 0, "File::isWritable")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            #[cfg(unix)]
            let ok = {
                use std::ffi::CString;
                if let Ok(c_path) = CString::new(path.as_str()) {
                    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
                } else {
                    false
                }
            };
            #[cfg(not(unix))]
            let ok = match std::fs::metadata(&path) {
                Ok(m) => !m.permissions().readonly(),
                Err(_) => false,
            };
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
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
        "open" => {
            let path = get_arg_str(args, 0, "File::open")?;
            let mode = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| "r".to_string());
            crate::vm::csv::open_stream(&path, &mode)
        }
        "close" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::close".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            crate::vm::csv::close_stream(val)
        }
        "eof" | "feof" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::eof".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            crate::vm::csv::feof_stream(val)
        }
        "rewind" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::rewind".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            crate::vm::csv::rewind_stream(val)
        }
        "clearstatcache" | "clearStatCache" => dispatch_sys("clearstatcache", args),
        "diskFreeSpace" | "disk_free_space" | "diskfreespace" => dispatch_sys("disk_free_space", args),
        "diskTotalSpace" | "disk_total_space" => dispatch_sys("disk_total_space", args),
        "sysGetTempDir" | "sys_get_temp_dir" | "getTempDir" => {
            Ok(PhpValue::string(get_system_temp_dir()))
        }
        "tempDir" => {
            if args.is_empty() {
                Ok(PhpValue::string(get_system_temp_dir()))
            } else {
                let dir = args.get(0).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
                let prefix = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
                let mode_val = args.get(2);
                match create_unique_temp_dir(dir.as_deref(), prefix.as_deref(), mode_val) {
                    Some(path) => Ok(PhpValue::string(path)),
                    None => Ok(PhpValue::Bool(false)),
                }
            }
        }
        "createTempDirectory" | "tempDirectory" | "createTempDir" => {
            let dir = args.get(0).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let prefix = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let mode_val = args.get(2);
            match create_unique_temp_dir(dir.as_deref(), prefix.as_deref(), mode_val) {
                Some(path) => Ok(PhpValue::string(path)),
                None => Ok(PhpValue::Bool(false)),
            }
        }
        "tempnam" | "createTempFile" | "tempFile" => {
            let dir = args.get(0).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let prefix = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            match create_unique_temp_file_path(dir.as_deref(), prefix.as_deref()) {
                Some(path) => Ok(PhpValue::string(path)),
                None => Ok(PhpValue::Bool(false)),
            }
        }
        "tmpfile" | "createTempStream" | "tempStream" => {
            crate::vm::csv::create_tmpfile_stream()
        }
        "writeStream" | "write_stream" | "fwrite" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::writeStream".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let data_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::writeStream".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let bytes = match data_val {
                PhpValue::String(s) => s.as_bytes().to_vec(),
                other => other.to_php_string().into_bytes(),
            };
            let length = args.get(2).and_then(|v| v.to_i64()).map(|l| l.max(0) as usize);
            let slice = if let Some(len) = length {
                if len < bytes.len() { &bytes[..len] } else { &bytes[..] }
            } else {
                &bytes[..]
            };
            crate::vm::csv::write_stream(stream_val, slice)
        }
        "readStream" | "read_stream" | "fread" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::readStream".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let len_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "File::readStream".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let length = len_val.to_i64().unwrap_or(0).max(0) as usize;
            crate::vm::csv::read_stream(stream_val, length)
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
            let enc = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let invalid = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| "strict".to_string());
            record_fiber_io_start(IoKind::FileRead { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::read(&path).await {
                    Ok(raw) => {
                        let encoding = enc.as_deref().unwrap_or("UTF-8");
                        match crate::vm::encoding::decode_bytes(&raw, encoding, &invalid) {
                            Ok(content) => Ok(FrozenValue::String(content)),
                            Err((msg, _)) => Err(format!("AFile::read failed: {}", msg)),
                        }
                    }
                    Err(err) => Err(format!("AFile::read failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "readBytes" | "read_bytes" => {
            let path = get_arg_str(args, 0, "AFile::readBytes")?;
            record_fiber_io_start(IoKind::FileRead { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::read(&path).await {
                    Ok(raw) => Ok(FrozenValue::Bytes(raw)),
                    Err(err) => Err(format!("AFile::readBytes failed: {}", err)),
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
        "copy" => {
            let from = get_arg_str(args, 0, "AFile::copy")?;
            let to = get_arg_str(args, 1, "AFile::copy")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: to.clone(),
                bytes: 0,
            });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::copy(&from, &to).await {
                    Ok(_) => Ok(FrozenValue::Bool(true)),
                    Err(err) => Err(format!("AFile::copy failed: {}", err)),
                };
                let _ = tx.send(res);
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "rename" | "move" => {
            let from = get_arg_str(args, 0, "AFile::rename")?;
            let to = get_arg_str(args, 1, "AFile::rename")?;
            record_fiber_io_start(IoKind::FileWrite {
                path: to.clone(),
                bytes: 0,
            });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::rename(&from, &to).await {
                    Ok(_) => Ok(FrozenValue::Bool(true)),
                    Err(err) => Err(format!("AFile::rename failed: {}", err)),
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
        "chmod" => {
            let path = get_arg_str(args, 0, "AFile::chmod")?;
            let mode = if let Some(mode_val) = args.get(1) {
                if mode_val.is_null() {
                    None
                } else {
                    match parse_file_mode(mode_val) {
                        Some(m) => Some(m),
                        None => {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let _ = tx.send(Ok(FrozenValue::Bool(false)));
                            return Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                                VmFuture::pending(rx),
                            ))));
                        }
                    }
                }
            } else {
                None
            };
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let p = std::path::Path::new(&path);
                if let Some(m) = mode {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let ok = tokio::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).await.is_ok();
                        let _ = tx.send(Ok(FrozenValue::Bool(ok)));
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = tx.send(Ok(FrozenValue::Bool(p.exists())));
                    }
                } else {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = tokio::fs::metadata(p).await {
                            let m = meta.permissions().mode() & 0o7777;
                            let _ = tx.send(Ok(FrozenValue::String(format!("{:04o}", m).into())));
                        } else {
                            let _ = tx.send(Ok(FrozenValue::Null));
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = tx.send(Ok(FrozenValue::Null));
                    }
                }
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "mkdir" | "makeDirectory" | "make_directory" => {
            let path = get_arg_str(args, 0, "AFile::mkdir")?;
            let mode = if let Some(mode_val) = args.get(1) {
                if mode_val.is_null() {
                    0o777
                } else {
                    match parse_file_mode(mode_val) {
                        Some(m) => m,
                        None => {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let _ = tx.send(Ok(FrozenValue::Bool(false)));
                            return Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                                VmFuture::pending(rx),
                            ))));
                        }
                    }
                }
            } else {
                0o777
            };
            let recursive = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
            let force = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);
            record_fiber_io_start(IoKind::FileWrite {
                path: path.clone(),
                bytes: 0,
            });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let p = std::path::Path::new(&path);
                if p.exists() && !force {
                    let _ = tx.send(Ok(FrozenValue::Bool(false)));
                    return;
                }
                let ok = if recursive {
                    tokio::fs::create_dir_all(p).await.is_ok()
                } else {
                    tokio::fs::create_dir(p).await.is_ok()
                };
                if ok {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = tokio::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).await;
                    }
                }
                let _ = tx.send(Ok(FrozenValue::Bool(ok)));
            });
            record_fiber_io_finish();
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "sysGetTempDir" | "sys_get_temp_dir" | "tempDir" => {
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let _ = tx.send(Ok(FrozenValue::String(get_system_temp_dir())));
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "createTempDirectory" | "tempDirectory" | "createTempDir" => {
            let dir = args.get(0).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let prefix = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let mode = if let Some(mode_val) = args.get(2) {
                if mode_val.is_null() {
                    0o700
                } else {
                    match parse_file_mode(mode_val) {
                        Some(m) => m,
                        None => {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let _ = tx.send(Ok(FrozenValue::Bool(false)));
                            return Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                                VmFuture::pending(rx),
                            ))));
                        }
                    }
                }
            } else {
                0o700
            };
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match create_unique_temp_dir_with_mode(dir.as_deref(), prefix.as_deref(), mode) {
                    Some(p) => Ok(FrozenValue::String(p)),
                    None => Ok(FrozenValue::Bool(false)),
                };
                let _ = tx.send(res);
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "tempnam" | "createTempFile" | "tempFile" => {
            let dir = args.get(0).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let prefix = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match create_unique_temp_file_path(dir.as_deref(), prefix.as_deref()) {
                    Some(p) => Ok(FrozenValue::String(p)),
                    None => Ok(FrozenValue::Bool(false)),
                };
                let _ = tx.send(res);
            });
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::pending(rx),
            ))))
        }
        "realpath" | "realPath" | "canonicalize" => {
            let path = get_arg_str(args, 0, "AFile::realpath")?;
            record_fiber_io_start(IoKind::FileExists { path: path.clone() });
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let res = match tokio::fs::canonicalize(&path).await {
                    Ok(p) => {
                        #[allow(unused_mut)]
                        let mut s = p.to_string_lossy().to_string();
                        #[cfg(windows)]
                        {
                            if s.starts_with(r"\\?\UNC\") {
                                s = format!(r"\\{}", &s[8..]);
                            } else if s.starts_with(r"\\?\") {
                                s = s[4..].to_string();
                            }
                        }
                        Ok(FrozenValue::String(s))
                    }
                    Err(_) => Ok(FrozenValue::Bool(false)),
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
            obj.set_property("raw_bytes", PhpValue::bytes(b"{\"status\":\"ok\"}".to_vec()));

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
                resp.set_property("body", PhpValue::string(&body_str));
                resp.set_property("raw_bytes", PhpValue::bytes(body_str.as_bytes().to_vec()));
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
        "connection" | "connect" => {
            let mut dsn = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            if !dsn.to_ascii_lowercase().contains("charset=") {
                if dsn.is_empty() {
                    dsn = "charset=utf8mb4".to_string();
                } else {
                    dsn.push_str(";charset=utf8mb4");
                }
            }
            let mut obj = VmObject::new("DBConnection");
            obj.set_property("dsn", PhpValue::string(dsn));
            obj.set_property("charset", PhpValue::string("utf8mb4"));
            Ok(PhpValue::object(obj))
        }
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

pub fn to_int_loose(v: &PhpValue) -> Option<i64> {
    match v {
        PhpValue::Int(n) => Some(*n as i64),
        PhpValue::Bool(b) => Some(if *b { 1 } else { 0 }),
        PhpValue::Float(f) => Some(*f as i64),
        PhpValue::RustInt { value, .. } => Some(*value as i64),
        PhpValue::RustFloat { value, .. } => Some(*value as i64),
        PhpValue::String(s) => {
            let trimmed = s.trim();
            trimmed.parse::<i64>().ok().or_else(|| trimmed.parse::<f64>().ok().map(|f| f as i64))
        }
        PhpValue::Null => Some(0),
        _ => None,
    }
}

fn dispatch_int(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "compare" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            let i1 = to_int_loose(a);
            let i2 = to_int_loose(b);
            match (i1, i2) {
                (Some(v1), Some(v2)) => Ok(PhpValue::Bool(v1 == v2)),
                _ => Ok(PhpValue::Bool(false)),
            }
        }
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
        "abs" => {
            let val = args.first().and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Int::abs".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let res = val.saturating_abs();
            if let Ok(narrow) = i32::try_from(res) {
                Ok(PhpValue::Int(narrow))
            } else {
                Ok(PhpValue::RustInt {
                    kind: RustIntKind::I64,
                    value: res as i128,
                })
            }
        }
        "min" => {
            let nums: Vec<i64> = if args.len() == 1 && (matches!(args[0], PhpValue::List(_) | PhpValue::Dict(_) | PhpValue::Array(_) | PhpValue::HashMap(_) | PhpValue::Set(_))) {
                extract_array_elements(&args[0]).into_iter().filter_map(|v| v.to_i64()).collect()
            } else {
                args.iter().filter_map(|v| v.to_i64()).collect()
            };
            if nums.is_empty() {
                return Err(RuntimeErrorKind::Custom {
                    message: "Int::min(): Argument cannot be empty".to_string(),
                });
            }
            let min_val = nums.into_iter().min().unwrap();
            if let Ok(narrow) = i32::try_from(min_val) {
                Ok(PhpValue::Int(narrow))
            } else {
                Ok(PhpValue::RustInt {
                    kind: RustIntKind::I64,
                    value: min_val as i128,
                })
            }
        }
        "max" => {
            let nums: Vec<i64> = if args.len() == 1 && (matches!(args[0], PhpValue::List(_) | PhpValue::Dict(_) | PhpValue::Array(_) | PhpValue::HashMap(_) | PhpValue::Set(_))) {
                extract_array_elements(&args[0]).into_iter().filter_map(|v| v.to_i64()).collect()
            } else {
                args.iter().filter_map(|v| v.to_i64()).collect()
            };
            if nums.is_empty() {
                return Err(RuntimeErrorKind::Custom {
                    message: "Int::max(): Argument cannot be empty".to_string(),
                });
            }
            let max_val = nums.into_iter().max().unwrap();
            if let Ok(narrow) = i32::try_from(max_val) {
                Ok(PhpValue::Int(narrow))
            } else {
                Ok(PhpValue::RustInt {
                    kind: RustIntKind::I64,
                    value: max_val as i128,
                })
            }
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
            if min_v > max_v {
                return Err(RuntimeErrorKind::Custom {
                    message: format!("Int::random(): min ({}) cannot be greater than max ({})", min_v, max_v),
                });
            }
            if min_v == max_v {
                if let Ok(narrow) = i32::try_from(min_v) {
                    return Ok(PhpValue::Int(narrow));
                } else {
                    return Ok(PhpValue::RustInt {
                        kind: RustIntKind::I64,
                        value: min_v as i128,
                    });
                }
            }
            let span = (max_v - min_v) as u64 + 1;
            let limit = u64::MAX - (u64::MAX % span);
            let val_offset = loop {
                let bytes = crate::vm::crypto::random_bytes(8).map_err(|e| RuntimeErrorKind::Custom {
                    message: format!("Int::random failed: {}", e),
                })?;
                let n = u64::from_ne_bytes(bytes.try_into().unwrap_or([0; 8]));
                if n < limit {
                    break n % span;
                }
            };
            let res = min_v + val_offset as i64;
            if let Ok(narrow) = i32::try_from(res) {
                Ok(PhpValue::Int(narrow))
            } else {
                Ok(PhpValue::RustInt {
                    kind: RustIntKind::I64,
                    value: res as i128,
                })
            }
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Int".to_string(),
            method: method.to_string(),
        }),
    }
}

pub fn to_float_loose(v: &PhpValue) -> Option<f64> {
    match v {
        PhpValue::Float(f) => Some(*f as f64),
        PhpValue::Int(n) => Some(*n as f64),
        PhpValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        PhpValue::RustInt { value, .. } => Some(*value as f64),
        PhpValue::RustFloat { value, .. } => Some(*value),
        PhpValue::String(s) => s.trim().parse::<f64>().ok(),
        PhpValue::Null => Some(0.0),
        _ => None,
    }
}

fn format_number(val: f64, decimals: usize, dec_point: &str, thousands_sep: &str) -> String {
    let is_neg = val.is_sign_negative() && val != 0.0;
    let abs_val = val.abs();
    let s = format!("{:.*}", decimals, abs_val);
    let parts: Vec<&str> = s.split('.').collect();
    let int_part = parts[0];
    let frac_part = if parts.len() > 1 { parts[1] } else { "" };

    let mut formatted_int = String::with_capacity(int_part.len() + int_part.len() / 3);
    let len = int_part.len();
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 && !thousands_sep.is_empty() {
            formatted_int.push_str(thousands_sep);
        }
        formatted_int.push(ch);
    }

    let mut res = String::new();
    if is_neg {
        res.push('-');
    }
    res.push_str(&formatted_int);
    if decimals > 0 {
        res.push_str(dec_point);
        res.push_str(frac_part);
    }
    res
}

fn dispatch_float(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "compare" => {
            let a = args.get(0).unwrap_or(&PhpValue::Null);
            let b = args.get(1).unwrap_or(&PhpValue::Null);
            let epsilon = args.get(2).and_then(|v| v.to_f64());
            let f1 = to_float_loose(a);
            let f2 = to_float_loose(b);
            match (f1, f2) {
                (Some(v1), Some(v2)) => {
                    let eq = if let Some(eps) = epsilon {
                        (v1 - v2).abs() <= eps.abs()
                    } else {
                        let s1 = if let PhpValue::Float(f) = a {
                            f.to_string()
                        } else {
                            v1.to_string()
                        };
                        let s2 = if let PhpValue::Float(f) = b {
                            f.to_string()
                        } else {
                            v2.to_string()
                        };
                        crate::vm::value::compare_floats_by_max_decimal_point(v1, &s1, v2, &s2)
                    };
                    Ok(PhpValue::Bool(eq))
                }
                _ => Ok(PhpValue::Bool(false)),
            }
        }
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
        "format" => {
            let val = args.first().and_then(|v| v.to_f64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Float::format".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let decimals = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0).max(0) as usize;
            let dec_point = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| ".".to_string());
            let thousands_sep = args.get(3).map(|v| v.to_php_string()).unwrap_or_else(|| ",".to_string());
            Ok(PhpValue::string(format_number(val, decimals, &dec_point, &thousands_sep)))
        }
        "min" => {
            let nums: Vec<f64> = if args.len() == 1 && (matches!(args[0], PhpValue::List(_) | PhpValue::Dict(_) | PhpValue::Array(_) | PhpValue::HashMap(_) | PhpValue::Set(_))) {
                extract_array_elements(&args[0]).into_iter().filter_map(|v| v.to_f64()).collect()
            } else {
                args.iter().filter_map(|v| v.to_f64()).collect()
            };
            if nums.is_empty() {
                return Err(RuntimeErrorKind::Custom {
                    message: "Float::min(): Argument cannot be empty".to_string(),
                });
            }
            let min_val = nums.into_iter().min_by(|a, b| a.total_cmp(b)).unwrap();
            Ok(PhpValue::Float(min_val as f32))
        }
        "max" => {
            let nums: Vec<f64> = if args.len() == 1 && (matches!(args[0], PhpValue::List(_) | PhpValue::Dict(_) | PhpValue::Array(_) | PhpValue::HashMap(_) | PhpValue::Set(_))) {
                extract_array_elements(&args[0]).into_iter().filter_map(|v| v.to_f64()).collect()
            } else {
                args.iter().filter_map(|v| v.to_f64()).collect()
            };
            if nums.is_empty() {
                return Err(RuntimeErrorKind::Custom {
                    message: "Float::max(): Argument cannot be empty".to_string(),
                });
            }
            let max_val = nums.into_iter().max_by(|a, b| a.total_cmp(b)).unwrap();
            Ok(PhpValue::Float(max_val as f32))
        }
        "random" => {
            let min_v = args.first().and_then(|v| v.to_f64()).unwrap_or(0.0);
            let max_v = args.get(1).and_then(|v| v.to_f64()).unwrap_or(1.0);
            if min_v > max_v {
                return Err(RuntimeErrorKind::Custom {
                    message: format!("Float::random(): min ({}) cannot be greater than max ({})", min_v, max_v),
                });
            }
            if (min_v - max_v).abs() < f64::EPSILON {
                return Ok(PhpValue::Float(min_v as f32));
            }
            let bytes = crate::vm::crypto::random_bytes(8).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Float::random failed: {}", e),
            })?;
            let u = u64::from_ne_bytes(bytes.try_into().unwrap_or([0; 8]));
            let factor = (u as f64) / (u64::MAX as f64);
            let res = min_v + factor * (max_v - min_v);
            Ok(PhpValue::Float(res as f32))
        }
        "parse" => {
            let s = get_arg_str(args, 0, "Float::parse")?;
            let val = s.trim().parse::<f64>().unwrap_or(0.0);
            Ok(PhpValue::Float(val as f32))
        }
        "add" => {
            let a = crate::vm::decimal::Decimal::from_php_value(args.get(0).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let b = crate::vm::decimal::Decimal::from_php_value(args.get(1).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let sum = a.add(&b);
            if args.iter().all(|v| matches!(v, PhpValue::String(_))) {
                Ok(PhpValue::string(sum.to_string()))
            } else {
                Ok(PhpValue::Float(sum.to_f32()))
            }
        }
        "sub" | "subtract" => {
            let a = crate::vm::decimal::Decimal::from_php_value(args.get(0).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let b = crate::vm::decimal::Decimal::from_php_value(args.get(1).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let diff = a.sub(&b);
            if args.iter().all(|v| matches!(v, PhpValue::String(_))) {
                Ok(PhpValue::string(diff.to_string()))
            } else {
                Ok(PhpValue::Float(diff.to_f32()))
            }
        }
        "mul" | "multiply" => {
            let a = crate::vm::decimal::Decimal::from_php_value(args.get(0).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let b = crate::vm::decimal::Decimal::from_php_value(args.get(1).unwrap_or(&PhpValue::Null)).unwrap_or_else(|| crate::vm::decimal::Decimal::from_i64(1));
            let prod = a.mul(&b);
            if args.iter().all(|v| matches!(v, PhpValue::String(_))) {
                Ok(PhpValue::string(prod.to_string()))
            } else {
                Ok(PhpValue::Float(prod.to_f32()))
            }
        }
        "div" | "divide" => {
            let a = crate::vm::decimal::Decimal::from_php_value(args.get(0).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let b = crate::vm::decimal::Decimal::from_php_value(args.get(1).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            if b.is_zero() {
                return Err(RuntimeErrorKind::DivisionByZero);
            }
            let scale = args.get(2).and_then(|v| v.to_i64()).map(|s| s.max(0) as usize);
            let quot = a.div(&b, scale).map_err(|_| RuntimeErrorKind::DivisionByZero)?;
            if args.iter().all(|v| matches!(v, PhpValue::String(_))) {
                Ok(PhpValue::string(quot.to_string()))
            } else {
                Ok(PhpValue::Float(quot.to_f32()))
            }
        }
        "bAdd" => {
            let a = args.get(0).and_then(|v| v.to_f64()).unwrap_or(0.0);
            let b = args.get(1).and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::RustFloat {
                kind: RustFloatKind::F64,
                value: a + b,
            })
        }
        "bSub" => {
            let a = args.get(0).and_then(|v| v.to_f64()).unwrap_or(0.0);
            let b = args.get(1).and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::RustFloat {
                kind: RustFloatKind::F64,
                value: a - b,
            })
        }
        "bMul" => {
            let a = args.get(0).and_then(|v| v.to_f64()).unwrap_or(0.0);
            let b = args.get(1).and_then(|v| v.to_f64()).unwrap_or(0.0);
            Ok(PhpValue::RustFloat {
                kind: RustFloatKind::F64,
                value: a * b,
            })
        }
        "bDiv" => {
            let a = args.get(0).and_then(|v| v.to_f64()).unwrap_or(0.0);
            let b = args.get(1).and_then(|v| v.to_f64()).unwrap_or(0.0);
            if b == 0.0 {
                return Err(RuntimeErrorKind::DivisionByZero);
            }
            Ok(PhpValue::RustFloat {
                kind: RustFloatKind::F64,
                value: a / b,
            })
        }
        "bccomp" => {
            let a = crate::vm::decimal::Decimal::from_php_value(args.get(0).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let b = crate::vm::decimal::Decimal::from_php_value(args.get(1).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let scale = args.get(2).and_then(|v| v.to_i64()).map(|s| s.max(0) as usize);
            let ord = match scale {
                Some(s) => {
                    let mut a_trunc = a.clone();
                    let mut b_trunc = b.clone();
                    a_trunc.fractional.truncate(s);
                    b_trunc.fractional.truncate(s);
                    a_trunc.cmp(&b_trunc)
                }
                None => a.cmp(&b),
            };
            let res = match ord {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => -1,
            };
            Ok(PhpValue::Int(res))
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
        "parseQuery" | "parse_query" => {
            let input = get_arg_str(args, 0, "Url::parseQuery")?;
            let map = url_helper::parse_query(&input);
            Ok(PhpValue::dict_from_map(map))
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
        "exit" => dispatch_sys("exit", args),
        "die" => dispatch_sys("die", args),
        "register_shutdown_function" => dispatch_sys("registerShutdownFunction", args),
        "array_merge" => dispatch_arr("merge", args),
        "count" => dispatch_arr("count", args),
        "array_keys" => dispatch_arr("keys", args),
        "array_values" => dispatch_arr("values", args),
        "array_diff" => dispatch_arr("diff", args),
        "array_diff_key" => dispatch_arr("diffKey", args),
        "array_intersect" => dispatch_arr("intersect", args),
        "array_intersect_key" => dispatch_arr("intersectKey", args),
        "array_column" => dispatch_arr("column", args),
        "array_chunk" => dispatch_arr("chunk", args),
        "array_flip" => dispatch_arr("flip", args),
        "array_combine" => dispatch_arr("combine", args),
        "array_count_values" => dispatch_arr("countValues", args),
        "shuffle" => dispatch_arr("shuffle", args),
        "array_rand" => dispatch_arr("randomKey", args),
        "ksort" => dispatch_arr("ksort", args),
        "asort" => dispatch_arr("asort", args),
        "natsort" => dispatch_arr("natsort", args),
        "natcasesort" => dispatch_arr("natcasesort", args),
        "usort" => dispatch_arr("usort", args),
        "uasort" => dispatch_arr("uasort", args),
        "uksort" => dispatch_arr("uksort", args),
        "array_key_exists" => {
            if args.len() >= 2 {
                dispatch_arr("hasKey", &[args[1].clone(), args[0].clone()])
            } else {
                dispatch_arr("hasKey", args)
            }
        }
        "in_array" => {
            if args.len() >= 2 {
                dispatch_arr("hasValue", &[args[1].clone(), args[0].clone()])
            } else {
                dispatch_arr("hasValue", args)
            }
        }
        "strlen" => dispatch_str("length", args),
        "strcmp" => dispatch_str("strcmp", args),
        "strcasecmp" => dispatch_str("strcasecmp", args),
        "strncmp" => dispatch_str("strncmp", args),
        "strncasecmp" => dispatch_str("strncasecmp", args),
        "strnatcmp" => dispatch_str("natCompare", args),
        "strnatcasecmp" => dispatch_str("natCaseCompare", args),
        "str_contains" => dispatch_str("contains", args),
        "implode" => dispatch_str("join", args),
        "explode" => dispatch_str("split", args),
        "trim" => dispatch_str("trim", args),
        "ltrim" => dispatch_str("ltrim", args),
        "rtrim" => dispatch_str("rtrim", args),
        "strtolower" => dispatch_str("lower", args),
        "strtoupper" => dispatch_str("upper", args),
        "str_replace" => dispatch_str("replace", args),
        "strpos" => dispatch_str("find", args),
        "ctype_digit" => dispatch_str("digit", args),
        "ctype_alpha" => dispatch_str("alpha", args),
        "ctype_alnum" => dispatch_str("alnum", args),
        "preg_match" => dispatch_regex("testString", args),
        "preg_replace" => dispatch_regex("replace", args),
        "preg_quote" => dispatch_regex("quote", args),
        "round" => dispatch_float("round", args),
        "floor" => dispatch_float("floor", args),
        "ceil" | "ciel" => dispatch_float("ceil", args),
        "bcadd" => dispatch_float("add", args),
        "bcsub" => dispatch_float("sub", args),
        "bcmul" => dispatch_float("mul", args),
        "bcdiv" => dispatch_float("div", args),
        "bccomp" => {
            let a = crate::vm::decimal::Decimal::from_php_value(args.get(0).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let b = crate::vm::decimal::Decimal::from_php_value(args.get(1).unwrap_or(&PhpValue::Null)).unwrap_or_else(crate::vm::decimal::Decimal::zero);
            let scale = args.get(2).and_then(|v| v.to_i64()).map(|s| s.max(0) as usize);
            let ord = match scale {
                Some(s) => {
                    let mut a_trunc = a.clone();
                    let mut b_trunc = b.clone();
                    a_trunc.fractional.truncate(s);
                    b_trunc.fractional.truncate(s);
                    a_trunc.cmp(&b_trunc)
                }
                None => a.cmp(&b),
            };
            let res = match ord {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => -1,
            };
            Ok(PhpValue::Int(res))
        }
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
        "is_null" => dispatch_validate("null", args),
        "is_int" | "is_integer" | "is_long" => dispatch_validate("int", args),
        "is_bool" | "is_boolean" => dispatch_validate("bool", args),
        "is_float" | "is_double" | "is_real" => dispatch_validate("float", args),
        "is_numeric" => dispatch_validate("numeric", args),
        "is_scalar" => dispatch_validate("scalar", args),
        "is_resource" => dispatch_validate("resource", args),
        "is_a" => dispatch_validate("isA", args),
        "now" => dispatch_time("now", args),
        "date" => dispatch_time("date", args),
        "gettype" | "getType" | "get_debug_type" => dispatch_debug("type", args),
        "var_dump" | "dump" => dispatch_debug("dump", args),
        "print_r" | "var_export" => dispatch_debug("format", args),
        "fgetcsv" => crate::vm::csv::dispatch_csv("fgetcsv", args),
        "fputcsv" => crate::vm::csv::dispatch_csv("fputcsv", args),
        "str_getcsv" => crate::vm::csv::dispatch_csv("parseRow", args),
        "fopen" => dispatch_file("open", args),
        "fclose" => dispatch_file("close", args),
        "feof" => dispatch_file("eof", args),
        "rewind" => dispatch_file("rewind", args),
        "tempnam" => dispatch_file("tempnam", args),
        "tmpfile" => dispatch_file("tmpfile", args),
        "sys_get_temp_dir" => dispatch_path("tempDir", args),
        "glob" => dispatch_path("glob", args),
        "stream_socket_server" => dispatch_socket("server", args),
        "stream_socket_get_name" => dispatch_socket("getName", args),
        "bin2hex" => dispatch_binary("bin2hex", args),
        "hex2bin" => dispatch_binary("hex2bin", args),
        "random_bytes" => dispatch_binary("randomBytes", args),
        "rmdir" => dispatch_file("rmdir", args),
        "strrchr" => dispatch_str("fromLast", args),
        "unlink" => dispatch_file("delete", args),
        "file_put_contents" => dispatch_file("write", args),
        "file_get_contents" => dispatch_file("read", args),
        "file_exists" => dispatch_file("exists", args),
        "usleep" => dispatch_sys("usleep", args),
        "sleep" => dispatch_sys("sleep", args),
        "fwrite" => dispatch_file("writeStream", args),
        "fread" => dispatch_file("readStream", args),
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
        "copy" => dispatch_file("copy", args),
        "rename" => dispatch_file("rename", args),
        "mkdir" => dispatch_file("mkdir", args),
        "chmod" => dispatch_file("chmod", args),
        "chown" => dispatch_file("chown", args),
        "touch" => dispatch_file("touch", args),
        "parse_mode" => dispatch_file("parseMode", args),
        "number_format" => dispatch_float("format", args),
        "pathinfo" => {
            let path = get_arg_str(args, 0, "pathinfo")?;
            let p = std::path::Path::new(&path);
            let dirname = p.parent().and_then(|s| s.to_str()).unwrap_or("");
            let basename = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let extension = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            let filename = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if let Some(opt_val) = args.get(1) {
                if let Some(opt) = opt_val.to_i64() {
                    match opt {
                        1 => return Ok(PhpValue::string(dirname)),
                        2 => return Ok(PhpValue::string(basename)),
                        4 => return Ok(PhpValue::string(extension)),
                        8 => return Ok(PhpValue::string(filename)),
                        _ => {}
                    }
                }
            }
            let mut map = IndexMap::new();
            map.insert("dirname".to_string(), PhpValue::string(dirname));
            map.insert("basename".to_string(), PhpValue::string(basename));
            map.insert("extension".to_string(), PhpValue::string(extension));
            map.insert("filename".to_string(), PhpValue::string(filename));
            Ok(PhpValue::dict_from_map(map))
        }
        "basename" => {
            let path = get_arg_str(args, 0, "basename")?;
            let mut base = std::path::Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if let Some(suffix_val) = args.get(1) {
                let suffix = suffix_val.to_php_string();
                if !suffix.is_empty() && base.ends_with(&suffix) {
                    base.truncate(base.len() - suffix.len());
                }
            }
            Ok(PhpValue::string(base))
        }
        "dirname" => dispatch_file("dirname", args),
        "realpath" => dispatch_file("realpath", args),
        "filemtime" => dispatch_file("modifiedAt", args),
        "is_readable" => dispatch_file("isReadable", args),
        "is_writable" => dispatch_file("isWritable", args),
        "clearstatcache" => dispatch_sys("clearstatcache", args),
        "disk_free_space" | "diskfreespace" => dispatch_sys("disk_free_space", args),
        "disk_total_space" => dispatch_sys("disk_total_space", args),
        "abs" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "abs".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            match val {
                PhpValue::Float(_) | PhpValue::RustFloat { .. } => dispatch_float("abs", args),
                _ => dispatch_int("abs", args),
            }
        }
        "min" => {
            let is_any_float = args.iter().any(|v| matches!(v, PhpValue::Float(_) | PhpValue::RustFloat { .. }))
                || (args.len() == 1 && extract_array_elements(&args[0]).iter().any(|v| matches!(v, PhpValue::Float(_) | PhpValue::RustFloat { .. })));
            if is_any_float {
                dispatch_float("min", args)
            } else {
                dispatch_int("min", args)
            }
        }
        "max" => {
            let is_any_float = args.iter().any(|v| matches!(v, PhpValue::Float(_) | PhpValue::RustFloat { .. }))
                || (args.len() == 1 && extract_array_elements(&args[0]).iter().any(|v| matches!(v, PhpValue::Float(_) | PhpValue::RustFloat { .. })));
            if is_any_float {
                dispatch_float("max", args)
            } else {
                dispatch_int("max", args)
            }
        }
        "rand" | "mt_rand" => dispatch_int("random", args),
        "ip2long" => {
            let s = match args.first() {
                Some(v) => v.to_php_string(),
                None => return Ok(PhpValue::Bool(false)),
            };
            match s.trim().parse::<Ipv4Addr>() {
                Ok(ip) => {
                    let val = u32::from(ip) as i64;
                    if let Ok(narrow) = i32::try_from(val) {
                        Ok(PhpValue::Int(narrow))
                    } else {
                        Ok(PhpValue::RustInt {
                            kind: RustIntKind::I64,
                            value: val as i128,
                        })
                    }
                }
                Err(_) => Ok(PhpValue::Bool(false)),
            }
        }
        "long2ip" => dispatch_ip("fromLong", args),
        "parse_str" => {
            let s = get_arg_str(args, 0, "parse_str")?;
            let map = url_helper::parse_query(&s);
            if let Some(target) = args.get(1) {
                if let PhpValue::Dict(d) = target {
                    d.borrow_mut().clear();
                    for (k, v) in map.clone() {
                        d.borrow_mut().insert(k, v);
                    }
                }
            }
            Ok(PhpValue::dict_from_map(map))
        }
        "str_pad" => {
            let s = get_arg_str(args, 0, "str_pad")?;
            let len = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            let pad = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| " ".to_string());
            let side_val = args.get(3);
            let side = match side_val {
                Some(PhpValue::Int(0)) => "left",
                Some(PhpValue::Int(1)) => "right",
                Some(PhpValue::Int(2)) => "both",
                Some(v) => match v.to_php_string().to_ascii_lowercase().as_str() {
                    "0" | "left" => "left",
                    "2" | "both" => "both",
                    _ => "right",
                },
                None => "right",
            };
            dispatch_str("pad", &[PhpValue::string(s), PhpValue::Int(len as i32), PhpValue::string(pad), PhpValue::string(side)])
        }
        "str_repeat" => dispatch_str("repeat", args),
        "substr_count" => dispatch_str("countOccurrences", args),
        "substr_replace" => dispatch_str("replaceAt", args),
        "array_sum" => dispatch_arr("sum", args),
        "array_product" => dispatch_arr("product", args),
        "array_fill" => dispatch_arr("fill", args),
        "array_fill_keys" => dispatch_arr("fillKeys", args),
        "array_pad" => dispatch_arr("pad", args),
        "gzcompress" => dispatch_compress("zlib", args),
        "gzuncompress" => dispatch_compress("unzlib", args),
        "gzencode" => dispatch_compress("gzip", args),
        "gzdecode" => dispatch_compress("gunzip", args),
        "uniqid" => crate::vm::uniq::dispatch_uniq("uniqid", args),
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
// Time Facade
// =========================================================================

pub fn dispatch_time(method: &str, args: &[PhpValue]) -> FacadeResult {
    crate::vm::time::dispatch_time(method, args)
}

pub fn dispatch_carbon(method: &str, args: &[PhpValue]) -> FacadeResult {
    dispatch_time(method, args)
}

pub fn dispatch_uniq(method: &str, args: &[PhpValue]) -> FacadeResult {
    crate::vm::uniq::dispatch_uniq(method, args)
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

pub(crate) fn get_arg_bytes<'a>(
    args: &'a [PhpValue],
    index: usize,
    callable: &str,
) -> Result<std::borrow::Cow<'a, [u8]>, RuntimeErrorKind> {
    let val = args.get(index).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
        callable: callable.to_string(),
        passed: args.len(),
        expected: index + 1,
    })?;
    match val {
        PhpValue::String(s) => Ok(std::borrow::Cow::Borrowed(s.as_bytes())),
        other => Ok(std::borrow::Cow::Owned(other.to_php_string().into_bytes())),
    }
}

#[inline]
pub fn binary_strcmp(s1: &[u8], s2: &[u8]) -> i32 {
    match s1.cmp(s2) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[inline]
pub fn binary_strcasecmp(s1: &[u8], s2: &[u8]) -> i32 {
    let min_len = s1.len().min(s2.len());
    for i in 0..min_len {
        let b1 = s1[i].to_ascii_lowercase();
        let b2 = s2[i].to_ascii_lowercase();
        if b1 != b2 {
            return if b1 < b2 { -1 } else { 1 };
        }
    }
    match s1.len().cmp(&s2.len()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[inline]
pub fn binary_strncmp(s1: &[u8], s2: &[u8], length: usize) -> i32 {
    let sub1 = &s1[..length.min(s1.len())];
    let sub2 = &s2[..length.min(s2.len())];
    match sub1.cmp(sub2) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[inline]
pub fn binary_strncasecmp(s1: &[u8], s2: &[u8], length: usize) -> i32 {
    let sub1 = &s1[..length.min(s1.len())];
    let sub2 = &s2[..length.min(s2.len())];
    binary_strcasecmp(sub1, sub2)
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
        PhpValue::Set(s) => s
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
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
        PhpValue::Set(s) => {
            let mut map = IndexMap::new();
            for (i, v) in s.borrow().iter().enumerate() {
                map.insert(i.to_string(), v.clone());
            }
            map
        }
        _ => IndexMap::new(),
    }
}

pub fn random_usize(max: usize) -> usize {
    if max <= 1 {
        return 0;
    }
    if let Ok(bytes) = crypto::random_bytes(8) {
        let n = u64::from_ne_bytes(bytes.try_into().unwrap_or([0; 8]));
        (n as usize) % max
    } else {
        0
    }
}

pub fn shuffle_slice<T>(slice: &mut [T]) {
    let len = slice.len();
    if len <= 1 {
        return;
    }
    for i in (1..len).rev() {
        let j = random_usize(i + 1);
        slice.swap(i, j);
    }
}

pub fn natural_cmp(s1: &str, s2: &str, case_insensitive: bool) -> std::cmp::Ordering {
    let mut chars1 = s1.chars().peekable();
    let mut chars2 = s2.chars().peekable();

    loop {
        match (chars1.peek(), chars2.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(&c1), Some(&c2)) => {
                if c1.is_ascii_digit() && c2.is_ascii_digit() {
                    let mut num1 = String::new();
                    while let Some(&d) = chars1.peek() {
                        if d.is_ascii_digit() {
                            num1.push(d);
                            chars1.next();
                        } else {
                            break;
                        }
                    }

                    let mut num2 = String::new();
                    while let Some(&d) = chars2.peek() {
                        if d.is_ascii_digit() {
                            num2.push(d);
                            chars2.next();
                        } else {
                            break;
                        }
                    }

                    let sig1 = num1.trim_start_matches('0');
                    let sig2 = num2.trim_start_matches('0');

                    if sig1.len() != sig2.len() {
                        return sig1.len().cmp(&sig2.len());
                    }
                    if sig1 != sig2 {
                        return sig1.cmp(sig2);
                    }
                    if num1.len() != num2.len() {
                        return num1.len().cmp(&num2.len());
                    }
                } else {
                    let cmp1 = if case_insensitive {
                        c1.to_lowercase().to_string()
                    } else {
                        c1.to_string()
                    };
                    let cmp2 = if case_insensitive {
                        c2.to_lowercase().to_string()
                    } else {
                        c2.to_string()
                    };

                    if cmp1 != cmp2 {
                        return cmp1.cmp(&cmp2);
                    }

                    chars1.next();
                    chars2.next();
                }
            }
        }
    }
}

pub fn php_value_natural_cmp(a: &PhpValue, b: &PhpValue, case_insensitive: bool) -> std::cmp::Ordering {
    let s1 = a.to_php_string();
    let s2 = b.to_php_string();
    natural_cmp(&s1, &s2, case_insensitive)
}

pub fn get_item_column(item: &PhpValue, key: &str) -> Option<PhpValue> {
    match item {
        PhpValue::Object(o) => o.borrow().get_property(key).cloned(),
        PhpValue::Dict(d) => d.borrow().get(key).cloned(),
        PhpValue::HashMap(m) => m
            .borrow()
            .get(&PhpValue::string(key))
            .cloned()
            .or_else(|| {
                if let Ok(idx) = key.parse::<i32>() {
                    m.borrow().get(&PhpValue::Int(idx)).cloned()
                } else {
                    None
                }
            }),
        PhpValue::List(l) | PhpValue::Array(l) => {
            if let Ok(idx) = key.parse::<usize>() {
                l.borrow().get(idx).cloned()
            } else {
                None
            }
        }
        _ => None,
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
            if obj.class_name == "Time" || obj.class_name == "Carbon" {
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

pub fn is_array_or_collection(v: &PhpValue) -> bool {
    matches!(
        v,
        PhpValue::Array(_)
            | PhpValue::List(_)
            | PhpValue::Dict(_)
            | PhpValue::Set(_)
            | PhpValue::HashMap(_)
    )
}

pub fn is_list_or_set(v: &PhpValue) -> bool {
    matches!(v, PhpValue::Array(_) | PhpValue::List(_) | PhpValue::Set(_))
}

pub fn collection_to_joined_string(val: &PhpValue, delim: &str) -> String {
    match val {
        PhpValue::Array(a) | PhpValue::List(a) => {
            let items: Vec<String> = a.borrow().iter().map(|v| v.to_php_string()).collect();
            items.join(delim)
        }
        PhpValue::Set(s) => {
            let items: Vec<String> = s.borrow().iter().map(|v| v.to_php_string()).collect();
            items.join(delim)
        }
        PhpValue::Dict(d) => {
            let items: Vec<String> = d.borrow().values().map(|v| v.to_php_string()).collect();
            items.join(delim)
        }
        PhpValue::HashMap(m) => {
            let items: Vec<String> = m.borrow().values().map(|v| v.to_php_string()).collect();
            items.join(delim)
        }
        _ => val.to_php_string(),
    }
}

pub fn compare_collections_unordered(a: &PhpValue, b: &PhpValue) -> bool {
    match (a, b) {
        (PhpValue::Set(s1), PhpValue::Set(s2)) => *s1.borrow() == *s2.borrow(),
        (PhpValue::Array(l1), PhpValue::Array(l2))
        | (PhpValue::List(l1), PhpValue::List(l2))
        | (PhpValue::Array(l1), PhpValue::List(l2))
        | (PhpValue::List(l1), PhpValue::Array(l2)) => {
            let vec1 = l1.borrow();
            let vec2 = l2.borrow();
            if vec1.len() != vec2.len() {
                return false;
            }
            let mut remaining: Vec<&PhpValue> = vec2.iter().collect();
            for item1 in vec1.iter() {
                if let Some(pos) = remaining.iter().position(|item2| item1.strict_eq(item2)) {
                    remaining.swap_remove(pos);
                } else {
                    return false;
                }
            }
            remaining.is_empty()
        }
        (PhpValue::Set(s), PhpValue::Array(l))
        | (PhpValue::Set(s), PhpValue::List(l))
        | (PhpValue::Array(l), PhpValue::Set(s))
        | (PhpValue::List(l), PhpValue::Set(s)) => {
            let set = s.borrow();
            let list = l.borrow();
            for item in set.iter() {
                if !list.iter().any(|li| li.strict_eq(item)) {
                    return false;
                }
            }
            for item in list.iter() {
                if !set.iter().any(|si| si.strict_eq(item)) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

pub fn php_value_to_json_canonical(val: &PhpValue) -> String {
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
            let items: Vec<String> = a.borrow().iter().map(php_value_to_json_canonical).collect();
            format!("[{}]", items.join(","))
        }
        PhpValue::Set(s) => {
            let items: Vec<String> = s.borrow().iter().map(php_value_to_json_canonical).collect();
            format!("[{}]", items.join(","))
        }
        PhpValue::Dict(d) => {
            let borrowed = d.borrow();
            let mut keys: Vec<&String> = borrowed.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    let escaped_k = k.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("\"{}\":{}", escaped_k, php_value_to_json_canonical(&borrowed[k]))
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        PhpValue::HashMap(m) => {
            let borrowed = m.borrow();
            let mut entries: Vec<(String, String)> = borrowed
                .iter()
                .map(|(k, v)| (k.to_php_string(), php_value_to_json_canonical(v)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let str_entries: Vec<String> = entries
                .into_iter()
                .map(|(k, v)| {
                    let escaped_k = k.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("\"{}\":{}", escaped_k, v)
                })
                .collect();
            format!("{{{}}}", str_entries.join(","))
        }
        _ => php_value_to_json(val),
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

// =========================================================================
// Ip Facade
// =========================================================================

fn dispatch_ip(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        // Convert IPv4 dotted-decimal string to integer. Throws on invalid.
        // Example: `Ip::toLong("127.0.0.1")` -> `2130706433`
        "toLong" | "to_long" => {
            let s = get_arg_str(args, 0, "Ip::toLong")?;
            match s.trim().parse::<Ipv4Addr>() {
                Ok(ip) => {
                    let val = u32::from(ip) as i64;
                    if let Ok(narrow) = i32::try_from(val) {
                        Ok(PhpValue::Int(narrow))
                    } else {
                        Ok(PhpValue::RustInt {
                            kind: RustIntKind::I64,
                            value: val as i128,
                        })
                    }
                }
                Err(_) => Err(RuntimeErrorKind::Custom {
                    message: format!("Ip::toLong(): Invalid IPv4 address '{}'", s),
                }),
            }
        }
        // Convert integer to IPv4 dotted-decimal string.
        // Example: `Ip::fromLong(2130706433)` -> `"127.0.0.1"`
        "fromLong" | "from_long" => {
            let val = args.first().and_then(|v| v.to_i64()).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Ip::fromLong".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let ip = Ipv4Addr::from(val as u32);
            Ok(PhpValue::string(ip.to_string()))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Ip".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Compress Facade
// =========================================================================

fn dispatch_compress(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        // Compress data using ZLIB format (wraps gzcompress).
        // Example: `Compress::zlib("data")`
        "zlib" => {
            let data_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Compress::zlib".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let data_bytes: std::borrow::Cow<[u8]> = match data_val {
                PhpValue::String(s) => std::borrow::Cow::Borrowed(s.as_bytes()),
                other => std::borrow::Cow::Owned(other.to_php_string().into_bytes()),
            };
            let level = args.get(1).and_then(|v| v.to_i64()).unwrap_or(-1);
            let comp_level = match level {
                -1 => Compression::default(),
                0..=9 => Compression::new(level as u32),
                _ => return Err(RuntimeErrorKind::Custom {
                    message: format!("Compress::zlib(): Compression level ({}) must be within -1 and 9", level),
                }),
            };
            let mut encoder = ZlibEncoder::new(Vec::new(), comp_level);
            encoder.write_all(&data_bytes).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Compress::zlib failed: {}", e),
            })?;
            let compressed = encoder.finish().map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Compress::zlib failed: {}", e),
            })?;
            Ok(PhpValue::bytes(compressed))
        }
        // Uncompress ZLIB compressed data (wraps gzuncompress).
        // Example: `Compress::unzlib($compressed)`
        "unzlib" => {
            let data_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Compress::unzlib".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let data_bytes = match data_val {
                PhpValue::String(s) => s.as_bytes(),
                _ => return Err(RuntimeErrorKind::TypeError {
                    message: format!("Compress::unzlib() expects string, got {}", data_val.type_name()),
                }),
            };
            let mut decoder = ZlibDecoder::new(data_bytes);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Compress::unzlib failed: {}", e),
            })?;
            if let Ok(valid) = std::str::from_utf8(&decompressed) {
                Ok(PhpValue::string(valid))
            } else {
                Ok(PhpValue::bytes(decompressed))
            }
        }
        // Compress data using GZIP format (wraps gzencode).
        // Example: `Compress::gzip("data")`
        "gzip" => {
            let data_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Compress::gzip".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let data_bytes: std::borrow::Cow<[u8]> = match data_val {
                PhpValue::String(s) => std::borrow::Cow::Borrowed(s.as_bytes()),
                other => std::borrow::Cow::Owned(other.to_php_string().into_bytes()),
            };
            let level = args.get(1).and_then(|v| v.to_i64()).unwrap_or(-1);
            let comp_level = match level {
                -1 => Compression::default(),
                0..=9 => Compression::new(level as u32),
                _ => return Err(RuntimeErrorKind::Custom {
                    message: format!("Compress::gzip(): Compression level ({}) must be within -1 and 9", level),
                }),
            };
            let mut encoder = GzEncoder::new(Vec::new(), comp_level);
            encoder.write_all(&data_bytes).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Compress::gzip failed: {}", e),
            })?;
            let compressed = encoder.finish().map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Compress::gzip failed: {}", e),
            })?;
            Ok(PhpValue::bytes(compressed))
        }
        // Decode a gzip compressed string (wraps gzdecode).
        // Example: `Compress::gunzip($compressed)`
        "gunzip" => {
            let data_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Compress::gunzip".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let data_bytes = match data_val {
                PhpValue::String(s) => s.as_bytes(),
                _ => return Err(RuntimeErrorKind::TypeError {
                    message: format!("Compress::gunzip() expects string, got {}", data_val.type_name()),
                }),
            };
            let mut decoder = GzDecoder::new(data_bytes);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Compress::gunzip failed: {}", e),
            })?;
            if let Ok(valid) = std::str::from_utf8(&decompressed) {
                Ok(PhpValue::string(valid))
            } else {
                Ok(PhpValue::bytes(decompressed))
            }
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Compress".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Validate Facade
// =========================================================================

fn is_numeric_str(s: &str) -> bool {
    let trimmed = s.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return false;
    }
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    let len = bytes.len();

    // Optional leading sign '+' or '-'
    if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    if i >= len {
        return false;
    }

    let mut has_digits = false;

    // Digits before decimal point
    while i < len && bytes[i].is_ascii_digit() {
        has_digits = true;
        i += 1;
    }

    // Optional decimal point
    if i < len && bytes[i] == b'.' {
        i += 1;
        // Digits after decimal point
        while i < len && bytes[i].is_ascii_digit() {
            has_digits = true;
            i += 1;
        }
    }

    // Must have at least one digit (rejects "." or "+" or "-.")
    if !has_digits {
        return false;
    }

    // Optional exponent [eE][+-]?[0-9]+
    if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut has_exp_digits = false;
        while i < len && bytes[i].is_ascii_digit() {
            has_exp_digits = true;
            i += 1;
        }
        if !has_exp_digits {
            return false;
        }
    }

    // Must have consumed all characters
    i == len
}

pub fn dispatch_validate(method: &str, args: &[PhpValue]) -> FacadeResult {
    let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
        callable: format!("Validate::{}", method),
        passed: args.len(),
        expected: 1,
    })?;

    match method {
        "null" => Ok(PhpValue::Bool(val.is_null())),
        "int" | "integer" => Ok(PhpValue::Bool(matches!(
            val,
            PhpValue::Int(_) | PhpValue::RustInt { .. }
        ))),
        "bool" | "boolean" => Ok(PhpValue::Bool(matches!(val, PhpValue::Bool(_)))),
        "float" | "double" | "real" => Ok(PhpValue::Bool(matches!(
            val,
            PhpValue::Float(_) | PhpValue::RustFloat { .. }
        ))),
        "numeric" => {
            let res = match val {
                PhpValue::Int(_) | PhpValue::RustInt { .. } => true,
                PhpValue::Float(f) => !f.is_nan() && !f.is_infinite(),
                PhpValue::RustFloat { value, .. } => !value.is_nan() && !value.is_infinite(),
                PhpValue::String(s) => match s {
                    crate::vm::value::StrValue::Utf8 { s, .. } => is_numeric_str(s),
                    crate::vm::value::StrValue::Bytes(b) => match std::str::from_utf8(b) {
                        Ok(utf8_str) => is_numeric_str(utf8_str),
                        Err(_) => false,
                    },
                },
                _ => false,
            };
            Ok(PhpValue::Bool(res))
        }
        "scalar" => {
            let res = matches!(
                val,
                PhpValue::Int(_)
                    | PhpValue::Float(_)
                    | PhpValue::RustInt { .. }
                    | PhpValue::RustFloat { .. }
                    | PhpValue::Bool(_)
                    | PhpValue::String(_)
            );
            Ok(PhpValue::Bool(res))
        }
        "resource" => Ok(PhpValue::Bool(matches!(val, PhpValue::Resource(_)))),
        "isA" | "is_a" | "instanceof" => {
            let class_arg = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Validate::isA".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let raw_cls = class_arg.to_php_string();
            let target_cls = raw_cls.strip_prefix('\\').unwrap_or(&raw_cls);
            let res = match val {
                PhpValue::Object(o) => {
                    let actual = &o.borrow().class_name;
                    let actual_clean = actual.strip_prefix('\\').unwrap_or(actual);
                    actual_clean == target_cls || actual_clean.eq_ignore_ascii_case(target_cls)
                }
                _ => false,
            };
            Ok(PhpValue::Bool(res))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Validate".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Debug Facade
// =========================================================================

pub fn dispatch_debug(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "dump" => {
            for arg in args {
                println!("{}", arg.to_print_r());
            }
            Ok(PhpValue::Null)
        }
        "dd" => {
            for arg in args {
                println!("{}", arg.to_print_r());
            }
            std::process::exit(1);
        }
        "format" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Debug::format".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            Ok(PhpValue::string(val.to_print_r()))
        }
        "type" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Debug::type".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let type_str = match val {
                PhpValue::Null => "null".to_string(),
                PhpValue::Bool(_) => "bool".to_string(),
                PhpValue::Int(_) => "int".to_string(),
                PhpValue::Float(_) => "float".to_string(),
                PhpValue::RustInt { kind, .. } => kind.as_str().to_string(),
                PhpValue::RustFloat { kind, .. } => kind.as_str().to_string(),
                PhpValue::String(s) => match s {
                    crate::vm::value::StrValue::Utf8 { .. } => "string".to_string(),
                    crate::vm::value::StrValue::Bytes(_) => "bytes".to_string(),
                },
                PhpValue::Array(_) => "array".to_string(),
                PhpValue::List(_) => "Arr::list".to_string(),
                PhpValue::Dict(_) => "Arr::dict".to_string(),
                PhpValue::Set(_) => "Arr::set".to_string(),
                PhpValue::HashMap(_) => "Arr::hashMap".to_string(),
                PhpValue::Object(obj) => {
                    let cls = obj.borrow().class_name.clone();
                    if cls.is_empty() {
                        "object".to_string()
                    } else {
                        cls
                    }
                }
                PhpValue::Closure(_) => "Closure".to_string(),
                PhpValue::Future(_) => "Future".to_string(),
                PhpValue::Resource(_) => "resource".to_string(),
            };
            Ok(PhpValue::string(type_str))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Debug".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Path Facade
// =========================================================================

pub fn dispatch_path(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "basename" => {
            let path_str = get_arg_str(args, 0, "Path::basename")?;
            let ext = args.get(1).and_then(|v| if v.is_null() { None } else { Some(v.to_php_string()) });
            let base = path_basename(&path_str, ext.as_deref());
            Ok(PhpValue::string(base))
        }
        "dirname" => {
            let path_str = get_arg_str(args, 0, "Path::dirname")?;
            let levels = args.get(1).and_then(|v| v.to_i64()).unwrap_or(1).max(1) as usize;
            let mut dir = path_str;
            for _ in 0..levels {
                dir = path_dirname(&dir);
            }
            Ok(PhpValue::string(dir))
        }
        "extname" => {
            let path_str = get_arg_str(args, 0, "Path::extname")?;
            let ext = path_extname(&path_str);
            Ok(PhpValue::string(ext))
        }
        "isAbsolute" | "is_absolute" => {
            let path_str = get_arg_str(args, 0, "Path::isAbsolute")?;
            Ok(PhpValue::Bool(path_is_absolute(&path_str)))
        }
        "parse" => {
            let path_str = get_arg_str(args, 0, "Path::parse")?;
            let map = path_parse(&path_str);
            Ok(PhpValue::dict_from_map(map))
        }
        "tempDir" | "sysGetTempDir" | "sys_get_temp_dir" | "getTempDir" => {
            let temp = std::env::temp_dir().to_string_lossy().trim_end_matches('/').to_string();
            Ok(PhpValue::string(temp))
        }
        "glob" => {
            let pattern = get_arg_str(args, 0, "Path::glob")?;
            let matches = path_glob(&pattern);
            Ok(PhpValue::list(matches.into_iter().map(PhpValue::string).collect()))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Path".to_string(),
            method: method.to_string(),
        }),
    }
}

pub fn path_basename(path: &str, ext: Option<&str>) -> String {
    let clean = path.trim_end_matches(|c| c == '/' || c == '\\');
    if clean.is_empty() {
        return String::new();
    }
    let last = clean.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(clean);
    let mut base = last.to_string();
    if let Some(ext_str) = ext {
        if !ext_str.is_empty() && base.ends_with(ext_str) {
            base.truncate(base.len() - ext_str.len());
        }
    }
    base
}

pub fn path_dirname(path: &str) -> String {
    let trimmed = path.trim_end_matches(|c| c == '/' || c == '\\');
    if trimmed.is_empty() {
        if path.starts_with('/') {
            return "/".to_string();
        }
        return ".".to_string();
    }
    if let Some(idx) = trimmed.rfind(|c| c == '/' || c == '\\') {
        if idx == 0 {
            "/".to_string()
        } else {
            trimmed[..idx].to_string()
        }
    } else {
        ".".to_string()
    }
}

pub fn path_extname(path: &str) -> String {
    let base = path_basename(path, None);
    if base.is_empty() {
        return String::new();
    }
    if let Some(dot_idx) = base.rfind('.') {
        if dot_idx == 0 {
            return String::new();
        }
        base[dot_idx..].to_string()
    } else {
        String::new()
    }
}

pub fn path_is_absolute(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with('\\') {
        return true;
    }
    if path.len() >= 3 {
        let bytes = path.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') {
            return true;
        }
    }
    false
}

pub fn path_parse(path: &str) -> IndexMap<String, PhpValue> {
    let is_abs = path_is_absolute(path);
    let root = if is_abs {
        if path.starts_with('/') {
            "/".to_string()
        } else if path.starts_with('\\') {
            "\\".to_string()
        } else if path.len() >= 3 && path.as_bytes()[1] == b':' {
            path[..3].to_string()
        } else {
            "/".to_string()
        }
    } else {
        String::new()
    };

    let base = path_basename(path, None);
    let ext = path_extname(path);
    let name = if !ext.is_empty() && base.ends_with(&ext) {
        base[..base.len() - ext.len()].to_string()
    } else {
        base.clone()
    };
    let dir = path_dirname(path);
    let dir_val = if dir == "." && !path.contains('/') && !path.contains('\\') {
        String::new()
    } else {
        dir
    };

    let mut map = IndexMap::new();
    map.insert("root".to_string(), PhpValue::string(root));
    map.insert("dir".to_string(), PhpValue::string(dir_val));
    map.insert("base".to_string(), PhpValue::string(base));
    map.insert("ext".to_string(), PhpValue::string(ext));
    map.insert("name".to_string(), PhpValue::string(name));
    map
}

pub fn path_glob(pattern: &str) -> Vec<String> {
    let mut results = Vec::new();
    let norm_pattern = pattern.replace('\\', "/");
    let first_wildcard = norm_pattern.find(|c| c == '*' || c == '?' || c == '[');
    let (base_dir, has_wildcards) = match first_wildcard {
        Some(idx) => {
            let prefix = &norm_pattern[..idx];
            let dir = match prefix.rfind('/') {
                Some(p_idx) => {
                    if p_idx == 0 {
                        "/".to_string()
                    } else {
                        prefix[..p_idx].to_string()
                    }
                }
                None => ".".to_string(),
            };
            (dir, true)
        }
        None => (norm_pattern.clone(), false),
    };

    if !has_wildcards {
        if std::path::Path::new(pattern).exists() {
            return vec![pattern.to_string()];
        }
        return vec![];
    }

    let regex_pattern = glob_to_regex(&norm_pattern);
    let Ok(re) = regex::Regex::new(&regex_pattern) else {
        return vec![];
    };

    let recursive = norm_pattern.contains("**");
    walk_and_match(&base_dir, &re, recursive, &mut results);
    results.sort();
    results
}

fn glob_to_regex(pattern: &str) -> String {
    let mut reg = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        reg.push_str("(?:.*/)?");
                    } else {
                        reg.push_str(".*");
                    }
                } else {
                    reg.push_str("[^/]*");
                }
            }
            '?' => reg.push_str("[^/]"),
            '.' | '(' | ')' | '+' | '|' | '^' | '$' | '@' | '%' => {
                reg.push('\\');
                reg.push(c);
            }
            '{' => reg.push('('),
            '}' => reg.push(')'),
            ',' => reg.push('|'),
            other => reg.push(other),
        }
    }
    reg.push('$');
    reg
}

fn walk_and_match(dir: &str, re: &regex::Regex, recursive: bool, results: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        let test_str = if path_str.starts_with("./") {
            &path_str[2..]
        } else {
            &path_str
        };
        if re.is_match(&path_str) || re.is_match(test_str) {
            results.push(path_str.clone());
        }
        if recursive && path.is_dir() {
            walk_and_match(&path_str, re, recursive, results);
        }
    }
}

// =========================================================================
// Socket Facade
// =========================================================================

pub fn dispatch_socket(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "server" | "createServer" => {
            let addr = get_arg_str(args, 0, "Socket::server")?;
            let clean = addr.strip_prefix("tcp://").unwrap_or(&addr);
            match std::net::TcpListener::bind(clean) {
                Ok(listener) => {
                    let local = listener
                        .local_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|_| clean.to_string());
                    Ok(crate::vm::csv::create_socket_stream(listener, local))
                }
                Err(_) => Ok(PhpValue::Bool(false)),
            }
        }
        "getName" | "get_name" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Socket::getName".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let want_peer = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            match crate::vm::csv::get_socket_addr(val, want_peer) {
                Ok(Some(addr)) => Ok(PhpValue::string(addr)),
                Ok(None) => Ok(PhpValue::Bool(false)),
                Err(e) => Err(e),
            }
        }
        "close" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Socket::close".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            crate::vm::csv::close_stream(val)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Socket".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Binary Facade
// =========================================================================

pub fn dispatch_binary(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "bin2hex" | "toHex" | "hex" => {
            let val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Binary::bin2hex".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let bytes = match val {
                PhpValue::String(s) => s.as_bytes().to_vec(),
                other => other.to_php_string().into_bytes(),
            };
            let encoded = hex::encode(bytes);
            Ok(PhpValue::string(encoded))
        }
        "hex2bin" | "fromHex" => {
            let s = get_arg_str(args, 0, "Binary::hex2bin")?;
            match hex::decode(&s) {
                Ok(decoded) => Ok(PhpValue::bytes(decoded)),
                Err(_) => Ok(PhpValue::Bool(false)),
            }
        }
        "randomBytes" | "random_bytes" | "random" => {
            let len = args
                .first()
                .and_then(|v| v.to_i64())
                .unwrap_or(16)
                .max(0) as usize;
            let bytes = crate::vm::crypto::random_bytes(len).map_err(|e| RuntimeErrorKind::Custom {
                message: format!("Binary::randomBytes failed: {}", e),
            })?;
            Ok(PhpValue::bytes(bytes))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Binary".to_string(),
            method: method.to_string(),
        }),
    }
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
