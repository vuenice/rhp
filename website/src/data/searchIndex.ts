export interface SearchItem {
  title: string;
  section: string;
  description: string;
  url: string;
  keywords: string[];
}

export const searchIndex: SearchItem[] = [
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
    description: "Decided runtime model: server.rhp/index.rhp split, bump arenas, zero-GC, and strict language rules.",
    url: "/docs/getting-started/core-architecture",
    keywords: ["architecture", "runtime", "server.rhp", "index.rhp", "arena", "zero-gc", "tokio", "boot heap", "milestone", "scope"]
  },
  {
    title: "Project Structure",
    section: "Getting Started",
    description: "Standard directory layout, entrypoint conventions, and config files for RHP apps.",
    url: "/docs/getting-started/project-structure",
    keywords: ["project", "directory", "structure", "layout", "config", "src", "rhp.json"]
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
    description: "Gradual type system, scalar primitives, subtyped collections, and COW semantics.",
    url: "/docs/guide/variables-and-types",
    keywords: ["variables", "types", "scalar", "int", "float", "string", "bool", "type hints", "cow"]
  },
  {
    title: "Operators",
    section: "Language Guide",
    description: "Arithmetic, null-coalescing (??), assignment, bitwise, and comparison operators.",
    url: "/docs/guide/operators",
    keywords: ["operators", "arithmetic", "null-coalesce", "ternary", "comparison", "spaceship"]
  },
  {
    title: "Control Flow",
    section: "Language Guide",
    description: "if/else, switch, match expressions, while, for, and foreach loops.",
    url: "/docs/guide/control-flow",
    keywords: ["control flow", "if", "else", "match", "switch", "for", "foreach", "while"]
  },
  {
    title: "Functions and Closures",
    section: "Language Guide",
    description: "Typed boundaries, fn arrow syntax, variadics, closures, and reference restrictions.",
    url: "/docs/guide/functions",
    keywords: ["functions", "arrow", "fn", "closures", "parameters", "returns", "lambdas"]
  },
  {
    title: "Classes, Structs & Singletons",
    section: "Language Guide",
    description: "OOP architecture, visibility, interfaces, traits, and persistent singleton classes.",
    url: "/docs/guide/classes-and-objects",
    keywords: ["classes", "structs", "objects", "singleton", "interfaces", "traits", "oop"]
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
    description: "Tier 1 native facades: Arr, Str, Future, Memory, File, HTTP, Log, and Sys.",
    url: "/docs/guide/standard-library",
    keywords: ["standard library", "facades", "arr", "str", "future", "memory", "file", "http"]
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
    description: "Complete formal EBNF grammar specifications and Pratt parser precedence table.",
    url: "/docs/reference/grammar",
    keywords: ["grammar", "ebnf", "syntax", "tokens", "pratt parser", "precedence"]
  },
  {
    title: "Keywords Reference",
    section: "Reference",
    description: "Exhaustive table of all reserved words, contextual keywords, and modifiers in RHP.",
    url: "/docs/reference/keywords",
    keywords: ["keywords", "reserved", "modifiers", "singleton", "defer", "async"]
  },
  {
    title: "Built-in Functions",
    section: "Reference",
    description: "Intrinsic intrinsics (env, print, dd) and legacy PHP bridge lowering (phpFun::*).",
    url: "/docs/reference/builtins",
    keywords: ["builtins", "env", "print", "dd", "phpFun", "legacy functions"]
  },
  {
    title: "Standard Library API Reference",
    section: "Reference",
    description: "Full method signatures for Arr, Str, HTTP, File, Memory, and Tier 2 native helper classes (Cache, Log).",
    url: "/docs/reference/stdlib-api",
    keywords: ["stdlib", "api", "signatures", "methods", "arr", "str", "http", "file", "cache", "log", "tier 2", "rust"]
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
    description: "AST-guided code formatting, 4-space indent rule, and brace normalization.",
    url: "/docs/tooling/formatter",
    keywords: ["formatter", "format", "indentation", "mago", "prettify", "code style"]
  },
  {
    title: "Linter and Static Typing",
    section: "Tooling",
    description: "Auto-typing variables, casing enforcement, and JSON diagnostic reporting.",
    url: "/docs/tooling/linter",
    keywords: ["linter", "type checker", "casing", "diagnostics", "json", "lint"]
  },
  {
    title: "Editor Setup & LSP",
    section: "Tooling",
    description: "VS Code extension, Neovim LSP configuration, and Tree-sitter grammar.",
    url: "/docs/tooling/editor-setup",
    keywords: ["editor", "vscode", "neovim", "lsp", "tree-sitter", "syntax highlighting"]
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
    title: "FFI and Rust Native Interop",
    section: "Advanced",
    description: "Direct Rust (.rs) file imports, Tier 2 helper classes, native crates, and C-ABI dynamic libraries.",
    url: "/docs/advanced/ffi-interop",
    keywords: ["ffi", "rust", "interop", "native", "c abi", "crates", "plugins", "rust file import", "import", "rs", "cache", "log", "helper class"]
  },
  {
    title: "Compiler Internals",
    section: "Advanced",
    description: "Logos lexer, Pratt parser, linear bytecode IR, register VM, and Cranelift JIT.",
    url: "/docs/advanced/compiler-internals",
    keywords: ["compiler", "vm", "jit", "cranelift", "bytecode", "logos", "ir", "internals"]
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
    description: "Answers to common questions about PHP compatibility, Octane vs RHP, and types.",
    url: "/docs/faq",
    keywords: ["faq", "questions", "php comparison", "octane", "swoole", "performance"]
  }
];
