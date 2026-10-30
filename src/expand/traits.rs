use crate::diagnostic::Diagnostic;
use crate::expand::context::ExpandCtx;
use crate::expand::expansion::Expansion;
use crate::expand::site::ExpansionSite;

/// Core interface for compile-time AST transformation.
///
/// Implemented by both attribute handlers (`#[fillable]`, `#[table]`) and
/// template directive handlers (`@if`, `@foreach`, `@datetime`).
///
/// ## Extensibility Design (Milestone 1 & Milestone 2)
/// - Milestone 1 ships built-in Rust handlers implementing this trait.
/// - Milestone 2 slots in sandboxed RHP comptime handlers that execute
///   userland RHP macros inside the VM and adapt into this exact trait.
pub trait Expander: Send + Sync {
    /// The registered identifier of this expander (e.g., `fillable`, `guarded`, `table`, `if`, `datetime`).
    fn name(&self) -> &str;

    /// Execute the compile-time expansion.
    ///
    /// - `ctx`: Provides fresh-name generation (`gensym`), span chaining, and diagnostics.
    /// - `site`: Immutable view of the target node and parsed arguments.
    ///
    /// Handlers return a structured [`Expansion`]; they never mutate the surrounding AST directly.
    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic>;
}
