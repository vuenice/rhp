use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use indexmap::IndexMap;

use crate::lexer::facade::{get_macro, register_tier2_facade, FacadeRegistry};
use crate::lexer::span::Span;
use crate::type_system::types::{RustFloatKind, RustIntKind};
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::fiber::{
    current_fiber_stats, get_fiber_local, is_current_fiber, record_fiber_io_finish,
    record_fiber_io_start, record_fiber_yield, set_fiber_local, with_current_fiber, IoKind,
};
use crate::vm::value::{PhpValue, VmClosure, VmFuture, VmObject};

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
        "HTTP" => dispatch_http(method, args),
        "Log" => dispatch_log(method, args),
        "DB" => dispatch_db(method, args),
        "Int" => dispatch_int(method, args),
        "Float" => dispatch_float(method, args),
        "Session" => dispatch_session(method, args),
        "Image" => dispatch_image(method, args),
        "URL" => dispatch_url(method, args),
        "Glob" => dispatch_glob(method, args),
        "phpFun" => dispatch_php_fun(method, args),
        "Fiber" => dispatch_fiber(method, args),
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
        "contains" => {
            let haystack = get_arg_str(args, 0, "Str::contains")?;
            let needle = get_arg_str(args, 1, "Str::contains")?;
            Ok(PhpValue::Bool(haystack.contains(&needle)))
        }
        "startsWith" => {
            let s = get_arg_str(args, 0, "Str::startsWith")?;
            let prefix = get_arg_str(args, 1, "Str::startsWith")?;
            Ok(PhpValue::Bool(s.starts_with(&prefix)))
        }
        "endsWith" => {
            let s = get_arg_str(args, 0, "Str::endsWith")?;
            let suffix = get_arg_str(args, 1, "Str::endsWith")?;
            Ok(PhpValue::Bool(s.ends_with(&suffix)))
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
            let search = get_arg_str(args, 0, "Str::replace")?;
            let replace = get_arg_str(args, 1, "Str::replace")?;
            let subject = get_arg_str(args, 2, "Str::replace")?;
            Ok(PhpValue::string(subject.replace(&search, &replace)))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Str".to_string(),
            method: method.to_string(),
        }),
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
            let start = args.first().and_then(|v| v.to_i64()).unwrap_or(0);
            let end = args.get(1).and_then(|v| v.to_i64()).unwrap_or(0);
            let step = args.get(2).and_then(|v| v.to_i64()).unwrap_or(1);
            let step = if step == 0 { 1 } else { step };
            let mut list = Vec::new();
            if start <= end && step > 0 {
                let mut curr = start;
                while curr <= end {
                    list.push(PhpValue::Int(curr as i32));
                    curr += step;
                }
            } else if start >= end && step < 0 {
                let mut curr = start;
                while curr >= end {
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
        "count" => {
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
        "contains" => {
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
        "has" => {
            let target = args.first().cloned().unwrap_or(PhpValue::Null);
            let key = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
            Ok(PhpValue::Bool(arr_has_dot(&target, &key)))
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
        "wait" => {
            // Eagerly resolve array of tasks/values
            let mut resolved = Vec::new();
            if let Some(first) = args.first() {
                for item in extract_array_elements(first) {
                    match item {
                        PhpValue::Future(f) => {
                            if let Some(ref val) = f.borrow().value {
                                resolved.push(val.clone());
                            } else {
                                resolved.push(PhpValue::Null);
                            }
                        }
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
                PhpValue::Future(f) => Ok(f.borrow().value.clone().unwrap_or(PhpValue::Null)),
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
        "print" => {
            for arg in args {
                print!("{}", arg.to_php_string());
            }
            Ok(PhpValue::Null)
        }
        "dd" => {
            for arg in args {
                println!("{:?}", arg);
            }
            Ok(PhpValue::Null)
        }
        "os" => Ok(PhpValue::string(std::env::consts::OS)),
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
        "delete" => {
            let path = get_arg_str(args, 0, "File::delete")?;
            record_fiber_io_start(IoKind::FileDelete { path: path.clone() });
            let ok = std::fs::remove_file(&path).is_ok();
            record_fiber_io_finish();
            Ok(PhpValue::Bool(ok))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "File".to_string(),
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
            let mut obj = VmObject::new("HTTPResponse");
            obj.set_property("url", PhpValue::string(url));
            obj.set_property("status", PhpValue::Int(200));
            obj.set_property("body", PhpValue::string("{\"status\":\"ok\"}"));
            record_fiber_io_finish();
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
    match method {
        "table" => {
            let tbl = get_arg_str(args, 0, "DB::table")?;
            record_fiber_io_start(IoKind::DbTable { table: tbl.clone() });
            let mut obj = VmObject::new("DBQuery");
            obj.set_property("table", PhpValue::string(tbl));
            record_fiber_io_finish();
            Ok(PhpValue::object(obj))
        }
        "raw" => {
            let sql = get_arg_str(args, 0, "DB::raw")?;
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
        "ceil" => {
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

fn dispatch_session(method: &str, _args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" | "put" | "has" | "forget" => Ok(PhpValue::Null),
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Session".to_string(),
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
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "URL".to_string(),
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
            Ok(PhpValue::Bool(subject.contains(&pattern)))
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
        "strlen" => dispatch_str("length", args),
        "str_contains" => dispatch_str("contains", args),
        "implode" => dispatch_str("join", args),
        "explode" => dispatch_str("split", args),
        _ => Err(RuntimeErrorKind::UndefinedFunction {
            name: method.to_string(),
        }),
    }
}

// =========================================================================
// Helpers
// =========================================================================

fn get_arg_str(args: &[PhpValue], index: usize, callable: &str) -> Result<String, RuntimeErrorKind> {
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
