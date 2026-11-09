//! # Package Scaffolding Engine
//!
//! Generates project layouts for pure RHP packages, native Rust Tier 2 packages,
//! sandboxed Wasm packages, and hybrid packages.

use std::fs;
use std::path::Path;

use super::error::Result;
use super::manifest::{PackageSafety, PackageType};

pub struct ScaffoldOptions {
    pub name: String,
    pub package_type: PackageType,
    pub safety: PackageSafety,
    pub description: Option<String>,
}

/// Convert a package name (e.g. `vuenice/mongo-driver`) into PascalCase (`MongoDriver`) and snake_case (`mongo_driver`).
pub fn sanitize_names(pkg_name: &str) -> (String, String, String) {
    let clean = pkg_name.split('/').last().unwrap_or(pkg_name);
    let mut pascal = String::new();
    let mut snake = String::new();
    let mut capitalize = true;

    for ch in clean.chars() {
        if ch == '-' || ch == '_' || ch == '.' {
            capitalize = true;
            if !snake.is_empty() && !snake.ends_with('_') {
                snake.push('_');
            }
        } else if capitalize {
            pascal.push(ch.to_ascii_uppercase());
            snake.push(ch.to_ascii_lowercase());
            capitalize = false;
        } else {
            pascal.push(ch);
            snake.push(ch.to_ascii_lowercase());
        }
    }

    let namespace = format!("Rhp\\{}", pascal);
    (pascal, snake, namespace)
}

/// Scaffold a package in `target_dir`.
pub fn scaffold_package(target_dir: &Path, options: &ScaffoldOptions) -> Result<()> {
    if !target_dir.exists() {
        fs::create_dir_all(target_dir)?;
    }

    let (pascal_name, snake_name, namespace) = sanitize_names(&options.name);

    match options.package_type {
        PackageType::Rhp => scaffold_pure_rhp(target_dir, options, &pascal_name, &namespace)?,
        PackageType::NativeRust => {
            scaffold_native_rust(target_dir, options, &pascal_name, &snake_name)?
        }
        PackageType::Wasm => scaffold_wasm(target_dir, options, &pascal_name, &snake_name)?,
        PackageType::Hybrid => {
            scaffold_hybrid(target_dir, options, &pascal_name, &snake_name, &namespace)?
        }
    }

    Ok(())
}

fn scaffold_pure_rhp(
    dir: &Path,
    opts: &ScaffoldOptions,
    class_name: &str,
    namespace: &str,
) -> Result<()> {
    let src_dir = dir.join("src");
    let tests_dir = dir.join("tests");
    fs::create_dir_all(&src_dir)?;
    fs::create_dir_all(&tests_dir)?;

    let escaped_ns = namespace.replace('\\', "\\\\");
    let rhp_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
type = "rhp"
safety = "{safety}"
description = "{desc}"
license = "MIT"
rhp-version = ">=0.8.0"

[autoload]
psr-4 = {{ "{escaped_ns}\\\\" = "src/" }}

[exports]
facades = ["{class_name}"]
namespaces = ["{escaped_ns}"]

[dependencies]
"#,
        name = opts.name,
        safety = opts.safety.as_str(),
        desc = opts.description.as_deref().unwrap_or("A pure RHP package"),
        escaped_ns = escaped_ns,
        class_name = class_name
    );
    fs::write(dir.join("rhp.toml"), rhp_toml)?;

    let sample_class = format!(
        r#"<?php
namespace {ns};

use Str;

/**
 * High performance static facade for {class_name}.
 */
final class {class_name} {{
    private function __construct() {{}}

    public static function execute(string $input): string {{
        return Str::upper($input);
    }}
}}
"#,
        ns = namespace,
        class_name = class_name
    );
    fs::write(src_dir.join(format!("{}.rhp", class_name)), sample_class)?;

    let test_file = format!(
        r#"<?php
use {ns}\{class_name};

$res = {class_name}::execute("hello");
if ($res !== "HELLO") {{
    throw new \Exception("Test failed! Expected HELLO, got " . $res);
}}
echo "✅ {class_name} tests passed!\n";
"#,
        ns = namespace,
        class_name = class_name
    );
    fs::write(tests_dir.join(format!("{}Test.rhp", class_name)), test_file)?;

    fs::write(
        dir.join("README.md"),
        format!("# {}\n\nPure RHP package.\n", opts.name),
    )?;
    Ok(())
}

fn scaffold_native_rust(
    dir: &Path,
    opts: &ScaffoldOptions,
    class_name: &str,
    crate_name: &str,
) -> Result<()> {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir)?;

    let rhp_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
type = "native-rust"
safety = "{safety}"
description = "{desc}"
license = "MIT"
rhp-version = ">=0.8.0"

[exports]
facades = ["{class_name}"]

[native]
crate-name = "{crate_name}"
target-dir = "."
abi-version = 1
entry-symbol = "rhp_plugin_init"
thread-safe = true
execution-target = "native"
marshalling = "typed-c-abi"
facade = "{class_name}"
"#,
        name = opts.name,
        safety = opts.safety.as_str(),
        desc = opts.description.as_deref().unwrap_or("High performance native Rust driver for RHP"),
        class_name = class_name,
        crate_name = crate_name,
    );
    fs::write(dir.join("rhp.toml"), rhp_toml)?;

    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
serde_json = "1.0"
"#,
        crate_name = crate_name
    );
    fs::write(dir.join("Cargo.toml"), cargo_toml)?;

    // Native implementation utilizing Typed C-ABI & Opaque Handles
    let lib_rs = format!(
        r#"//! Native RHP Tier 2 Dynamic Plugin for {class_name}
//! Compiled as a `cdylib` and loaded dynamically via `libloading`.

use std::collections::HashMap;
use std::sync::{{Arc, RwLock, OnceLock}};
use std::ffi::{{CStr, CString}};
use std::os::raw::c_char;

/// Native client structure holding persistent connection pools and configuration.
pub struct {class_name}Client {{
    pub endpoint: String,
    pub connections: u32,
}}

static CLIENT_POOL: OnceLock<RwLock<HashMap<u64, Arc<{class_name}Client>>>> = OnceLock::new();
static NEXT_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn get_pool() -> &'static RwLock<HashMap<u64, Arc<{class_name}Client>>> {{
    CLIENT_POOL.get_or_init(|| RwLock::new(HashMap::new()))
}}

/// Fast C-ABI dispatch function.
#[no_mangle]
pub extern "C" fn rhp_call(method_ptr: *const c_char, args_json_ptr: *const c_char) -> *mut c_char {{
    let method = unsafe {{
        if method_ptr.is_null() {{ "" }} else {{ CStr::from_ptr(method_ptr).to_str().unwrap_or("") }}
    }};
    let args_json = unsafe {{
        if args_json_ptr.is_null() {{ "[]" }} else {{ CStr::from_ptr(args_json_ptr).to_str().unwrap_or("[]") }}
    }};

    let response = match method {{
        "connect" => {{
            let handle = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let client = Arc::new({class_name}Client {{
                endpoint: args_json.trim_matches('"').to_string(),
                connections: 16,
            }});
            if let Ok(mut pool) = get_pool().write() {{
                pool.insert(handle, client);
            }}
            format!("{{{{\"handle\": {{}}}}}}", handle)
        }}
        "ping" => "{{{{\"status\":\"pong\"}}}}".to_string(),
        _ => format!("{{{{\"error\":\"Unknown method '{{}}'\"}}}}", method),
    }};

    CString::new(response).unwrap_or_default().into_raw()
}}

#[no_mangle]
pub extern "C" fn rhp_free_string(ptr: *mut c_char) {{
    if !ptr.is_null() {{
        unsafe {{ drop(CString::from_raw(ptr)); }}
    }}
}}
"#,
        class_name = class_name
    );
    fs::write(src_dir.join("lib.rs"), lib_rs)?;

    let d_rhp = format!(
        r#"<?php
// Type signatures for {class_name} static facade
namespace {{
    /**
     * @immutable
     * @final
     */
    final class {class_name} {{
        private function __construct() {{}}

        public static function connect(string $endpoint): int {{}}
        public static function ping(): array {{}}
    }}
}}
"#,
        class_name = class_name
    );
    fs::write(dir.join(format!("{}.d.rhp", crate_name)), d_rhp)?;

    Ok(())
}

fn scaffold_wasm(
    dir: &Path,
    opts: &ScaffoldOptions,
    class_name: &str,
    crate_name: &str,
) -> Result<()> {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir)?;

    let rhp_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
type = "wasm"
safety = "{safety}"
description = "{desc}"
license = "MIT"
rhp-version = ">=0.8.0"

[exports]
facades = ["{class_name}"]

[native]
crate-name = "{crate_name}"
target-dir = "."
execution-target = "wasm"
marshalling = "msgpack"
facade = "{class_name}"
"#,
        name = opts.name,
        safety = opts.safety.as_str(),
        desc = opts.description.as_deref().unwrap_or("Sandboxed WebAssembly package for RHP"),
        class_name = class_name,
        crate_name = crate_name,
    );
    fs::write(dir.join("rhp.toml"), rhp_toml)?;

    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
"#,
        crate_name = crate_name
    );
    fs::write(dir.join("Cargo.toml"), cargo_toml)?;

    let lib_rs = format!(
        r#"//! Sandboxed WebAssembly Module for {class_name}
//! Compiled to wasm32-wasip1 and executed within wasmtime sandbox.

#[no_mangle]
pub extern "C" fn execute_wasm(input_val: i64) -> i64 {{
    input_val * 2
}}
"#,
        class_name = class_name
    );
    fs::write(src_dir.join("lib.rs"), lib_rs)?;

    Ok(())
}

fn scaffold_hybrid(
    dir: &Path,
    opts: &ScaffoldOptions,
    class_name: &str,
    crate_name: &str,
    namespace: &str,
) -> Result<()> {
    scaffold_native_rust(dir, opts, class_name, crate_name)?;

    let rhp_dir = dir.join("rhp");
    fs::create_dir_all(&rhp_dir)?;

    // Append hybrid autoloading to rhp.toml
    let mut manifest_content = fs::read_to_string(dir.join("rhp.toml"))?;
    manifest_content = manifest_content.replace("type = \"native-rust\"", "type = \"hybrid\"");
    let escaped_ns = namespace.replace('\\', "\\\\");
    manifest_content.push_str(&format!(
        "\n[autoload]\npsr-4 = {{ \"{}\\\\\" = \"rhp/\" }}\n",
        escaped_ns
    ));
    fs::write(dir.join("rhp.toml"), manifest_content)?;

    let facade_code = format!(
        r#"<?php
namespace {ns};

use {class_name} as NativeKernel;

/**
 * Fluent RHP facade wrapping high-performance native Rust kernel.
 */
class {class_name} {{
    protected int $handle;

    public function __construct(string $endpoint) {{
        $this->handle = NativeKernel::connect($endpoint);
    }}

    public function ping(): array {{
        return NativeKernel::ping();
    }}
}}
"#,
        ns = namespace,
        class_name = class_name
    );
    fs::write(rhp_dir.join(format!("{}.rhp", class_name)), facade_code)?;

    Ok(())
}
