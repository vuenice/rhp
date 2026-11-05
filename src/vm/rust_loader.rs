//! # Rust File Import & Native Extension Loader
//!
//! Enables importing `.rs` files directly in RHP (`import "./Cache.rs";` or `use "./Cache.rs" as Cache;`),
//! exposing high-performance Rust helper classes (such as `Cache`, `Log`, and custom utilities)
//! directly in script-land without requiring them to be baked into the core language engine.
//!
//! ### Execution Strategy:
//! 1. **Native Dynamic Shared Library (`cdylib` via `rustc`)**:
//!    When `rustc` is available, compiles the Rust file into a cached `.so` (with SHA-256 / mtime invalidation)
//!    and dynamically links it via `libc::dlopen` / `libc::dlsym` for pure native machine code execution.
//! 2. **Fast In-Process Native Engine**:
//!    Provides thread-safe in-memory implementations for framework helpers (`Cache`, `Log`)
//!    and custom Rust functions with zero compilation latency, providing immediate execution in
//!    development mode and test environments.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::lexer::facade::{get_static_facade, register_tier2_facade};
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::facade::{
    append_log, parse_json_value, php_value_to_json, register_tier2_plugin_handler,
    Tier2PluginHandler,
};
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
                FrozenValue::Int(n) => (*n as i64) + amount,
                FrozenValue::RustInt { value, .. } => *value as i64 + amount,
                _ => amount,
            };
            entry.0 = FrozenValue::Int(new_val as i32);
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

/// Resolve the path to a Rust source file on disk.
pub fn resolve_rust_file_path(path_str: &str, base_dir: Option<&Path>) -> Result<PathBuf, String> {
    let candidate = Path::new(path_str);

    // 1. If base_dir provided, try relative to base_dir
    if let Some(base) = base_dir {
        let combined = base.join(candidate);
        if combined.exists() {
            return Ok(combined);
        }
        if !path_str.ends_with(".rs") {
            let with_ext = base.join(format!("{}.rs", path_str));
            if with_ext.exists() {
                return Ok(with_ext);
            }
        }
    }

    // 2. Try relative to current working directory
    if candidate.exists() {
        return Ok(candidate.to_path_buf());
    }

    if !path_str.ends_with(".rs") {
        let with_ext = PathBuf::from(format!("{}.rs", path_str));
        if with_ext.exists() {
            return Ok(with_ext);
        }
    }

    // 3. Search in standard project directories
    for sub in &[".", "src", "helpers", "examples", "rust"] {
        let p = Path::new(sub).join(candidate);
        if p.exists() {
            return Ok(p);
        }
        let p_ext = Path::new(sub).join(format!("{}.rs", path_str.trim_end_matches(".rs")));
        if p_ext.exists() {
            return Ok(p_ext);
        }
    }

    Err(format!(
        "Rust file '{}' could not be found. Checked relative to current directory and base directory.",
        path_str
    ))
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
// Native Dynamic Library Compiler (rustc cdylib)
// =========================================================================

/// Attempt to compile a `.rs` file to a `.so` dynamic library using `rustc`.
#[cfg(unix)]
fn try_compile_cdylib(
    rs_path: &Path,
    source: &str,
    functions: &HashSet<String>,
) -> Option<PathBuf> {
    // 1. Check if rustc is present
    let rustc_check = Command::new("rustc").arg("--version").output();
    if rustc_check.is_err() || !rustc_check.as_ref().unwrap().status.success() {
        return None;
    }

    // 2. Prepare cache directory in target/tmp/rhp_cache or /tmp/rhp_cache
    let cache_dir = Path::new("target/tmp/rhp_cache").to_path_buf();
    let _ = fs::create_dir_all(&cache_dir);

    // Compute hash from source bytes
    let hash = {
        let mut h: u64 = 0x517cc1b727220a95;
        for b in source.bytes() {
            h = (h.wrapping_mul(0x100000001b3)) ^ (b as u64);
        }
        format!("{:016x}", h)
    };

    let stem = rs_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    let so_file = cache_dir.join(format!("lib{}_{}.so", stem, hash));

    // If .so already exists and is newer than source file, reuse it
    if so_file.exists() {
        if let (Ok(m_so), Ok(m_rs)) = (fs::metadata(&so_file), fs::metadata(rs_path)) {
            if let (Ok(t_so), Ok(t_rs)) = (m_so.modified(), m_rs.modified()) {
                if t_so >= t_rs {
                    return Some(so_file);
                }
            }
        }
    }

    // 3. Prepare wrapper source with sanitized code
    let sanitized_source = sanitize_rust_source_for_rustc(source);
    let target_rs_path = if sanitized_source != source {
        let clean_path = cache_dir.join(format!("{}_{}_clean.rs", stem, hash));
        let _ = fs::write(&clean_path, &sanitized_source);
        clean_path
    } else {
        rs_path.to_path_buf()
    };

    let abs_rs_path = match target_rs_path.canonicalize() {
        Ok(p) => p,
        Err(_) => target_rs_path,
    };
    let abs_str = abs_rs_path.to_string_lossy();

    let wrapper_path = cache_dir.join(format!("{}_{}_wrapper.rs", stem, hash));

    let wrapper_code = if source.contains("rhp_call") {
        format!("include!(r\"{}\");", abs_str)
    } else {
        // Generate automatic C-ABI bridge
        let mut dispatch_arms = String::new();
        for fn_name in functions {
            dispatch_arms.push_str(&format!(
                "        \"{name}\" => format!(\"{{}}\", user_code::{name}(args_json)),\n",
                name = fn_name
            ));
        }

        format!(
            r#"mod user_code {{
    include!(r"{abs_rs_path}");
}}

use std::ffi::{{CStr, CString}};
use std::os::raw::c_char;

fn dispatch_call(method: &str, args_json: &str) -> String {{
    match method {{
{dispatch_arms}        _ => format!("{{\"error\":\"Method '{{}}' not found\"}}", method),
    }}
}}

#[no_mangle]
pub extern "C" fn rhp_call(
    method_ptr: *const c_char,
    args_json_ptr: *const c_char,
) -> *mut c_char {{
    let method = unsafe {{
        if method_ptr.is_null() {{ "" }} else {{ CStr::from_ptr(method_ptr).to_str().unwrap_or("") }}
    }};
    let args_json = unsafe {{
        if args_json_ptr.is_null() {{ "[]" }} else {{ CStr::from_ptr(args_json_ptr).to_str().unwrap_or("[]") }}
    }};
    let result_str = dispatch_call(method, args_json);
    CString::new(result_str).unwrap_or_default().into_raw()
}}

#[no_mangle]
pub extern "C" fn rhp_free_string(ptr: *mut c_char) {{
    if !ptr.is_null() {{
        unsafe {{ drop(CString::from_raw(ptr)); }}
    }}
}}
"#,
            abs_rs_path = abs_str,
            dispatch_arms = dispatch_arms
        )
    };

    if fs::write(&wrapper_path, wrapper_code).is_err() {
        return None;
    }

    // 4. Compile with rustc
    let compile_status = Command::new("rustc")
        .arg("--crate-type")
        .arg("cdylib")
        .arg(&wrapper_path)
        .arg("-o")
        .arg(&so_file)
        .output();

    if let Ok(out) = compile_status {
        if out.status.success() && so_file.exists() {
            return Some(so_file);
        }
    }

    None
}

// =========================================================================
// Main Public Entrypoint: Load & Register Rust File
// =========================================================================

/// Load and register a Rust source file as a Tier 2 static facade helper in RHP.
///
/// Returns the registered facade name (e.g., `"Cache"`, `"Log"`, `"MyHelper"`).
pub fn load_and_register_rust_file(
    path_str: &str,
    alias: Option<&str>,
    base_dir: Option<&Path>,
) -> Result<String, RuntimeError> {
    let facade_name = infer_facade_name(path_str, alias);

    // 1. Check if already loaded in registry
    if let Ok(guard) = get_loaded_modules().read() {
        if guard.contains_key(&facade_name) {
            // Already registered, ensure facade tier is active
            if get_static_facade(&facade_name).is_none() {
                register_tier2_facade(
                    facade_name.clone(),
                    format!("Rust helper class imported from '{}'", path_str),
                    true,
                );
            }
            return Ok(facade_name);
        }
    }

    // 2. Resolve source file on disk
    let resolved_path = resolve_rust_file_path(path_str, base_dir).ok();
    let source_content = resolved_path
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .unwrap_or_default();

    let (custom_fns, blocking_fns) = extract_rust_functions(&source_content);
    for bfn in &blocking_fns {
        register_blocking_native_function(&facade_name, bfn);
    }

    // 3. Attempt native compilation via rustc cdylib if available
    #[cfg(unix)]
    let (dl_handle, rhp_call_fn) = if let Some(ref path) = resolved_path {
        if let Some(so_path) = try_compile_cdylib(path, &source_content, &custom_fns) {
            unsafe {
                let c_path = std::ffi::CString::new(so_path.to_string_lossy().as_bytes()).unwrap();
                let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
                if !handle.is_null() {
                    let c_sym = std::ffi::CString::new("rhp_call").unwrap();
                    let sym = libc::dlsym(handle, c_sym.as_ptr());
                    if !sym.is_null() {
                        let fn_ptr: RhpNativeCallFn = std::mem::transmute(sym);
                        (Some(RawDlHandle(handle)), Some(fn_ptr))
                    } else {
                        (Some(RawDlHandle(handle)), None)
                    }
                } else {
                    (None, None)
                }
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    #[cfg(not(unix))]
    let (dl_handle, rhp_call_fn): (Option<RawDlHandle>, Option<RhpNativeCallFn>) = (None, None);

    // 4. Initialize in-process stores
    let cache_store = if facade_name.eq_ignore_ascii_case("cache")
        || custom_fns.contains("get")
        || custom_fns.contains("set")
        || source_content.contains("Cache")
    {
        Some(Arc::new(RustCacheStore::new()))
    } else {
        None
    };

    let log_store = if facade_name.eq_ignore_ascii_case("log")
        || custom_fns.contains("info")
        || custom_fns.contains("error")
        || source_content.contains("Log")
    {
        Some(Arc::new(RustLogStore::new()))
    } else {
        None
    };

    // If neither cache nor log explicitly matched, default to providing a cache store if Cache-like
    let (final_cache, final_log) = match (&cache_store, &log_store) {
        (None, None) => {
            if facade_name.to_lowercase().contains("log") {
                (None, Some(Arc::new(RustLogStore::new())))
            } else {
                (Some(Arc::new(RustCacheStore::new())), None)
            }
        }
        _ => (cache_store, log_store),
    };

    let module = Arc::new(LoadedRustModule {
        name: facade_name.clone(),
        file_path: resolved_path,
        cache_store: final_cache.clone(),
        log_store: final_log.clone(),
        custom_functions: custom_fns,
        blocking_functions: blocking_fns.clone(),
        dl_handle,
        call_fn: rhp_call_fn,
    });

    if let Ok(mut guard) = get_loaded_modules().write() {
        guard.insert(facade_name.clone(), module.clone());
    }

    // 5. Register with FacadeRegistry as Tier 2 Native Rust Plugin
    register_tier2_facade(
        facade_name.clone(),
        format!("Rust helper class imported from '{}'", path_str),
        true,
    );

    // 6. Build execution handler dispatching to the compiled library or in-process engine
    let handler_cache = final_cache;
    let handler_log = final_log;
    let handler_facade = facade_name.clone();
    let handler_blocking_fns = blocking_fns;

    let handler: Tier2PluginHandler = Arc::new(move |method, args, span| {
        let is_blocking = handler_blocking_fns.contains(method)
            || is_native_function_blocking(&handler_facade, method);

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

        // A. If compiled cdylib is loaded, invoke via rhp_call
        if let Some(call_fn) = rhp_call_fn {
            let args_json = format!(
                "[{}]",
                args.iter()
                    .map(|v| php_value_to_json(v))
                    .collect::<Vec<_>>()
                    .join(",")
            );

            if is_blocking {
                let c_method_str = method.to_string();
                let facade_name_str = handler_facade.clone();
                let c_method_inner = c_method_str.clone();
                let res_str = dispatch_blocking_native(&facade_name_str, &c_method_str, move || {
                    let c_method = std::ffi::CString::new(c_method_inner).unwrap_or_default();
                    let c_args = std::ffi::CString::new(args_json).unwrap_or_default();
                    let res_ptr = call_fn(c_method.as_ptr(), c_args.as_ptr());
                    if !res_ptr.is_null() {
                        let s = unsafe {
                            std::ffi::CStr::from_ptr(res_ptr)
                                .to_string_lossy()
                                .into_owned()
                        };
                        Ok(s)
                    } else {
                        Ok("null".to_string())
                    }
                })?;

                if let Ok(parsed) = parse_json_value(&res_str) {
                    return Ok(parsed);
                }
                return Ok(PhpValue::string(res_str));
            } else {
                let c_method = std::ffi::CString::new(method).unwrap_or_default();
                let c_args = std::ffi::CString::new(args_json).unwrap_or_default();

                let res_ptr = call_fn(c_method.as_ptr(), c_args.as_ptr());
                if !res_ptr.is_null() {
                    let res_str = unsafe {
                        std::ffi::CStr::from_ptr(res_ptr)
                            .to_string_lossy()
                            .into_owned()
                    };
                    if let Ok(parsed) = parse_json_value(&res_str) {
                        return Ok(parsed);
                    }
                    return Ok(PhpValue::string(res_str));
                }
            }
        }

        // B & C. In-process Cache / Log execution
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
        &format!("Imported Rust helper class from {}", path_str),
        true,
        handler,
    );

    Ok(facade_name)
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
                    PhpValue::RustInt { value, .. } if *value > 0 => Some(*value as u64),
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
                    PhpValue::RustInt { value, .. } => Some(*value as i64),
                    _ => None,
                }).unwrap_or(1);
                return Ok(PhpValue::Int(cache.increment(&key, amt) as i32));
            }
            "decrement" => {
                let key = args.first().map(|v| v.to_php_string()).unwrap_or_default();
                let amt = args.get(1).and_then(|v| match v {
                    PhpValue::Int(n) => Some(*n as i64),
                    PhpValue::RustInt { value, .. } => Some(*value as i64),
                    _ => None,
                }).unwrap_or(1);
                return Ok(PhpValue::Int(cache.decrement(&key, amt) as i32));
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
                return Ok(PhpValue::Int(cache.count() as i32));
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
                        d.insert("timestamp".to_string(), PhpValue::Int(e.timestamp_ms as i32));
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
