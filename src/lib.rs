//! # RHP: High-Performance PHP-Family Programming Language Engine
//!
//! RHP implements a high-throughput runtime engine with dual execution modes:
//! - **Development Mode**: Direct AST tree-walking interpreter with sub-millisecond hot-reloading and exact source mapping.
//! - **Production Mode**: Linear IR opcode register VM with Cranelift JIT compilation.

pub mod lexer;

pub use lexer::{
    error::{LexError, SpannedLexError},
    facade::{get_static_facade, is_static_facade, validate_instantiation, FacadeInfo, FacadeTier},
    span::{SourceFile, SourceMap, Span},
    token::Token,
    Lexer, SpannedToken,
};
