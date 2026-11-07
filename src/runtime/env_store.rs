//! # Vite-Style Environment Store & Configuration Loader
//!
//! Loads `.env` configuration files matching Vite's resolution hierarchy:
//! 1. `env` / `.env`
//! 2. `.env.local`
//! 3. `.env.[mode]` (e.g. `.env.development` or `.env.production`)
//! 4. `.env.[mode].local`
//!
//! Searches at:
//! - The directory containing the entry script (`index.rhp`, `app.rhp`, etc.)
//! - The working directory where `dev` or `build` occurred (`cwd`)
//!
//! Provides `Sys::env()` access to loaded variables with no in-memory hot reloading:
//! when an env file changes in Development Mode (`dev`), the process automatically
//! restarts via `execvp` or supervisor process restart, mirroring Vite's behavior.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

/// Internal state of loaded environment variables.
#[derive(Debug, Clone, Default)]
pub struct EnvStore {
    /// Merged key-value pairs loaded from `.env*` files.
    vars: HashMap<String, String>,
    /// Mode used during resolution (e.g. "development", "production").
    mode: String,
    /// Absolute paths of files that were actually read and loaded.
    loaded_files: Vec<PathBuf>,
    /// All candidate file paths monitored by the development file watcher.
    watched_paths: Vec<PathBuf>,
}

impl EnvStore {
    pub fn new(mode: &str) -> Self {
        Self {
            vars: HashMap::new(),
            mode: mode.to_string(),
            loaded_files: Vec::new(),
            watched_paths: Vec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.vars.get(key)
    }

    pub fn all(&self) -> BTreeMap<String, String> {
        self.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }

    pub fn loaded_files(&self) -> &[PathBuf] {
        &self.loaded_files
    }

    pub fn watched_paths(&self) -> &[PathBuf] {
        &self.watched_paths
    }
}

/// Global singleton environment store.
static GLOBAL_ENV_STORE: OnceLock<RwLock<EnvStore>> = OnceLock::new();
static WATCHER_RUNNING: AtomicBool = AtomicBool::new(false);

fn get_global_store() -> &'static RwLock<EnvStore> {
    GLOBAL_ENV_STORE.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        RwLock::new(load_env(None, &cwd, "development"))
    })
}

/// Parse the contents of an env file into a key-value map.
///
/// Implements standard dotenv and Vite semantics:
/// - Strips optional `export ` prefix
/// - Ignores comments (`#`, `//`, `;`)
/// - Handles single quotes `'...'` (raw string)
/// - Handles double quotes `"..."` (escaped `\n`, `\r`, `\t`, `\\`, `\"`)
/// - Strips unquoted inline comments preceded by whitespace (`#`)
/// - Supports multiline quoted values
/// - Expands variable references (`${VAR}` or `$VAR`) against parsed keys and OS env
pub fn parse_env_content(content: &str) -> HashMap<String, String> {
    let mut raw_pairs = Vec::new();
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with(';')
        {
            continue;
        }

        let clean_line = if let Some(stripped) = trimmed.strip_prefix("export ") {
            stripped.trim()
        } else {
            trimmed
        };

        if let Some((raw_key, raw_val)) = clean_line.split_once('=') {
            let key = raw_key.trim().to_string();
            if key.is_empty() {
                continue;
            }

            let mut val_str = raw_val.trim().to_string();

            // Check if value starts with a quote but does not close on the same line (multiline)
            if (val_str.starts_with('"') && !is_closed_quote(&val_str, '"'))
                || (val_str.starts_with('\'') && !is_closed_quote(&val_str, '\''))
            {
                let quote_char = val_str.chars().next().unwrap();
                while let Some(next_line) = lines.next() {
                    val_str.push('\n');
                    val_str.push_str(next_line);
                    if is_closed_quote(&val_str, quote_char) {
                        break;
                    }
                }
            }

            let parsed_val = parse_env_value(&val_str);
            raw_pairs.push((key, parsed_val));
        }
    }

    // Second pass: variable expansion (${VAR} or $VAR)
    let mut result = HashMap::new();
    for (key, val) in raw_pairs {
        let expanded = expand_variables(&val, &result);
        result.insert(key, expanded);
    }

    result
}

/// Helper to check if a quoted string is properly terminated with matching closing quote.
fn is_closed_quote(s: &str, quote: char) -> bool {
    let trimmed = s.trim_end();
    if trimmed.len() < 2 || !trimmed.starts_with(quote) || !trimmed.ends_with(quote) {
        return false;
    }
    // Check that closing quote is not escaped
    let without_last = &trimmed[..trimmed.len() - 1];
    let mut backslash_count = 0;
    for c in without_last.chars().rev() {
        if c == '\\' {
            backslash_count += 1;
        } else {
            break;
        }
    }
    backslash_count % 2 == 0
}

/// Parse a single env value string (unquoting and unescaping if needed).
fn parse_env_value(val: &str) -> String {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Double-quoted string
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return unescape_double_quoted(inner);
    }

    // Single-quoted string
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return inner.to_string();
    }

    // Unquoted string: strip inline comments preceded by whitespace (` # ...`)
    let mut unquoted = trimmed;
    if let Some(idx) = unquoted.find(" #") {
        unquoted = unquoted[..idx].trim_end();
    } else if let Some(idx) = unquoted.find("\t#") {
        unquoted = unquoted[..idx].trim_end();
    }

    unquoted.to_string()
}

/// Expand escape sequences in double quoted strings.
fn unescape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('$') => out.push('$'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

/// Expand `${VAR}` or `$VAR` references from current map or OS environment variables.
fn expand_variables(val: &str, current_map: &HashMap<String, String>) -> String {
    if !val.contains('$') {
        return val.to_string();
    }

    let mut out = String::with_capacity(val.len());
    let chars: Vec<char> = val.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '\\' && i + 1 < len && chars[i + 1] == '$' {
            out.push('$');
            i += 2;
            continue;
        }

        if chars[i] == '$' {
            i += 1;
            if i < len && chars[i] == '{' {
                // Bracketed expansion: ${VAR_NAME} or ${VAR_NAME:-default}
                i += 1;
                let start = i;
                while i < len && chars[i] != '}' {
                    i += 1;
                }
                let var_expr: String = chars[start..i].iter().collect();
                if i < len && chars[i] == '}' {
                    i += 1;
                }

                let (var_name, default_val) = if let Some((name, def)) = var_expr.split_once(":-") {
                    (name.trim(), Some(def.trim()))
                } else if let Some((name, def)) = var_expr.split_once(':') {
                    (name.trim(), Some(def.trim()))
                } else {
                    (var_expr.trim(), None)
                };

                let resolved = current_map
                    .get(var_name)
                    .cloned()
                    .or_else(|| std::env::var(var_name).ok())
                    .or_else(|| default_val.map(|s| s.to_string()))
                    .unwrap_or_default();

                out.push_str(&resolved);
            } else {
                // Unbracketed expansion: $VAR_NAME
                let start = i;
                while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                if start == i {
                    out.push('$');
                } else {
                    let var_name: String = chars[start..i].iter().collect();
                    let resolved = current_map
                        .get(&var_name)
                        .cloned()
                        .or_else(|| std::env::var(&var_name).ok())
                        .unwrap_or_default();
                    out.push_str(&resolved);
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

/// Compute list of candidate env files for a given directory in order of Vite priority.
fn candidate_env_files(dir: &Path, mode: &str) -> Vec<PathBuf> {
    vec![
        dir.join("env"),
        dir.join(".env"),
        dir.join(".env.local"),
        dir.join(format!(".env.{}", mode)),
        dir.join(format!(".env.{}.local", mode)),
    ]
}

/// Load environment files matching Vite hierarchy.
///
/// Checks both the directory containing the entry script (`index.rhp`, `app.rhp`)
/// and the current working directory where `dev` or `build` occurred.
pub fn load_env(entry_script: Option<&Path>, cwd: &Path, mode: &str) -> EnvStore {
    let mut store = EnvStore::new(mode);
    let mut watched_paths = Vec::new();

    // Determine target script directory
    let script_dir = entry_script.and_then(|p| {
        if p.is_dir() {
            Some(p.to_path_buf())
        } else {
            p.parent().map(|parent| {
                if parent.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                }
            })
        }
    });

    let mut directories = Vec::new();

    // If script_dir is specified and distinct from cwd, add cwd first, then script_dir
    let cwd_canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    directories.push(cwd.to_path_buf());

    if let Some(ref s_dir) = script_dir {
        let s_dir_canonical = s_dir.canonicalize().unwrap_or_else(|_| s_dir.to_path_buf());
        if s_dir_canonical != cwd_canonical {
            directories.push(s_dir.clone());
        }
    }

    // Load candidate files for each directory in sequence
    for dir in directories {
        let candidates = candidate_env_files(&dir, mode);
        for candidate in candidates {
            if !watched_paths.contains(&candidate) {
                watched_paths.push(candidate.clone());
            }

            if candidate.is_file() {
                if let Ok(content) = fs::read_to_string(&candidate) {
                    let parsed = parse_env_content(&content);
                    for (k, v) in parsed {
                        store.vars.insert(k, v);
                    }
                    if !store.loaded_files.contains(&candidate) {
                        store.loaded_files.push(candidate);
                    }
                }
            }
        }
    }

    store.watched_paths = watched_paths;
    store
}

/// Initialize the global environment store.
pub fn init_env(entry_script: Option<&Path>, cwd: &Path, mode: &str) {
    let store = load_env(entry_script, cwd, mode);
    let lock = GLOBAL_ENV_STORE.get_or_init(|| RwLock::new(store.clone()));
    if let Ok(mut guard) = lock.write() {
        *guard = store;
    }
}

/// Explicitly set variables in test environments without reading from disk.
pub fn set_env_for_test(vars: &[(&str, &str)]) {
    let lock = GLOBAL_ENV_STORE.get_or_init(|| RwLock::new(EnvStore::new("test")));
    if let Ok(mut guard) = lock.write() {
        for (k, v) in vars {
            guard.vars.insert(k.to_string(), v.to_string());
        }
    }
}

/// Reset the global env store (primarily for unit tests).
pub fn reset_env_for_test() {
    let lock = GLOBAL_ENV_STORE.get_or_init(|| RwLock::new(EnvStore::new("test")));
    if let Ok(mut guard) = lock.write() {
        guard.vars.clear();
        guard.loaded_files.clear();
        guard.watched_paths.clear();
    }
}

/// Retrieve a value from the loaded env file store.
pub fn get_env(key: &str) -> Option<String> {
    let store = get_global_store();
    if let Ok(guard) = store.read() {
        guard.get(key).cloned()
    } else {
        None
    }
}

/// Retrieve all variables loaded from the env file store as a sorted dictionary.
pub fn get_all_env() -> BTreeMap<String, String> {
    let store = get_global_store();
    if let Ok(guard) = store.read() {
        guard.all()
    } else {
        BTreeMap::new()
    }
}

/// Snapshot representation of a file's state on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    exists: bool,
    modified: Option<SystemTime>,
    len: u64,
}

impl FileSnapshot {
    fn capture(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(meta) => Self {
                exists: true,
                modified: meta.modified().ok(),
                len: meta.len(),
            },
            Err(_) => Self {
                exists: false,
                modified: None,
                len: 0,
            },
        }
    }
}

/// Spawns a background thread that monitors all watched `.env*` candidate files.
///
/// When any monitored env file is modified, created, or deleted in Development Mode,
/// the process automatically restarts (replacing the process image via `execvp`).
///
/// Note: There is NO in-memory hot reloading of `.env` values.
pub fn start_env_watcher() {
    if WATCHER_RUNNING.swap(true, Ordering::SeqCst) {
        // Watcher already running
        return;
    }

    let watched_paths = {
        let store = get_global_store();
        if let Ok(guard) = store.read() {
            guard.watched_paths.clone()
        } else {
            Vec::new()
        }
    };

    if watched_paths.is_empty() {
        return;
    }

    std::thread::Builder::new()
        .name("rhp-env-watcher".to_string())
        .spawn(move || {
            let mut snapshots: HashMap<PathBuf, FileSnapshot> = HashMap::new();
            for path in &watched_paths {
                snapshots.insert(path.clone(), FileSnapshot::capture(path));
            }

            loop {
                std::thread::sleep(Duration::from_millis(250));

                let mut changed = false;
                let mut changed_path: Option<PathBuf> = None;

                for path in &watched_paths {
                    let current = FileSnapshot::capture(path);
                    if let Some(prev) = snapshots.get(path) {
                        if prev != &current {
                            changed = true;
                            changed_path = Some(path.clone());
                            break;
                        }
                    } else if current.exists {
                        changed = true;
                        changed_path = Some(path.clone());
                        break;
                    }
                }

                if changed {
                    // Debounce 75ms for editor atomic rename/write operations
                    std::thread::sleep(Duration::from_millis(75));

                    if let Some(ref p) = changed_path {
                        let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or(".env");
                        eprintln!("\n🔄 [rhp] {} changed, restarting process (no hot reloading)...", file_name);
                    } else {
                        eprintln!("\n🔄 [rhp] env file changed, restarting process (no hot reloading)...");
                    }

                    restart_process();
                }
            }
        })
        .expect("Failed to spawn rhp-env-watcher thread");
}

/// Restart the current process image matching Vite's server restart behavior.
pub fn restart_process() -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rhp"));
        let args: Vec<String> = std::env::args().skip(1).collect();
        let err = std::process::Command::new(current_exe)
            .args(&args)
            .exec();
        eprintln!("Failed to restart process: {}", err);
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rhp"));
        let args: Vec<String> = std::env::args().skip(1).collect();
        let _ = std::process::Command::new(current_exe)
            .args(&args)
            .spawn();
        std::process::exit(0);
    }
}
