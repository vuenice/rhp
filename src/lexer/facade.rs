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
        "Bool",
        "Static facade helper for boolean operations and loose truthiness comparisons (compare)",
    ),
    FacadeInfo::native(
        "Int",
        "Static facade helper for integer utilities and specialized Rust numeric types (i64, u32, etc.)",
    ),
    FacadeInfo::native(
        "Float",
        "Static facade helper for float operations, formatting, and high-precision types (f64, toDecimal)",
    ),
    FacadeInfo::native(
        "App",
        "Static application IoC service container facade (bind, shared, singleton, instance, swap, make, has, resolve)",
    ),
    FacadeInfo::native(
        "Arr",
        "Static collection factory (list, dict, set, hashMap) with fluent chaining and runtime macro support",
    ),
    FacadeInfo::native(
        "Array",
        "Canonical static facade alias for Arr collections (compareJson, compareRef, compareJoined, compareUnordered)",
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
        "Response",
        "HTTP response builder and factory facade (json, html, make, redirect, redirectTo, status, header, noContent, download)",
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
        "System environment, output, and debugging intrinsics facade (env, osEnv, echo, print, dd, os, version, timestamp, clearstatcache, disk_free_space, disk_total_space)",
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
    FacadeInfo::native(
        "Time",
        "Static date and time manipulation facade inspired by Laravel Carbon (now, parse, today, tomorrow, yesterday, create, toDateTimeString, toDateString, addDay, subDay, lastOfMonth, diffForHumans)",
    ),
    FacadeInfo::native(
        "Carbon",
        "Date and time manipulation alias facade for Time",
    ),
    FacadeInfo::native(
        "Duration",
        "Static interval helper with CarbonInterval behavior (hours, fromString, cascade, forHumans, spec, total, add, sub, times)",
    ),
    FacadeInfo::native(
        "CarbonInterval",
        "Interval alias facade for Duration",
    ),
    FacadeInfo::native(
        "Ip",
        "Static IPv4 address conversion and utility facade (toLong, fromLong)",
    ),
    FacadeInfo::native(
        "Compress",
        "Static data compression and decompression facade (zlib, unzlib, gzip, gunzip)",
    ),
    FacadeInfo::native(
        "Validate",
        "Static type inspection and validation facade (null, int, integer, bool, boolean, float, numeric, scalar)",
    ),
    FacadeInfo::native(
        "Csv",
        "Static CSV stream processing, parsing, generation, and file handling facade (fgetcsv, fputcsv, parse, generate, open, close, memory)",
    ),
    FacadeInfo::native(
        "Uniq",
        "Static unique identifier facade (uniqid, uuid4, uuid5, uuid7, uuid3, isValid, version, parse, nil, max)",
    ),
    FacadeInfo::native(
        "Debug",
        "Runtime diagnostics, value inspection, and type reflection facade (type, dump, dd, format)",
    ),
    FacadeInfo::native(
        "Path",
        "Filesystem path operations, extension inspection, and glob pattern matching facade (basename, dirname, extname, parse, isAbsolute, glob, tempDir)",
    ),
    FacadeInfo::native(
        "Socket",
        "Stream socket networking and TCP listener facade (server, getName, close)",
    ),
    FacadeInfo::native(
        "Binary",
        "Binary data manipulation, hex encoding/decoding, and cryptographic byte generation facade (bin2hex, hex2bin, randomBytes, toHex, fromHex)",
    ),
    FacadeInfo::native(
        "WebSocket",
        "High-performance RFC 6455 real-time WebSocket facade (isUpgrade, send, broadcast, clients, channels, subscribe, unsubscribe, close, acceptKey)",
    ),
    FacadeInfo::native(
        "Ws",
        "Canonical alias for WebSocket facade",
    ),
];

/// Reserved global helper intrinsics that the Pratt parser statically rewrites
/// directly into `Sys::<name>(args)` in the AST (`Sys::env()`, `Sys::print()`, `Sys::echo()`, `Sys::println()`, `Sys::dd()`, `Sys::print_r()`, `Sys::dump()`, `Sys::isset()`, `Sys::empty()`, `Sys::exit()`).
pub static RESERVED_INTRINSICS: &[&str] = &[
    "env", "os_env", "osEnv", "print", "println", "echo", "dd", "print_r", "var_dump", "dump", "isset", "empty",
    "exit", "die", "register_shutdown_function",
    "set_error_handler", "restore_error_handler", "set_exception_handler", "restore_exception_handler",
    "trigger_error",
];

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
        php_name: "usort",
        rhp_facade: "Arr",
        rhp_method: "usort",
        rhp_signature: "Arr::usort(array $array, callable $callback): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "uasort",
        rhp_facade: "Arr",
        rhp_method: "uasort",
        rhp_signature: "Arr::uasort(array $array, callable $callback): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "uksort",
        rhp_facade: "Arr",
        rhp_method: "uksort",
        rhp_signature: "Arr::uksort(array $array, callable $callback): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "ksort",
        rhp_facade: "Arr",
        rhp_method: "ksort",
        rhp_signature: "Arr::ksort(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "asort",
        rhp_facade: "Arr",
        rhp_method: "asort",
        rhp_signature: "Arr::asort(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "natsort",
        rhp_facade: "Arr",
        rhp_method: "natsort",
        rhp_signature: "Arr::natsort(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "natcasesort",
        rhp_facade: "Arr",
        rhp_method: "natcasesort",
        rhp_signature: "Arr::natcasesort(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_diff",
        rhp_facade: "Arr",
        rhp_method: "diff",
        rhp_signature: "Arr::diff(array $array, array ...$arrays): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_diff_key",
        rhp_facade: "Arr",
        rhp_method: "diffKey",
        rhp_signature: "Arr::diffKey(array $array, array ...$arrays): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_intersect",
        rhp_facade: "Arr",
        rhp_method: "intersect",
        rhp_signature: "Arr::intersect(array $array, array ...$arrays): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_intersect_key",
        rhp_facade: "Arr",
        rhp_method: "intersectKey",
        rhp_signature: "Arr::intersectKey(array $array, array ...$arrays): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_column",
        rhp_facade: "Arr",
        rhp_method: "column",
        rhp_signature: "Arr::column(array $array, int|string|null $column_key, int|string|null $index_key = null): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_chunk",
        rhp_facade: "Arr",
        rhp_method: "chunk",
        rhp_signature: "Arr::chunk(array $array, int $length, bool $preserve_keys = false): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_flip",
        rhp_facade: "Arr",
        rhp_method: "flip",
        rhp_signature: "Arr::flip(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_combine",
        rhp_facade: "Arr",
        rhp_method: "combine",
        rhp_signature: "Arr::combine(array $keys, array $values): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_count_values",
        rhp_facade: "Arr",
        rhp_method: "countValues",
        rhp_signature: "Arr::countValues(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "shuffle",
        rhp_facade: "Arr",
        rhp_method: "shuffle",
        rhp_signature: "Arr::shuffle(array $array): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_rand",
        rhp_facade: "Arr",
        rhp_method: "randomKey",
        rhp_signature: "Arr::randomKey(array $array, int $num = 1): int|string|array",
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
        php_name: "ctype_digit",
        rhp_facade: "Str",
        rhp_method: "digit",
        rhp_signature: "Str::digit(string $string): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "ctype_alpha",
        rhp_facade: "Str",
        rhp_method: "alpha",
        rhp_signature: "Str::alpha(string $string): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "ctype_alnum",
        rhp_facade: "Str",
        rhp_method: "alnum",
        rhp_signature: "Str::alnum(string $string): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "strlen",
        rhp_facade: "Str",
        rhp_method: "length",
        rhp_signature: "Str::length(string $string): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "strnatcmp",
        rhp_facade: "Str",
        rhp_method: "natCompare",
        rhp_signature: "Str::natCompare(string $string1, string $string2): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "strnatcasecmp",
        rhp_facade: "Str",
        rhp_method: "natCaseCompare",
        rhp_signature: "Str::natCaseCompare(string $string1, string $string2): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "strcmp",
        rhp_facade: "Str",
        rhp_method: "strcmp",
        rhp_signature: "Str::strcmp(string $string1, string $string2): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "strcasecmp",
        rhp_facade: "Str",
        rhp_method: "strcasecmp",
        rhp_signature: "Str::strcasecmp(string $string1, string $string2): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "strncmp",
        rhp_facade: "Str",
        rhp_method: "strncmp",
        rhp_signature: "Str::strncmp(string $string1, string $string2, int $length): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "strncasecmp",
        rhp_facade: "Str",
        rhp_method: "strncasecmp",
        rhp_signature: "Str::strncasecmp(string $string1, string $string2, int $length): int",
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
        rhp_method: "lowerCase",
        rhp_signature: "Str::lowerCase(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "strtoupper",
        rhp_facade: "Str",
        rhp_method: "upperCase",
        rhp_signature: "Str::upperCase(string $string): string",
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
        php_name: "strrchr",
        rhp_facade: "Str",
        rhp_method: "fromLast",
        rhp_signature: "Str::fromLast(string $string, string $needle): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "strpos",
        rhp_facade: "Str",
        rhp_method: "find",
        rhp_signature: "Str::find(string $haystack, string $needle, int $offset = 0): ?int",
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
    LegacyPhpFunctionMapping {
        php_name: "copy",
        rhp_facade: "File",
        rhp_method: "copy",
        rhp_signature: "File::copy(string $from, string $to): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "rename",
        rhp_facade: "File",
        rhp_method: "rename",
        rhp_signature: "File::rename(string $from, string $to): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "mkdir",
        rhp_facade: "File",
        rhp_method: "mkdir",
        rhp_signature: "File::mkdir(string $path, string $mode = \"0777\", bool $recursive = false): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "rmdir",
        rhp_facade: "File",
        rhp_method: "rmdir",
        rhp_signature: "File::rmdir(string $path): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "chmod",
        rhp_facade: "File",
        rhp_method: "chmod",
        rhp_signature: "File::chmod(string $path, ?string $mode = null): mixed",
    },
    LegacyPhpFunctionMapping {
        php_name: "parse_mode",
        rhp_facade: "File",
        rhp_method: "parseMode",
        rhp_signature: "File::parseMode(string $mode): int|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "chown",
        rhp_facade: "File",
        rhp_method: "chown",
        rhp_signature: "File::chown(string $path, string|int $user): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "touch",
        rhp_facade: "File",
        rhp_method: "touch",
        rhp_signature: "File::touch(string $path, ?int $time = null, ?int $atime = null): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "mime_content_type",
        rhp_facade: "File",
        rhp_method: "mimeType",
        rhp_signature: "File::mimeType(string $path): string|bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "filesize",
        rhp_facade: "File",
        rhp_method: "size",
        rhp_signature: "File::size(string $path): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_file",
        rhp_facade: "File",
        rhp_method: "isFile",
        rhp_signature: "File::isFile(string $path): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_dir",
        rhp_facade: "File",
        rhp_method: "isDirectory",
        rhp_signature: "File::isDirectory(string $path): bool",
    },
    // System & Type functions -> Sys::* / Glob::*
    LegacyPhpFunctionMapping {
        php_name: "clearstatcache",
        rhp_facade: "Sys",
        rhp_method: "clearstatcache",
        rhp_signature: "Sys::clearstatcache(bool $clear_realpath_cache = false, string $filename = \"\"): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "disk_free_space",
        rhp_facade: "Sys",
        rhp_method: "disk_free_space",
        rhp_signature: "Sys::disk_free_space(string $directory): float|bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "diskfreespace",
        rhp_facade: "Sys",
        rhp_method: "disk_free_space",
        rhp_signature: "Sys::disk_free_space(string $directory): float|bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "disk_total_space",
        rhp_facade: "Sys",
        rhp_method: "disk_total_space",
        rhp_signature: "Sys::disk_total_space(string $directory): float|bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "getenv",
        rhp_facade: "Sys",
        rhp_method: "osEnv",
        rhp_signature: "Sys::osEnv(string $key, mixed $default = null): mixed",
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
        php_name: "isset",
        rhp_facade: "Sys",
        rhp_method: "isset",
        rhp_signature: "Sys::isset(mixed ...$vars): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "empty",
        rhp_facade: "Sys",
        rhp_method: "empty",
        rhp_signature: "Sys::empty(mixed $val): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "exit",
        rhp_facade: "Sys",
        rhp_method: "exit",
        rhp_signature: "Sys::exit(mixed $status = 0): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "die",
        rhp_facade: "Sys",
        rhp_method: "die",
        rhp_signature: "Sys::die(mixed $status = 0): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "register_shutdown_function",
        rhp_facade: "Sys",
        rhp_method: "registerShutdownFunction",
        rhp_signature: "Sys::registerShutdownFunction(callable $callback, mixed ...$args): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "set_error_handler",
        rhp_facade: "Sys",
        rhp_method: "setErrorHandler",
        rhp_signature: "Sys::setErrorHandler(?callable $callback, int $error_levels = E_ALL): ?callable",
    },
    LegacyPhpFunctionMapping {
        php_name: "restore_error_handler",
        rhp_facade: "Sys",
        rhp_method: "restoreErrorHandler",
        rhp_signature: "Sys::restoreErrorHandler(): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "set_exception_handler",
        rhp_facade: "Sys",
        rhp_method: "setExceptionHandler",
        rhp_signature: "Sys::setExceptionHandler(?callable $callback): ?callable",
    },
    LegacyPhpFunctionMapping {
        php_name: "restore_exception_handler",
        rhp_facade: "Sys",
        rhp_method: "restoreExceptionHandler",
        rhp_signature: "Sys::restoreExceptionHandler(): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "trigger_error",
        rhp_facade: "Sys",
        rhp_method: "triggerError",
        rhp_signature: "Sys::triggerError(string $message, int $error_level = E_USER_NOTICE): bool",
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
    // Type validation functions -> Validate::*
    LegacyPhpFunctionMapping {
        php_name: "is_null",
        rhp_facade: "Validate",
        rhp_method: "null",
        rhp_signature: "Validate::null(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_int",
        rhp_facade: "Validate",
        rhp_method: "int",
        rhp_signature: "Validate::int(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_integer",
        rhp_facade: "Validate",
        rhp_method: "int",
        rhp_signature: "Validate::int(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_long",
        rhp_facade: "Validate",
        rhp_method: "int",
        rhp_signature: "Validate::int(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_bool",
        rhp_facade: "Validate",
        rhp_method: "bool",
        rhp_signature: "Validate::bool(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_boolean",
        rhp_facade: "Validate",
        rhp_method: "bool",
        rhp_signature: "Validate::bool(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_float",
        rhp_facade: "Validate",
        rhp_method: "float",
        rhp_signature: "Validate::float(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_double",
        rhp_facade: "Validate",
        rhp_method: "float",
        rhp_signature: "Validate::float(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_real",
        rhp_facade: "Validate",
        rhp_method: "float",
        rhp_signature: "Validate::float(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_numeric",
        rhp_facade: "Validate",
        rhp_method: "numeric",
        rhp_signature: "Validate::numeric(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_scalar",
        rhp_facade: "Validate",
        rhp_method: "scalar",
        rhp_signature: "Validate::scalar(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_resource",
        rhp_facade: "Validate",
        rhp_method: "resource",
        rhp_signature: "Validate::resource(mixed $value): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_a",
        rhp_facade: "Validate",
        rhp_method: "isA",
        rhp_signature: "Validate::isA(mixed $object_or_class, string $class, bool $allow_string = false): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "gettype",
        rhp_facade: "Debug",
        rhp_method: "type",
        rhp_signature: "Debug::type(mixed $value): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "getType",
        rhp_facade: "Debug",
        rhp_method: "type",
        rhp_signature: "Debug::type(mixed $value): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "get_debug_type",
        rhp_facade: "Debug",
        rhp_method: "type",
        rhp_signature: "Debug::type(mixed $value): string",
    },
    // CSV stream and string processing functions -> Csv::*
    LegacyPhpFunctionMapping {
        php_name: "fgetcsv",
        rhp_facade: "Csv",
        rhp_method: "fgetcsv",
        rhp_signature: "Csv::fgetcsv(resource $stream, ?int $length = null, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\"): array|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "fputcsv",
        rhp_facade: "Csv",
        rhp_method: "fputcsv",
        rhp_signature: "Csv::fputcsv(resource $stream, array $fields, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\", string $eol = \"\\n\"): int|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "str_getcsv",
        rhp_facade: "Csv",
        rhp_method: "parseRow",
        rhp_signature: "Csv::parseRow(string $string, string $separator = \",\", string $enclosure = \"\\\"\", string $escape = \"\\\\\"): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "fopen",
        rhp_facade: "File",
        rhp_method: "open",
        rhp_signature: "File::open(string $filename, string $mode): resource|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "fclose",
        rhp_facade: "File",
        rhp_method: "close",
        rhp_signature: "File::close(resource $stream): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "feof",
        rhp_facade: "File",
        rhp_method: "eof",
        rhp_signature: "File::eof(resource $stream): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "rewind",
        rhp_facade: "File",
        rhp_method: "rewind",
        rhp_signature: "File::rewind(resource $stream): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "tempnam",
        rhp_facade: "File",
        rhp_method: "tempnam",
        rhp_signature: "File::tempnam(?string $dir = null, ?string $prefix = null): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "tmpfile",
        rhp_facade: "File",
        rhp_method: "tmpfile",
        rhp_signature: "File::tmpfile(): resource|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "sys_get_temp_dir",
        rhp_facade: "Path",
        rhp_method: "tempDir",
        rhp_signature: "Path::tempDir(): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "glob",
        rhp_facade: "Path",
        rhp_method: "glob",
        rhp_signature: "Path::glob(string $pattern, int $flags = 0): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "stream_socket_server",
        rhp_facade: "Socket",
        rhp_method: "server",
        rhp_signature: "Socket::server(string $address): resource|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "stream_socket_get_name",
        rhp_facade: "Socket",
        rhp_method: "getName",
        rhp_signature: "Socket::getName(resource $handle, bool $want_peer = false): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "fwrite",
        rhp_facade: "File",
        rhp_method: "writeStream",
        rhp_signature: "File::writeStream(resource $stream, string $data, ?int $length = null): int|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "fread",
        rhp_facade: "File",
        rhp_method: "readStream",
        rhp_signature: "File::readStream(resource $stream, int $length): string|false",
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
    // Binary & Hex functions -> Binary::*
    LegacyPhpFunctionMapping {
        php_name: "random_bytes",
        rhp_facade: "Binary",
        rhp_method: "randomBytes",
        rhp_signature: "Binary::randomBytes(int $length): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "bin2hex",
        rhp_facade: "Binary",
        rhp_method: "bin2hex",
        rhp_signature: "Binary::bin2hex(string $string): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "hex2bin",
        rhp_facade: "Binary",
        rhp_method: "hex2bin",
        rhp_signature: "Binary::hex2bin(string $string): string|false",
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
    // Array arithmetic and padding
    LegacyPhpFunctionMapping {
        php_name: "array_sum",
        rhp_facade: "Arr",
        rhp_method: "sum",
        rhp_signature: "Arr::sum(array $a): int|float",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_product",
        rhp_facade: "Arr",
        rhp_method: "product",
        rhp_signature: "Arr::product(array $a): int|float",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_fill",
        rhp_facade: "Arr",
        rhp_method: "fill",
        rhp_signature: "Arr::fill(int $start, int $count, mixed $value): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_fill_keys",
        rhp_facade: "Arr",
        rhp_method: "fillKeys",
        rhp_signature: "Arr::fillKeys(array $keys, mixed $value): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "array_pad",
        rhp_facade: "Arr",
        rhp_method: "pad",
        rhp_signature: "Arr::pad(array $a, int $size, mixed $value): array",
    },
    // Numeric & Math
    LegacyPhpFunctionMapping {
        php_name: "number_format",
        rhp_facade: "Float",
        rhp_method: "format",
        rhp_signature: "Float::format(float $n, int $decimals = 2, string $decimalSeparator = '.', string $thousandsSeparator = ','): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "abs",
        rhp_facade: "Float",
        rhp_method: "abs",
        rhp_signature: "Float::abs(float $n): float",
    },
    LegacyPhpFunctionMapping {
        php_name: "min",
        rhp_facade: "Float",
        rhp_method: "min",
        rhp_signature: "Float::min(float ...$n): float",
    },
    LegacyPhpFunctionMapping {
        php_name: "max",
        rhp_facade: "Float",
        rhp_method: "max",
        rhp_signature: "Float::max(float ...$n): float",
    },
    LegacyPhpFunctionMapping {
        php_name: "rand",
        rhp_facade: "Int",
        rhp_method: "random",
        rhp_signature: "Int::random(int $min, int $max): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "mt_rand",
        rhp_facade: "Int",
        rhp_method: "random",
        rhp_signature: "Int::random(int $min, int $max): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "bcadd",
        rhp_facade: "Float",
        rhp_method: "add",
        rhp_signature: "Float::add(string|float $num1, string|float $num2, ?int $scale = null): string|float",
    },
    LegacyPhpFunctionMapping {
        php_name: "bcsub",
        rhp_facade: "Float",
        rhp_method: "sub",
        rhp_signature: "Float::sub(string|float $num1, string|float $num2, ?int $scale = null): string|float",
    },
    LegacyPhpFunctionMapping {
        php_name: "bcmul",
        rhp_facade: "Float",
        rhp_method: "mul",
        rhp_signature: "Float::mul(string|float $num1, string|float $num2, ?int $scale = null): string|float",
    },
    LegacyPhpFunctionMapping {
        php_name: "bcdiv",
        rhp_facade: "Float",
        rhp_method: "div",
        rhp_signature: "Float::div(string|float $num1, string|float $num2, ?int $scale = null): string|float",
    },
    LegacyPhpFunctionMapping {
        php_name: "bccomp",
        rhp_facade: "Float",
        rhp_method: "bccomp",
        rhp_signature: "Float::bccomp(string|float $num1, string|float $num2, ?int $scale = null): int",
    },
    // String helpers
    LegacyPhpFunctionMapping {
        php_name: "str_pad",
        rhp_facade: "Str",
        rhp_method: "pad",
        rhp_signature: "Str::pad(string $s, int $length, string $pad = ' ', string $side = 'right'): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "str_repeat",
        rhp_facade: "Str",
        rhp_method: "repeat",
        rhp_signature: "Str::repeat(string $s, int $times): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "substr_count",
        rhp_facade: "Str",
        rhp_method: "countOccurrences",
        rhp_signature: "Str::countOccurrences(string $haystack, string $needle): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "substr_replace",
        rhp_facade: "Str",
        rhp_method: "replaceAt",
        rhp_signature: "Str::replaceAt(string $s, string $replacement, int $start, ?int $length = null): string",
    },
    // File helpers
    LegacyPhpFunctionMapping {
        php_name: "realpath",
        rhp_facade: "File",
        rhp_method: "realpath",
        rhp_signature: "File::realpath(string $path): string|false",
    },
    LegacyPhpFunctionMapping {
        php_name: "dirname",
        rhp_facade: "Path",
        rhp_method: "dirname",
        rhp_signature: "Path::dirname(string $path): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "basename",
        rhp_facade: "Path",
        rhp_method: "basename",
        rhp_signature: "Path::basename(string $path, ?string $ext = null): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "pathinfo",
        rhp_facade: "File",
        rhp_method: "info",
        rhp_signature: "File::info(string $path): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "filemtime",
        rhp_facade: "File",
        rhp_method: "modifiedAt",
        rhp_signature: "File::modifiedAt(string $path): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_readable",
        rhp_facade: "File",
        rhp_method: "isReadable",
        rhp_signature: "File::isReadable(string $path): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "is_writable",
        rhp_facade: "File",
        rhp_method: "isWritable",
        rhp_signature: "File::isWritable(string $path): bool",
    },
    // Url query
    LegacyPhpFunctionMapping {
        php_name: "parse_str",
        rhp_facade: "Url",
        rhp_method: "parseQuery",
        rhp_signature: "Url::parseQuery(string $query): array",
    },
    // Ip conversion
    LegacyPhpFunctionMapping {
        php_name: "ip2long",
        rhp_facade: "Ip",
        rhp_method: "toLong",
        rhp_signature: "Ip::toLong(string $ip): int",
    },
    LegacyPhpFunctionMapping {
        php_name: "long2ip",
        rhp_facade: "Ip",
        rhp_method: "fromLong",
        rhp_signature: "Ip::fromLong(int $long): string",
    },
    // Compression
    LegacyPhpFunctionMapping {
        php_name: "gzcompress",
        rhp_facade: "Compress",
        rhp_method: "zlib",
        rhp_signature: "Compress::zlib(string $data, int $level = -1): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "gzuncompress",
        rhp_facade: "Compress",
        rhp_method: "unzlib",
        rhp_signature: "Compress::unzlib(string $data): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "gzencode",
        rhp_facade: "Compress",
        rhp_method: "gzip",
        rhp_signature: "Compress::gzip(string $data, int $level = -1): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "gzdecode",
        rhp_facade: "Compress",
        rhp_method: "gunzip",
        rhp_signature: "Compress::gunzip(string $data): string",
    },
    // Uniq functions -> Uniq::*
    LegacyPhpFunctionMapping {
        php_name: "uniqid",
        rhp_facade: "Uniq",
        rhp_method: "uniqid",
        rhp_signature: "Uniq::uniqid(string $prefix = '', bool $more_entropy = false): string",
    },
    // Time functions -> Time::*
    LegacyPhpFunctionMapping {
        php_name: "now",
        rhp_facade: "Time",
        rhp_method: "now",
        rhp_signature: "Time::now(?string $tz = null): Time",
    },
    LegacyPhpFunctionMapping {
        php_name: "date",
        rhp_facade: "Time",
        rhp_method: "date",
        rhp_signature: "Time::date(string $format, ?int $timestamp = null): string",
    },
    LegacyPhpFunctionMapping {
        php_name: "usleep",
        rhp_facade: "Sys",
        rhp_method: "usleep",
        rhp_signature: "Sys::usleep(int $microseconds): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "sleep",
        rhp_facade: "Sys",
        rhp_method: "sleep",
        rhp_signature: "Sys::sleep(int $seconds): int",
    },
    // HTTP Header & Cookie functions -> phpFun::*
    LegacyPhpFunctionMapping {
        php_name: "setcookie",
        rhp_facade: "phpFun",
        rhp_method: "setcookie",
        rhp_signature: "phpFun::setcookie(string $name, string $value = '', int|array $expires_or_options = 0, string $path = '', string $domain = '', bool $secure = false, bool $httponly = false): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "setrawcookie",
        rhp_facade: "phpFun",
        rhp_method: "setrawcookie",
        rhp_signature: "phpFun::setrawcookie(string $name, string $value = '', int|array $expires_or_options = 0, string $path = '', string $domain = '', bool $secure = false, bool $httponly = false): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "header",
        rhp_facade: "phpFun",
        rhp_method: "header",
        rhp_signature: "phpFun::header(string $header, bool $replace = true, int $response_code = 0): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "header_remove",
        rhp_facade: "phpFun",
        rhp_method: "header_remove",
        rhp_signature: "phpFun::header_remove(?string $name = null): void",
    },
    LegacyPhpFunctionMapping {
        php_name: "headers_sent",
        rhp_facade: "phpFun",
        rhp_method: "headers_sent",
        rhp_signature: "phpFun::headers_sent(): bool",
    },
    LegacyPhpFunctionMapping {
        php_name: "headers_list",
        rhp_facade: "phpFun",
        rhp_method: "headers_list",
        rhp_signature: "phpFun::headers_list(): array",
    },
    LegacyPhpFunctionMapping {
        php_name: "http_response_code",
        rhp_facade: "phpFun",
        rhp_method: "http_response_code",
        rhp_signature: "phpFun::http_response_code(?int $response_code = null): int|bool",
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
            "bool" | "boolean" => "Bool",
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
            "response" => "Response",
            "regex" => "Regex",
            "reflect" => "Reflect",
            "crypt" => "Crypt",
            "html" => "Html",
            "url" => "Url",
            "time" => "Time",
            "carbon" => "Carbon",
            "validate" => "Validate",
            "csv" => "Csv",
            "uniq" => "Uniq",
            "debug" => "Debug",
            "path" => "Path",
            "socket" => "Socket",
            "binary" => "Binary",
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

    /// Clear dynamically registered Tier 2 facades.
    pub fn clear_tier2(&self) {
        if let Ok(mut g) = self.tier2.write() {
            g.clear();
        }
    }

    /// Clear dynamically registered Tier 3 facades.
    pub fn clear_tier3(&self) {
        if let Ok(mut g) = self.tier3.write() {
            g.clear();
        }
    }

    /// Clear dynamically registered Tier 2 and Tier 3 facades (used for test isolation).
    pub fn clear_dynamic(&self) {
        self.clear_tier2();
        self.clear_tier3();
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
        "bool" | "boolean" | "int" | "float" | "str" | "string" | "arr" | "array" | "future" | "log" | "db"
            | "file" | "http" | "sys" | "glob" | "session" | "cookie" | "request" | "response" | "image" | "url" | "phpfun"
            | "adb" | "afile" | "ahttp" | "crypt" | "html" | "validate" | "csv" | "uniq" | "debug"
            | "time" | "carbon" | "path" | "socket" | "binary"
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

