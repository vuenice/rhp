//! Tiered Facade Architecture & Static Facade Registry
//!
//! As specified in LANGUAGE_SPECIFICATION_AND_ARCHITECTURE.md and the
//! "Designing Numeric Types in RHP" architectural guide:
//!
//! 1. Tier 1 Native Facades (`Int`, `Float`, `Arr`, `Str`, `Future`, `HTTP`, `File`, `Session`, `Image`, `URL`, `DB`, `Log`, `Sys`)
//!    are stateless utility wrappers and factory builders pre-compiled inside the engine.
//! 2. They are static-only classes (`is_static_only: true`).
//! 3. Instantiation via `new` is strictly forbidden:
//!    `Fatal Error: Cannot instantiate static facade class '...'. Use static methods directly.`
//! 4. Base datatypes (`int`, `float`, `String`) are lightweight primitives and are NOT instances of these facades.

/// Facade tier classification as defined in the architectural tiering strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacadeTier {
    /// Tier 1: Built-in Native Facades pre-compiled inside the engine core.
    /// Strictly static-only. Instantiation via `new` is forbidden.
    Tier1Native,
    /// Tier 2: Custom Rust Plugins included as crates during an AOT build step.
    Tier2RustPlugin,
    /// Tier 3: Script-land Facades written in userland code using static methods.
    Tier3Script,
}

/// Metadata describing a system facade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacadeInfo {
    pub name: &'static str,
    pub tier: FacadeTier,
    pub is_static_only: bool,
    pub description: &'static str,
}

/// All registered Tier 1 Native Facades pre-compiled inside the RHP engine.
pub static NATIVE_FACADES: &[FacadeInfo] = &[
    FacadeInfo {
        name: "Int",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Static facade helper for integer utilities and specialized Rust numeric types (i64, u32, etc.)",
    },
    FacadeInfo {
        name: "Float",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Static facade helper for float operations, formatting, and high-precision types (f64, toDecimal)",
    },
    FacadeInfo {
        name: "Arr",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Static collection factory (list, dict, set, hashMap) with fluent chaining",
    },
    FacadeInfo {
        name: "Str",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Static UTF-8 string transformation and inspection utilities (slug, contains, random)",
    },
    FacadeInfo {
        name: "Future",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Dart-style non-blocking concurrency facade (run, wait, any, delayed, defer)",
    },
    FacadeInfo {
        name: "HTTP",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Non-blocking async HTTP client facade (get, post, withHeaders)",
    },
    FacadeInfo {
        name: "File",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "High-level cooperative fiber file I/O facade (read, write, exists, delete)",
    },
    FacadeInfo {
        name: "Session",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Session state management facade across requests (get, put, has, forget)",
    },
    FacadeInfo {
        name: "Image",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Fluent image manipulation facade (make, resize, crop, save)",
    },
    FacadeInfo {
        name: "URL",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "URL generation and route inspection facade (to, route, current, full)",
    },
    FacadeInfo {
        name: "DB",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Centralized database pool query facade (table, find)",
    },
    FacadeInfo {
        name: "Log",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "Non-blocking centralized logging facade (info, error, debug)",
    },
    FacadeInfo {
        name: "Sys",
        tier: FacadeTier::Tier1Native,
        is_static_only: true,
        description: "System environment and runtime inspection facade (env, os)",
    },
];

/// Returns true if the specified class name is a registered native static facade.
#[inline]
pub fn is_static_facade(name: &str) -> bool {
    NATIVE_FACADES.iter().any(|f| f.name == name)
}

/// Retrieve the metadata info for a static facade if registered.
#[inline]
pub fn get_static_facade(name: &str) -> Option<&'static FacadeInfo> {
    NATIVE_FACADES.iter().find(|f| f.name == name)
}

/// Checks whether a class can be instantiated with `new`.
///
/// Returns an error if the class is marked `is_static_only: true`,
/// matching the exact compiler diagnostic specified in the PDF:
/// `Fatal Error: Cannot instantiate static facade class '{name}'. Use static methods directly.`
pub fn validate_instantiation(name: &str) -> Result<(), String> {
    if let Some(facade) = get_static_facade(name) {
        if facade.is_static_only {
            return Err(format!(
                "Fatal Error: Cannot instantiate static facade class '{}'. Use static methods directly.",
                facade.name
            ));
        }
    }
    Ok(())
}
