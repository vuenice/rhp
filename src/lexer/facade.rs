//! Tiered Facade Architecture, Static Facade Registry & Runtime Macro System
//!
//! As specified in LANGUAGE_SPECIFICATION_AND_ARCHITECTURE.md and the
//! "Designing Numeric Types in RHP" architectural guide:
//!
//! 1. Tier 1 Native Facades (`Int`, `Float`, `Arr`, `Str`, `Future`, `HTTP`, `File`, `Session`, `Image`, `URL`, `DB`, `Log`, `Sys`)
//!    are stateless utility wrappers and factory builders pre-compiled inside the engine.
//! 2. They are static-only classes (`is_static_only: true`).
//! 3. Instantiation via `new` is strictly forbidden:
//!    `Fatal Error: Cannot instantiate static facade class '...'. Use static methods directly.`
//! 4. Inheritance via `extends` is strictly forbidden to prevent tight coupling to Rust internals
//!    (e.g. `Box<Vec<PhpValue>>`) and stateless design violations in Tokio workers:
//!    `Fatal Error: Cannot extend static facade class '...'. Use '...::macro()' to register custom methods dynamically.`
//! 5. Base datatypes (`int`, `float`, `String`) are lightweight primitives and are NOT instances of these facades.
//! 6. Custom methods can be registered dynamically at runtime via `Facade::macro('name', closure)`
//!    into a thread-safe global runtime macro registry with zero performance cost on core types.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Facade tier classification as defined in the architectural tiering strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FacadeTier {
    /// Tier 1: Built-in Native Facades pre-compiled inside the engine core.
    /// Strictly static-only. Instantiation via `new` and inheritance via `extends` are forbidden.
    Tier1Native,
    /// Tier 2: Custom Rust Plugins included as crates during an AOT build step.
    Tier2RustPlugin,
    /// Tier 3: Script-land Facades written in userland code using static methods.
    Tier3Script,
}

/// Metadata describing a system facade across Tier 1, Tier 2, and Tier 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadeInfo {
    pub name: Cow<'static, str>,
    pub tier: FacadeTier,
    pub is_static_only: bool,
    pub supports_macros: bool,
    pub description: Cow<'static, str>,
}

impl FacadeInfo {
    pub const fn native(name: &'static str, description: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            tier: FacadeTier::Tier1Native,
            is_static_only: true,
            supports_macros: true,
            description: Cow::Borrowed(description),
        }
    }

    pub fn new(
        name: impl Into<Cow<'static, str>>,
        tier: FacadeTier,
        is_static_only: bool,
        supports_macros: bool,
        description: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            name: name.into(),
            tier,
            is_static_only,
            supports_macros,
            description: description.into(),
        }
    }
}

/// All registered Tier 1 Native Facades pre-compiled inside the RHP engine.
pub static NATIVE_FACADES: &[FacadeInfo] = &[
    FacadeInfo::native(
        "Int",
        "Static facade helper for integer utilities and specialized Rust numeric types (i64, u32, etc.)",
    ),
    FacadeInfo::native(
        "Float",
        "Static facade helper for float operations, formatting, and high-precision types (f64, toDecimal)",
    ),
    FacadeInfo::native(
        "Arr",
        "Static collection factory (list, dict, set, hashMap) with fluent chaining and runtime macro support",
    ),
    FacadeInfo::native(
        "Str",
        "Static UTF-8 string transformation and inspection utilities (slug, contains, random) with runtime macro support",
    ),
    FacadeInfo::native(
        "Regex",
        "Static regular expression facade (isValid, testString, match, replace, quote)",
    ),
    FacadeInfo::native(
        "Future",
        "Dart-style non-blocking concurrency facade (run, wait, any, delayed, defer)",
    ),
    FacadeInfo::native(
        "HTTP",
        "Non-blocking async HTTP client facade (get, post, withHeaders)",
    ),
    FacadeInfo::native(
        "File",
        "High-level cooperative fiber file I/O facade (read, write, exists, delete)",
    ),
    FacadeInfo::native(
        "Session",
        "Session state management facade across requests (get, set, put, has, all, forget, clear)",
    ),
    FacadeInfo::native(
        "Cookie",
        "HTTP cookie management facade (get, set, put, has, all, forget, clear)",
    ),
    FacadeInfo::native(
        "Request",
        "HTTP request inspection, input parameters, and type formatting facade (get, all, only, has, set, format, merge, clear)",
    ),
    FacadeInfo::native(
        "Image",
        "Fluent image manipulation facade (make, resize, crop, save)",
    ),
    FacadeInfo::native(
        "Crypt",
        "Static cryptographic hashing, HMAC, OpenSSL encryption/decryption, and random bytes facade (hash, hashHmac, opensslEncrypt, opensslDecrypt, randomBytes, md5)",
    ),
    FacadeInfo::native(
        "Html",
        "Static HTML encoding, decoding, entity transformation, and tag stripping facade (specialChars, stripTags, entities, specialCharsDecode, entityDecode)",
    ),
    FacadeInfo::native(
        "Url",
        "URL parsing, query string construction, base64 encoding, and route inspection facade (parseUrl, httpBuildQuery, base64Encode, base64Decode, encode, decode)",
    ),
    FacadeInfo::native(
        "URL",
        "URL generation and route inspection facade (to, route, current, full, parseUrl, httpBuildQuery)",
    ),
    FacadeInfo::native(
        "DB",
        "Centralized database pool query facade (table, find, select, insert, update, delete)",
    ),
    FacadeInfo::native(
        "ADB",
        "Asynchronous database query and connection pool operations running on Rust Tokio (table, query, select, insert, update, delete, find, raw)",
    ),
    FacadeInfo::native(
        "AFile",
        "Asynchronous non-blocking file I/O operations running on Rust Tokio (read, write, exists, delete, append, size)",
    ),
    FacadeInfo::native(
        "AHTTP",
        "Asynchronous non-blocking HTTP client running on Rust Tokio (get, post, put, delete, patch, request, withHeaders)",
    ),
    FacadeInfo::native(
        "Log",
        "Non-blocking centralized logging facade (info, error, debug, warn, channel)",
    ),
    FacadeInfo::native(
        "Sys",
        "System environment, output, and debugging intrinsics facade (env, echo, print, dd, os, version, timestamp)",
    ),
    FacadeInfo::native(
        "Glob",
        "Static facade for disambiguating reserved keyword operations and backward-compatible custom helpers (getClass, match, customHelper)",
    ),
    FacadeInfo::native(
        "phpFun",
        "Backward-compatibility static facade bridging legacy PHP functions (e.g., phpFun::array_merge() -> Arr::merge())",
    ),
    FacadeInfo::native(
        "Fiber",
        "Local cooperative execution fiber facade (current, isFiber, yield, stats, context, all)",
    ),
    FacadeInfo::native(
        "Memory",
        "Memory ownership, deterministic RAII safety, and zero-GC telemetry facade (usage, peak, stats, isGcFree, scope, assertThreadSafe)",
    ),
    FacadeInfo::native(
        "Weak",
        "Weak reference facade for breaking circular reference graphs (Weak::to, Weak::get, Weak::isValid)",
    ),
    FacadeInfo::native(
        "Trace",
        "OpenTelemetry distributed tracing, metrics, and APM facade (startSpan, endSpan, span, currentSpan, setAttribute, addEvent, recordException, injectContext, extractContext, counter, histogram, exportOtlp, exportSentry, exportDatadog)",
    ),
    FacadeInfo::native(
        "Telemetry",
        "OpenTelemetry distributed tracing and metrics alias facade",
    ),
    FacadeInfo::native(
        "Reflect",
        "Static reflection and inspection facade (classExists, functionExists)",
    ),
];

/// Reserved global helper intrinsics that the Pratt parser statically rewrites
/// directly into `Sys::<name>(args)` in the AST (`Sys::env()`, `Sys::print()`, `Sys::echo()`, `Sys::println()`, `Sys::dd()`, `Sys::print_r()`, `Sys::dump()`).
pub static RESERVED_INTRINSICS: &[&str] = &["env", "print", "println", "echo", "dd", "print_r", "var_dump", "dump"];

/// Returns true if the given function name is a reserved global helper intrinsic (`env`, `print`, `dd`, `print_r`, `var_dump`, `dump`).
#[inline]
pub fn is_reserved_intrinsic(name: &str) -> bool {
    RESERVED_INTRINSICS.contains(&name)
}

/// Key-value mapping entry connecting a legacy procedural PHP function to its
/// target RHP facade, method, and RHP function call signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyPhpFunctionMapping {
    /// Legacy PHP procedural function name (e.g., `"array_merge"`).
    pub php_name: &'static str,
    /// Target RHP static facade name (e.g., `"Arr"`).
    pub rhp_facade: &'static str,
    /// Target RHP method name on the facade (e.g., `"merge"`).
    pub rhp_method: &'static str,
    /// Canonical RHP function call signature (e.g., `"Arr::merge(array ...$arrays): array"`).
    pub rhp_signature: &'static str,
}

/// Key-value registry of legacy PHP built-in functions mapped to their RHP facade call signatures.
///
/// When the Pratt parser encounters a legacy PHP function call like `array_merge($a, $b)`,
/// it rewrites the AST node into `phpFun::array_merge($a, $b)` which dispatches to `Arr::merge($a, $b)`.
pub static LEGACY_PHP_FUNCTIONS: &[LegacyPhpFunctionMapping] = &[
    // Array functions -> Arr::*
    LegacyPhpFunctionMapping {
        php_name: "array_merge",
        rhp_facade: "Arr",
        rhp_method: "merge",
        rhp_signature: "Arr::merge(array ...$arrays): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_map",
        rhp_facade: "Arr",
        rhp_method: "map",
        rhp_signature: "Arr::map(callable $callback, array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_filter",
        rhp_facade: "Arr",
        rhp_method: "filter",
        rhp_signature: "Arr::filter(array $array, ?callable $callback = null): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_reduce",
        rhp_facade: "Arr",
        rhp_method: "reduce",
        rhp_signature: "Arr::reduce(array $array, callable $callback, mixed $initial = null): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_keys",
        rhp_facade: "Arr",
        rhp_method: "keys",
        rhp_signature: "Arr::keys(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_values",
        rhp_facade: "Arr",
        rhp_method: "values",
        rhp_signature: "Arr::values(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_unique",
        rhp_facade: "Arr",
        rhp_method: "unique",
        rhp_signature: "Arr::unique(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_slice",
        rhp_facade: "Arr",
        rhp_method: "slice",
        rhp_signature: "Arr::slice(array $array, int $offset, ?int $length = null): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_push",
        rhp_facade: "Arr",
        rhp_method: "push",
        rhp_signature: "Arr::push(array $array, mixed ...$values): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_pop",
        rhp_facade: "Arr",
        rhp_method: "pop",
        rhp_signature: "Arr::pop(array $array): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_shift",
        rhp_facade: "Arr",
        rhp_method: "shift",
        rhp_signature: "Arr::shift(array $array): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_unshift",
        rhp_facade: "Arr",
        rhp_method: "unshift",
        rhp_signature: "Arr::unshift(array $array, mixed ...$values): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_reverse",
        rhp_facade: "Arr",
        rhp_method: "reverse",
        rhp_signature: "Arr::reverse(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_search",
        rhp_facade: "Arr",
        rhp_method: "search",
        rhp_signature: "Arr::search(mixed $needle, array $haystack, bool $strict = false): int|string|bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "in_array",
        rhp_facade: "Arr",
        rhp_method: "contains",
        rhp_signature: "Arr::contains(array $haystack, mixed $needle, bool $strict = false): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_key_exists",
        rhp_facade: "Arr",
        rhp_method: "hasKey",
        rhp_signature: "Arr::hasKey(array $array, int|string $key): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "count",
        rhp_facade: "Arr",
        rhp_method: "count",
        rhp_signature: "Arr::count(array $array): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "implode",
        rhp_facade: "Str",
        rhp_method: "join",
        rhp_signature: "Str::join(string $separator, array $array): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "explode",
        rhp_facade: "Str",
        rhp_method: "split",
        rhp_signature: "Str::split(string $separator, string $string, ?int $limit = null): array",
    },
    // String functions -> Str::*
    LegacyPhpFunctionMapping {
        php_name: "strlen",
        rhp_facade: "Str",
        rhp_method: "length",
        rhp_signature: "Str::length(string $string): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "str_contains",
        rhp_facade: "Str",
        rhp_method: "contains",
        rhp_signature: "Str::contains(string $haystack, string $needle): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "str_starts_with",
        rhp_facade: "Str",
        rhp_method: "startsWith",
        rhp_signature: "Str::startsWith(string $haystack, string $needle): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "str_ends_with",
        rhp_facade: "Str",
        rhp_method: "endsWith",
        rhp_signature: "Str::endsWith(string $haystack, string $needle): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "strtolower",
        rhp_facade: "Str",
        rhp_method: "lower",
        rhp_signature: "Str::lower(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "strtoupper",
        rhp_facade: "Str",
        rhp_method: "upper",
        rhp_signature: "Str::upper(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "trim",
        rhp_facade: "Str",
        rhp_method: "trim",
        rhp_signature: "Str::trim(string $string, string $characters = \" \\n\\r\\t\\v\\0\"): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "ltrim",
        rhp_facade: "Str",
        rhp_method: "ltrim",
        rhp_signature: "Str::ltrim(string $string, string $characters = \" \\n\\r\\t\\v\\0\"): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "rtrim",
        rhp_facade: "Str",
        rhp_method: "rtrim",
        rhp_signature: "Str::rtrim(string $string, string $characters = \" \\n\\r\\t\\v\\0\"): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "str_replace",
        rhp_facade: "Str",
        rhp_method: "replace",
        rhp_signature: "Str::replace(string|array $search, string|array $replace, string $subject): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "substr",
        rhp_facade: "Str",
        rhp_method: "substr",
        rhp_signature: "Str::substr(string $string, int $offset, ?int $length = null): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "strpos",
        rhp_facade: "Str",
        rhp_method: "pos",
        rhp_signature: "Str::pos(string $haystack, string $needle, int $offset = 0): int|bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "sprintf",
        rhp_facade: "Str",
        rhp_method: "format",
        rhp_signature: "Str::format(string $format, mixed ...$values): string",
    },
    // File functions -> File::*
    LegacyPhpFunctionMapping {
        php_name: "file_get_contents",
        rhp_facade: "File",
        rhp_method: "read",
        rhp_signature: "File::read(string $path): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "file_put_contents",
        rhp_facade: "File",
        rhp_method: "write",
        rhp_signature: "File::write(string $path, string $contents): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "file_exists",
        rhp_facade: "File",
        rhp_method: "exists",
        rhp_signature: "File::exists(string $path): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "unlink",
        rhp_facade: "File",
        rhp_method: "delete",
        rhp_signature: "File::delete(string $path): bool",
    },
    // System & Type functions -> Sys::* / Glob::*
    LegacyPhpFunctionMapping {
        php_name: "getenv",
        rhp_facade: "Sys",
        rhp_method: "env",
        rhp_signature: "Sys::env(string $key, mixed $default = null): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "var_dump",
        rhp_facade: "Sys",
        rhp_method: "dump",
        rhp_signature: "Sys::dump(mixed ...$vars): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "dump",
        rhp_facade: "Sys",
        rhp_method: "dump",
        rhp_signature: "Sys::dump(mixed ...$vars): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "print_r",
        rhp_facade: "Sys",
        rhp_method: "print_r",
        rhp_signature: "Sys::print_r(mixed $value, bool $return = false): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "get_class",
        rhp_facade: "Glob",
        rhp_method: "getClass",
        rhp_signature: "Glob::getClass(object $object): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "preg_match",
        rhp_facade: "Glob",
        rhp_method: "match",
        rhp_signature: "Glob::match(string $pattern, string $subject): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "json_encode",
        rhp_facade: "phpFun",
        rhp_method: "json_encode",
        rhp_signature: "phpFun::json_encode(mixed $value): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "json_decode",
        rhp_facade: "phpFun",
        rhp_method: "json_decode",
        rhp_signature: "phpFun::json_decode(string $json, bool $associative = true): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "round",
        rhp_facade: "Float",
        rhp_method: "round",
        rhp_signature: "Float::round(float $num, int $precision = 0): float",
    },
    LegacyPhpFunctionMapping {
        php_name: "floor",
        rhp_facade: "Float",
        rhp_method: "floor",
        rhp_signature: "Float::floor(float $num): float",
    },
    LegacyPhpFunctionMapping {
        php_name: "ceil",
        rhp_facade: "Float",
        rhp_method: "ceil",
        rhp_signature: "Float::ceil(float $num): float",
    },
    LegacyPhpFunctionMapping {
        php_name: "ciel",
        rhp_facade: "Float",
        rhp_method: "ciel",
        rhp_signature: "Float::ciel(float $num): float",
    },
    // Reflection functions -> Reflect::*
    LegacyPhpFunctionMapping {
        php_name: "class_exists",
        rhp_facade: "Reflect",
        rhp_method: "classExists",
        rhp_signature: "Reflect::classExists(string $class): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "function_exists",
        rhp_facade: "Reflect",
        rhp_method: "functionExists",
        rhp_signature: "Reflect::functionExists(string $function): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "collect",
        rhp_facade: "phpFun",
        rhp_method: "collect",
        rhp_signature: "phpFun::collect(mixed $items = null): object",
    },
    // Crypt functions -> Crypt::*
    LegacyPhpFunctionMapping {
        php_name: "md5",
        rhp_facade: "Crypt",
        rhp_method: "md5",
        rhp_signature: "Crypt::md5(string $string, bool $binary = false): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "sha1",
        rhp_facade: "Crypt",
        rhp_method: "sha1",
        rhp_signature: "Crypt::sha1(string $string, bool $binary = false): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "hash",
        rhp_facade: "Crypt",
        rhp_method: "hash",
        rhp_signature: "Crypt::hash(string $algo, string $data, bool $binary = false): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "hash_hmac",
        rhp_facade: "Crypt",
        rhp_method: "hashHmac",
        rhp_signature: "Crypt::hashHmac(string $algo, string $data, string $key, bool $binary = false): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "openssl_encrypt",
        rhp_facade: "Crypt",
        rhp_method: "opensslEncrypt",
        rhp_signature: "Crypt::opensslEncrypt(string $data, string $cipher_algo, string $passphrase, int $options = 0, string $iv = ''): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "openssl_decrypt",
        rhp_facade: "Crypt",
        rhp_method: "opensslDecrypt",
        rhp_signature: "Crypt::opensslDecrypt(string $data, string $cipher_algo, string $passphrase, int $options = 0, string $iv = ''): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "random_bytes",
        rhp_facade: "Crypt",
        rhp_method: "randomBytes",
        rhp_signature: "Crypt::randomBytes(int $length): string",
    },
    // HTML functions -> Html::*
    LegacyPhpFunctionMapping {
        php_name: "htmlspecialchars",
        rhp_facade: "Html",
        rhp_method: "specialChars",
        rhp_signature: "Html::specialChars(string $string, int $flags = 11, ?string $encoding = 'UTF-8', bool $double_encode = true): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "htmlspecialchars_decode",
        rhp_facade: "Html",
        rhp_method: "specialCharsDecode",
        rhp_signature: "Html::specialCharsDecode(string $string, int $flags = 11): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "strip_tags",
        rhp_facade: "Html",
        rhp_method: "stripTags",
        rhp_signature: "Html::stripTags(string $string, array|string|null $allowed_tags = null): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "htmlentities",
        rhp_facade: "Html",
        rhp_method: "entities",
        rhp_signature: "Html::entities(string $string, int $flags = 11, ?string $encoding = 'UTF-8', bool $double_encode = true): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "html_entity_decode",
        rhp_facade: "Html",
        rhp_method: "entityDecode",
        rhp_signature: "Html::entityDecode(string $string, int $flags = 11, ?string $encoding = 'UTF-8'): string",
    },
    // URL functions -> Url::*
    LegacyPhpFunctionMapping {
        php_name: "parse_url",
        rhp_facade: "Url",
        rhp_method: "parseUrl",
        rhp_signature: "Url::parseUrl(string $url, int $component = -1): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "http_build_query",
        rhp_facade: "Url",
        rhp_method: "httpBuildQuery",
        rhp_signature: "Url::httpBuildQuery(mixed $data, string $numeric_prefix = '', string $arg_separator = '&', int $encoding_type = 1): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "base64_encode",
        rhp_facade: "Url",
        rhp_method: "base64Encode",
        rhp_signature: "Url::base64Encode(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "base64_decode",
        rhp_facade: "Url",
        rhp_method: "base64Decode",
        rhp_signature: "Url::base64Decode(string $string, bool $strict = false): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "urlencode",
        rhp_facade: "Url",
        rhp_method: "encode",
        rhp_signature: "Url::encode(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "urldecode",
        rhp_facade: "Url",
        rhp_method: "decode",
        rhp_signature: "Url::decode(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "rawurlencode",
        rhp_facade: "Url",
        rhp_method: "rawUrlEncode",
        rhp_signature: "Url::rawUrlEncode(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "rawurldecode",
        rhp_facade: "Url",
        rhp_method: "rawUrlDecode",
        rhp_signature: "Url::rawUrlDecode(string $string): string",
    },
];

/// Returns true if the given function name is a recognized legacy PHP standard library function
/// that should be rewritten to `phpFun::<name>()` in the AST.
#[inline]
pub fn is_legacy_php_function(name: &str) -> bool {
    LEGACY_PHP_FUNCTIONS.iter().any(|f| f.php_name == name)
}

/// Retrieve the key-value mapping metadata (`LegacyPhpFunctionMapping`) for a legacy PHP function.
#[inline]
pub fn get_legacy_php_function(name: &str) -> Option<&'static LegacyPhpFunctionMapping> {
    LEGACY_PHP_FUNCTIONS.iter().find(|f| f.php_name == name)
}

/// Retrieve a HashMap of all legacy PHP function names mapped to their RHP function call signatures.
pub fn php_function_signature_map() -> HashMap<&'static str, &'static str> {
    LEGACY_PHP_FUNCTIONS
        .iter()
        .map(|entry| (entry.php_name, entry.rhp_signature))
        .collect()
}

/// Thread-safe registry for managing the Three-Tier Facade Architecture.
///
/// - **Tier 1 (Built-in Native Engine Facades):** `Str`, `Arr`, `Future`, `Log`, `DB`, `Int`, `Float`, etc.
/// - **Tier 2 (Custom Rust Plugins / Crates):** Included via AOT builds or dynamic crate registration.
/// - **Tier 3 (Script-land Static Facades):** Userland static classes declared with static methods.
#[derive(Debug, Default)]
pub struct FacadeRegistry {
    tier1: RwLock<HashMap<String, FacadeInfo>>,
    tier2: RwLock<HashMap<String, FacadeInfo>>,
    tier3: RwLock<HashMap<String, FacadeInfo>>,
}

impl FacadeRegistry {
    /// Create a new `FacadeRegistry` pre-populated with Tier 1 Native Facades.
    pub fn new() -> Self {
        let registry = Self {
            tier1: RwLock::new(HashMap::new()),
            tier2: RwLock::new(HashMap::new()),
            tier3: RwLock::new(HashMap::new()),
        };
        for facade in NATIVE_FACADES {
            registry.register_tier1(facade.clone());
        }
        registry
    }

    /// Access the process-wide global Three-Tier Facade Registry.
    pub fn global() -> &'static FacadeRegistry {
        static GLOBAL_REGISTRY: OnceLock<FacadeRegistry> = OnceLock::new();
        GLOBAL_REGISTRY.get_or_init(FacadeRegistry::new)
    }

    /// Register a Tier 1 native engine facade.
    pub fn register_tier1(&self, info: FacadeInfo) {
        if let Ok(mut g) = self.tier1.write() {
            g.insert(info.name.to_string(), info);
        }
    }

    /// Register a Tier 2 custom Rust plugin facade.
    pub fn register_tier2(&self, info: FacadeInfo) {
        if let Ok(mut g) = self.tier2.write() {
            g.insert(info.name.to_string(), info);
        }
    }

    /// Register a Tier 3 script-land static facade.
    pub fn register_tier3(&self, info: FacadeInfo) {
        if let Ok(mut g) = self.tier3.write() {
            g.insert(info.name.to_string(), info);
        }
    }

    /// Retrieve the metadata info for a static facade if registered across any tier.
    pub fn get_facade(&self, name: &str) -> Option<FacadeInfo> {
        if let Ok(g) = self.tier1.read() {
            if let Some(f) = g.get(name) {
                return Some(f.clone());
            }
        }
        if let Ok(g) = self.tier2.read() {
            if let Some(f) = g.get(name) {
                return Some(f.clone());
            }
        }
        if let Ok(g) = self.tier3.read() {
            if let Some(f) = g.get(name) {
                return Some(f.clone());
            }
        }
        None
    }

    /// Returns true if the specified class name is a registered static facade in any tier.
    pub fn is_static_facade(&self, name: &str) -> bool {
        self.get_facade(name).is_some()
    }

    /// Returns true if the specified class is marked static-only across any tier.
    pub fn is_static_only(&self, name: &str) -> bool {
        if let Some(f) = self.get_facade(name) {
            return f.is_static_only;
        }
        // Normalize primitive/canonical names (e.g. 'int' -> 'Int', 'float' -> 'Float', etc.)
        let normalized = match name.to_ascii_lowercase().as_str() {
            "int" => "Int",
            "float" => "Float",
            "str" | "string" => "Str",
            "arr" | "array" => "Arr",
            "future" => "Future",
            "log" => "Log",
            "db" => "DB",
            "adb" => "ADB",
            "file" => "File",
            "afile" => "AFile",
            "http" => "HTTP",
            "ahttp" => "AHTTP",
            "session" => "Session",
            "cookie" => "Cookie",
            "request" => "Request",
            "regex" => "Regex",
            "reflect" => "Reflect",
            "crypt" => "Crypt",
            "html" => "Html",
            "url" => "Url",
            _ => return false,
        };
        self.get_facade(normalized)
            .map(|f| f.is_static_only)
            .unwrap_or(false)
    }

    /// Retrieve all registered facades belonging to a specific tier.
    pub fn get_facades_by_tier(&self, tier: FacadeTier) -> Vec<FacadeInfo> {
        match tier {
            FacadeTier::Tier1Native => {
                self.tier1.read().map(|g| g.values().cloned().collect()).unwrap_or_default()
            }
            FacadeTier::Tier2RustPlugin => {
                self.tier2.read().map(|g| g.values().cloned().collect()).unwrap_or_default()
            }
            FacadeTier::Tier3Script => {
                self.tier3.read().map(|g| g.values().cloned().collect()).unwrap_or_default()
            }
        }
    }

    /// Clear dynamically registered Tier 2 and Tier 3 facades (used for test isolation).
    pub fn clear_dynamic(&self) {
        if let Ok(mut g) = self.tier2.write() {
            g.clear();
        }
        if let Ok(mut g) = self.tier3.write() {
            g.clear();
        }
    }
}

/// Returns true if the specified class name is a registered static facade in any tier.
#[inline]
pub fn is_static_facade(name: &str) -> bool {
    FacadeRegistry::global().is_static_facade(name)
}

/// Returns true if the facade is a Tier 1 (Native) or Tier 2 (Rust Plugin) facade
/// that dispatches directly to native handlers via OpCode::FacadeCall.
#[inline]
pub fn is_native_or_plugin_facade(name: &str) -> bool {
    if let Some(f) = FacadeRegistry::global().get_facade(name) {
        return matches!(f.tier, FacadeTier::Tier1Native | FacadeTier::Tier2RustPlugin);
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int" | "float" | "str" | "string" | "arr" | "array" | "future" | "log" | "db"
            | "file" | "http" | "sys" | "glob" | "session" | "cookie" | "request" | "image" | "url" | "phpfun"
            | "adb" | "afile" | "ahttp" | "crypt" | "html"
    )
}

/// Returns true if the specified class is marked static-only (`is_static_only: true`).
#[inline]
pub fn is_static_only(name: &str) -> bool {
    FacadeRegistry::global().is_static_only(name)
}

/// Retrieve the metadata info for a static facade if registered in any tier.
#[inline]
pub fn get_static_facade(name: &str) -> Option<FacadeInfo> {
    FacadeRegistry::global().get_facade(name)
}

/// Register a custom Tier 2 Rust plugin facade.
pub fn register_tier2_facade(
    name: impl Into<String>,
    description: impl Into<String>,
    supports_macros: bool,
) -> FacadeInfo {
    let info = FacadeInfo {
        name: Cow::Owned(name.into()),
        tier: FacadeTier::Tier2RustPlugin,
        is_static_only: true,
        supports_macros,
        description: Cow::Owned(description.into()),
    };
    FacadeRegistry::global().register_tier2(info.clone());
    info
}

/// Register a Tier 3 script-land static facade.
pub fn register_tier3_facade(
    name: impl Into<String>,
    description: impl Into<String>,
    supports_macros: bool,
) -> FacadeInfo {
    let info = FacadeInfo {
        name: Cow::Owned(name.into()),
        tier: FacadeTier::Tier3Script,
        is_static_only: true,
        supports_macros,
        description: Cow::Owned(description.into()),
    };
    FacadeRegistry::global().register_tier3(info.clone());
    info
}

/// Retrieve all facades registered for a given tier.
pub fn get_facades_by_tier(tier: FacadeTier) -> Vec<FacadeInfo> {
    FacadeRegistry::global().get_facades_by_tier(tier)
}

/// Checks whether a class can be instantiated with `new`.
///
/// Returns an error if the class is marked `is_static_only: true`,
/// matching the exact compiler diagnostic specified in the PDF:
/// `Fatal Error: Cannot instantiate static facade class '{name}'. Use static methods directly.`
pub fn validate_instantiation(name: &str) -> Result<(), String> {
    if is_static_only(name) {
        return Err(format!(
            "Fatal Error: Cannot instantiate static facade class '{}'. Use static methods directly.",
            name
        ));
    }
    Ok(())
}

/// Checks whether a class can be extended via `extends`.
///
/// Core helper classes like `Arr` and `Str` wrap optimized Rust heap allocations
/// (such as `Box<Vec<PhpValue>>` or UTF-8 string buffers) and are stateless utility facades.
/// Subclassing them is strictly forbidden; developers must use `Facade::macro()` instead.
pub fn validate_inheritance(name: &str) -> Result<(), String> {
    if is_static_only(name) {
        return Err(format!(
            "Fatal Error: Cannot extend static facade class '{}'. Use '{}::macro()' to register custom methods dynamically.",
            name, name
        ));
    }
    Ok(())
}

/// Metadata and definition for a dynamically registered facade macro (`Arr::macro('toCsv', ...)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroEntry {
    /// Target facade name (e.g., `"Arr"`, `"Str"`).
    pub facade: String,
    /// Registered custom method name (e.g., `"toCsv"`).
    pub method_name: String,
    /// Parameter names with optional default value expressions (e.g., `[("$delimiter", Some("','"))]`).
    pub params: Vec<(String, Option<String>)>,
    /// Source representation or body summary of the registered closure/callable.
    pub body_source: String,
}

impl MacroEntry {
    pub fn new(
        facade: impl Into<String>,
        method_name: impl Into<String>,
        params: Vec<(String, Option<String>)>,
        body_source: impl Into<String>,
    ) -> Self {
        Self {
            facade: facade.into(),
            method_name: method_name.into(),
            params,
            body_source: body_source.into(),
        }
    }
}

/// Thread-safe global runtime macro registry.
///
/// Macros are registered during the initial application bootstrap phase in the parent process
/// and stored in a global runtime registry rather than altering core Rust struct layouts,
/// making them safely accessible across all concurrent Tokio worker threads with zero overhead
/// on native operations.
#[derive(Debug, Default)]
pub struct MacroRegistry {
    macros: RwLock<HashMap<(String, String), MacroEntry>>,
}

impl MacroRegistry {
    /// Create a new empty `MacroRegistry`.
    pub fn new() -> Self {
        Self {
            macros: RwLock::new(HashMap::new()),
        }
    }

    /// Access the process-wide global runtime macro registry.
    pub fn global() -> &'static MacroRegistry {
        static GLOBAL_REGISTRY: OnceLock<MacroRegistry> = OnceLock::new();
        GLOBAL_REGISTRY.get_or_init(MacroRegistry::new)
    }

    /// Register a custom macro method on a static facade.
    pub fn register(&self, entry: MacroEntry) -> Result<(), String> {
        let facade_info = get_static_facade(&entry.facade).ok_or_else(|| {
            format!(
                "Fatal Error: Cannot register macro '{}' on unknown or non-facade class '{}'.",
                entry.method_name, entry.facade
            )
        })?;

        if !facade_info.supports_macros {
            return Err(format!(
                "Fatal Error: Facade '{}' does not support runtime macro registration.",
                entry.facade
            ));
        }

        if entry.method_name.trim().is_empty() {
            return Err(format!(
                "Fatal Error: Invalid macro registration on facade '{}': macro name cannot be empty.",
                entry.facade
            ));
        }

        let mut guard = self
            .macros
            .write()
            .map_err(|_| "MacroRegistry lock poisoned".to_string())?;
        guard.insert((entry.facade.clone(), entry.method_name.clone()), entry);
        Ok(())
    }

    /// Check if a macro is registered on the given facade.
    pub fn has(&self, facade: &str, method_name: &str) -> bool {
        self.macros
            .read()
            .map(|guard| guard.contains_key(&(facade.to_string(), method_name.to_string())))
            .unwrap_or(false)
    }

    /// Retrieve a registered macro entry by facade and method name.
    pub fn get(&self, facade: &str, method_name: &str) -> Option<MacroEntry> {
        self.macros
            .read()
            .ok()
            .and_then(|guard| guard.get(&(facade.to_string(), method_name.to_string())).cloned())
    }

    /// List all macro method names registered for a specific facade.
    pub fn list_for_facade(&self, facade: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .macros
            .read()
            .map(|guard| {
                guard
                    .keys()
                    .filter(|(f, _)| f == facade)
                    .map(|(_, m)| m.clone())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Remove all registered macros (used for test isolation and dev-mode hot reload).
    pub fn clear(&self) {
        if let Ok(mut guard) = self.macros.write() {
            guard.clear();
        }
    }
}

/// Register a custom macro in the global runtime macro registry.
pub fn register_macro(
    facade: &str,
    method_name: &str,
    params: Vec<(String, Option<String>)>,
    body_source: &str,
) -> Result<(), String> {
    MacroRegistry::global().register(MacroEntry::new(facade, method_name, params, body_source))
}

/// Check if a macro is registered on a facade in the global runtime macro registry.
#[inline]
pub fn has_macro(facade: &str, method_name: &str) -> bool {
    MacroRegistry::global().has(facade, method_name)
}

/// Retrieve a macro definition from the global runtime macro registry.
#[inline]
pub fn get_macro(facade: &str, method_name: &str) -> Option<MacroEntry> {
    MacroRegistry::global().get(facade, method_name)
}

/// Register a backward-compatible global custom helper function on the `Glob` facade.
pub fn register_custom_helper(
    helper_name: &str,
    params: Vec<(String, Option<String>)>,
    body_source: &str,
) -> Result<(), String> {
    register_macro("Glob", helper_name, params, body_source)
}

/// Check if a custom global helper is registered on the `Glob` facade.
#[inline]
pub fn has_custom_helper(helper_name: &str) -> bool {
    has_macro("Glob", helper_name)
}

/// Retrieve a registered custom global helper definition from the `Glob` facade.
#[inline]
pub fn get_custom_helper(helper_name: &str) -> Option<MacroEntry> {
    get_macro("Glob", helper_name)
}

