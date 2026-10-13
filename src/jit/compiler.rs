//! # Cranelift JIT Code Generator
//!
//! Lowers RHP bytecode chunks and hot loops into Cranelift intermediate representation (IR)
//! and compiles them into native executable machine code.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlagsData};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

use crate::ast::expr::BinaryOp;
use crate::ast::types::CastType;
use crate::ir::opcode::{BytecodeChunk, Constant, IrUnaryOp, OpCode};
use crate::jit::config::JitConfig;
use crate::jit::runtime::{NativeSlot, TAG_INT};

/// Error types emitted during JIT compilation.
#[derive(Debug, thiserror::Error)]
pub enum JitError {
    #[error("Cranelift codegen error: {0}")]
    Codegen(String),
    #[error("Cranelift module error: {0}")]
    Module(String),
    #[error("Unsupported opcode for JIT: {0:?}")]
    UnsupportedOpcode(OpCode),
    #[error("JIT target configuration error: {0}")]
    Target(String),
    #[error("Loop too small or invalid: header {header}, backedge {backedge}")]
    InvalidLoop { header: u32, backedge: u32 },
}

/// Function pointer type for compiled hot loops:
/// `unsafe extern "C" fn(slots: *mut NativeSlot) -> u32`
/// Returns the instruction pointer `target_ip` where VM execution resumes.
pub type CompiledLoopFn = unsafe extern "C" fn(*mut NativeSlot) -> u32;

/// Function pointer type for compiled functions:
/// `unsafe extern "C" fn(slots: *mut NativeSlot) -> u32`
/// Sets return value in `slots[0]` and returns `0xFFFFFFFF` on successful return.
pub type CompiledFunctionFn = unsafe extern "C" fn(*mut NativeSlot) -> u32;

/// Cranelift-backed JIT compiler capable of compiling bytecode chunks and hot loops.
pub struct JitCompiler {
    module: JITModule,
    builder_context: FunctionBuilderContext,
}

impl JitCompiler {
    /// Initialize a new [`JitCompiler`] with host ISA settings and optimization flags.
    pub fn new(config: &JitConfig) -> Result<Self, JitError> {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| JitError::Target(e.to_string()))?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| JitError::Target(e.to_string()))?;

        let opt_str = match config.opt_level {
            0 => "none",
            1 => "speed",
            _ => "speed_and_size",
        };
        flag_builder
            .set("opt_level", opt_str)
            .map_err(|e| JitError::Target(e.to_string()))?;

        let isa_builder = cranelift_native::builder()
            .map_err(|e| JitError::Target(format!("Host ISA detection failed: {}", e)))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| JitError::Target(e.to_string()))?;

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);

        Ok(Self {
            module,
            builder_context: FunctionBuilderContext::new(),
        })
    }

    /// Check whether a bytecode range contains only JIT-compilable instructions.
    pub fn can_compile_range(chunk: &BytecodeChunk, start: usize, end: usize) -> bool {
        let instrs = &chunk.instructions[start..=end.min(chunk.instructions.len().saturating_sub(1))];
        for op in instrs {
            match op {
                OpCode::LoadNull { .. }
                | OpCode::LoadBool { .. }
                | OpCode::LoadInt { .. }
                | OpCode::Move { .. }
                | OpCode::Unary { .. }
                | OpCode::Binary { .. }
                | OpCode::IncDec { .. }
                | OpCode::Cast { .. }
                | OpCode::AssertType { .. }
                | OpCode::Jump { .. }
                | OpCode::JumpIfTrue { .. }
                | OpCode::JumpIfFalse { .. }
                | OpCode::Return { .. } => {}
                OpCode::LoadConst { const_id, .. } => {
                    if let Some(c) = chunk.get_constant(*const_id) {
                        match c {
                            Constant::Int(_) | Constant::Bool(_) | Constant::Null => {}
                            _ => return false,
                        }
                    } else {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    /// Helper to store variables back into NativeSlot array and return the exit instruction pointer.
    fn emit_exit(
        builder: &mut FunctionBuilder,
        slots_ptr: cranelift_codegen::ir::Value,
        vars: &[Variable],
        exit_ip: u32,
    ) {
        let slot_size = std::mem::size_of::<NativeSlot>() as i32;
        let tag_offset = 16i32;
        let int_tag = builder.ins().iconst(types::I64, TAG_INT);

        let flags = MemFlagsData::trusted();
        for (i, &var) in vars.iter().enumerate() {
            let val = builder.use_var(var);
            let offset = (i as i32) * slot_size;
            builder.ins().store(flags, val, slots_ptr, offset);
            builder
                .ins()
                .store(flags, int_tag, slots_ptr, offset + tag_offset);
        }
        let ret_code = builder.ins().iconst(types::I32, exit_ip as i64);
        builder.ins().return_(&[ret_code]);
    }

    /// Compile a hot execution loop bounded between `header_ip` and `backedge_ip`.
    pub fn compile_loop(
        &mut self,
        name: &str,
        chunk: &BytecodeChunk,
        header_ip: u32,
        backedge_ip: u32,
    ) -> Result<CompiledLoopFn, JitError> {
        let header = header_ip as usize;
        let backedge = backedge_ip as usize;
        if header > backedge || backedge >= chunk.instructions.len() {
            return Err(JitError::InvalidLoop {
                header: header_ip,
                backedge: backedge_ip,
            });
        }

        if !Self::can_compile_range(chunk, header, backedge) {
            return Err(JitError::UnsupportedOpcode(chunk.instructions[header]));
        }

        let mut ctx = self.module.make_context();
        let ptr_type = self.module.target_config().pointer_type();

        // Signature: (slots: *mut NativeSlot) -> u32
        ctx.func.signature.params.push(AbiParam::new(ptr_type));
        ctx.func.signature.returns.push(AbiParam::new(types::I32));

        let func_id = self
            .module
            .declare_function(name, Linkage::Export, &ctx.func.signature)
            .map_err(|e| JitError::Module(e.to_string()))?;

        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut self.builder_context);

        let num_regs = chunk.num_registers as usize;
        let mut vars = Vec::with_capacity(num_regs);
        for _ in 0..num_regs {
            let var = builder.declare_var(types::I64);
            vars.push(var);
        }

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);

        // Blocks for instructions in the loop
        let mut inst_blocks = Vec::with_capacity(backedge - header + 1);
        for _ in header..=backedge {
            inst_blocks.push(builder.create_block());
        }

        // Build Entry Block: Load registers from slots into Cranelift variables
        builder.switch_to_block(entry_block);
        let slots_ptr = builder.block_params(entry_block)[0];
        let slot_size = std::mem::size_of::<NativeSlot>() as i32;
        let flags = MemFlagsData::trusted();
        for i in 0..num_regs {
            let offset = (i as i32) * slot_size;
            let val = builder
                .ins()
                .load(types::I64, flags, slots_ptr, offset);
            builder.def_var(vars[i], val);
        }
        builder.ins().jump(inst_blocks[0], &[]);

        // Build each instruction in the loop
        for (idx, ip) in (header..=backedge).enumerate() {
            let curr_block = inst_blocks[idx];
            let next_block = if idx + 1 < inst_blocks.len() {
                Some(inst_blocks[idx + 1])
            } else {
                None
            };

            builder.switch_to_block(curr_block);
            let op = chunk.instructions[ip];

            match op {
                OpCode::LoadNull { dst } => {
                    let zero = builder.ins().iconst(types::I64, 0);
                    builder.def_var(vars[dst.index()], zero);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::LoadBool { dst, value } => {
                    let v = builder.ins().iconst(types::I64, if value { 1 } else { 0 });
                    builder.def_var(vars[dst.index()], v);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::LoadInt { dst, value } => {
                    let v = builder.ins().iconst(types::I64, value as i64);
                    builder.def_var(vars[dst.index()], v);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::LoadConst { dst, const_id } => {
                    let raw_val = match chunk.get_constant(const_id) {
                        Some(Constant::Int(n)) => *n,
                        Some(Constant::Bool(b)) => if *b { 1 } else { 0 },
                        _ => 0,
                    };
                    let v = builder.ins().iconst(types::I64, raw_val);
                    builder.def_var(vars[dst.index()], v);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::Move { dst, src } => {
                    let v = builder.use_var(vars[src.index()]);
                    builder.def_var(vars[dst.index()], v);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::IncDec { dst, src, is_inc } => {
                    let v = builder.use_var(vars[src.index()]);
                    let delta = if is_inc { 1 } else { -1 };
                    let updated = builder.ins().iadd_imm_s(v, delta);
                    builder.def_var(vars[dst.index()], updated);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::Unary { dst, op: uop, src } => {
                    let operand = builder.use_var(vars[src.index()]);
                    let res = match uop {
                        IrUnaryOp::Neg => builder.ins().ineg(operand),
                        IrUnaryOp::BitNot => builder.ins().bnot(operand),
                        IrUnaryOp::Not => {
                            let is_zero = builder.ins().icmp_imm_s(IntCC::Equal, operand, 0);
                            builder.ins().uextend(types::I64, is_zero)
                        }
                        IrUnaryOp::Plus | IrUnaryOp::Silence => operand,
                    };
                    builder.def_var(vars[dst.index()], res);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::Binary {
                    dst,
                    op: bop,
                    lhs,
                    rhs,
                } => {
                    let left = builder.use_var(vars[lhs.index()]);
                    let right = builder.use_var(vars[rhs.index()]);
                    let res = match bop {
                        BinaryOp::Add => builder.ins().iadd(left, right),
                        BinaryOp::Sub => builder.ins().isub(left, right),
                        BinaryOp::Mul => builder.ins().imul(left, right),
                        BinaryOp::Div => builder.ins().sdiv(left, right),
                        BinaryOp::Mod => builder.ins().srem(left, right),
                        BinaryOp::BitAnd => builder.ins().band(left, right),
                        BinaryOp::BitOr => builder.ins().bor(left, right),
                        BinaryOp::BitXor => builder.ins().bxor(left, right),
                        BinaryOp::Shl => builder.ins().ishl(left, right),
                        BinaryOp::Shr => builder.ins().sshr(left, right),
                        BinaryOp::Lt => {
                            let cmp = builder.ins().icmp(IntCC::SignedLessThan, left, right);
                            builder.ins().uextend(types::I64, cmp)
                        }
                        BinaryOp::LtEq => {
                            let cmp =
                                builder.ins().icmp(IntCC::SignedLessThanOrEqual, left, right);
                            builder.ins().uextend(types::I64, cmp)
                        }
                        BinaryOp::Gt => {
                            let cmp =
                                builder.ins().icmp(IntCC::SignedGreaterThan, left, right);
                            builder.ins().uextend(types::I64, cmp)
                        }
                        BinaryOp::GtEq => {
                            let cmp = builder
                                .ins()
                                .icmp(IntCC::SignedGreaterThanOrEqual, left, right);
                            builder.ins().uextend(types::I64, cmp)
                        }
                        BinaryOp::Eq | BinaryOp::Identical => {
                            let cmp = builder.ins().icmp(IntCC::Equal, left, right);
                            builder.ins().uextend(types::I64, cmp)
                        }
                        BinaryOp::NotEq | BinaryOp::NotIdentical => {
                            let cmp = builder.ins().icmp(IntCC::NotEqual, left, right);
                            builder.ins().uextend(types::I64, cmp)
                        }
                        _ => left,
                    };
                    builder.def_var(vars[dst.index()], res);
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::Jump { target } => {
                    let t = target as usize;
                    if t >= header && t <= backedge {
                        builder.ins().jump(inst_blocks[t - header], &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, target);
                    }
                }
                OpCode::JumpIfTrue { cond, target } => {
                    let c = builder.use_var(vars[cond.index()]);
                    let is_true = builder.ins().icmp_imm_s(IntCC::NotEqual, c, 0);
                    let t = target as usize;
                    if t >= header && t <= backedge {
                        let target_blk = inst_blocks[t - header];
                        if let Some(next) = next_block {
                            builder.ins().brif(is_true, target_blk, &[], next, &[]);
                        } else {
                            let fall_b = builder.create_block();
                            builder.ins().brif(is_true, target_blk, &[], fall_b, &[]);
                            builder.switch_to_block(fall_b);
                            Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                        }
                    } else {
                        // Target is outside the loop
                        let out_b = builder.create_block();
                        let fall_b = next_block.unwrap_or_else(|| builder.create_block());
                        builder.ins().brif(is_true, out_b, &[], fall_b, &[]);

                        builder.switch_to_block(out_b);
                        Self::emit_exit(&mut builder, slots_ptr, &vars, target);

                        if next_block.is_none() {
                            builder.switch_to_block(fall_b);
                            Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                        }
                    }
                }
                OpCode::JumpIfFalse { cond, target } => {
                    let c = builder.use_var(vars[cond.index()]);
                    let is_false = builder.ins().icmp_imm_s(IntCC::Equal, c, 0);
                    let t = target as usize;
                    if t >= header && t <= backedge {
                        let target_blk = inst_blocks[t - header];
                        if let Some(next) = next_block {
                            builder.ins().brif(is_false, target_blk, &[], next, &[]);
                        } else {
                            let fall_b = builder.create_block();
                            builder.ins().brif(is_false, target_blk, &[], fall_b, &[]);
                            builder.switch_to_block(fall_b);
                            Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                        }
                    } else {
                        // Target is outside the loop (common loop exit condition!)
                        let out_b = builder.create_block();
                        let fall_b = next_block.unwrap_or_else(|| builder.create_block());
                        builder.ins().brif(is_false, out_b, &[], fall_b, &[]);

                        builder.switch_to_block(out_b);
                        Self::emit_exit(&mut builder, slots_ptr, &vars, target);

                        if next_block.is_none() {
                            builder.switch_to_block(fall_b);
                            Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                        }
                    }
                }
                OpCode::AssertType { .. } => {
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::Cast { dst, kind, src } => {
                    let v = builder.use_var(vars[src.index()]);
                    match kind {
                        CastType::Int => builder.def_var(vars[dst.index()], v),
                        CastType::Bool => {
                            let b = builder.ins().icmp_imm_s(IntCC::NotEqual, v, 0);
                            let ext = builder.ins().uextend(types::I64, b);
                            builder.def_var(vars[dst.index()], ext);
                        }
                        _ => builder.def_var(vars[dst.index()], v),
                    }
                    if let Some(next) = next_block {
                        builder.ins().jump(next, &[]);
                    } else {
                        Self::emit_exit(&mut builder, slots_ptr, &vars, (ip + 1) as u32);
                    }
                }
                OpCode::Return { value } => {
                    if let Some(r) = value {
                        let ret_val = builder.use_var(vars[r.index()]);
                        builder.def_var(vars[0], ret_val);
                    }
                    Self::emit_exit(&mut builder, slots_ptr, &vars, 0xFFFF_FFFFu32);
                }
                _ => {
                    // Bail out to VM interpreter at current IP
                    Self::emit_exit(&mut builder, slots_ptr, &vars, ip as u32);
                }
            }
        }

        builder.seal_all_blocks();
        builder.finalize(self.module.target_config());

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| JitError::Module(e.to_string()))?;
        self.module.clear_context(&mut ctx);
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::Module(e.to_string()))?;

        let code_ptr = self.module.get_finalized_function(func_id);
        let compiled_fn: CompiledLoopFn = unsafe { std::mem::transmute(code_ptr) };
        Ok(compiled_fn)
    }

    /// Compile a whole function into native machine code.
    pub fn compile_function(
        &mut self,
        name: &str,
        chunk: &BytecodeChunk,
    ) -> Result<CompiledFunctionFn, JitError> {
        let len = chunk.instructions.len();
        if len == 0 {
            return Err(JitError::Codegen("Empty bytecode chunk".into()));
        }
        let backedge_ip = (len - 1) as u32;
        self.compile_loop(name, chunk, 0, backedge_ip)
    }
}
