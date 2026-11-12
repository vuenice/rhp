import { siteUrl } from '../lib/paths';

export interface SearchItem {
  title: string;
  section: string;
  description: string;
  url: string;
  keywords: string[];
}

const rawSearchIndex: SearchItem[] = [
  // Getting Started
  {
    title: "Installation",
    section: "Getting Started",
    description: "Install RHP via curl script, cargo, or standalone binary for Linux and macOS.",
    url: "/docs/getting-started/installation",
    keywords: ["install", "curl", "cargo", "binary", "macos", "linux", "setup", "homebrew"]
  },
  {
    title: "Hello World",
    section: "Getting Started",
    description: "Write, check, and execute your first .rhp script with dev and build modes.",
    url: "/docs/getting-started/hello-world",
    keywords: ["hello world", "script", "first", "quickstart", "run dev"]
  },
  {
    title: "Core Architecture",
    section: "Getting Started",
    description: "Decided runtime model: master supervisor, pre-forked worker pool, single index.rhp, bump arenas, and zero-GC.",
    url: "/docs/getting-started/core-architecture",
    keywords: ["architecture", "runtime", "supervisor", "pre-fork", "index.rhp", "arena", "zero-gc", "worker pool", "boot phase", "rhp.toml"]
  },
  {
    title: "CLI Reference",
    section: "Getting Started",
    description: "Comprehensive command-line options: rhp run dev, rhp run build, and rhp run lint.",
    url: "/docs/getting-started/cli-reference",
    keywords: ["cli", "commands", "run", "dev", "build", "lint", "options", "flags"]
  },

  // Language Guide
  {
    title: "Variables and Types",
    section: "Language Guide",
    description: "Gradual type system, scalar primitives, subtyped collections (list, dict, set, hashMap), complex keys, and COW semantics.",
    url: "/docs/guide/variables-and-types",
    keywords: ["variables", "types", "scalar", "int", "float", "string", "bool", "type hints", "cow", "arr", "list", "dict", "indexmap", "laravel collections", "set", "hashmap", "map", "complex keys", "set algebra", "hashset", "hashmap"]
  },
  {
    title: "Operators",
    section: "Language Guide",
    description: "Arithmetic, null-coalescing (??), assignment, bitwise, and comparison operators.",
    url: "/docs/guide/operators",
    keywords: ["operators", "arithmetic", "null-coalesce", "ternary", "elvis", "comparison", "relational", "spaceship", "bitwise", "logical", "precedence", "less than", "greater than", "?:", "??", "<", ">"]
  },
  {
    title: "Control Flow",
    section: "Language Guide",
    description: "if/else, switch, match expressions, while, for, and foreach loops.",
    url: "/docs/guide/control-flow",
    keywords: ["control flow", "if", "else", "match", "switch", "for", "foreach", "while", "streaming iterator"]
  },
  {
    title: "Functions and Closures",
    section: "Language Guide",
    description: "Typed boundaries, fn arrow syntax, variadics, closures, and reference restrictions.",
    url: "/docs/guide/functions",
    keywords: ["functions", "arrow", "fn", "closures", "$this", "lexical capture", "parameters", "returns", "lambdas"]
  },
  {
    title: "Classes, Objects, Enums & Shared Classes",
    section: "Language Guide",
    description: "OOP architecture, visibility, invokable objects (__invoke), enums (cases, from, tryFrom), interfaces, traits, persistent shared classes, and facade macro extensibility.",
    url: "/docs/guide/classes-and-objects",
    keywords: ["classes", "objects", "shared", "shared class", "singleton", "interfaces", "traits", "enums", "enum", "cases", "from", "tryfrom", "unitenum", "backedenum", "invokable", "__invoke", "single action controller", "oop", "macro", "macros", "macroable", "extends arr", "facade extension"]
  },
  {
    title: "Error Handling",
    section: "Language Guide",
    description: "Exceptions, try/catch/finally blocks, fatal type errors, and stack traces.",
    url: "/docs/guide/error-handling",
    keywords: ["error", "handling", "exceptions", "try", "catch", "throw", "finally", "panics"]
  },
  {
    title: "Modules and Imports",
    section: "Language Guide",
    description: "Namespaces, use statements, auto-loading, and importing Rust (.rs) files for high-speed helper classes.",
    url: "/docs/guide/modules-and-imports",
    keywords: ["modules", "imports", "use", "namespace", "autoload", "require", "rust import", "rs", "cache", "log", "helper classes"]
  },
  {
    title: "Concurrency and Async",
    section: "Language Guide",
    description: "Tokio fiber pool, Future::run, Future::wait, Future::any, and post-response defer().",
    url: "/docs/guide/concurrency-async",
    keywords: ["concurrency", "async", "fibers", "tokio", "future", "wait", "defer", "parallel"]
  },
  {
    title: "Standard Library Overview",
    section: "Language Guide",
    description: "Tier 1 native facades: Arr (list, dict, set, hashMap), Str, Crypt, Html, Url, Time, Uniq, Future, Memory, File, HTTP, Log, Sys, Debug, Json, Bin, Validate, and Csv, plus runtime macro registration.",
    url: "/docs/guide/standard-library",
    keywords: ["standard library", "facades", "realpath", "dirname", "basename", "file::realpath", "file::dirname", "tempnam", "tmpfile", "sys_get_temp_dir", "file::tempnam", "file::tmpfile", "file::sysgettempdir", "file::createtempdirectory", "parsemode", "file::parsemode", "chmod", "mkdir", "natsort", "natcasesort", "sortNatural", "sortNaturalCase", "strnatcmp", "strnatcasecmp", "array_diff", "array_diff_key", "array_intersect", "array_intersect_key", "array_column", "array_chunk", "array_flip", "array_combine", "array_count_values", "array_rand", "shuffle", "usort", "uasort", "uksort", "ksort", "asort", "uniq", "uniq::uniqid", "uniq::uuid4", "uniq::uuid5", "uniq::uuid7", "uniq::uuid3", "uniqid", "uuid", "uuid4", "uuid5", "uuid7", "uuid3", "csv", "csv::fgetcsv", "csv::fputcsv", "fgetcsv", "fputcsv", "str_getcsv", "stream", "validate", "validate::null", "validate::int", "validate::bool", "validate::float", "validate::numeric", "validate::scalar", "is_null", "is_int", "is_bool", "is_float", "is_numeric", "is_scalar", "is_resource", "debug", "debug::dump", "debug::dd", "debug::format", "json", "json::encode", "json::decode", "bin", "bin::encode", "bin::decode", "var_export", "__set_state", "arr", "list", "dict", "indexmap", "laravel collections", "dot notation", "set", "hashmap", "str", "crypt", "html", "url", "time", "carbon", "now", "today", "toDateTimeString", "toDateString", "addDay", "lastOfMonth", "diffForHumans", "hash", "hmac", "openssl", "stripTags", "specialChars", "parseUrl", "future", "memory", "file", "http", "macro", "macros", "arr::macro", "macroable"]
  },
  {
    title: "Observability & OpenTelemetry",
    section: "Language Guide",
    description: "Native OpenTelemetry distributed tracing, W3C traceparent, metrics percentiles, OTLP, and APM bridges.",
    url: "/docs/guide/observability-opentelemetry",
    keywords: ["opentelemetry", "otel", "tracing", "spans", "telemetry", "metrics", "otlp", "sentry", "datadog", "traceparent", "baggage", "histograms", "observability"]
  },

  // Reference
  {
    title: "Grammar & Syntax Reference",
    section: "Reference",
    description: "Complete formal EBNF grammar specifications, custom Pratt parser operator precedence table, and typed AST structure.",
    url: "/docs/reference/grammar",
    keywords: ["grammar", "ebnf", "syntax", "tokens", "pratt parser", "precedence", "binding power", "combinator", "ast", "expressions", "destructuring"]
  },
  {
    title: "Lexical & Syntax Architecture",
    section: "Reference",
    description: "Refined lexical rules, keyword resolution, numeric validation, quoting strategies, maximal munch, case sensitivity, and variable validation.",
    url: "/docs/reference/syntax-architecture",
    keywords: ["lexical", "syntax", "architecture", "lexer", "logos", "maximal munch", "numeric literals", "trailing underscore", "leading dot", "quotes", "backticks", "case sensitivity", "attributes", "elseif", "variable names"]
  },
  {
    title: "Keywords Reference",
    section: "Reference",
    description: "Exhaustive table of all reserved words, contextual keywords, and modifiers in RHP.",
    url: "/docs/reference/keywords",
    keywords: ["keywords", "reserved", "modifiers", "shared", "singleton", "defer", "async"]
  },
  {
    title: "Built-in Functions",
    section: "Reference",
    description: "Intrinsic intrinsics (env, print, dd, dump) and legacy PHP bridge lowering (phpFun::*, var_export, var_dump, json_encode, serialize, is_*, fgetcsv, fputcsv, natsort, array_*).",
    url: "/docs/reference/builtins",
    keywords: ["builtins", "env", "print", "dd", "dump", "realpath", "dirname", "basename", "tempnam", "tmpfile", "sys_get_temp_dir", "parse_mode", "var_dump", "var_export", "natsort", "natcasesort", "strnatcmp", "strnatcasecmp", "array_diff", "array_diff_key", "array_intersect", "array_intersect_key", "array_column", "array_chunk", "array_flip", "array_combine", "array_count_values", "array_rand", "shuffle", "usort", "uasort", "uksort", "ksort", "asort", "debug::dump", "debug::format", "json_encode", "json_decode", "serialize", "unserialize", "is_null", "is_int", "is_bool", "is_float", "is_numeric", "is_scalar", "is_resource", "validate::null", "fgetcsv", "fputcsv", "str_getcsv", "fopen", "fclose", "csv::fgetcsv", "phpFun", "legacy functions", "md5", "sha1", "htmlspecialchars", "strip_tags", "parse_url", "http_build_query", "base64_encode"]
  },
  {
    title: "Standard Library API Reference",
    section: "Reference",
    description: "Full method signatures for Arr (list, dict, set, hashMap), Str, Crypt, Html, Url, Time, HTTP, File, Memory, Debug, Json, Bin, Validate, Csv, and Tier 2 native helper classes (Cache, Log).",
    url: "/docs/reference/stdlib-api",
    keywords: ["stdlib", "api", "signatures", "methods", "realpath", "dirname", "file::realpath", "file::dirname", "file::tempnam", "file::tmpfile", "file::sysgettempdir", "file::createtempdirectory", "file::parsemode", "afile::realpath", "afile::tempnam", "afile::createtempdirectory", "natsort", "natcasesort", "strnatcmp", "strnatcasecmp", "array_diff", "array_diff_key", "array_intersect", "array_intersect_key", "array_column", "array_chunk", "array_flip", "array_combine", "array_count_values", "array_rand", "shuffle", "csv", "csv::fgetcsv", "csv::fputcsv", "csv::open", "csv::close", "csv::parse", "csv::generate", "fgetcsv", "fputcsv", "str_getcsv", "validate", "validate::null", "validate::int", "validate::bool", "validate::float", "validate::numeric", "validate::scalar", "debug", "debug::dump", "debug::dd", "debug::format", "json", "json::encode", "json::decode", "bin", "bin::encode", "bin::decode", "arr", "list", "dict", "set", "hashmap", "map", "complex keys", "set algebra", "str", "crypt", "html", "url", "time", "carbon", "now", "today", "toDateTimeString", "toDateString", "addDay", "lastOfMonth", "diffForHumans", "hash", "hashHmac", "opensslEncrypt", "randomBytes", "specialChars", "stripTags", "parseUrl", "httpBuildQuery", "base64Encode", "http", "file", "cache", "log", "tier 2", "rust"]
  },
  {
    title: "Deprecated PHP Features",
    section: "Reference",
    description: "Detailed overview of deprecated and unsupported PHP features (Reflection, Output Buffering, cURL, Legacy Serialization, Directives, list(), die(), error_reporting, var_export), reasons for rejection, modern RHP replacements, and migration guide.",
    url: "/docs/reference/unsupported-php",
    keywords: ["deprecated", "deprecated php", "depriciated", "depriciated php features", "unsupported", "php", "var_export", "var_dump", "__set_state", "debug::format", "debug::dump", "bin::encode", "json::encode", "error_reporting", "error reporting", "set_error_handler", "@ error suppression", "reflection", "reflectionclass", "output buffering", "ob_start", "ob_get_clean", "curl", "curl_init", "curl_setopt", "serialize", "unserialize", "__sleep", "__wakeup", "gadget chains", "serialization", "migration", "replacements", "di codegen", "output::capture", "http facade", "declare", "strict_types", "ticks", "list", "destructuring", "die", "sys::exit", "abort"]
  },

  // Tooling
  {
    title: "Package Manager",
    section: "Tooling",
    description: "RHP package manager roadmap, composer interoperability, and Rust crate linking.",
    url: "/docs/tooling/package-manager",
    keywords: ["packages", "composer", "cargo", "dependencies", "modules"]
  },
  {
    title: "Auto-Formatter",
    section: "Tooling",
    description: "AST-guided code formatting, 4-space indent rule, semicolon statement splitting, parameter spacing, and brace normalization.",
    url: "/docs/tooling/formatter",
    keywords: ["formatter", "format", "indentation", "mago", "prettify", "code style", "semicolon", "semicolon splitting", "parameter spacing"]
  },
  {
    title: "Linter and Static Typing",
    section: "Tooling",
    description: "Auto-typing variables, copy-by-reference array parameters, collection call auto-assignment, casing enforcement, and JSON diagnostics.",
    url: "/docs/tooling/linter",
    keywords: ["linter", "type checker", "casing", "diagnostics", "json", "lint", "copy by reference", "pass by reference", "reference", "mutated array", "auto assign", "map", "arr::list"]
  },
  {
    title: "Editor Setup & LSP",
    section: "Tooling",
    description: "VS Code extension, Neovim LSP configuration, and Tree-sitter grammar.",
    url: "/docs/tooling/editor-setup",
    keywords: ["editor", "vscode", "neovim", "lsp", "tree-sitter", "syntax highlighting"]
  },
  {
    title: "Playground App",
    section: "Tooling",
    description: "Interactive desktop app to run RHP scripts and debug local, Docker, or remote applications.",
    url: "/docs/tooling/playground",
    keywords: ["playground", "rhp-play", "gui", "desktop", "tweakphp", "debug", "docker", "ssh", "repl", "scratchpad", "script runner"]
  },

  // Advanced
  {
    title: "Memory Model & Zero-GC",
    section: "Advanced",
    description: "RAII destruction, Copy-on-Write detaching, bump request arena, and cycle prevention.",
    url: "/docs/advanced/memory-model",
    keywords: ["memory", "gc", "zero-gc", "raii", "cow", "arena", "request arena", "leaks"]
  },
  {
    title: "Runtime & Process Architecture",
    section: "Advanced",
    description: "Master supervisor process, pre-forked worker pool, socket ownership, Tokio/sqlx post-fork init, hot reload, and rhpm.",
    url: "/docs/advanced/runtime-architecture",
    keywords: ["runtime", "supervisor", "pre-fork", "fork", "workers", "tokio", "sqlx", "socket", "hot reload", "rhpm", "defer", "process model"]
  },
  {
    title: "Language Layer & Semantics",
    section: "Advanced",
    description: "Grammar, gradual typing, Cranelift unboxed scalars, subtyped collections, crash containment, and the CLI Test.",
    url: "/docs/advanced/language-layer",
    keywords: ["language", "semantics", "cli test", "types", "compiler", "phpvalue", "list", "dict", "set", "future", "arr", "str", "crash containment"]
  },
  {
    title: "FFI and Rust Native Interop",
    section: "Advanced",
    description: "Direct Rust (.rs) file imports, Tier 2 helper classes, native crates, and C-ABI dynamic libraries.",
    url: "/docs/advanced/ffi-interop",
    keywords: ["ffi", "rust", "interop", "native", "c abi", "crates", "plugins", "rust file import", "import", "rs", "cache", "log", "helper class"]
  },
  {
    title: "Blocking Native/FFI Extensions Escape Hatch",
    section: "Advanced",
    description: "Handling synchronous C/Rust libraries (libvips, argon2id, synchronous DNS) with #[blocking], tokio::task::spawn_blocking offloading, and fiber suspend_for_io.",
    url: "/docs/advanced/blocking-native-ffi",
    keywords: ["blocking", "#[blocking]", "ffi", "native", "tokio", "spawn_blocking", "fiber", "suspend_for_io", "escape hatch", "libvips", "argon2", "argon2id", "frozenvalue", "io_breakdown", "nativeblocking"]
  },
  {
    title: "Compiler & VM Internals",
    section: "Advanced",
    description: "Logos lexer, custom Pratt parser & combinators, strongly-typed AST, register bytecode VM engine, PhpValue, and Cranelift JIT.",
    url: "/docs/advanced/compiler-internals",
    keywords: ["compiler", "vm", "jit", "cranelift", "bytecode", "logos", "ir", "internals", "pratt parser", "combinators", "ast", "ast visitor", "span tracking", "tree-walk", "register vm", "phpvalue", "dispatch loop", "callframe", "exception table"]
  },
  {
    title: "Pure Closure Optimization",
    section: "Advanced",
    description: "Closure purity categorization, $this elision, zero-allocation inline struct layout, and raw Cranelift machine-code compilation.",
    url: "/docs/advanced/pure-closure-optimization",
    keywords: ["closure", "pure closure", "arrow function", "anonymous function", "cranelift", "jit", "fn_ptr", "zero allocation", "this elision", "closurecategory", "upvalues", "reference capture", "capturing", "closure purity", "cycle elimination"]
  },
  {
    title: "Collection Compiler Optimizations",
    section: "Advanced",
    description: "Under-the-hood FBIP in-place buffer reuse (refcount == 1 zero-allocation sort) and pure stream fusion (single-pass loop deforestation with O(K) early-exit).",
    url: "/docs/advanced/collection-compiler-optimizations",
    keywords: ["collection", "compiler optimization", "fbip", "in-place sort", "refcount", "stream fusion", "loop deforestation", "pipeline", "map", "filter", "take", "skip", "reject", "zero allocation", "short-circuit", "memory efficiency"]
  },
  {
    title: "Safe Addition & Arithmetic Semantics",
    section: "Advanced",
    description: "Checked addition (+), wrapping (+%), saturating (+|), constant folding, LLVM overflow intrinsics, and eliminating PHP coercion pitfalls.",
    url: "/docs/advanced/safe-addition-semantics",
    keywords: ["safe addition", "addition", "arithmetic", "overflow", "checked add", "+%", "+|", "wrapping", "saturating", "checked_add", "overflowing_add", "llvm", "intrinsics", "inkwell", "php pitfalls", "constant folding", "type checking", "traits", "Add interface"]
  },
  {
    title: "CPU Preemption & Instruction Budget",
    section: "Advanced",
    description: "Cooperative preemption, instruction tick budget (10,000 ticks), Cranelift JIT yield trampolines, Dev Mode infinite loop detection (1,000 warning, 100,000 hard limit), and ini_set exemptions.",
    url: "/docs/advanced/cpu-preemption",
    keywords: ["preemption", "cpu preemption", "instruction budget", "ticks", "tick_counter", "fiber", "yield", "cooperative", "jit", "cranelift", "infinite loop", "dev mode", "ini_set", "max_loop_iterations", "loop warning", "scheduler"]
  },

  // Meta
  {
    title: "Contributing Guide",
    section: "Community",
    description: "How to build RHP from source, run cargo test suites, and submit pull requests.",
    url: "/docs/contributing",
    keywords: ["contributing", "git", "cargo", "tests", "pull requests", "development"]
  },
  {
    title: "Changelog",
    section: "Community",
    description: "Release history, version notes, and migration guidance for RHP.",
    url: "/docs/changelog",
    keywords: ["changelog", "releases", "versions", "roadmap", "v0.1.0"]
  },
  {
    title: "Frequently Asked Questions (FAQ)",
    section: "Community",
    description: "Answers to common questions about PHP compatibility, Octane vs RHP, types, and facade macro extensibility.",
    url: "/docs/faq",
    keywords: ["faq", "questions", "php comparison", "octane", "swoole", "performance", "extends arr", "macro", "macros", "macroable"]
  },

  // Package
  {
    title: "Package",
    section: "Package",
    description: "Learn how to use existing packages, create your own packages in pure RHP or Rust, and understand safety, sandboxing, and native drivers.",
    url: "/docs/package",
    keywords: ["package", "packages", "rhp add", "rhp init", "rhp install", "rhp audit", "composer", "cargo", "native-rust", "wasm", "hybrid", "rhp.toml", "safety", "opaque handle", "database driver", "jwt", "mongo", "cassandra"]
  }
];

export const searchIndex: SearchItem[] = rawSearchIndex.map(item => ({
  ...item,
  url: siteUrl(item.url),
}));
