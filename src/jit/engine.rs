//! # Cranelift JIT Execution Engine
//!
//! Tracks runtime hotness profiling counters for function invocations and loop backedges,
//! triggers conditional native compilation via Cranelift, and executes native machine code
//! with transparent On-Stack Replacement (OSR) and register writeback.

use std::collections::{HashMap, HashSet};

use crate::ir::opcode::{BytecodeChunk, FunctionId};
use crate::jit::compiler::{CompiledFunctionFn, CompiledLoopFn, JitCompiler};
use crate::jit::config::JitConfig;
use crate::jit::runtime::NativeFrame;
use crate::vm::PhpValue;

/// High-Performance JIT Execution Manager.
pub struct JitEngine {
    config: JitConfig,
    compiler: Option<JitCompiler>,
    /// Invocation counts per function: `func_id -> invocation_count`.
    function_invocations: HashMap<FunctionId, usize>,
    /// Loop backedge iteration counts: `(func_id, loop_header_ip) -> count`.
    loop_backedges: HashMap<(Option<FunctionId>, u32), usize>,
    /// Registry of compiled hot loops: `(func_id, loop_header_ip) -> CompiledLoopFn`.
    compiled_loops: HashMap<(Option<FunctionId>, u32), CompiledLoopFn>,
    /// Registry of compiled hot functions: `func_id -> CompiledFunctionFn`.
    compiled_functions: HashMap<FunctionId, CompiledFunctionFn>,
    /// Set of loops that failed compilation to avoid repeated compilation attempts.
    failed_loops: HashSet<(Option<FunctionId>, u32)>,
    /// Set of functions that failed compilation.
    failed_functions: HashSet<FunctionId>,
    /// Reusable contiguous register buffer for zero-allocation transitions.
    native_frame: NativeFrame,
    /// Monotonically increasing symbol counter for unique Cranelift JIT symbols.
    symbol_counter: usize,
}

impl JitEngine {
    /// Construct a new [`JitEngine`] with the given configuration.
    pub fn new(config: JitConfig) -> Self {
        let compiler = if config.enabled {
            JitCompiler::new(&config).ok()
        } else {
            None
        };

        Self {
            config,
            compiler,
            function_invocations: HashMap::new(),
            loop_backedges: HashMap::new(),
            compiled_loops: HashMap::new(),
            compiled_functions: HashMap::new(),
            failed_loops: HashSet::new(),
            failed_functions: HashSet::new(),
            native_frame: NativeFrame::new(128),
            symbol_counter: 0,
        }
    }

    /// Access the current [`JitConfig`].
    pub fn config(&self) -> &JitConfig {
        &self.config
    }

    /// Check if JIT is enabled and compiler backend is initialized.
    pub fn is_active(&self) -> bool {
        self.config.enabled && self.compiler.is_some()
    }

    /// Number of hot loops successfully compiled into native machine code.
    pub fn compiled_loops_count(&self) -> usize {
        self.compiled_loops.len()
    }

    /// Number of hot functions successfully compiled into native machine code.
    pub fn compiled_functions_count(&self) -> usize {
        self.compiled_functions.len()
    }

    /// Check if a specific loop header has been compiled to native machine code.
    pub fn is_loop_compiled(&self, func_id: Option<FunctionId>, header_ip: u32) -> bool {
        self.compiled_loops.contains_key(&(func_id, header_ip))
    }

    /// Check if a specific function has been compiled to native machine code.
    pub fn is_function_compiled(&self, func_id: FunctionId) -> bool {
        self.compiled_functions.contains_key(&func_id)
    }

    /// Record a backward loop jump (backedge). If the loop exceeds the hotness threshold,
    /// conditionally compiles the loop body into native machine code via Cranelift.
    pub fn record_backedge(
        &mut self,
        func_id: Option<FunctionId>,
        header_ip: u32,
        backedge_ip: u32,
        chunk: &BytecodeChunk,
    ) -> Option<CompiledLoopFn> {
        if !self.config.enabled || !self.config.enable_loop_jit {
            return None;
        }

        let key = (func_id, header_ip);
        if let Some(&compiled) = self.compiled_loops.get(&key) {
            return Some(compiled);
        }

        if self.failed_loops.contains(&key) {
            return None;
        }

        let count = self.loop_backedges.entry(key).or_insert(0);
        *count += 1;

        if *count >= self.config.hot_loop_threshold {
            let compiler = self.compiler.as_mut()?;
            self.symbol_counter += 1;
            let symbol_name = format!(
                "rhp_jit_loop_{}_{}_{}_{}",
                func_id.map(|f| f.0 as i64).unwrap_or(-1),
                header_ip,
                backedge_ip,
                self.symbol_counter
            );

            match compiler.compile_loop(&symbol_name, chunk, header_ip, backedge_ip) {
                Ok(loop_fn) => {
                    self.compiled_loops.insert(key, loop_fn);
                    Some(loop_fn)
                }
                Err(_) => {
                    self.failed_loops.insert(key);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Record a function invocation. If the invocation count exceeds the hotness threshold,
    /// conditionally compiles the function into native machine code via Cranelift.
    pub fn record_function_call(
        &mut self,
        func_id: FunctionId,
        chunk: &BytecodeChunk,
    ) -> Option<CompiledFunctionFn> {
        if !self.config.enabled || !self.config.enable_function_jit {
            return None;
        }

        if let Some(&compiled) = self.compiled_functions.get(&func_id) {
            return Some(compiled);
        }

        if self.failed_functions.contains(&func_id) {
            return None;
        }

        let count = self.function_invocations.entry(func_id).or_insert(0);
        *count += 1;

        if *count >= self.config.hot_function_threshold {
            let compiler = self.compiler.as_mut()?;
            self.symbol_counter += 1;
            let symbol_name = format!("rhp_jit_fn_{}_{}", func_id.0, self.symbol_counter);

            match compiler.compile_function(&symbol_name, chunk) {
                Ok(fn_ptr) => {
                    self.compiled_functions.insert(func_id, fn_ptr);
                    Some(fn_ptr)
                }
                Err(_) => {
                    self.failed_functions.insert(func_id);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Execute a compiled hot loop natively, unboxing registers, running at CPU speed,
    /// writing back the modified registers, and returning the exit instruction pointer.
    pub fn execute_loop(&mut self, loop_fn: CompiledLoopFn, regs: &mut [PhpValue]) -> u32 {
        self.native_frame.load_from_registers(regs);
        let exit_ip = unsafe { loop_fn(self.native_frame.as_mut_ptr()) };
        self.native_frame.write_back_to_registers(regs);
        exit_ip
    }

    /// Execute a compiled function natively, unboxing arguments, running native code,
    /// writing back registers, and extracting the return value.
    pub fn execute_function(
        &mut self,
        func_fn: CompiledFunctionFn,
        regs: &mut [PhpValue],
    ) -> Option<PhpValue> {
        self.native_frame.load_from_registers(regs);
        let ret_code = unsafe { func_fn(self.native_frame.as_mut_ptr()) };
        self.native_frame.write_back_to_registers(regs);
        if ret_code == 0xFFFF_FFFFu32 {
            Some(self.native_frame.slots.first().map(|s| s.to_php_value(None)).unwrap_or(PhpValue::Null))
        } else {
            None
        }
    }
}
