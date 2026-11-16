//! # Package Plugin Loader & Native Extension Manager
//!
//! Enables importing package plugins in RHP (`import "cache";` or `import "vuenice/cache" as Cache;`),
//! exposing high-performance helper facades (such as `Cache`, `Log`, and custom utilities) and functions
//! directly in script-land without requiring direct `.rs` file imports or exposing Rust structs/traits.
//!
//! Direct importing of `.rs` files and on-the-fly compiling via `rustc` is prohibited. Native extensions
//! must be distributed as package plugins exposing only helper facades and functions.

use std::collections::{HashMap, HashSet};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::lexer::facade::{get_static_facade, register_tier2_facade};
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::facade::{append_log, register_tier2_plugin_handler, Tier2PluginHandler};
use crate::vm::fiber::dispatch_blocking_native;
use crate::vm::parent_process::FrozenValue;
use crate::vm::value::PhpValue;

// =========================================================================
// In-Memory High-Performance Cache Store
// =========================================================================

/// Thread-safe in-memory cache store with optional TTL expiry.
#[derive(Debug, Default)]
pub struct RustCacheStore {
    items: RwLock<HashMap<String, (FrozenValue, Option<Instant>)>>,
}

impl RustCacheStore {
    pub fn new() -> Self {
        Self {
            items: RwLock::new(HashMap::new()),
        }
    }

    /// Retrieve an item from the cache. Returns `default` (or `Null`) if expired or missing.
    pub fn get(&self, key: &str, default: Option<PhpValue>) -> PhpValue {
        let now = Instant::now();
        let mut expired = false;

        if let Ok(guard) = self.items.read() {
            if let Some((val, expires_at)) = guard.get(key) {
                if let Some(exp) = expires_at {
                    if now >= *exp {
                        expired = true;
                    } else {
                        return val.to_php_value();
                    }
                } else {
                    return val.to_php_value();
                }
            }
        }

        if expired {
            if let Ok(mut guard) = self.items.write() {
                guard.remove(key);
            }
        }

        default.unwrap_or(PhpValue::Null)
    }

    /// Store an item in the cache with an optional TTL in seconds.
    pub fn set(&self, key: &str, val: &PhpValue, ttl_seconds: Option<u64>) -> bool {
        let expires_at = ttl_seconds.map(|ttl| Instant::now() + Duration::from_secs(ttl));
        let frozen = FrozenValue::from_php_value(val);
        if let Ok(mut guard) = self.items.write() {
            guard.insert(key.to_string(), (frozen, expires_at));
            true
        } else {
            false
        }
    }

    /// Check if an item exists and has not expired.
    pub fn has(&self, key: &str) -> bool {
        let now = Instant::now();
        if let Ok(guard) = self.items.read() {
            if let Some((_, expires_at)) = guard.get(key) {
                if let Some(exp) = expires_at {
                    if now < *exp {
                        return true;
                    }
                } else {
                    return true;
                }
            }
        }
        false
    }

    /// Delete an item from the cache.
    pub fn delete(&self, key: &str) -> bool {
        if let Ok(mut guard) = self.items.write() {
            guard.remove(key).is_some()
        } else {
            false
        }
    }

    /// Clear all items from the cache.
    pub fn clear(&self) -> bool {
        if let Ok(mut guard) = self.items.write() {
            guard.clear();
            true
        } else {
            false
        }
    }

    /// Increment an integer value in the cache.
    pub fn increment(&self, key: &str, amount: i64) -> i64 {
        if let Ok(mut guard) = self.items.write() {
            let entry = guard.entry(key.to_string()).or_insert((FrozenValue::Int(0), None));
            let new_val = match &entry.0 {
                FrozenValue::Int(n) => *n + amount,
                _ => amount,
            };
            entry.0 = FrozenValue::Int(new_val);
            new_val
        } else {
            amount
        }
    }

    /// Decrement an integer value in the cache.
    pub fn decrement(&self, key: &str, amount: i64) -> i64 {
        self.increment(key, -amount)
    }

    /// Store item only if it does not already exist.
    pub fn add(&self, key: &str, val: &PhpValue, ttl_seconds: Option<u64>) -> bool {
        if self.has(key) {
            false
        } else {
            self.set(key, val, ttl_seconds)
        }
    }

    /// Retrieve an item and delete it from the cache.
    pub fn pull(&self, key: &str, default: Option<PhpValue>) -> PhpValue {
        let val = self.get(key, default);
        self.delete(key);
        val
    }

    /// Retrieve all non-expired entries as an IndexMap.
    pub fn all(&self) -> IndexMap<String, PhpValue> {
        let mut result = IndexMap::new();
        let now = Instant::now();
        if let Ok(guard) = self.items.read() {
            for (k, (v, exp)) in guard.iter() {
                if let Some(exp_time) = exp {
                    if now < *exp_time {
                        result.insert(k.clone(), v.to_php_value());
                    }
                } else {
                    result.insert(k.clone(), v.to_php_value());
                }
            }
        }
        result
    }

    /// Retrieve all active keys.
    pub fn keys(&self) -> Vec<String> {
        let now = Instant::now();
        let mut keys = Vec::new();
        if let Ok(guard) = self.items.read() {
            for (k, (_, exp)) in guard.iter() {
                if let Some(exp_time) = exp {
                    if now < *exp_time {
                        keys.push(k.clone());
                    }
                } else {
                    keys.push(k.clone());
                }
            }
        }
        keys
    }

    /// Count active non-expired entries.
    pub fn count(&self) -> usize {
        self.keys().len()
    }
}

// =========================================================================
// In-Memory High-Performance Log Store
// =========================================================================

/// Structured log record captured by the Rust logger.
#[derive(Debug, Clone, PartialEq)]
pub struct RustLogEntry {
    pub level: String,
    pub message: String,
    pub context: Option<FrozenValue>,
    pub channel: String,
    pub timestamp_ms: u64,
}

/// Thread-safe in-memory log store.
#[derive(Debug, Default)]
pub struct RustLogStore {
    entries: RwLock<Vec<RustLogEntry>>,
    current_channel: RwLock<String>,
}

impl RustLogStore {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            current_channel: RwLock::new("default".to_string()),
        }
    }

    pub fn log(&self, level: &str, message: &str, context: Option<&PhpValue>) {
        let ch = self
            .current_channel
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "default".to_string());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let entry = RustLogEntry {
            level: level.to_lowercase(),
            message: message.to_string(),
            context: context.map(FrozenValue::from_php_value),
            channel: ch.clone(),
            timestamp_ms: now_ms,
        };

        if let Ok(mut guard) = self.entries.write() {
            guard.push(entry);
        }

        // Also push to centralized VM log buffer for unified observability
        append_log(level, message, context.cloned(), &ch);
    }

    pub fn channel(&self, name: &str) {
        if let Ok(mut guard) = self.current_channel.write() {
            *guard = name.to_string();
        }
    }

    pub fn history(&self) -> Vec<RustLogEntry> {
        self.entries
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.entries.write() {
            guard.clear();
        }
    }
}

// =========================================================================
// Loaded Module Registry
// =========================================================================

#[derive(Clone, Copy)]
pub struct RawDlHandle(pub *mut std::ffi::c_void);
unsafe impl Send for RawDlHandle {}
unsafe impl Sync for RawDlHandle {}

pub type RhpNativeCallFn = extern "C" fn(*const c_char, *const c_char) -> *mut c_char;

/// State for a loaded Rust helper class.
pub struct LoadedRustModule {
    pub name: String,
    pub file_path: Option<PathBuf>,
    pub cache_store: Option<Arc<RustCacheStore>>,
    pub log_store: Option<Arc<RustLogStore>>,
    pub custom_functions: HashSet<String>,
    pub blocking_functions: HashSet<String>,
    pub dl_handle: Option<RawDlHandle>,
    pub call_fn: Option<RhpNativeCallFn>,
}

impl LoadedRustModule {
    /// Check if a method on this loaded module is annotated with `#[blocking]`.
    pub fn is_blocking(&self, method: &str) -> bool {
        self.blocking_functions.contains(method)
            || is_native_function_blocking(&self.name, method)
    }
}

unsafe impl Send for LoadedRustModule {}
unsafe impl Sync for LoadedRustModule {}

static LOADED_MODULES: OnceLock<RwLock<HashMap<String, Arc<LoadedRustModule>>>> = OnceLock::new();

fn get_loaded_modules() -> &'static RwLock<HashMap<String, Arc<LoadedRustModule>>> {
    LOADED_MODULES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Global registry for native/FFI functions explicitly registered as blocking.
static BLOCKING_NATIVE_FUNCTIONS: OnceLock<RwLock<HashSet<(String, String)>>> = OnceLock::new();

fn get_blocking_registry() -> &'static RwLock<HashSet<(String, String)>> {
    BLOCKING_NATIVE_FUNCTIONS.get_or_init(|| RwLock::new(HashSet::new()))
}

/// Mark a native/FFI function as blocking (`#[blocking]`).
pub fn register_blocking_native_function(extension: &str, function: &str) {
    if let Ok(mut g) = get_blocking_registry().write() {
        g.insert((extension.to_string(), function.to_string()));
    }
}

/// Check if a native/FFI function is marked as blocking (`#[blocking]`).
pub fn is_native_function_blocking(extension: &str, function: &str) -> bool {
    if let Ok(guard) = get_loaded_modules().read() {
        if let Some(module) = guard.get(extension) {
            if module.blocking_functions.contains(function) {
                return true;
            }
        }
    }
    if let Ok(g) = get_blocking_registry().read() {
        if g.contains(&(extension.to_string(), function.to_string())) {
            return true;
        }
    }
    false
}

/// Clear all registered blocking native functions.
pub fn clear_blocking_native_functions() {
    if let Ok(mut g) = get_blocking_registry().write() {
        g.clear();
    }
}

/// In-process native execution callback.
pub type InProcessNativeHandler =
    Arc<dyn Fn(&[FrozenValue]) -> Result<FrozenValue, RuntimeError> + Send + Sync>;

static IN_PROCESS_NATIVE_FUNCTIONS: OnceLock<
    RwLock<HashMap<(String, String), (InProcessNativeHandler, bool)>>,
> = OnceLock::new();

fn get_in_process_registry(
) -> &'static RwLock<HashMap<(String, String), (InProcessNativeHandler, bool)>> {
    IN_PROCESS_NATIVE_FUNCTIONS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Register an in-process native function with optional `#[blocking]` behavior.
pub fn register_in_process_native_function(
    facade: &str,
    method: &str,
    is_blocking: bool,
    handler: impl Fn(&[FrozenValue]) -> Result<FrozenValue, RuntimeError> + Send + Sync + 'static,
) {
    register_tier2_facade(
        facade.to_string(),
        format!("Native extension '{}'", facade),
        true,
    );
    if let Ok(mut g) = get_in_process_registry().write() {
        g.insert(
            (facade.to_string(), method.to_string()),
            (Arc::new(handler), is_blocking),
        );
    }
    if is_blocking {
        register_blocking_native_function(facade, method);
    }
}

/// Retrieve a registered in-process native function.
pub fn get_in_process_native_function(
    facade: &str,
    method: &str,
) -> Option<(InProcessNativeHandler, bool)> {
    get_in_process_registry()
        .read()
        .ok()
        .and_then(|g| g.get(&(facade.to_string(), method.to_string())).cloned())
}

/// Remove all loaded Rust modules and native registrations (for testing and dev hot reload).
pub fn clear_rust_modules() {
    clear_blocking_native_functions();
    crate::vm::facade::clear_tier2_plugins();
    if let Ok(mut g) = get_in_process_registry().write() {
        g.clear();
    }
    if let Ok(mut guard) = get_loaded_modules().write() {
        #[cfg(unix)]
        for module in guard.values() {
            if let Some(handle) = module.dl_handle {
                unsafe {
                    libc::dlclose(handle.0);
                }
            }
        }
        guard.clear();
    }
}

// =========================================================================
// Name Inference & File Resolution
// =========================================================================

/// Convert a string to PascalCase (e.g. `cache` -> `Cache`, `my_logger` -> `MyLogger`).
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;

    for ch in s.chars() {
        if ch == '_' || ch == '-' || ch == '.' || ch == '/' || ch == '\\' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    if result.is_empty() {
        "Helper".to_string()
    } else {
        result
    }
}

/// Infer the facade class name from an imported path or explicit alias.
pub fn infer_facade_name(path_str: &str, alias: Option<&str>) -> String {
    if let Some(a) = alias {
        let trimmed = a.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let p = Path::new(path_str);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Helper");

    to_pascal_case(stem)
}

/// Resolve a package path (direct .rs file resolution is prohibited).
pub fn resolve_rust_file_path(path_str: &str, _base_dir: Option<&Path>) -> Result<PathBuf, String> {
    if path_str.ends_with(".rs") || path_str.ends_with(".rs\"") || path_str.ends_with(".rs'") {
        return Err(format!(
            "Direct usage of Rust source files ('.rs') is prohibited. Native extensions must be imported as a package plugin exposing a helper facade or function: '{}'",
            path_str
        ));
    }
    Err(format!("Package '{}' not found", path_str))
}

/// Parse public functions and detect `#[blocking]` attribute annotations from a `.rs` file.
pub fn extract_rust_functions(source: &str) -> (HashSet<String>, HashSet<String>) {
    let mut functions = HashSet::new();
    let mut blocking_functions = HashSet::new();

    // Check for file/module-level blocking attribute `#![blocking]`
    let module_level_blocking = source.lines().any(|l| {
        let t = l.trim();
        t.starts_with("#![blocking]")
            || t.starts_with("#![Blocking]")
            || t.starts_with("#![rhp::blocking]")
    });

    let mut pending_blocking = module_level_blocking;

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Line comment without attribute
        if trimmed.starts_with("//") && !trimmed.contains("#[") {
            continue;
        }

        let has_blocking_attr = trimmed.contains("#[blocking]")
            || trimmed.contains("#[Blocking]")
            || trimmed.contains("#[rhp::blocking]")
            || trimmed.contains("#[blocking ]")
            || trimmed.contains("#[ blocking ]")
            || trimmed.contains("#[blocking()]");

        if has_blocking_attr {
            pending_blocking = true;
        }

        let fn_ident = if let Some(rest) = trimmed.strip_prefix("pub fn ") {
            rest.split('(').next()
        } else if let Some(rest) = trimmed.strip_prefix("pub extern \"C\" fn ") {
            rest.split('(').next()
        } else if let Some(rest) = trimmed.strip_prefix("pub unsafe extern \"C\" fn ") {
            rest.split('(').next()
        } else if let Some(rest) = trimmed.strip_prefix("pub unsafe fn ") {
            rest.split('(').next()
        } else if let Some(idx) = trimmed.find("pub fn ") {
            trimmed[idx + 7..].split('(').next()
        } else if let Some(idx) = trimmed.find("pub extern \"C\" fn ") {
            trimmed[idx + 18..].split('(').next()
        } else {
            None
        };

        if let Some(ident) = fn_ident {
            let name = ident.trim().trim_start_matches("r#");
            if !name.is_empty() {
                functions.insert(name.to_string());
                if pending_blocking || module_level_blocking {
                    blocking_functions.insert(name.to_string());
                }
            }
            pending_blocking = module_level_blocking;
        } else if !trimmed.starts_with("#[")
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("/*")
            && !trimmed.starts_with('*')
        {
            if !has_blocking_attr {
                pending_blocking = module_level_blocking;
            }
        }
    }

    (functions, blocking_functions)
}

/// Parse function names from a `.rs` file (matches `pub fn <name>`).
pub fn extract_pub_functions(source: &str) -> HashSet<String> {
    extract_rust_functions(source).0
}

/// Extract function names annotated with `#[blocking]` from a `.rs` file.
pub fn extract_blocking_functions(source: &str) -> HashSet<String> {
    extract_rust_functions(source).1
}

/// Sanitize `#[blocking]` annotations from Rust source code so `rustc` will not reject them
/// as unknown attributes when compiling dynamic libraries (`cdylib`).
pub fn sanitize_rust_source_for_rustc(source: &str) -> String {
    let re = regex::Regex::new(r"(?i)#\[\s*(?:rhp::)?blocking(?:\([^)]*\))?\s*\]").unwrap();
    let res = re.replace_all(source, "/* #[blocking] */");
    let re_inner =
        regex::Regex::new(r"(?i)#!\s*\[\s*(?:rhp::)?blocking(?:\([^)]*\))?\s*\]").unwrap();
    re_inner.replace_all(&res, "/* #![blocking] */").into_owned()
}

// =========================================================================
// Package Plugin Loader (Tier 2 Helper Facades & Functions Only)
// =========================================================================

/// Find package manifest `rhp.toml` for a package if available.
pub fn find_package_manifest(pkg_name: &str, base_dir: Option<&Path>) -> Option<crate::pkg::Manifest> {
    let clean = pkg_name.trim_matches('"').trim_matches('\'');
    let mut candidate_dirs = vec![
        PathBuf::from(clean),
        PathBuf::from("vendor").join(clean),
    ];
    if let Some(base) = base_dir {
        candidate_dirs.push(base.join(clean));
        candidate_dirs.push(base.join("vendor").join(clean));
    }
    candidate_dirs.push(PathBuf::from("."));

    for dir in candidate_dirs {
        if let Some(manifest_path) = crate::pkg::Manifest::find_in_dir(&dir) {
            if let Ok(m) = crate::pkg::Manifest::load_from_file(&manifest_path) {
                return Some(m);
            }
        }
    }
    None
}

/// Load and register a package plugin as a Tier 2 static facade helper in RHP.
///
/// Returns the registered facade name (e.g., `"Cache"`, `"Log"`, `"Mongo"`).
pub fn load_and_register_package_plugin(
    pkg_name: &str,
    alias: Option<&str>,
    base_dir: Option<&Path>,
) -> Result<String, RuntimeError> {
    let clean_name = pkg_name.trim_matches('"').trim_matches('\'');

    // 0. Prohibit direct importing of .rs files
    if clean_name.ends_with(".rs") {
        return Err(RuntimeError::new(
            RuntimeErrorKind::Custom {
                message: format!(
                    "Direct importing of Rust source files ('.rs') is prohibited. Native extensions must be imported as a package plugin exposing a helper facade or function: '{}'",
                    clean_name
                ),
            },
            crate::lexer::span::Span::default(),
        ));
    }

    let facade_name = infer_facade_name(clean_name, alias);

    // 1. Check if package manifest exists and enforce:
    // "a rust package providing struct or trait can't be used in RHP"
    if let Some(manifest) = find_package_manifest(clean_name, base_dir) {
        if let Some(ref exports) = manifest.exports {
            if !exports.structs.is_empty() {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: format!(
                            "Rust package '{}' cannot export structs to RHP ({:?}). Only helper facades and functions are supported via plugins.",
                            clean_name, exports.structs
                        ),
                    },
                    crate::lexer::span::Span::default(),
                ));
            }
            if !exports.traits.is_empty() {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::Custom {
                        message: format!(
                            "Rust package '{}' cannot export traits to RHP ({:?}). Only helper facades and functions are supported via plugins.",
                            clean_name, exports.traits
                        ),
                    },
                    crate::lexer::span::Span::default(),
                ));
            }
        }
    }

    // 2. Check if already loaded in registry
    if let Ok(guard) = get_loaded_modules().read() {
        if guard.contains_key(&facade_name) && crate::vm::facade::has_tier2_plugin(&facade_name) {
            if get_static_facade(&facade_name).is_none() {
                register_tier2_facade(
                    facade_name.clone(),
                    format!("Package plugin helper '{}'", clean_name),
                    true,
                );
            }
            return Ok(facade_name);
        }
    }

    // 3. Initialize in-process stores for built-in/first-party package plugins (Cache, Log)
    let is_cache_facade = facade_name.eq_ignore_ascii_case("cache")
        || clean_name.to_lowercase().contains("cache");
    let is_log_facade = facade_name.eq_ignore_ascii_case("log")
        || clean_name.to_lowercase().contains("log");

    let cache_store = if is_cache_facade {
        Some(Arc::new(RustCacheStore::new()))
    } else {
        None
    };

    let log_store = if is_log_facade {
        Some(Arc::new(RustLogStore::new()))
    } else {
        None
    };

    let (final_cache, final_log) = match (&cache_store, &log_store) {
        (None, None) => {
            if facade_name.to_lowercase().contains("log") {
                (None, Some(Arc::new(RustLogStore::new())))
            } else if is_cache_facade {
                (Some(Arc::new(RustCacheStore::new())), None)
            } else {
                (None, None)
            }
        }
        _ => (cache_store, log_store),
    };

    let module = Arc::new(LoadedRustModule {
        name: facade_name.clone(),
        file_path: None,
        cache_store: final_cache.clone(),
        log_store: final_log.clone(),
        custom_functions: HashSet::new(),
        blocking_functions: HashSet::new(),
        dl_handle: None,
        call_fn: None,
    });

    if let Ok(mut guard) = get_loaded_modules().write() {
        guard.insert(facade_name.clone(), module.clone());
    }

    // 4. Register with FacadeRegistry as Tier 2 Native Helper Facade
    register_tier2_facade(
        facade_name.clone(),
        format!("Package plugin helper '{}'", clean_name),
        true,
    );

    // 5. Build execution handler dispatching to the in-process plugin or registered functions
    let handler_cache = final_cache;
    let handler_log = final_log;
    let handler_facade = facade_name.clone();

    let handler: Tier2PluginHandler = Arc::new(move |method, args, span| {
        let is_blocking = is_native_function_blocking(&handler_facade, method);

        // Check if an in-process native function was registered
        if let Some((in_proc_fn, in_proc_blocking)) =
            get_in_process_native_function(&handler_facade, method)
        {
            let blocking = is_blocking || in_proc_blocking;
            let frozen_args: Vec<FrozenValue> =
                args.iter().map(FrozenValue::from_php_value).collect();
            if blocking {
                let fn_clone = in_proc_fn.clone();
                let facade_owned = handler_facade.clone();
                let method_owned = method.to_string();
                let frozen_res = dispatch_blocking_native(&facade_owned, &method_owned, move || {
                    fn_clone(&frozen_args)
                })?;
                return Ok(frozen_res.to_php_value());
            } else {
                let frozen_res = in_proc_fn(&frozen_args)?;
                return Ok(frozen_res.to_php_value());
            }
        }

        // In-process Cache / Log execution
        if is_blocking {
            let method_owned = method.to_string();
            let facade_name_str = handler_facade.clone();
            let cache_clone = handler_cache.clone();
            let log_clone = handler_log.clone();
            let frozen_args: Vec<FrozenValue> =
                args.iter().map(FrozenValue::from_php_value).collect();
            let method_call = method_owned.clone();
            let facade_call = facade_name_str.clone();
            let frozen_res = dispatch_blocking_native(&facade_name_str, &method_owned, move || {
                let thawed_args: Vec<PhpValue> =
                    frozen_args.iter().map(|f| f.to_php_value()).collect();
                execute_in_process_cache_or_log(
                    &method_call,
                    &thawed_args,
                    &cache_clone,
                    &log_clone,
                    &facade_call,
                    span,
                ).map(|val| FrozenValue::from_php_value(&val))
            })?;
            return Ok(frozen_res.to_php_value());
        }

        execute_in_process_cache_or_log(
            method,
            args,
            &handler_cache,
            &handler_log,
            &handler_facade,
            span,
        )
    });

    register_tier2_plugin_handler(
        &facade_name,
        &format!("Imported package plugin helper '{}'", clean_name),
        true,
        handler,
    );

    Ok(facade_name)
}

/// Backwards compatibility alias for `load_and_register_package_plugin`.
pub fn load_and_register_rust_file(
    path_str: &str,
    alias: Option<&str>,
    base_dir: Option<&Path>,
) -> Result<String, RuntimeError> {
    load_and_register_package_plugin(path_str, alias, base_dir)
}

fn execute_in_process_cache_or_log(
    method: &str,
    args: &[PhpValue],
    cache: &Option<Arc<RustCacheStore>>,
    log: &Option<Arc<RustLogStore>>,
    facade_name: &str,
    span: crate::lexer::span::Span,
) -> Result<PhpValue, RuntimeError> {
    if let Some(cache) = cache {
        match method {
            "get" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let default = args.get(1).cloned();
                return Ok(cache.get(&key, default));
            }
            "set" | "put" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                let ttl = args.get(2).and_then(|v| match v {
                    PhpValue::Int(n) if *n > 0 => Some(*n as u64),
                    _ => None,
                });
                return Ok(PhpValue::Bool(cache.set(&key, &val, ttl)));
            }
            "has" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                return Ok(PhpValue::Bool(cache.has(&key)));
            }
            "delete" | "forget" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                return Ok(PhpValue::Bool(cache.delete(&key)));
            }
            "clear" | "flush" => {
                return Ok(PhpValue::Bool(cache.clear()));
            }
            "increment" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let amt = args.get(1).and_then(|v| match v {
                    PhpValue::Int(n) => Some(*n as i64),
                    _ => None,
                }).unwrap_or(1);
                return Ok(PhpValue::Int(cache.increment(&key, amt) as i64));
            }
            "decrement" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let amt = args.get(1).and_then(|v| match v {
                    PhpValue::Int(n) => Some(*n as i64),
                    _ => None,
                }).unwrap_or(1);
                return Ok(PhpValue::Int(cache.decrement(&key, amt) as i64));
            }
            "add" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                let ttl = args.get(2).and_then(|v| match v {
                    PhpValue::Int(n) if *n > 0 => Some(*n as u64),
                    _ => None,
                });
                return Ok(PhpValue::Bool(cache.add(&key, &val, ttl)));
            }
            "pull" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let default = args.get(1).cloned();
                return Ok(cache.pull(&key, default));
            }
            "all" => {
                return Ok(PhpValue::dict(cache.all()));
            }
            "keys" => {
                let keys: Vec<PhpValue> = cache.keys().into_iter().map(PhpValue::string).collect();
                return Ok(PhpValue::list(keys));
            }
            "count" => {
                return Ok(PhpValue::Int(cache.count() as i64));
            }
            _ => {}
        }
    }

    if let Some(log) = log {
        match method {
            "info" | "error" | "warn" | "warning" | "debug" | "notice" | "critical" | "emergency" | "alert" => {
                let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let ctx = args.get(1);
                let lvl = if method == "warning" { "warn" } else { method };
                log.log(lvl, &msg, ctx);
                return Ok(PhpValue::Null);
            }
            "log" => {
                let lvl = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| "info".to_string());
                let msg = args.get(1).map(|v| v.to_php_string()).unwrap_or_default();
                let ctx = args.get(2);
                log.log(&lvl, &msg, ctx);
                return Ok(PhpValue::Null);
            }
            "history" => {
                let entries = log.history();
                let list: Vec<PhpValue> = entries
                    .into_iter()
                    .map(|e| {
                        let mut d = IndexMap::new();
                        d.insert("level".to_string(), PhpValue::string(e.level));
                        d.insert("message".to_string(), PhpValue::string(e.message));
                        d.insert("context".to_string(), e.context.map(|c| c.to_php_value()).unwrap_or(PhpValue::Null));
                        d.insert("channel".to_string(), PhpValue::string(e.channel));
                        d.insert("timestamp".to_string(), PhpValue::Int(e.timestamp_ms as i64));
                        PhpValue::dict(d)
                    })
                    .collect();
                return Ok(PhpValue::list(list));
            }
            "channel" => {
                if let Some(ch) = args.first().map(|v| v.to_php_string()) {
                    log.channel(&ch);
                }
                return Ok(PhpValue::Null);
            }
            "clear" => {
                log.clear();
                return Ok(PhpValue::Null);
            }
            _ => {}
        }
    }

    Err(RuntimeError::new(
        RuntimeErrorKind::UndefinedFacadeMethod {
            facade: facade_name.to_string(),
            method: method.to_string(),
        },
        span,
    ))
}
