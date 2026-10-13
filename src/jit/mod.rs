//! # RHP Just-In-Time (JIT) Compilation Pipeline
//!
//! Implements Technical Point #6 of the RHP Architecture Specification:
//! - **Cranelift Compiler Backend Integration:** Conditionally compiles hot execution loops
//!   and long-lived functions into native machine code at runtime.
//! - **On-Stack Replacement (OSR) & Loop JIT:** Identifies hot backedges and executes tight
//!   iteration loops in native CPU registers with zero interpreter overhead.
//! - **Safe Deoptimization & Fallback:** Seamlessly bails out to the Register Bytecode VM
//!   whenever non-JIT-compilable dynamic operations are encountered.

pub mod compiler;
pub mod config;
pub mod engine;
pub mod runtime;

pub use compiler::{CompiledFunctionFn, CompiledLoopFn, JitCompiler, JitError};
pub use config::JitConfig;
pub use engine::JitEngine;
pub use runtime::{NativeFrame, NativeSlot, TAG_BOOL, TAG_FLOAT, TAG_INT, TAG_NULL, TAG_OTHER};
