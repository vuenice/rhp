//! # RHP Compiler Extension & Macro Expansion Subsystem
//!
//! Provides a unified compile-time AST transformation pipeline for:
//! 1. **Code Attributes (`#[...]`)**: Custom class, method, and property transformations
//!    such as `#[fillable]`, `#[guarded]`, and `#[table]`.
//! 2. **Template Directives (`@...`)**: Blade-like template control flow and output directives
//!    such as `@if`, `@for`, `@foreach`, `@datetime`, and `{{ expr }}` escaping.
//!
//! ## Pipeline Invariants
//! - **Placement:** Expansion runs strictly after parsing and before type checking.
//!   Handlers receive an untyped AST and cannot rely on type information.
//! - **Hygiene:** Unique non-capturing symbols via [`ExpandCtx::gensym`].
//! - **Ordering:** Strict declaration order (top to bottom, outer before inner).
//! - **Determinism:** Bit-identical output for identical input (caching baseline).
//! - **Fault Containment:** Handler panics are caught and converted into structured [`Diagnostic`] errors.

pub mod builder;
pub mod builtin;
pub mod context;
pub mod engine;
pub mod expansion;
pub mod registry;
pub mod site;
pub mod template;
pub mod traits;

pub use builder::AstBuilder;
pub use builtin::{
    register_builtins, DatetimeExpander, FillableExpander, ForExpander, ForeachExpander,
    GuardedExpander, IfExpander, TableExpander,
};
pub use context::{ExpandCtx, SpanChain};
pub use engine::ExpansionEngine;
pub use expansion::Expansion;
pub use registry::ExpanderRegistry;
pub use site::ExpansionSite;
pub use template::{
    compile_template, TemplateDirective, TemplateElseBranch, TemplateLexer, TemplateNode,
    TemplateParser, TemplateToken,
};
pub use traits::Expander;

use std::path::Path;

use crate::ast::Program;
use crate::diagnostic::Diagnostic;
use crate::lexer::span::SourceFile;

/// Expand an already-parsed AST program using the provided registry and context.
pub fn expand_program(
    program: &Program<'_>,
    registry: &ExpanderRegistry,
    ctx: &mut ExpandCtx,
) -> Result<Program<'static>, Diagnostic> {
    let mut engine = ExpansionEngine::new(registry, ctx);
    engine.expand_program(program)
}

/// Compile and expand a template source file (.rhp.html, .blade.rhp) into an RHP AST program.
pub fn expand_template(
    source: &str,
    file_id: usize,
    registry: &ExpanderRegistry,
    ctx: &mut ExpandCtx,
) -> Result<Program<'static>, Diagnostic> {
    template::compile_template(source, file_id, registry, ctx)
}

/// Parse, expand attributes, and return an expanded program ready for type checking.
pub fn parse_and_expand(source: &str) -> Result<Program<'static>, Diagnostic> {
    parse_and_expand_with_file_id(source, 0)
}

/// Parse and expand attributes with an explicit `file_id`.
pub fn parse_and_expand_with_file_id(
    source: &str,
    file_id: usize,
) -> Result<Program<'static>, Diagnostic> {
    let program = crate::parser::parse_with_file_id(source, file_id).map_err(|e| {
        let file = SourceFile::new(file_id, format!("source_{}", file_id), source);
        let builder = crate::diagnostic::DiagnosticBuilder::new(&file);
        builder.from_parse_error(&e)
    })?;

    let file = SourceFile::new(file_id, format!("source_{}", file_id), source);
    let mut ctx = ExpandCtx::with_source_file(file);
    let registry = ExpanderRegistry::default_with_builtins();

    expand_program(&program, &registry, &mut ctx)
}

/// Discovery hook to configure and populate the expander registry from project config (`rhp.toml`).
pub fn discover_expanders(config_path: Option<&Path>) -> Result<ExpanderRegistry, Diagnostic> {
    let mut registry = ExpanderRegistry::default_with_builtins();
    registry.populate_from_config(config_path)?;
    Ok(registry)
}
