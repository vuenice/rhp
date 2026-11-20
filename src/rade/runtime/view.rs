use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::rade::compiler::codegen::CodeGenerator;
use crate::rade::compiler::registry::DirectiveRegistry;
use crate::rade::compiler::{compile_to_rhp, CompileError, CompilerOptions};

/// Computes the standard SHA-1 digest of binary data, returning a 40-character lowercase hexadecimal string.
pub fn sha1(data: &[u8]) -> String {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let ml = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    format!("{:08x}{:08x}{:08x}{:08x}{:08x}", h0, h1, h2, h3, h4)
}

/// Computes the SHA-1 digest of a string slice.
pub fn sha1_str(s: &str) -> String {
    sha1(s.as_bytes())
}

/// Generates a cache key for a template file path based on its SHA-1 hash.
/// Uses the canonicalized absolute path when available for consistency across relative/absolute lookups.
pub fn cache_key_for_path(path: &Path) -> String {
    let s = path
        .canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    sha1(s.as_bytes())
}

/// Environment mode for the view engine and view cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// In development mode, check if source template mtime > cached file mtime; recompile only when changed.
    #[default]
    Development,

    /// In production mode, load cached compiled .rhp directly without checking template mtime.
    Production,
}

impl ViewMode {
    /// Returns true if development mode is active.
    pub fn is_development(&self) -> bool {
        matches!(self, ViewMode::Development)
    }

    /// Returns true if production mode is active.
    pub fn is_production(&self) -> bool {
        matches!(self, ViewMode::Production)
    }
}

/// Errors that can occur during view resolution, caching, or rendering.
#[derive(Debug)]
pub enum ViewError {
    /// Underlying filesystem I/O error
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Template compilation error
    Compile {
        path: PathBuf,
        source: CompileError,
    },

    /// View template could not be resolved from any candidate location
    ViewNotFound {
        view: String,
        searched_paths: Vec<PathBuf>,
    },

    /// Cached compiled file was missing in production mode
    CacheNotFound {
        cached_path: PathBuf,
        source_path: PathBuf,
    },
}

impl fmt::Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewError::Io { path, source } => {
                write!(f, "I/O error at '{}': {}", path.display(), source)
            }
            ViewError::Compile { path, source } => {
                write!(f, "Compilation error in '{}': {}", path.display(), source)
            }
            ViewError::ViewNotFound {
                view,
                searched_paths,
            } => {
                write!(
                    f,
                    "View '{}' not found. Searched candidate paths: {:?}",
                    view, searched_paths
                )
            }
            ViewError::CacheNotFound {
                cached_path,
                source_path,
            } => {
                write!(
                    f,
                    "Production view cache file not found at '{}' for source template '{}'",
                    cached_path.display(),
                    source_path.display()
                )
            }
        }
    }
}

impl std::error::Error for ViewError {}

/// Default views directory discovery (checks `resources/views` then `views`).
pub fn default_views_dir() -> PathBuf {
    if Path::new("resources/views").is_dir() {
        PathBuf::from("resources/views")
    } else if Path::new("views").is_dir() {
        PathBuf::from("views")
    } else {
        PathBuf::from("views")
    }
}

/// Default views cache directory discovery (checks `storage/framework/views` then `.rhp/cache/views`).
pub fn default_storage_dir() -> PathBuf {
    if Path::new("storage/framework/views").is_dir() {
        PathBuf::from("storage/framework/views")
    } else if Path::new("storage").is_dir() {
        PathBuf::from("storage/framework/views")
    } else {
        PathBuf::from(".rhp/cache/views")
    }
}

/// Recursively traverses a directory to find all template files ending with `.rade.rhp`, `.rade`, or `.blade.rhp`.
fn find_blade_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            find_blade_files(&path, out)?;
        } else {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if filename.ends_with(".rade.rhp") || filename.ends_with(".rade") || filename.ends_with(".blade.rhp") {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Recursively traverses a directory to find all `.rhp` files.
fn find_rhp_cache_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            find_rhp_cache_files(&path, out)?;
        } else if let Some(ext) = path.extension() {
            if ext == "rhp" {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Recursively compiles all `.rade.rhp` files in `views_dir` into hashed cache files in `storage_dir` (e.g. `<sha1_path>.rhp`).
pub fn cache_views(
    views_dir: impl AsRef<Path>,
    storage_dir: impl AsRef<Path>,
) -> Result<usize, ViewError> {
    let views_dir = views_dir.as_ref();
    let storage_dir = storage_dir.as_ref();

    if !views_dir.exists() {
        return Err(ViewError::Io {
            path: views_dir.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Views directory not found: {}", views_dir.display()),
            ),
        });
    }

    fs::create_dir_all(storage_dir).map_err(|e| ViewError::Io {
        path: storage_dir.to_path_buf(),
        source: e,
    })?;

    let mut files = Vec::new();
    find_blade_files(views_dir, &mut files).map_err(|e| ViewError::Io {
        path: views_dir.to_path_buf(),
        source: e,
    })?;

    files.sort();

    let mut count = 0;
    for file in files {
        let content = fs::read_to_string(&file).map_err(|e| ViewError::Io {
            path: file.clone(),
            source: e,
        })?;

        let compiled = compile_to_rhp(&content).map_err(|e| ViewError::Compile {
            path: file.clone(),
            source: e,
        })?;

        let key = cache_key_for_path(&file);
        let cached_path = storage_dir.join(format!("{key}.rhp"));

        fs::write(&cached_path, compiled).map_err(|e| ViewError::Io {
            path: cached_path,
            source: e,
        })?;

        count += 1;
    }

    Ok(count)
}

/// Clears all cached `.rhp` view files from `storage_dir`.
pub fn clear_cache(storage_dir: impl AsRef<Path>) -> Result<usize, ViewError> {
    let storage_dir = storage_dir.as_ref();
    if !storage_dir.exists() {
        return Ok(0);
    }

    let mut files = Vec::new();
    find_rhp_cache_files(storage_dir, &mut files).map_err(|e| ViewError::Io {
        path: storage_dir.to_path_buf(),
        source: e,
    })?;

    let mut count = 0;
    for file in files {
        fs::remove_file(&file).map_err(|e| ViewError::Io {
            path: file,
            source: e,
        })?;
        count += 1;
    }

    Ok(count)
}

/// View cache manager implementing development mtime check and production direct loading.
#[derive(Debug, Clone)]
pub struct ViewCache {
    storage_dir: PathBuf,
    mode: ViewMode,
    options: CompilerOptions,
    registry: DirectiveRegistry,
}

impl ViewCache {
    /// Creates a new `ViewCache` with the target cache storage directory and mode.
    pub fn new(storage_dir: impl Into<PathBuf>, mode: ViewMode) -> Self {
        Self {
            storage_dir: storage_dir.into(),
            mode,
            options: CompilerOptions::standalone(),
            registry: DirectiveRegistry::new(),
        }
    }

    /// Creates a new `ViewCache` in development mode.
    pub fn dev(storage_dir: impl Into<PathBuf>) -> Self {
        Self::new(storage_dir, ViewMode::Development)
    }

    /// Creates a new `ViewCache` in production mode.
    pub fn prod(storage_dir: impl Into<PathBuf>) -> Self {
        Self::new(storage_dir, ViewMode::Production)
    }

    /// Sets the compiler options for this view cache.
    pub fn with_options(mut self, options: CompilerOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets the directive registry for this view cache.
    pub fn with_registry(mut self, registry: DirectiveRegistry) -> Self {
        self.registry = registry;
        self
    }

    /// Registers a custom directive on this view cache.
    pub fn register_directive<F>(&mut self, name: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Option<&str>) -> String + Send + Sync + 'static,
    {
        self.registry.register(name, handler);
        self
    }

    /// Returns the cache storage directory path.
    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
    }

    /// Returns the active view mode.
    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// Sets the active view mode.
    pub fn set_mode(&mut self, mode: ViewMode) {
        self.mode = mode;
    }

    /// Returns a reference to the compiler options.
    pub fn options(&self) -> &CompilerOptions {
        &self.options
    }

    /// Returns a reference to the directive registry.
    pub fn registry(&self) -> &DirectiveRegistry {
        &self.registry
    }

    /// Computes the cached file destination path for a given source template file.
    pub fn get_cache_path(&self, source_path: &Path) -> PathBuf {
        let key = cache_key_for_path(source_path);
        self.storage_dir.join(format!("{key}.rhp"))
    }

    /// Checks if a compiled cache file is expired or missing.
    /// In development mode: returns true if cache does not exist OR source template `mtime` > cached file `mtime`.
    pub fn is_expired(&self, source_path: &Path) -> Result<bool, ViewError> {
        let cached_path = self.get_cache_path(source_path);
        if !cached_path.exists() {
            return Ok(true);
        }

        let source_meta = fs::metadata(source_path).map_err(|e| ViewError::Io {
            path: source_path.to_path_buf(),
            source: e,
        })?;

        let cached_meta = fs::metadata(&cached_path).map_err(|e| ViewError::Io {
            path: cached_path.clone(),
            source: e,
        })?;

        let source_mtime = source_meta.modified().map_err(|e| ViewError::Io {
            path: source_path.to_path_buf(),
            source: e,
        })?;

        let cached_mtime = cached_meta.modified().map_err(|e| ViewError::Io {
            path: cached_path,
            source: e,
        })?;

        Ok(source_mtime > cached_mtime)
    }

    /// Compiles a single template file and writes it to its hashed cache file.
    pub fn compile(&self, source_path: &Path) -> Result<PathBuf, ViewError> {
        fs::create_dir_all(&self.storage_dir).map_err(|e| ViewError::Io {
            path: self.storage_dir.clone(),
            source: e,
        })?;

        let content = fs::read_to_string(source_path).map_err(|e| ViewError::Io {
            path: source_path.to_path_buf(),
            source: e,
        })?;

        let compiled = CodeGenerator::new(self.options)
            .compile(&content, &self.registry)
            .map_err(|e| ViewError::Compile {
                path: source_path.to_path_buf(),
                source: e,
            })?;

        let cached_path = self.get_cache_path(source_path);
        fs::write(&cached_path, compiled).map_err(|e| ViewError::Io {
            path: cached_path.clone(),
            source: e,
        })?;

        Ok(cached_path)
    }

    /// Loads the compiled RHP code for a template file.
    /// - In development mode: checks if source template `mtime` > cached file `mtime`; recompiles only when changed.
    /// - In production mode: loads cached compiled `.rhp` directly without checking template `mtime`.
    pub fn load(&self, source_path: &Path) -> Result<String, ViewError> {
        let cached_path = self.get_cache_path(source_path);

        match self.mode {
            ViewMode::Development => {
                if self.is_expired(source_path)? {
                    self.compile(source_path)?;
                }
                fs::read_to_string(&cached_path).map_err(|e| ViewError::Io {
                    path: cached_path,
                    source: e,
                })
            }
            ViewMode::Production => {
                if !cached_path.exists() {
                    return Err(ViewError::CacheNotFound {
                        cached_path,
                        source_path: source_path.to_path_buf(),
                    });
                }
                fs::read_to_string(&cached_path).map_err(|e| ViewError::Io {
                    path: cached_path,
                    source: e,
                })
            }
        }
    }

    /// Recursively compiles all `.rade.rhp` files in `views_dir` into this cache.
    pub fn cache_all(&self, views_dir: &Path) -> Result<usize, ViewError> {
        cache_views(views_dir, &self.storage_dir)
    }

    /// Clears all cached `.rhp` files from this cache.
    pub fn clear(&self) -> Result<usize, ViewError> {
        clear_cache(&self.storage_dir)
    }
}

/// High-level view engine managing template lookup, compilation, caching, and rendering.
#[derive(Debug, Clone)]
pub struct ViewEngine {
    views_dir: PathBuf,
    cache: ViewCache,
}

impl ViewEngine {
    /// Creates a new `ViewEngine` with views directory, storage directory, and mode.
    pub fn new(
        views_dir: impl Into<PathBuf>,
        storage_dir: impl Into<PathBuf>,
        mode: ViewMode,
    ) -> Self {
        Self {
            views_dir: views_dir.into(),
            cache: ViewCache::new(storage_dir, mode),
        }
    }

    /// Creates a new `ViewEngine` in development mode.
    pub fn dev(views_dir: impl Into<PathBuf>, storage_dir: impl Into<PathBuf>) -> Self {
        Self::new(views_dir, storage_dir, ViewMode::Development)
    }

    /// Creates a new `ViewEngine` in production mode.
    pub fn prod(views_dir: impl Into<PathBuf>, storage_dir: impl Into<PathBuf>) -> Self {
        Self::new(views_dir, storage_dir, ViewMode::Production)
    }

    /// Configures compiler options on the engine's cache.
    pub fn with_options(mut self, options: CompilerOptions) -> Self {
        self.cache = self.cache.with_options(options);
        self
    }

    /// Configures a directive registry on the engine's cache.
    pub fn with_registry(mut self, registry: DirectiveRegistry) -> Self {
        self.cache = self.cache.with_registry(registry);
        self
    }

    /// Registers a custom directive on this view engine.
    pub fn register_directive<F>(&mut self, name: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Option<&str>) -> String + Send + Sync + 'static,
    {
        self.cache.register_directive(name, handler);
        self
    }

    /// Returns the configured views directory.
    pub fn views_dir(&self) -> &Path {
        &self.views_dir
    }

    /// Returns a reference to the underlying `ViewCache`.
    pub fn cache(&self) -> &ViewCache {
        &self.cache
    }

    /// Returns a mutable reference to the underlying `ViewCache`.
    pub fn cache_mut(&mut self) -> &mut ViewCache {
        &mut self.cache
    }

    /// Returns the active view mode.
    pub fn mode(&self) -> ViewMode {
        self.cache.mode()
    }

    /// Sets the active view mode.
    pub fn set_mode(&mut self, mode: ViewMode) {
        self.cache.set_mode(mode);
    }

    /// Resolves a template view name or path to a filesystem path.
    /// Supports dot notation (e.g. "auth.login" -> "views_dir/auth/login.rade.rhp")
    /// as well as direct relative and absolute filenames, with `.rade.rhp`, `.rade`, `.blade.rhp`, and `.rhp` extensions.
    pub fn resolve_view_path(&self, view: &str) -> Result<PathBuf, ViewError> {
        let mut candidates = Vec::new();
        let path_with_dots = view.replace('.', "/");

        let mut add_candidates = |base: &Path| {
            candidates.push(base.join(view));
            candidates.push(base.join(format!("{path_with_dots}.rade.rhp")));
            candidates.push(base.join(format!("{path_with_dots}.rade")));
            candidates.push(base.join(format!("{path_with_dots}.blade.rhp")));
            candidates.push(base.join(format!("{path_with_dots}.rhp")));
            if !view.ends_with(".rade.rhp") && !view.ends_with(".rade") && !view.ends_with(".blade.rhp") && !view.ends_with(".rhp") {
                candidates.push(base.join(format!("{view}.rade.rhp")));
                candidates.push(base.join(format!("{view}.rade")));
                candidates.push(base.join(format!("{view}.blade.rhp")));
                candidates.push(base.join(format!("{view}.rhp")));
            }
        };

        // 1. Primary configured views_dir
        add_candidates(&self.views_dir);

        // 2. Common fallback template directories if different from views_dir
        let res_views = Path::new("resources/views");
        if self.views_dir != res_views && res_views.is_dir() {
            add_candidates(res_views);
        }
        let views_dir = Path::new("views");
        if self.views_dir != views_dir && views_dir.is_dir() {
            add_candidates(views_dir);
        }

        // 3. Direct filesystem path outside views_dir
        candidates.push(PathBuf::from(view));
        if !view.ends_with(".rade.rhp") && !view.ends_with(".rade") && !view.ends_with(".blade.rhp") && !view.ends_with(".rhp") {
            candidates.push(PathBuf::from(format!("{view}.rade.rhp")));
            candidates.push(PathBuf::from(format!("{view}.rade")));
            candidates.push(PathBuf::from(format!("{path_with_dots}.rade.rhp")));
            candidates.push(PathBuf::from(format!("{path_with_dots}.rade")));
            candidates.push(PathBuf::from(format!("{view}.blade.rhp")));
            candidates.push(PathBuf::from(format!("{path_with_dots}.blade.rhp")));
        }

        for cand in &candidates {
            if cand.is_file() {
                return Ok(cand.clone());
            }
        }

        Err(ViewError::ViewNotFound {
            view: view.to_string(),
            searched_paths: candidates,
        })
    }

    /// Returns the hashed cache path for a template file.
    pub fn get_cache_path(&self, source_path: &Path) -> PathBuf {
        self.cache.get_cache_path(source_path)
    }

    /// Checks if a template's cache is expired.
    pub fn is_expired(&self, source_path: &Path) -> Result<bool, ViewError> {
        self.cache.is_expired(source_path)
    }

    /// Compiles a resolved view name into its cache file.
    pub fn compile_view(&self, view: &str) -> Result<PathBuf, ViewError> {
        let path = self.resolve_view_path(view)?;
        self.cache.compile(&path)
    }

    /// Compiles a specific template file path into its cache file.
    pub fn compile_file(&self, source_path: &Path) -> Result<PathBuf, ViewError> {
        self.cache.compile(source_path)
    }

    /// Loads the compiled RHP code for a view by name (e.g. "home", "admin.users").
    pub fn load(&self, view: &str) -> Result<String, ViewError> {
        let path = self.resolve_view_path(view)?;
        self.cache.load(&path)
    }

    /// Loads the compiled RHP code for a specific template file path.
    pub fn load_file(&self, source_path: &Path) -> Result<String, ViewError> {
        self.cache.load(source_path)
    }

    /// Recursively compiles all `.rade.rhp` files in `views_dir` into this engine's cache.
    pub fn cache_all(&self) -> Result<usize, ViewError> {
        self.cache.cache_all(&self.views_dir)
    }

    /// Clears all cached view files.
    pub fn clear(&self) -> Result<usize, ViewError> {
        self.cache.clear()
    }
}

/// Convenience alias for `ViewEngine`.
pub type ViewManager = ViewEngine;

/// Convenience alias for `ViewEngine`.
pub type View = ViewEngine;
