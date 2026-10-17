use std::collections::{HashMap, HashSet};

use crate::ast::{
    expr::{Argument, ArrayElement, AssignOp, BinaryOp, Expr, ExprKind, Literal, StringPart, UnaryOp},
    stmt::{
        ClassDecl, ClassMemberKind, EnumDecl, FunctionDecl, InterfaceDecl, Param, Stmt, StmtKind,
        SwitchCase, TraitDecl,
    },
    types::{CastType, TypeHint, Visibility},
    Program,
};
use crate::ir::{
    error::{IrError, IrErrorKind},
    opcode::{
        BytecodeChunk, CatchTarget, ClassId, Constant, ConstantId, ExceptionHandler, FunctionId,
        IrCollectionKind, IrUnaryOp, OpCode, Reg, RegisterInfo,
    },
    CompiledClass, CompiledClassConst, CompiledEnum, CompiledEnumCase, CompiledFunction,
    CompiledInterface, CompiledParam, CompiledProperty, IrModule,
};
use crate::lexer::{
    facade::{get_legacy_php_function, is_native_or_plugin_facade},
    span::Span,
};
use crate::type_system::{
    types::{CollectionSubtype, RhpType, RustFloatKind, RustIntKind},
    TypeCheckResult,
};

/// Active loop or `switch` control-flow frame for resolving `break` and `continue` jumps.
#[derive(Debug, Clone)]
struct LoopFrame {
    /// True if this frame is a `switch` block (`continue` inside `switch` acts like `break` in PHP, or targets enclosing loop).
    is_switch: bool,
    /// Instruction pointer target for `continue` (if already known).
    continue_target: Option<u32>,
    /// Instruction indices of forward `continue` jumps that must be patched once the update/condition label is emitted.
    continue_patches: Vec<usize>,
    /// Instruction indices of forward `break` jumps that must be patched once the loop/switch exit is emitted.
    break_patches: Vec<usize>,
}

/// Per-function/script register allocator and linear bytecode emitter.
struct ChunkBuilder {
    scope_name: String,
    chunk: BytecodeChunk,
    /// Deduplication map for string constants (`string -> ConstantId`).
    string_pool: HashMap<String, ConstantId>,
    /// Deduplication map for 64-bit integer constants (`i64 -> ConstantId`).
    int_pool: HashMap<i64, ConstantId>,
    /// Deduplication map for float bit patterns (`u64 -> ConstantId`).
    float_pool: HashMap<u64, ConstantId>,
    /// Deduplication map for semantic `RhpType` constants (`RhpType -> ConstantId`).
    type_pool: HashMap<RhpType, ConstantId>,
    /// Variables declared `global $var;` in this scope.
    globals: HashSet<String>,
    /// Stack of active loop/switch frames for `break` and `continue`.
    loop_stack: Vec<LoopFrame>,
    /// Expected return type of the enclosing function/method (`None` for top-level script).
    expected_return_type: Option<RhpType>,
    /// Enclosing class name if compiling a method.
    enclosing_class: Option<String>,
    /// Pre-populated variable type inferences from the `TypeChecker` (if available).
    inferred_var_types: HashMap<String, RhpType>,
}

impl ChunkBuilder {
    fn new(
        scope_name: impl Into<String>,
        expected_return_type: Option<RhpType>,
        enclosing_class: Option<String>,
        inferred_var_types: HashMap<String, RhpType>,
    ) -> Self {
        Self {
            scope_name: scope_name.into(),
            chunk: BytecodeChunk::new(),
            string_pool: HashMap::new(),
            int_pool: HashMap::new(),
            float_pool: HashMap::new(),
            type_pool: HashMap::new(),
            globals: HashSet::new(),
            loop_stack: Vec::new(),
            expected_return_type,
            enclosing_class,
            inferred_var_types,
        }
    }

    /// Current instruction pointer (index of next instruction to be emitted).
    #[inline]
    fn current_ip(&self) -> u32 {
        self.chunk.instructions.len() as u32
    }

    /// Emit a single flat `OpCode` with its corresponding source `Span`, returning its instruction index.
    #[inline]
    fn emit(&mut self, op: OpCode, span: Span) -> usize {
        let idx = self.chunk.instructions.len();
        self.chunk.instructions.push(op);
        self.chunk.spans.push(span);
        idx
    }

    /// Patch the jump target of a previously emitted jump/branch/iterator instruction at `instr_idx`.
    fn patch_jump(&mut self, instr_idx: usize, target_ip: u32) {
        match &mut self.chunk.instructions[instr_idx] {
            OpCode::Jump { target }
            | OpCode::JumpIfTrue { target, .. }
            | OpCode::JumpIfFalse { target, .. }
            | OpCode::JumpIfNull { target, .. }
            | OpCode::JumpIfNotNull { target, .. } => {
                *target = target_ip;
            }
            OpCode::IterNext { jump_end, .. } => {
                *jump_end = target_ip;
            }
            other => {
                debug_assert!(false, "Attempted to patch non-jump instruction: {:?}", other);
            }
        }
    }

    /// Allocate a fresh temporary virtual register.
    fn alloc_temp(&mut self, rhp_type: RhpType, span: Span) -> Result<Reg, IrError> {
        if self.chunk.num_registers == u16::MAX {
            return Err(IrError::new(
                IrErrorKind::RegisterOverflow {
                    scope_name: self.scope_name.clone(),
                },
                span,
            ));
        }
        let reg = Reg(self.chunk.num_registers);
        self.chunk.num_registers += 1;
        self.chunk.register_info.push(RegisterInfo {
            reg,
            name: None,
            rhp_type,
            is_param: false,
            is_locked_type: false,
        });
        Ok(reg)
    }

    /// Get or allocate a dedicated virtual register for a named local variable (`$var`).
    fn get_or_alloc_local(
        &mut self,
        raw_name: &str,
        default_type: Option<RhpType>,
        is_param: bool,
        is_locked_type: bool,
        span: Span,
    ) -> Result<Reg, IrError> {
        let name = normalize_var_name(raw_name);
        if let Some(&existing) = self.chunk.local_map.get(&name) {
            if let Some(ty) = default_type {
                if let Some(info) = self.chunk.register_info.get_mut(existing.index()) {
                    if is_locked_type {
                        info.rhp_type = ty;
                        info.is_locked_type = true;
                    } else if matches!(info.rhp_type, RhpType::Dynamic | RhpType::Mixed) {
                        info.rhp_type = ty;
                    }
                }
            }
            return Ok(existing);
        }

        if self.chunk.num_registers == u16::MAX {
            return Err(IrError::new(
                IrErrorKind::RegisterOverflow {
                    scope_name: self.scope_name.clone(),
                },
                span,
            ));
        }

        let reg = Reg(self.chunk.num_registers);
        self.chunk.num_registers += 1;
        let rhp_type = default_type
            .or_else(|| self.inferred_var_types.get(&name).cloned())
            .unwrap_or(RhpType::Dynamic);

        self.chunk.local_map.insert(name.clone(), reg);
        self.chunk.register_info.push(RegisterInfo {
            reg,
            name: Some(name),
            rhp_type,
            is_param,
            is_locked_type,
        });
        Ok(reg)
    }

    /// Intern a UTF-8 string into the chunk's constant pool.
    fn intern_string(&mut self, value: &str) -> ConstantId {
        if let Some(&id) = self.string_pool.get(value) {
            return id;
        }
        let id = ConstantId(self.chunk.constants.len() as u32);
        self.chunk.constants.push(Constant::String(value.to_string()));
        self.string_pool.insert(value.to_string(), id);
        id
    }

    /// Intern a 64-bit integer into the chunk's constant pool.
    fn intern_int(&mut self, value: i64) -> ConstantId {
        if let Some(&id) = self.int_pool.get(&value) {
            return id;
        }
        let id = ConstantId(self.chunk.constants.len() as u32);
        self.chunk.constants.push(Constant::Int(value));
        self.int_pool.insert(value, id);
        id
    }

    /// Intern a 64-bit float into the chunk's constant pool.
    fn intern_float(&mut self, value: f64) -> ConstantId {
        let bits = value.to_bits();
        if let Some(&id) = self.float_pool.get(&bits) {
            return id;
        }
        let id = ConstantId(self.chunk.constants.len() as u32);
        self.chunk.constants.push(Constant::Float(value));
        self.float_pool.insert(bits, id);
        id
    }

    /// Intern a semantic `RhpType` into the chunk's constant pool for runtime boundary checks.
    fn intern_type(&mut self, rhp_type: RhpType) -> ConstantId {
        if let Some(&id) = self.type_pool.get(&rhp_type) {
            return id;
        }
        let id = ConstantId(self.chunk.constants.len() as u32);
        self.chunk.constants.push(Constant::Type(rhp_type.clone()));
        self.type_pool.insert(rhp_type, id);
        id
    }
}

/// AST-to-Bytecode Intermediate Representation (IR) Lowering Compiler.
///
/// Translates a parsed (and optionally type-checked) RHP AST into an [`IrModule`]
/// containing flat, linear register-based bytecode instructions.
pub struct IrLowerer<'a> {
    type_info: Option<&'a TypeCheckResult>,
    module: IrModule,
    current_namespace: Option<String>,
    use_aliases: HashMap<String, String>,
    synthetic_counter: u32,
}

impl<'a> IrLowerer<'a> {
    pub fn new(type_info: Option<&'a TypeCheckResult>) -> Self {
        Self {
            type_info,
            module: IrModule::new(),
            current_namespace: None,
            use_aliases: HashMap::new(),
            synthetic_counter: 0,
        }
    }

    /// Construct a new [`IrLowerer`] seeded with an existing parent [`IrModule`].
    pub fn with_base_module(type_info: Option<&'a TypeCheckResult>, base_module: IrModule) -> Self {
        Self {
            type_info,
            module: base_module,
            current_namespace: None,
            use_aliases: HashMap::new(),
            synthetic_counter: 0,
        }
    }

    /// Lower a complete AST [`Program`] into a compiled [`IrModule`].
    pub fn lower_program(mut self, program: &Program<'_>) -> Result<IrModule, IrError> {
        let top_inferences = self
            .type_info
            .map(|ti| ti.top_level_inferences.clone())
            .unwrap_or_default();

        let mut main_builder = ChunkBuilder::new("<main>", None, None, top_inferences);

        for stmt in &program.statements {
            self.lower_stmt(&mut main_builder, stmt)?;
        }

        // Ensure the top-level script chunk ends with an explicit Return instruction
        let needs_terminal_return = main_builder
            .chunk
            .instructions
            .last()
            .map(|op| !matches!(op, OpCode::Return { .. } | OpCode::Throw { .. }))
            .unwrap_or(true);

        if needs_terminal_return {
            main_builder.emit(OpCode::Return { value: None }, program.span);
        }

        self.module.main_chunk = main_builder.chunk;
        Ok(self.module)
    }

    /// Lower a single standalone [`Expr`] (`ASTNode`) into a [`BytecodeChunk`] that evaluates
    /// the expression and returns its resulting register value.
    pub fn lower_expr_standalone(mut self, expr: &Expr<'_>) -> Result<BytecodeChunk, IrError> {
        let mut builder = ChunkBuilder::new("<expr>", None, None, HashMap::new());
        let result_reg = self.lower_expr(&mut builder, expr, None)?;
        builder.emit(
            OpCode::Return {
                value: Some(result_reg),
            },
            expr.span,
        );
        Ok(builder.chunk)
    }

    // =========================================================================
    // Statement Lowering
    // =========================================================================

    fn lower_stmt(&mut self, b: &mut ChunkBuilder, stmt: &Stmt<'_>) -> Result<(), IrError> {
        match &stmt.kind {
            StmtKind::Empty => Ok(()),

            StmtKind::Expr(expr) => {
                self.lower_expr(b, expr, None)?;
                Ok(())
            }

            StmtKind::VarDecl {
                var,
                type_hint,
                init,
            } => {
                let locked_ty = type_hint
                    .as_ref()
                    .and_then(|th| RhpType::from_type_hint(th, b.enclosing_class.as_deref()).ok());

                let reg = b.get_or_alloc_local(
                    var.name,
                    locked_ty.clone(),
                    false,
                    locked_ty.is_some(),
                    stmt.span,
                )?;

                if let Some(init_expr) = init {
                    // Implicit Lowering to Factory Calls:
                    // When declared type is a subtyped collection (list, dict, set, hashmap) and init is an Array literal,
                    // compile it directly into OpCode::NewCollection { kind, source: ... }
                    let col_kind = match &locked_ty {
                        Some(RhpType::Collection(CollectionSubtype::List(_))) => {
                            Some(IrCollectionKind::List)
                        }
                        Some(RhpType::Collection(CollectionSubtype::Dict(_))) => {
                            Some(IrCollectionKind::Dict)
                        }
                        Some(RhpType::Collection(CollectionSubtype::Set(_))) => {
                            Some(IrCollectionKind::Set)
                        }
                        Some(RhpType::Collection(CollectionSubtype::HashMap(_, _))) => {
                            Some(IrCollectionKind::HashMap)
                        }
                        _ => None,
                    };

                    if let (Some(kind), ExprKind::Array(_)) = (col_kind, &init_expr.kind) {
                        let arr_reg = self.lower_expr(b, init_expr, None)?;
                        b.emit(
                            OpCode::NewCollection {
                                dst: reg,
                                kind,
                                source: Some(arr_reg),
                            },
                            stmt.span,
                        );
                    } else {
                        self.lower_expr(b, init_expr, Some(reg))?;
                    }
                } else {
                    b.emit(OpCode::LoadNull { dst: reg }, stmt.span);
                }

                if let Some(ty) = locked_ty {
                    if !matches!(ty, RhpType::Mixed | RhpType::Dynamic) {
                        let type_const = b.intern_type(ty);
                        b.emit(OpCode::AssertType { reg, type_const }, stmt.span);
                    }
                }

                if b.globals.contains(&normalize_var_name(var.name)) {
                    let name_const = b.intern_string(var.bare_name());
                    b.emit(
                        OpCode::StoreGlobal {
                            name: name_const,
                            src: reg,
                        },
                        stmt.span,
                    );
                }
                Ok(())
            }

            StmtKind::Block(stmts) => {
                for s in stmts {
                    self.lower_stmt(b, s)?;
                }
                Ok(())
            }

            StmtKind::If {
                condition,
                then_branch,
                elseif_branches,
                else_branch,
            } => {
                let mut end_jumps = Vec::new();

                let cond_reg = self.lower_expr(b, condition, None)?;
                let mut false_jump =
                    b.emit(OpCode::JumpIfFalse { cond: cond_reg, target: 0 }, condition.span);

                self.lower_stmt(b, then_branch)?;

                if !elseif_branches.is_empty() || else_branch.is_some() {
                    end_jumps.push(b.emit(OpCode::Jump { target: 0 }, then_branch.span));
                }

                for elseif in elseif_branches {
                    let branch_ip = b.current_ip();
                    b.patch_jump(false_jump, branch_ip);

                    let elif_cond = self.lower_expr(b, &elseif.condition, None)?;
                    false_jump = b.emit(
                        OpCode::JumpIfFalse {
                            cond: elif_cond,
                            target: 0,
                        },
                        elseif.condition.span,
                    );

                    self.lower_stmt(b, &elseif.body)?;
                    end_jumps.push(b.emit(OpCode::Jump { target: 0 }, elseif.span));
                }

                let else_ip = b.current_ip();
                b.patch_jump(false_jump, else_ip);

                if let Some(else_stmt) = else_branch {
                    self.lower_stmt(b, else_stmt)?;
                }

                let end_ip = b.current_ip();
                for jmp in end_jumps {
                    b.patch_jump(jmp, end_ip);
                }
                Ok(())
            }

            StmtKind::While { condition, body } => {
                let loop_start = b.current_ip();
                let cond_reg = self.lower_expr(b, condition, None)?;
                let exit_jump =
                    b.emit(OpCode::JumpIfFalse { cond: cond_reg, target: 0 }, condition.span);

                b.loop_stack.push(LoopFrame {
                    is_switch: false,
                    continue_target: Some(loop_start),
                    continue_patches: Vec::new(),
                    break_patches: Vec::new(),
                });

                self.lower_stmt(b, body)?;
                b.emit(OpCode::Jump { target: loop_start }, stmt.span);

                let loop_end = b.current_ip();
                b.patch_jump(exit_jump, loop_end);

                let frame = b.loop_stack.pop().unwrap();
                for patch in frame.continue_patches {
                    b.patch_jump(patch, loop_start);
                }
                for patch in frame.break_patches {
                    b.patch_jump(patch, loop_end);
                }
                Ok(())
            }

            StmtKind::DoWhile { body, condition } => {
                let loop_start = b.current_ip();
                b.loop_stack.push(LoopFrame {
                    is_switch: false,
                    continue_target: None,
                    continue_patches: Vec::new(),
                    break_patches: Vec::new(),
                });

                self.lower_stmt(b, body)?;

                let cond_ip = b.current_ip();
                let cond_reg = self.lower_expr(b, condition, None)?;
                b.emit(
                    OpCode::JumpIfTrue {
                        cond: cond_reg,
                        target: loop_start,
                    },
                    condition.span,
                );

                let loop_end = b.current_ip();
                let frame = b.loop_stack.pop().unwrap();
                for patch in frame.continue_patches {
                    b.patch_jump(patch, cond_ip);
                }
                for patch in frame.break_patches {
                    b.patch_jump(patch, loop_end);
                }
                Ok(())
            }

            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                for init_expr in init {
                    self.lower_expr(b, init_expr, None)?;
                }

                let loop_start = b.current_ip();
                let mut exit_jump = None;
                if !condition.is_empty() {
                    let mut last_cond_reg = None;
                    for cond_expr in condition {
                        last_cond_reg = Some(self.lower_expr(b, cond_expr, None)?);
                    }
                    if let Some(cond_reg) = last_cond_reg {
                        exit_jump = Some(b.emit(
                            OpCode::JumpIfFalse {
                                cond: cond_reg,
                                target: 0,
                            },
                            stmt.span,
                        ));
                    }
                }

                b.loop_stack.push(LoopFrame {
                    is_switch: false,
                    continue_target: None,
                    continue_patches: Vec::new(),
                    break_patches: Vec::new(),
                });

                self.lower_stmt(b, body)?;

                let update_ip = b.current_ip();
                for upd_expr in update {
                    self.lower_expr(b, upd_expr, None)?;
                }
                b.emit(OpCode::Jump { target: loop_start }, stmt.span);

                let loop_end = b.current_ip();
                if let Some(ej) = exit_jump {
                    b.patch_jump(ej, loop_end);
                }

                let frame = b.loop_stack.pop().unwrap();
                for patch in frame.continue_patches {
                    b.patch_jump(patch, update_ip);
                }
                for patch in frame.break_patches {
                    b.patch_jump(patch, loop_end);
                }
                Ok(())
            }

            StmtKind::Foreach {
                expr,
                key_var,
                value_var,
                body,
            } => {
                let iterable_reg = self.lower_expr(b, expr, None)?;
                let iter_reg = b.alloc_temp(RhpType::Dynamic, expr.span)?;
                b.emit(
                    OpCode::IterInit {
                        iter_reg,
                        iterable: iterable_reg,
                    },
                    expr.span,
                );

                let key_dst = if let Some(kv) = key_var {
                    Some(b.get_or_alloc_local(kv.name, None, false, false, kv.span)?)
                } else {
                    None
                };

                // Value target can be a simple variable (`$item`) or a destructuring list (`[$id, $name]`)
                let (val_dst, needs_destructure) = match &value_var.kind {
                    ExprKind::Variable(v) => (
                        b.get_or_alloc_local(v.name, None, false, false, v.span)?,
                        false,
                    ),
                    _ => (b.alloc_temp(RhpType::Dynamic, value_var.span)?, true),
                };

                let loop_start = b.current_ip();
                let iter_next_idx = b.emit(
                    OpCode::IterNext {
                        iter_reg,
                        key_dst,
                        val_dst,
                        jump_end: 0,
                    },
                    stmt.span,
                );

                if needs_destructure {
                    self.lower_destructuring_assignment(b, value_var, val_dst, value_var.span)?;
                }

                b.loop_stack.push(LoopFrame {
                    is_switch: false,
                    continue_target: Some(loop_start),
                    continue_patches: Vec::new(),
                    break_patches: Vec::new(),
                });

                self.lower_stmt(b, body)?;
                b.emit(OpCode::Jump { target: loop_start }, stmt.span);

                let loop_end = b.current_ip();
                b.patch_jump(iter_next_idx, loop_end);

                let frame = b.loop_stack.pop().unwrap();
                for patch in frame.continue_patches {
                    b.patch_jump(patch, loop_start);
                }
                for patch in frame.break_patches {
                    b.patch_jump(patch, loop_end);
                }
                Ok(())
            }

            StmtKind::Switch { condition, cases } => {
                self.lower_switch(b, condition, cases, stmt.span)
            }

            StmtKind::Break(level_expr) => {
                let level = self.extract_break_continue_level("break", level_expr.as_ref(), stmt.span)?;
                if b.loop_stack.is_empty() {
                    return Err(IrError::new(
                        IrErrorKind::ControlFlowOutsideLoop {
                            statement: "break".to_string(),
                        },
                        stmt.span,
                    ));
                }
                if level > b.loop_stack.len() {
                    return Err(IrError::new(
                        IrErrorKind::InvalidBreakContinueLevel {
                            statement: "break".to_string(),
                            requested_level: level,
                            available_depth: b.loop_stack.len(),
                        },
                        stmt.span,
                    ));
                }
                let jmp_idx = b.emit(OpCode::Jump { target: 0 }, stmt.span);
                let target_frame_idx = b.loop_stack.len() - level;
                b.loop_stack[target_frame_idx].break_patches.push(jmp_idx);
                Ok(())
            }

            StmtKind::Continue(level_expr) => {
                let level =
                    self.extract_break_continue_level("continue", level_expr.as_ref(), stmt.span)?;
                if b.loop_stack.is_empty() {
                    return Err(IrError::new(
                        IrErrorKind::ControlFlowOutsideLoop {
                            statement: "continue".to_string(),
                        },
                        stmt.span,
                    ));
                }
                if level > b.loop_stack.len() {
                    return Err(IrError::new(
                        IrErrorKind::InvalidBreakContinueLevel {
                            statement: "continue".to_string(),
                            requested_level: level,
                            available_depth: b.loop_stack.len(),
                        },
                        stmt.span,
                    ));
                }
                let target_frame_idx = b.loop_stack.len() - level;
                if b.loop_stack[target_frame_idx].is_switch {
                    // In PHP/RHP, `continue` targeting a `switch` frame acts as `break` out of the switch
                    let jmp_idx = b.emit(OpCode::Jump { target: 0 }, stmt.span);
                    b.loop_stack[target_frame_idx].break_patches.push(jmp_idx);
                } else if let Some(target_ip) = b.loop_stack[target_frame_idx].continue_target {
                    b.emit(OpCode::Jump { target: target_ip }, stmt.span);
                } else {
                    let jmp_idx = b.emit(OpCode::Jump { target: 0 }, stmt.span);
                    b.loop_stack[target_frame_idx]
                        .continue_patches
                        .push(jmp_idx);
                }
                Ok(())
            }

            StmtKind::Return(val_opt) => {
                if let Some(expr) = val_opt {
                    let ret_reg = self.lower_expr(b, expr, None)?;
                    if let Some(ref expected) = b.expected_return_type {
                        if !matches!(
                            expected,
                            RhpType::Void | RhpType::Mixed | RhpType::Dynamic | RhpType::Never
                        ) {
                            let type_const = b.intern_type(expected.clone());
                            b.emit(
                                OpCode::AssertType {
                                    reg: ret_reg,
                                    type_const,
                                },
                                stmt.span,
                            );
                        }
                    }
                    b.emit(
                        OpCode::Return {
                            value: Some(ret_reg),
                        },
                        stmt.span,
                    );
                } else {
                    b.emit(OpCode::Return { value: None }, stmt.span);
                }
                Ok(())
            }

            StmtKind::Throw(expr) => {
                let exc_reg = self.lower_expr(b, expr, None)?;
                b.emit(OpCode::Throw { exception: exc_reg }, stmt.span);
                Ok(())
            }

            StmtKind::TryCatch {
                try_block,
                catches,
                finally_block,
            } => {
                let handler_id = b.chunk.exception_handlers.len() as u16;
                b.chunk.exception_handlers.push(ExceptionHandler {
                    handler_id,
                    try_start: 0,
                    try_end: 0,
                    catches: Vec::new(),
                    finally_target: None,
                });

                b.emit(OpCode::EnterTry { handler_id }, stmt.span);
                let try_start = b.current_ip();
                for s in try_block {
                    self.lower_stmt(b, s)?;
                }
                let try_end = b.current_ip();
                b.emit(OpCode::LeaveTry { handler_id }, stmt.span);

                let mut end_jumps = Vec::new();
                end_jumps.push(b.emit(OpCode::Jump { target: 0 }, stmt.span));

                let mut compiled_catches = Vec::with_capacity(catches.len());
                for catch in catches {
                    let catch_ip = b.current_ip();
                    let exc_reg = b.get_or_alloc_local(
                        catch.var.name,
                        catch
                            .types
                            .first()
                            .map(|id| RhpType::Class(id.name.to_string())),
                        false,
                        false,
                        catch.span,
                    )?;
                    compiled_catches.push(CatchTarget {
                        catch_types: catch.types.iter().map(|t| t.name.to_string()).collect(),
                        exception_reg: exc_reg,
                        target_ip: catch_ip,
                    });
                    for s in &catch.body {
                        self.lower_stmt(b, s)?;
                    }
                    end_jumps.push(b.emit(OpCode::Jump { target: 0 }, catch.span));
                }

                let finally_target = if let Some(finally_stmts) = finally_block {
                    let fin_ip = b.current_ip();
                    for jmp in &end_jumps {
                        b.patch_jump(*jmp, fin_ip);
                    }
                    end_jumps.clear();
                    for s in finally_stmts {
                        self.lower_stmt(b, s)?;
                    }
                    Some(fin_ip)
                } else {
                    None
                };

                let after_try_ip = b.current_ip();
                for jmp in end_jumps {
                    b.patch_jump(jmp, after_try_ip);
                }

                let handler = &mut b.chunk.exception_handlers[handler_id as usize];
                handler.try_start = try_start;
                handler.try_end = try_end;
                handler.catches = compiled_catches;
                handler.finally_target = finally_target;
                Ok(())
            }

            StmtKind::FunctionDecl(func) => {
                let func_id = self.compile_function_decl(
                    func,
                    &func.name.name,
                    None,
                    Visibility::Public,
                    false,
                    false,
                    false,
                )?;
                self.module
                    .function_index
                    .insert(func.name.name.to_string(), func_id);
                Ok(())
            }

            StmtKind::ClassDecl(cls) => {
                self.compile_class_decl(cls)?;
                Ok(())
            }

            StmtKind::InterfaceDecl(iface) => {
                self.compile_interface_decl(iface)?;
                Ok(())
            }

            StmtKind::TraitDecl(tr) => {
                self.compile_trait_decl(tr)?;
                Ok(())
            }

            StmtKind::EnumDecl(enm) => {
                self.compile_enum_decl(enm)?;
                Ok(())
            }

            StmtKind::Namespace { name, body } => {
                let prev_ns = self.current_namespace.clone();
                self.current_namespace = name.as_ref().map(|id| id.name.to_string());
                if let Some(stmts) = body {
                    for s in stmts {
                        self.lower_stmt(b, s)?;
                    }
                    self.current_namespace = prev_ns;
                }
                Ok(())
            }

            StmtKind::Use(imports) => {
                for imp in imports {
                    let alias = imp
                        .alias
                        .as_ref()
                        .map(|a| a.name.to_string())
                        .unwrap_or_else(|| {
                            imp.name
                                .name
                                .rsplit('\\')
                                .next()
                                .unwrap_or(imp.name.name)
                                .to_string()
                        });
                    self.use_aliases.insert(alias, imp.name.name.to_string());
                }
                Ok(())
            }

            StmtKind::Global(vars) => {
                for var in vars {
                    let norm = normalize_var_name(var.name);
                    b.globals.insert(norm);
                    let dst = b.get_or_alloc_local(var.name, None, false, false, var.span)?;
                    let name_const = b.intern_string(var.bare_name());
                    b.emit(
                        OpCode::LoadGlobal {
                            dst,
                            name: name_const,
                        },
                        var.span,
                    );
                }
                Ok(())
            }

            StmtKind::Echo(exprs) => {
                for expr in exprs {
                    let reg = self.lower_expr(b, expr, None)?;
                    b.emit(OpCode::Echo { src: reg }, expr.span);
                }
                Ok(())
            }

            StmtKind::Defer(deferred_stmt) => {
                let defer_name = format!("<defer#{}>", self.synthetic_counter);
                self.synthetic_counter += 1;

                // Capture current local variables so the deferred hook can reference them
                let parent_inferences = b.inferred_var_types.clone();
                let mut defer_builder = ChunkBuilder::new(
                    &defer_name,
                    Some(RhpType::Void),
                    b.enclosing_class.clone(),
                    parent_inferences,
                );
                self.lower_stmt(&mut defer_builder, deferred_stmt)?;
                defer_builder.emit(OpCode::Return { value: None }, stmt.span);

                let func_id = FunctionId(self.module.functions.len() as u32);
                self.module.functions.push(CompiledFunction {
                    id: func_id,
                    name: defer_name,
                    params: Vec::new(),
                    return_type: RhpType::Void,
                    upvalues: Vec::new(),
                    is_async: false,
                    is_static: false,
                    is_abstract: false,
                    is_final: false,
                    is_closure: true,
                    visibility: Visibility::Public,
                    enclosing_class: b.enclosing_class.clone(),
                    chunk: defer_builder.chunk,
                    span: stmt.span,
                });

                b.emit(OpCode::Defer { func_id }, stmt.span);
                Ok(())
            }
        }
    }

    fn lower_switch(
        &mut self,
        b: &mut ChunkBuilder,
        condition: &Expr<'_>,
        cases: &[SwitchCase<'_>],
        span: Span,
    ) -> Result<(), IrError> {
        let cond_reg = self.lower_expr(b, condition, None)?;

        b.loop_stack.push(LoopFrame {
            is_switch: true,
            continue_target: None,
            continue_patches: Vec::new(),
            break_patches: Vec::new(),
        });

        let mut case_jumps: Vec<usize> = Vec::with_capacity(cases.len());
        let mut default_case_idx: Option<usize> = None;

        // First emit condition dispatch table for all non-default cases
        for (idx, case) in cases.iter().enumerate() {
            if let Some(ref val_expr) = case.value {
                let case_val_reg = self.lower_expr(b, val_expr, None)?;
                let cmp_reg = b.alloc_temp(RhpType::Bool, case.span)?;
                b.emit(
                    OpCode::Binary {
                        dst: cmp_reg,
                        op: BinaryOp::Eq,
                        lhs: cond_reg,
                        rhs: case_val_reg,
                    },
                    case.span,
                );
                let jmp = b.emit(
                    OpCode::JumpIfTrue {
                        cond: cmp_reg,
                        target: 0,
                    },
                    case.span,
                );
                case_jumps.push(jmp);
            } else {
                default_case_idx = Some(idx);
                case_jumps.push(usize::MAX); // sentinel for default
            }
        }

        // If no case matched, jump to default (or end of switch)
        let nomatch_jump = b.emit(OpCode::Jump { target: 0 }, span);

        // Emit case bodies sequentially (preserving fallthrough semantics unless `break` is hit)
        let mut default_ip = None;
        for (idx, case) in cases.iter().enumerate() {
            let body_ip = b.current_ip();
            if case.value.is_some() {
                b.patch_jump(case_jumps[idx], body_ip);
            } else {
                default_ip = Some(body_ip);
            }
            for s in &case.body {
                self.lower_stmt(b, s)?;
            }
        }

        let switch_end = b.current_ip();
        let fallback_ip = if default_case_idx.is_some() {
            default_ip.unwrap_or(switch_end)
        } else {
            switch_end
        };
        b.patch_jump(nomatch_jump, fallback_ip);

        let frame = b.loop_stack.pop().unwrap();
        for patch in frame.break_patches {
            b.patch_jump(patch, switch_end);
        }
        Ok(())
    }

    fn extract_break_continue_level(
        &self,
        keyword: &str,
        level_expr: Option<&Expr<'_>>,
        span: Span,
    ) -> Result<usize, IrError> {
        match level_expr {
            None => Ok(1),
            Some(Expr {
                kind: ExprKind::Literal(Literal::Int { value, .. }),
                ..
            }) => {
                if *value < 1 {
                    Err(IrError::new(
                        IrErrorKind::NonPositiveBreakContinueLevel {
                            statement: keyword.to_string(),
                        },
                        span,
                    ))
                } else {
                    Ok(*value as usize)
                }
            }
            Some(_) => Err(IrError::new(
                IrErrorKind::NonPositiveBreakContinueLevel {
                    statement: keyword.to_string(),
                },
                span,
            )),
        }
    }

    // =========================================================================
    // Expression Lowering
    // =========================================================================

    fn lower_expr(
        &mut self,
        b: &mut ChunkBuilder,
        expr: &Expr<'_>,
        target_reg: Option<Reg>,
    ) -> Result<Reg, IrError> {
        let inferred_ty = self
            .type_info
            .and_then(|ti| ti.expr_type(&expr.span).cloned())
            .unwrap_or(RhpType::Dynamic);

        match &expr.kind {
            ExprKind::Grouping(inner) => self.lower_expr(b, inner, target_reg),

            ExprKind::Literal(lit) => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                match lit {
                    Literal::Null => {
                        b.emit(OpCode::LoadNull { dst }, expr.span);
                    }
                    Literal::Bool(value) => {
                        b.emit(OpCode::LoadBool { dst, value: *value }, expr.span);
                    }
                    Literal::Int { value, .. } => {
                        if let Ok(narrow) = i32::try_from(*value) {
                            b.emit(OpCode::LoadInt { dst, value: narrow }, expr.span);
                        } else {
                            let const_id = b.intern_int(*value);
                            b.emit(OpCode::LoadConst { dst, const_id }, expr.span);
                        }
                    }
                    Literal::Float { value, .. } => {
                        let const_id = b.intern_float(*value);
                        b.emit(OpCode::LoadConst { dst, const_id }, expr.span);
                    }
                    Literal::String { value, .. } => {
                        let const_id = b.intern_string(value);
                        b.emit(OpCode::LoadConst { dst, const_id }, expr.span);
                    }
                    Literal::InterpolatedString { parts, .. } => {
                        let mut part_regs = Vec::with_capacity(parts.len());
                        for part in parts {
                            match part {
                                StringPart::Literal(s) => {
                                    let r = b.alloc_temp(RhpType::String, expr.span)?;
                                    let cid = b.intern_string(s);
                                    b.emit(OpCode::LoadConst { dst: r, const_id: cid }, expr.span);
                                    part_regs.push(r);
                                }
                                StringPart::Expr(sub_expr) => {
                                    let r = self.lower_expr(b, sub_expr, None)?;
                                    part_regs.push(r);
                                }
                            }
                        }
                        for r in &part_regs {
                            b.emit(
                                OpCode::PushArg {
                                    src: *r,
                                    name: None,
                                    is_unpack: false,
                                },
                                expr.span,
                            );
                        }
                        b.emit(
                            OpCode::Interpolate {
                                dst,
                                part_count: part_regs.len() as u16,
                            },
                            expr.span,
                        );
                    }
                }
                Ok(dst)
            }

            ExprKind::Variable(var) => {
                let norm = normalize_var_name(var.name);
                let var_reg = b.get_or_alloc_local(var.name, None, false, false, expr.span)?;
                if b.globals.contains(&norm) {
                    let name_const = b.intern_string(var.bare_name());
                    b.emit(
                        OpCode::LoadGlobal {
                            dst: var_reg,
                            name: name_const,
                        },
                        expr.span,
                    );
                }
                if let Some(dst) = target_reg {
                    if dst != var_reg {
                        b.emit(OpCode::Move { dst, src: var_reg }, expr.span);
                    }
                    Ok(dst)
                } else {
                    Ok(var_reg)
                }
            }

            ExprKind::Ident(ident) => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let const_id = b.intern_string(ident.name);
                b.emit(OpCode::LoadConst { dst, const_id }, expr.span);
                Ok(dst)
            }

            ExprKind::Unary { op, expr: inner, .. } => {
                match op {
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec => {
                        let is_inc = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
                        let is_prefix = matches!(op, UnaryOp::PreInc | UnaryOp::PreDec);
                        self.lower_inc_dec(b, inner, is_inc, is_prefix, target_reg, expr.span)
                    }
                    _ => {
                        let src = self.lower_expr(b, inner, None)?;
                        let dst = match target_reg {
                            Some(r) => r,
                            None => b.alloc_temp(inferred_ty, expr.span)?,
                        };
                        let ir_op = match op {
                            UnaryOp::Plus => IrUnaryOp::Plus,
                            UnaryOp::Neg => IrUnaryOp::Neg,
                            UnaryOp::Not => IrUnaryOp::Not,
                            UnaryOp::BitNot => IrUnaryOp::BitNot,
                            UnaryOp::Silence => IrUnaryOp::Silence,
                            _ => unreachable!(),
                        };
                        b.emit(
                            OpCode::Unary {
                                dst,
                                op: ir_op,
                                src,
                            },
                            expr.span,
                        );
                        Ok(dst)
                    }
                }
            }

            ExprKind::Binary { op, left, right } => {
                match op {
                    // Short-circuit logical AND (`&&`, `and`)
                    BinaryOp::LogicalAnd | BinaryOp::WordAnd => {
                        let lhs = self.lower_expr(b, left, None)?;
                        let dst = match target_reg {
                            Some(r) => r,
                            None => b.alloc_temp(inferred_ty, expr.span)?,
                        };
                        b.emit(
                            OpCode::Cast {
                                dst,
                                kind: CastType::Bool,
                                src: lhs,
                            },
                            left.span,
                        );
                        let short_circuit =
                            b.emit(OpCode::JumpIfFalse { cond: dst, target: 0 }, expr.span);
                        let rhs = self.lower_expr(b, right, None)?;
                        b.emit(
                            OpCode::Cast {
                                dst,
                                kind: CastType::Bool,
                                src: rhs,
                            },
                            right.span,
                        );
                        let end_ip = b.current_ip();
                        b.patch_jump(short_circuit, end_ip);
                        Ok(dst)
                    }

                    // Short-circuit logical OR (`||`, `or`)
                    BinaryOp::LogicalOr | BinaryOp::WordOr => {
                        let lhs = self.lower_expr(b, left, None)?;
                        let dst = match target_reg {
                            Some(r) => r,
                            None => b.alloc_temp(inferred_ty, expr.span)?,
                        };
                        b.emit(
                            OpCode::Cast {
                                dst,
                                kind: CastType::Bool,
                                src: lhs,
                            },
                            left.span,
                        );
                        let short_circuit =
                            b.emit(OpCode::JumpIfTrue { cond: dst, target: 0 }, expr.span);
                        let rhs = self.lower_expr(b, right, None)?;
                        b.emit(
                            OpCode::Cast {
                                dst,
                                kind: CastType::Bool,
                                src: rhs,
                            },
                            right.span,
                        );
                        let end_ip = b.current_ip();
                        b.patch_jump(short_circuit, end_ip);
                        Ok(dst)
                    }

                    // Short-circuit Null Coalescing (`??`)
                    BinaryOp::Coalesce => {
                        let dst = match target_reg {
                            Some(r) => r,
                            None => b.alloc_temp(inferred_ty, expr.span)?,
                        };
                        self.lower_expr(b, left, Some(dst))?;
                        let not_null_jump =
                            b.emit(OpCode::JumpIfNotNull { cond: dst, target: 0 }, expr.span);
                        self.lower_expr(b, right, Some(dst))?;
                        let end_ip = b.current_ip();
                        b.patch_jump(not_null_jump, end_ip);
                        Ok(dst)
                    }

                    // Eager binary operations
                    _ => {
                        let lhs = self.lower_expr(b, left, None)?;
                        let rhs = self.lower_expr(b, right, None)?;
                        let dst = match target_reg {
                            Some(r) => r,
                            None => b.alloc_temp(inferred_ty, expr.span)?,
                        };
                        b.emit(
                            OpCode::Binary {
                                dst,
                                op: *op,
                                lhs,
                                rhs,
                            },
                            expr.span,
                        );
                        Ok(dst)
                    }
                }
            }

            ExprKind::Assign { op, target, value } => {
                self.lower_assignment(b, *op, target, value, target_reg, expr.span)
            }

            ExprKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };

                if let Some(true_expr) = if_true {
                    // Standard ternary: `$cond ? $then : $else`
                    let cond_reg = self.lower_expr(b, condition, None)?;
                    let false_jump = b.emit(
                        OpCode::JumpIfFalse {
                            cond: cond_reg,
                            target: 0,
                        },
                        condition.span,
                    );
                    self.lower_expr(b, true_expr, Some(dst))?;
                    let end_jump = b.emit(OpCode::Jump { target: 0 }, true_expr.span);

                    let else_ip = b.current_ip();
                    b.patch_jump(false_jump, else_ip);
                    self.lower_expr(b, if_false, Some(dst))?;

                    let end_ip = b.current_ip();
                    b.patch_jump(end_jump, end_ip);
                } else {
                    // Elvis operator: `$cond ?: $else`
                    self.lower_expr(b, condition, Some(dst))?;
                    let truthy_jump =
                        b.emit(OpCode::JumpIfTrue { cond: dst, target: 0 }, condition.span);
                    self.lower_expr(b, if_false, Some(dst))?;
                    let end_ip = b.current_ip();
                    b.patch_jump(truthy_jump, end_ip);
                }

                Ok(dst)
            }

            ExprKind::Call { callee, args } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };

                if let ExprKind::Ident(ident) = &callee.kind {
                    let arg_count = self.evaluate_and_push_args(b, args)?;
                    let func_name_const = b.intern_string(ident.name);
                    b.emit(
                        OpCode::CallFunction {
                            dst,
                            name: func_name_const,
                            arg_count,
                        },
                        expr.span,
                    );
                } else {
                    let callee_reg = self.lower_expr(b, callee, None)?;
                    let arg_count = self.evaluate_and_push_args(b, args)?;
                    b.emit(
                        OpCode::CallDynamic {
                            dst,
                            callee: callee_reg,
                            arg_count,
                        },
                        expr.span,
                    );
                }
                Ok(dst)
            }

            ExprKind::StaticCall {
                target,
                method,
                args,
            } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };

                if let ExprKind::Ident(target_ident) = &target.kind {
                    let target_name = target_ident.name;
                    let method_name = method.name;

                    // 1. Subtyped Collection Constructors (`Arr::list`, `Arr::dict`, `Arr::set`, `Arr::hashMap`)
                    if target_name == "Arr" {
                        let col_kind = match method_name {
                            "list" => Some(IrCollectionKind::List),
                            "dict" => Some(IrCollectionKind::Dict),
                            "set" if args.len() <= 1 => Some(IrCollectionKind::Set),
                            "hashMap" | "map_collection" => Some(IrCollectionKind::HashMap),
                            _ => None,
                        };
                        if let Some(kind) = col_kind {
                            let source = if let Some(first_arg) = args.first() {
                                Some(self.lower_expr(b, &first_arg.value, None)?)
                            } else {
                                None
                            };
                            b.emit(
                                OpCode::NewCollection { dst, kind, source },
                                expr.span,
                            );
                            return Ok(dst);
                        }
                    }

                    // 2. Native Rust Integer Wrappers (`Int::i64()`, `Int::u32()`, etc.)
                    if target_name == "Int" {
                        if let Some(int_kind) = RustIntKind::from_name(method_name) {
                            let src = if let Some(first_arg) = args.first() {
                                Some(self.lower_expr(b, &first_arg.value, None)?)
                            } else {
                                None
                            };
                            b.emit(
                                OpCode::WrapRustInt {
                                    dst,
                                    src,
                                    kind: int_kind,
                                },
                                expr.span,
                            );
                            return Ok(dst);
                        }
                    }

                    // 3. Native Rust Float Wrappers (`Float::f64()`, `Float::f32()`, `Float::i64()`)
                    if target_name == "Float" {
                        let float_kind = match method_name {
                            "f32" => Some(RustFloatKind::F32),
                            "f64" | "i64" => Some(RustFloatKind::F64),
                            _ => None,
                        };
                        if let Some(fk) = float_kind {
                            let src = if let Some(first_arg) = args.first() {
                                Some(self.lower_expr(b, &first_arg.value, None)?)
                            } else {
                                None
                            };
                            b.emit(
                                OpCode::WrapRustFloat { dst, src, kind: fk },
                                expr.span,
                            );
                            return Ok(dst);
                        }
                    }

                    // 4. Legacy PHP function bridge (`phpFun::array_merge` -> canonical `Arr::merge`)
                    if target_name == "phpFun" {
                        let (resolved_facade, resolved_method) =
                            if let Some(mapping) = get_legacy_php_function(method_name) {
                                (mapping.rhp_facade, mapping.rhp_method)
                            } else {
                                ("phpFun", method_name)
                            };
                        let arg_count = self.evaluate_and_push_args(b, args)?;
                        let facade_const = b.intern_string(resolved_facade);
                        let method_const = b.intern_string(resolved_method);
                        b.emit(
                            OpCode::FacadeCall {
                                dst,
                                facade: facade_const,
                                method: method_const,
                                arg_count,
                            },
                            expr.span,
                        );
                        return Ok(dst);
                    }

                    // 5. Tier 1 Native Facades & Tier 2 Rust Plugins
                    if is_native_or_plugin_facade(target_name) {
                        let arg_count = self.evaluate_and_push_args(b, args)?;
                        let facade_const = b.intern_string(target_name);
                        let method_const = b.intern_string(method_name);
                        b.emit(
                            OpCode::FacadeCall {
                                dst,
                                facade: facade_const,
                                method: method_const,
                                arg_count,
                            },
                            expr.span,
                        );
                        return Ok(dst);
                    }

                    // 6. Standard User Class Static Call (`User::create(...)`, `self::helper(...)`)
                    let resolved_class = self.resolve_class_name(target_name, b.enclosing_class.as_deref());
                    let arg_count = self.evaluate_and_push_args(b, args)?;
                    let class_const = b.intern_string(&resolved_class);
                    let method_const = b.intern_string(method_name);
                    b.emit(
                        OpCode::StaticCall {
                            dst,
                            class: class_const,
                            method: method_const,
                            arg_count,
                        },
                        expr.span,
                    );
                    Ok(dst)
                } else {
                    let target_reg_val = self.lower_expr(b, target, None)?;
                    let arg_count = self.evaluate_and_push_args(b, args)?;
                    let method_const = b.intern_string(method.name);
                    b.emit(
                        OpCode::MethodCall {
                            dst,
                            object: target_reg_val,
                            method: method_const,
                            arg_count,
                            is_nullsafe: false,
                        },
                        expr.span,
                    );
                    Ok(dst)
                }
            }

            ExprKind::MethodCall {
                object,
                method,
                args,
                is_nullsafe,
            } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let obj_reg = self.lower_expr(b, object, None)?;

                if *is_nullsafe {
                    b.emit(OpCode::LoadNull { dst }, expr.span);
                    let null_jump =
                        b.emit(OpCode::JumpIfNull { cond: obj_reg, target: 0 }, expr.span);
                    let arg_count = self.evaluate_and_push_args(b, args)?;
                    let method_const = b.intern_string(method.name);
                    b.emit(
                        OpCode::MethodCall {
                            dst,
                            object: obj_reg,
                            method: method_const,
                            arg_count,
                            is_nullsafe: true,
                        },
                        expr.span,
                    );
                    let end_ip = b.current_ip();
                    b.patch_jump(null_jump, end_ip);
                } else {
                    let arg_count = self.evaluate_and_push_args(b, args)?;
                    let method_const = b.intern_string(method.name);
                    b.emit(
                        OpCode::MethodCall {
                            dst,
                            object: obj_reg,
                            method: method_const,
                            arg_count,
                            is_nullsafe: false,
                        },
                        expr.span,
                    );
                }
                Ok(dst)
            }

            ExprKind::PropertyFetch {
                object,
                property,
                is_nullsafe,
            } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let obj_reg = self.lower_expr(b, object, None)?;
                let prop_const = b.intern_string(property.name);

                if *is_nullsafe {
                    b.emit(OpCode::LoadNull { dst }, expr.span);
                    let null_jump =
                        b.emit(OpCode::JumpIfNull { cond: obj_reg, target: 0 }, expr.span);
                    b.emit(
                        OpCode::GetProp {
                            dst,
                            object: obj_reg,
                            prop: prop_const,
                        },
                        expr.span,
                    );
                    let end_ip = b.current_ip();
                    b.patch_jump(null_jump, end_ip);
                } else {
                    b.emit(
                        OpCode::GetProp {
                            dst,
                            object: obj_reg,
                            prop: prop_const,
                        },
                        expr.span,
                    );
                }
                Ok(dst)
            }

            ExprKind::StaticPropertyFetch { class, property } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let resolved_class =
                    self.resolve_class_name(class.name, b.enclosing_class.as_deref());
                let class_const = b.intern_string(&resolved_class);
                let prop_const = b.intern_string(property.bare_name());
                b.emit(
                    OpCode::GetStaticProp {
                        dst,
                        class: class_const,
                        prop: prop_const,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::ClassConstFetch { class, const_name } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let resolved_class =
                    self.resolve_class_name(class.name, b.enclosing_class.as_deref());
                let class_const = b.intern_string(&resolved_class);
                let name_const = b.intern_string(const_name.name);
                b.emit(
                    OpCode::GetClassConst {
                        dst,
                        class: class_const,
                        const_name: name_const,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::Array(elements) => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                b.emit(
                    OpCode::NewArray {
                        dst,
                        capacity: elements.len() as u16,
                    },
                    expr.span,
                );
                for elem in elements {
                    if elem.is_unpack {
                        let src_arr = self.lower_expr(b, &elem.value, None)?;
                        b.emit(
                            OpCode::ArrayUnpack {
                                dst_array: dst,
                                src_array: src_arr,
                            },
                            elem.span,
                        );
                    } else if let Some(ref key_expr) = elem.key {
                        let key_reg = self.lower_expr(b, key_expr, None)?;
                        let val_reg = self.lower_expr(b, &elem.value, None)?;
                        b.emit(
                            OpCode::ArraySet {
                                array: dst,
                                key: key_reg,
                                value: val_reg,
                            },
                            elem.span,
                        );
                    } else {
                        let val_reg = self.lower_expr(b, &elem.value, None)?;
                        b.emit(
                            OpCode::ArrayPush {
                                array: dst,
                                value: val_reg,
                            },
                            elem.span,
                        );
                    }
                }
                Ok(dst)
            }

            ExprKind::List(_) => Err(IrError::new(
                IrErrorKind::InvalidAssignmentTarget {
                    reason: "destructuring list cannot be evaluated as a standalone rvalue"
                        .to_string(),
                },
                expr.span,
            )),

            ExprKind::ArrayIndex { array, index } => {
                let idx_expr = index.as_ref().ok_or_else(|| {
                    IrError::new(
                        IrErrorKind::InvalidAssignmentTarget {
                            reason: "empty subscript '[]' is only valid as an assignment target"
                                .to_string(),
                        },
                        expr.span,
                    )
                })?;
                let arr_reg = self.lower_expr(b, array, None)?;
                let idx_reg = self.lower_expr(b, idx_expr, None)?;
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                b.emit(
                    OpCode::ArrayGet {
                        dst,
                        array: arr_reg,
                        index: idx_reg,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::ArrowFunction {
                params,
                return_type,
                body,
            } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let (func_id, upvalues) =
                    self.compile_arrow_function(b, params, return_type.as_ref(), body, expr.span)?;
                for upval in &upvalues {
                    let src_reg = b.get_or_alloc_local(upval, None, false, false, expr.span)?;
                    let name_const = b.intern_string(upval);
                    b.emit(
                        OpCode::CaptureUpvalue {
                            src: src_reg,
                            name: name_const,
                        },
                        expr.span,
                    );
                }
                b.emit(
                    OpCode::MakeClosure {
                        dst,
                        func_id,
                        upvalue_count: upvalues.len() as u16,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::Closure {
                is_static,
                params,
                uses,
                return_type,
                body,
            } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let upvalues: Vec<String> = uses
                    .iter()
                    .map(|u| normalize_var_name(u.var.name))
                    .collect();
                let func_id = self.compile_closure(
                    b,
                    *is_static,
                    params,
                    &upvalues,
                    return_type.as_ref(),
                    body,
                    expr.span,
                )?;
                for upval in &upvalues {
                    let src_reg = b.get_or_alloc_local(upval, None, false, false, expr.span)?;
                    let name_const = b.intern_string(upval);
                    b.emit(
                        OpCode::CaptureUpvalue {
                            src: src_reg,
                            name: name_const,
                        },
                        expr.span,
                    );
                }
                b.emit(
                    OpCode::MakeClosure {
                        dst,
                        func_id,
                        upvalue_count: upvalues.len() as u16,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::Match { condition, arms } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                let subject_reg = self.lower_expr(b, condition, None)?;

                let mut arm_pattern_jumps: Vec<Vec<usize>> = Vec::with_capacity(arms.len());
                let mut default_arm_idx: Option<usize> = None;

                // Emit strict `===` pattern comparisons first
                for (idx, arm) in arms.iter().enumerate() {
                    if arm.is_default {
                        default_arm_idx = Some(idx);
                        arm_pattern_jumps.push(Vec::new());
                    } else {
                        let mut jumps = Vec::with_capacity(arm.patterns.len());
                        for pat in &arm.patterns {
                            let pat_reg = self.lower_expr(b, pat, None)?;
                            let cmp_reg = b.alloc_temp(RhpType::Bool, pat.span)?;
                            b.emit(
                                OpCode::Binary {
                                    dst: cmp_reg,
                                    op: BinaryOp::Identical,
                                    lhs: subject_reg,
                                    rhs: pat_reg,
                                },
                                pat.span,
                            );
                            jumps.push(b.emit(
                                OpCode::JumpIfTrue {
                                    cond: cmp_reg,
                                    target: 0,
                                },
                                pat.span,
                            ));
                        }
                        arm_pattern_jumps.push(jumps);
                    }
                }

                // Fallback when no pattern matched
                let unmatched_jump = b.emit(OpCode::Jump { target: 0 }, expr.span);

                let mut end_jumps = Vec::with_capacity(arms.len());
                let mut default_ip = None;

                for (idx, arm) in arms.iter().enumerate() {
                    let arm_ip = b.current_ip();
                    if arm.is_default {
                        default_ip = Some(arm_ip);
                    } else {
                        for jmp in &arm_pattern_jumps[idx] {
                            b.patch_jump(*jmp, arm_ip);
                        }
                    }
                    self.lower_expr(b, &arm.body, Some(dst))?;
                    end_jumps.push(b.emit(OpCode::Jump { target: 0 }, arm.span));
                }

                if let Some(def_ip) = default_ip {
                    b.patch_jump(unmatched_jump, def_ip);
                } else {
                    let err_ip = b.current_ip();
                    b.patch_jump(unmatched_jump, err_ip);
                    debug_assert!(default_arm_idx.is_none());
                    b.emit(
                        OpCode::ThrowMatchError {
                            subject: subject_reg,
                        },
                        expr.span,
                    );
                }

                let end_ip = b.current_ip();
                for jmp in end_jumps {
                    b.patch_jump(jmp, end_ip);
                }
                Ok(dst)
            }

            ExprKind::New { class, args } => {
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                match &class.kind {
                    ExprKind::Ident(id) => {
                        let class_name =
                            self.resolve_class_name(id.name, b.enclosing_class.as_deref());
                        let arg_count = self.evaluate_and_push_args(b, args)?;
                        let class_const = b.intern_string(&class_name);
                        b.emit(
                            OpCode::NewObject {
                                dst,
                                class: class_const,
                                arg_count,
                            },
                            expr.span,
                        );
                    }
                    _ => {
                        let class_reg = self.lower_expr(b, class, None)?;
                        let arg_count = self.evaluate_and_push_args(b, args)?;
                        b.emit(
                            OpCode::NewObjectDynamic {
                                dst,
                                class_reg,
                                arg_count,
                            },
                            expr.span,
                        );
                    }
                }
                Ok(dst)
            }

            ExprKind::Clone(inner) => {
                let src = self.lower_expr(b, inner, None)?;
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                b.emit(OpCode::Clone { dst, src }, expr.span);
                Ok(dst)
            }

            ExprKind::Instanceof {
                expr: obj_expr,
                class,
            } => {
                let obj_reg = self.lower_expr(b, obj_expr, None)?;
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(RhpType::Bool, expr.span)?,
                };
                let class_name = match &class.kind {
                    ExprKind::Ident(id) => {
                        self.resolve_class_name(id.name, b.enclosing_class.as_deref())
                    }
                    _ => "<dynamic>".to_string(),
                };
                let class_const = b.intern_string(&class_name);
                b.emit(
                    OpCode::Instanceof {
                        dst,
                        object: obj_reg,
                        class: class_const,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::Cast { kind, expr: inner } => {
                let src = self.lower_expr(b, inner, None)?;
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                b.emit(
                    OpCode::Cast {
                        dst,
                        kind: *kind,
                        src,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::Yield {
                key,
                value,
                is_from,
            } => {
                let key_reg = match key {
                    Some(k) => Some(self.lower_expr(b, k, None)?),
                    None => None,
                };
                let val_reg = match value {
                    Some(v) => Some(self.lower_expr(b, v, None)?),
                    None => None,
                };
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                b.emit(
                    OpCode::Yield {
                        dst,
                        key: key_reg,
                        value: val_reg,
                        is_from: *is_from,
                    },
                    expr.span,
                );
                Ok(dst)
            }

            ExprKind::Await(inner) => {
                let fut_reg = self.lower_expr(b, inner, None)?;
                let dst = match target_reg {
                    Some(r) => r,
                    None => b.alloc_temp(inferred_ty, expr.span)?,
                };
                b.emit(
                    OpCode::Await {
                        dst,
                        future: fut_reg,
                    },
                    expr.span,
                );
                Ok(dst)
            }
        }
    }

    /// Evaluate call arguments into registers first, then emit a contiguous block of `PushArg`
    /// instructions immediately before the corresponding `Call*` / `NewObject` instruction.
    fn evaluate_and_push_args(
        &mut self,
        b: &mut ChunkBuilder,
        args: &[Argument<'_>],
    ) -> Result<u16, IrError> {
        let mut staged = Vec::with_capacity(args.len());
        for arg in args {
            let val_reg = self.lower_expr(b, &arg.value, None)?;
            let name_const = arg.name.as_ref().map(|id| b.intern_string(id.name));
            staged.push((val_reg, name_const, arg.is_unpack, arg.span));
        }

        for (src, name, is_unpack, span) in staged {
            b.emit(
                OpCode::PushArg {
                    src,
                    name,
                    is_unpack,
                },
                span,
            );
        }

        Ok(args.len() as u16)
    }

    fn lower_inc_dec(
        &mut self,
        b: &mut ChunkBuilder,
        target: &Expr<'_>,
        is_inc: bool,
        is_prefix: bool,
        target_reg: Option<Reg>,
        span: Span,
    ) -> Result<Reg, IrError> {
        match &target.kind {
            ExprKind::Variable(var) => {
                let var_reg = b.get_or_alloc_local(var.name, None, false, false, span)?;
                let result_reg = if is_prefix {
                    b.emit(
                        OpCode::IncDec {
                            dst: var_reg,
                            src: var_reg,
                            is_inc,
                        },
                        span,
                    );
                    if let Some(dst) = target_reg {
                        if dst != var_reg {
                            b.emit(OpCode::Move { dst, src: var_reg }, span);
                        }
                        dst
                    } else {
                        var_reg
                    }
                } else {
                    let old_reg = match target_reg {
                        Some(r) if r != var_reg => r,
                        _ => b.alloc_temp(RhpType::Int, span)?,
                    };
                    b.emit(
                        OpCode::Move {
                            dst: old_reg,
                            src: var_reg,
                        },
                        span,
                    );
                    b.emit(
                        OpCode::IncDec {
                            dst: var_reg,
                            src: var_reg,
                            is_inc,
                        },
                        span,
                    );
                    old_reg
                };

                if b.globals.contains(&normalize_var_name(var.name)) {
                    let name_const = b.intern_string(var.bare_name());
                    b.emit(
                        OpCode::StoreGlobal {
                            name: name_const,
                            src: var_reg,
                        },
                        span,
                    );
                }

                Ok(result_reg)
            }

            ExprKind::PropertyFetch {
                object, property, ..
            } => {
                let obj_reg = self.lower_expr(b, object, None)?;
                let prop_const = b.intern_string(property.name);
                let old_reg = b.alloc_temp(RhpType::Int, span)?;
                b.emit(
                    OpCode::GetProp {
                        dst: old_reg,
                        object: obj_reg,
                        prop: prop_const,
                    },
                    span,
                );
                let new_reg = b.alloc_temp(RhpType::Int, span)?;
                b.emit(
                    OpCode::IncDec {
                        dst: new_reg,
                        src: old_reg,
                        is_inc,
                    },
                    span,
                );
                b.emit(
                    OpCode::SetProp {
                        object: obj_reg,
                        prop: prop_const,
                        value: new_reg,
                    },
                    span,
                );
                let res = if is_prefix { new_reg } else { old_reg };
                if let Some(dst) = target_reg {
                    if dst != res {
                        b.emit(OpCode::Move { dst, src: res }, span);
                    }
                    Ok(dst)
                } else {
                    Ok(res)
                }
            }

            ExprKind::ArrayIndex {
                array,
                index: Some(idx_expr),
            } => {
                let arr_reg = self.lower_expr(b, array, None)?;
                let idx_reg = self.lower_expr(b, idx_expr, None)?;
                let old_reg = b.alloc_temp(RhpType::Int, span)?;
                b.emit(
                    OpCode::ArrayGet {
                        dst: old_reg,
                        array: arr_reg,
                        index: idx_reg,
                    },
                    span,
                );
                let new_reg = b.alloc_temp(RhpType::Int, span)?;
                b.emit(
                    OpCode::IncDec {
                        dst: new_reg,
                        src: old_reg,
                        is_inc,
                    },
                    span,
                );
                b.emit(
                    OpCode::ArraySet {
                        array: arr_reg,
                        key: idx_reg,
                        value: new_reg,
                    },
                    span,
                );
                let res = if is_prefix { new_reg } else { old_reg };
                if let Some(dst) = target_reg {
                    if dst != res {
                        b.emit(OpCode::Move { dst, src: res }, span);
                    }
                    Ok(dst)
                } else {
                    Ok(res)
                }
            }

            _ => Err(IrError::new(
                IrErrorKind::InvalidAssignmentTarget {
                    reason: "increment/decrement target must be a variable, property, or array element".to_string(),
                },
                span,
            )),
        }
    }

    fn lower_assignment(
        &mut self,
        b: &mut ChunkBuilder,
        op: AssignOp,
        target: &Expr<'_>,
        value: &Expr<'_>,
        target_reg: Option<Reg>,
        span: Span,
    ) -> Result<Reg, IrError> {
        match &target.kind {
            // 1. Variable assignment (`$x = 10`, `$x += 5`, `$x ??= 'default'`)
            ExprKind::Variable(var) => {
                let var_reg = b.get_or_alloc_local(var.name, None, false, false, target.span)?;
                let locked_type = b
                    .chunk
                    .register_info
                    .get(var_reg.index())
                    .filter(|info| info.is_locked_type)
                    .map(|info| info.rhp_type.clone());

                match op {
                    AssignOp::Assign => {
                        let col_kind = match &locked_type {
                            Some(RhpType::Collection(CollectionSubtype::List(_))) => {
                                Some(IrCollectionKind::List)
                            }
                            Some(RhpType::Collection(CollectionSubtype::Dict(_))) => {
                                Some(IrCollectionKind::Dict)
                            }
                            Some(RhpType::Collection(CollectionSubtype::Set(_))) => {
                                Some(IrCollectionKind::Set)
                            }
                            Some(RhpType::Collection(CollectionSubtype::HashMap(_, _))) => {
                                Some(IrCollectionKind::HashMap)
                            }
                            _ => None,
                        };

                        if let (Some(kind), ExprKind::Array(_)) = (col_kind, &value.kind) {
                            let arr_reg = self.lower_expr(b, value, None)?;
                            b.emit(
                                OpCode::NewCollection {
                                    dst: var_reg,
                                    kind,
                                    source: Some(arr_reg),
                                },
                                span,
                            );
                        } else {
                            self.lower_expr(b, value, Some(var_reg))?;
                        }
                    }
                    AssignOp::CoalesceAssign => {
                        let skip_jmp = b.emit(
                            OpCode::JumpIfNotNull {
                                cond: var_reg,
                                target: 0,
                            },
                            span,
                        );
                        self.lower_expr(b, value, Some(var_reg))?;
                        let end_ip = b.current_ip();
                        b.patch_jump(skip_jmp, end_ip);
                    }
                    compound => {
                        let bin_op = assign_op_to_binary_op(compound);
                        let rhs_reg = self.lower_expr(b, value, None)?;
                        b.emit(
                            OpCode::Binary {
                                dst: var_reg,
                                op: bin_op,
                                lhs: var_reg,
                                rhs: rhs_reg,
                            },
                            span,
                        );
                    }
                }

                // If the variable has a locked type annotation, emit AssertType

                if let Some(ty) = locked_type {
                    if !matches!(ty, RhpType::Mixed | RhpType::Dynamic) {
                        let type_const = b.intern_type(ty);
                        b.emit(
                            OpCode::AssertType {
                                reg: var_reg,
                                type_const,
                            },
                            span,
                        );
                    }
                }

                if b.globals.contains(&normalize_var_name(var.name)) {
                    let name_const = b.intern_string(var.bare_name());
                    b.emit(
                        OpCode::StoreGlobal {
                            name: name_const,
                            src: var_reg,
                        },
                        span,
                    );
                }

                if let Some(dst) = target_reg {
                    if dst != var_reg {
                        b.emit(OpCode::Move { dst, src: var_reg }, span);
                    }
                    Ok(dst)
                } else {
                    Ok(var_reg)
                }
            }

            // 2. Destructuring assignment (`[$user, $posts] = Future::wait(...)`)
            ExprKind::List(_) | ExprKind::Array(_) => {
                let rhs_reg = self.lower_expr(b, value, None)?;
                self.lower_destructuring_assignment(b, target, rhs_reg, span)?;
                if let Some(dst) = target_reg {
                    if dst != rhs_reg {
                        b.emit(OpCode::Move { dst, src: rhs_reg }, span);
                    }
                    Ok(dst)
                } else {
                    Ok(rhs_reg)
                }
            }

            // 3. Instance property assignment (`$this->name = $name`, `$obj->count += 1`)
            ExprKind::PropertyFetch {
                object, property, ..
            } => {
                let obj_reg = self.lower_expr(b, object, None)?;
                let prop_const = b.intern_string(property.name);

                let final_val_reg = match op {
                    AssignOp::Assign => self.lower_expr(b, value, None)?,
                    AssignOp::CoalesceAssign => {
                        let cur_reg = b.alloc_temp(RhpType::Dynamic, span)?;
                        b.emit(
                            OpCode::GetProp {
                                dst: cur_reg,
                                object: obj_reg,
                                prop: prop_const,
                            },
                            span,
                        );
                        let skip_jmp = b.emit(
                            OpCode::JumpIfNotNull {
                                cond: cur_reg,
                                target: 0,
                            },
                            span,
                        );
                        self.lower_expr(b, value, Some(cur_reg))?;
                        b.emit(
                            OpCode::SetProp {
                                object: obj_reg,
                                prop: prop_const,
                                value: cur_reg,
                            },
                            span,
                        );
                        let end_ip = b.current_ip();
                        b.patch_jump(skip_jmp, end_ip);
                        return Ok(cur_reg);
                    }
                    compound => {
                        let bin_op = assign_op_to_binary_op(compound);
                        let cur_reg = b.alloc_temp(RhpType::Dynamic, span)?;
                        b.emit(
                            OpCode::GetProp {
                                dst: cur_reg,
                                object: obj_reg,
                                prop: prop_const,
                            },
                            span,
                        );
                        let rhs_reg = self.lower_expr(b, value, None)?;
                        b.emit(
                            OpCode::Binary {
                                dst: cur_reg,
                                op: bin_op,
                                lhs: cur_reg,
                                rhs: rhs_reg,
                            },
                            span,
                        );
                        cur_reg
                    }
                };

                b.emit(
                    OpCode::SetProp {
                        object: obj_reg,
                        prop: prop_const,
                        value: final_val_reg,
                    },
                    span,
                );
                if let Some(dst) = target_reg {
                    if dst != final_val_reg {
                        b.emit(
                            OpCode::Move {
                                dst,
                                src: final_val_reg,
                            },
                            span,
                        );
                    }
                    Ok(dst)
                } else {
                    Ok(final_val_reg)
                }
            }

            // 4. Static property assignment (`User::$count = 1`, `User::$count += 1`)
            ExprKind::StaticPropertyFetch { class, property } => {
                let resolved_class =
                    self.resolve_class_name(class.name, b.enclosing_class.as_deref());
                let class_const = b.intern_string(&resolved_class);
                let prop_const = b.intern_string(property.bare_name());

                let final_val_reg = match op {
                    AssignOp::Assign => self.lower_expr(b, value, None)?,
                    compound => {
                        let bin_op = assign_op_to_binary_op(compound);
                        let cur_reg = b.alloc_temp(RhpType::Dynamic, span)?;
                        b.emit(
                            OpCode::GetStaticProp {
                                dst: cur_reg,
                                class: class_const,
                                prop: prop_const,
                            },
                            span,
                        );
                        let rhs_reg = self.lower_expr(b, value, None)?;
                        b.emit(
                            OpCode::Binary {
                                dst: cur_reg,
                                op: bin_op,
                                lhs: cur_reg,
                                rhs: rhs_reg,
                            },
                            span,
                        );
                        cur_reg
                    }
                };

                b.emit(
                    OpCode::SetStaticProp {
                        class: class_const,
                        prop: prop_const,
                        value: final_val_reg,
                    },
                    span,
                );
                Ok(final_val_reg)
            }

            // 5. Array subscript assignment (`$arr[$i] = $val` or `$arr[] = $val`)
            ExprKind::ArrayIndex { array, index } => {
                let arr_reg = self.lower_expr(b, array, None)?;

                if let Some(idx_expr) = index {
                    let idx_reg = self.lower_expr(b, idx_expr, None)?;
                    let final_val_reg = match op {
                        AssignOp::Assign => self.lower_expr(b, value, None)?,
                        compound => {
                            let bin_op = assign_op_to_binary_op(compound);
                            let cur_reg = b.alloc_temp(RhpType::Dynamic, span)?;
                            b.emit(
                                OpCode::ArrayGet {
                                    dst: cur_reg,
                                    array: arr_reg,
                                    index: idx_reg,
                                },
                                span,
                            );
                            let rhs_reg = self.lower_expr(b, value, None)?;
                            b.emit(
                                OpCode::Binary {
                                    dst: cur_reg,
                                    op: bin_op,
                                    lhs: cur_reg,
                                    rhs: rhs_reg,
                                },
                                span,
                            );
                            cur_reg
                        }
                    };
                    b.emit(
                        OpCode::ArraySet {
                            array: arr_reg,
                            key: idx_reg,
                            value: final_val_reg,
                        },
                        span,
                    );
                    Ok(final_val_reg)
                } else {
                    // `$arr[] = $val`
                    let val_reg = self.lower_expr(b, value, None)?;
                    b.emit(
                        OpCode::ArrayPush {
                            array: arr_reg,
                            value: val_reg,
                        },
                        span,
                    );
                    Ok(val_reg)
                }
            }

            _ => Err(IrError::new(
                IrErrorKind::InvalidAssignmentTarget {
                    reason: "unsupported left-hand side in assignment".to_string(),
                },
                span,
            )),
        }
    }

    fn lower_destructuring_assignment(
        &mut self,
        b: &mut ChunkBuilder,
        target: &Expr<'_>,
        rhs_reg: Reg,
        span: Span,
    ) -> Result<(), IrError> {
        match &target.kind {
            ExprKind::List(items) => {
                for (idx, item_opt) in items.iter().enumerate() {
                    if let Some(elem) = item_opt {
                        self.lower_destructuring_element(b, elem, idx, rhs_reg, span)?;
                    }
                }
                Ok(())
            }
            ExprKind::Array(items) => {
                for (idx, elem) in items.iter().enumerate() {
                    self.lower_destructuring_element(b, elem, idx, rhs_reg, span)?;
                }
                Ok(())
            }
            _ => Err(IrError::new(
                IrErrorKind::InvalidAssignmentTarget {
                    reason: "invalid destructuring target".to_string(),
                },
                span,
            )),
        }
    }

    fn lower_destructuring_element(
        &mut self,
        b: &mut ChunkBuilder,
        elem: &ArrayElement<'_>,
        positional_index: usize,
        rhs_reg: Reg,
        span: Span,
    ) -> Result<(), IrError> {
        let key_reg = if let Some(ref key_expr) = elem.key {
            self.lower_expr(b, key_expr, None)?
        } else {
            let r = b.alloc_temp(RhpType::Int, elem.span)?;
            b.emit(
                OpCode::LoadInt {
                    dst: r,
                    value: positional_index as i32,
                },
                elem.span,
            );
            r
        };

        match &elem.value.kind {
            ExprKind::Variable(var) => {
                let dst = b.get_or_alloc_local(var.name, None, false, false, elem.span)?;
                b.emit(
                    OpCode::ArrayGet {
                        dst,
                        array: rhs_reg,
                        index: key_reg,
                    },
                    elem.span,
                );
                Ok(())
            }
            ExprKind::List(_) | ExprKind::Array(_) => {
                let sub_rhs = b.alloc_temp(RhpType::Dynamic, elem.span)?;
                b.emit(
                    OpCode::ArrayGet {
                        dst: sub_rhs,
                        array: rhs_reg,
                        index: key_reg,
                    },
                    elem.span,
                );
                self.lower_destructuring_assignment(b, &elem.value, sub_rhs, span)
            }
            _ => Err(IrError::new(
                IrErrorKind::InvalidAssignmentTarget {
                    reason: "destructuring element must be a variable or nested list".to_string(),
                },
                elem.span,
            )),
        }
    }

    // =========================================================================
    // Function, Closure, Arrow Function, & Class Declaration Lowering
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    fn compile_function_decl(
        &mut self,
        func: &FunctionDecl<'_>,
        callable_key: &str,
        enclosing_class: Option<String>,
        visibility: Visibility,
        is_static: bool,
        is_abstract: bool,
        is_final: bool,
    ) -> Result<FunctionId, IrError> {
        let return_type = func
            .return_type
            .as_ref()
            .and_then(|th| RhpType::from_type_hint(th, enclosing_class.as_deref()).ok())
            .unwrap_or(RhpType::Void);

        let body_inferences = self
            .type_info
            .and_then(|ti| ti.function_body_inferences.get(callable_key).cloned())
            .unwrap_or_default();

        let mut fb = ChunkBuilder::new(
            callable_key,
            Some(return_type.clone()),
            enclosing_class.clone(),
            body_inferences,
        );

        // For instance methods, reserve register `r0` for `$this`
        let mut this_reg_opt = None;
        if let Some(ref cls_name) = enclosing_class {
            if !is_static {
                let this_reg = fb.get_or_alloc_local(
                    "$this",
                    Some(RhpType::Class(cls_name.clone())),
                    true,
                    true,
                    func.span,
                )?;
                this_reg_opt = Some(this_reg);
            }
        }

        // Allocate parameter registers in declaration order
        let mut compiled_params = Vec::with_capacity(func.params.len());
        for param in &func.params {
            let param_ty = param
                .type_hint
                .as_ref()
                .and_then(|th| RhpType::from_type_hint(th, enclosing_class.as_deref()).ok())
                .unwrap_or(RhpType::Dynamic);

            let reg = fb.get_or_alloc_local(
                param.name.name,
                Some(param_ty.clone()),
                true,
                param.type_hint.is_some(),
                param.span,
            )?;

            compiled_params.push(CompiledParam {
                name: normalize_var_name(param.name.name),
                reg,
                rhp_type: param_ty,
                has_default: param.default_value.is_some(),
                is_variadic: param.is_variadic,
                is_readonly: param.is_readonly,
                visibility: param.visibility,
                span: param.span,
            });
        }

        if let Some(ref body_stmts) = func.body {
            // 1. Emit default parameter initialization prologue
            for (param_ast, compiled_param) in func.params.iter().zip(compiled_params.iter()) {
                if let Some(ref def_expr) = param_ast.default_value {
                    let skip_jmp = fb.emit(
                        OpCode::JumpIfNotNull {
                            cond: compiled_param.reg,
                            target: 0,
                        },
                        param_ast.span,
                    );
                    self.lower_expr(&mut fb, def_expr, Some(compiled_param.reg))?;
                    let after_def = fb.current_ip();
                    fb.patch_jump(skip_jmp, after_def);
                }

                // 2. Emit boundary parameter type contract assertion
                if !matches!(compiled_param.rhp_type, RhpType::Mixed | RhpType::Dynamic) {
                    let type_const = fb.intern_type(compiled_param.rhp_type.clone());
                    fb.emit(
                        OpCode::AssertType {
                            reg: compiled_param.reg,
                            type_const,
                        },
                        param_ast.span,
                    );
                }

                // 3. Constructor property promotion (`public readonly int $id` in `__construct`)
                if func.name.name == "__construct" && param_ast.visibility.is_some() {
                    if let Some(this_reg) = this_reg_opt {
                        let prop_const = fb.intern_string(param_ast.name.bare_name());
                        fb.emit(
                            OpCode::SetProp {
                                object: this_reg,
                                prop: prop_const,
                                value: compiled_param.reg,
                            },
                            param_ast.span,
                        );
                    }
                }
            }

            // 4. Lower function body statements
            for s in body_stmts {
                self.lower_stmt(&mut fb, s)?;
            }

            // 5. Ensure trailing implicit `Return` for `void` functions/constructors
            let needs_terminal_return = fb
                .chunk
                .instructions
                .last()
                .map(|op| !matches!(op, OpCode::Return { .. } | OpCode::Throw { .. }))
                .unwrap_or(true);

            if needs_terminal_return {
                fb.emit(OpCode::Return { value: None }, func.span);
            }
        }

        let func_id = FunctionId(self.module.functions.len() as u32);
        self.module.functions.push(CompiledFunction {
            id: func_id,
            name: callable_key.to_string(),
            params: compiled_params,
            return_type,
            upvalues: Vec::new(),
            is_async: func.is_async,
            is_static,
            is_abstract,
            is_final,
            is_closure: false,
            visibility,
            enclosing_class,
            chunk: fb.chunk,
            span: func.span,
        });

        Ok(func_id)
    }

    fn compile_arrow_function(
        &mut self,
        parent: &ChunkBuilder,
        params: &[Param<'_>],
        return_type_hint: Option<&TypeHint<'_>>,
        body: &Expr<'_>,
        span: Span,
    ) -> Result<(FunctionId, Vec<String>), IrError> {
        let arrow_name = format!("<arrow#{}>", self.synthetic_counter);
        self.synthetic_counter += 1;

        let return_type = return_type_hint
            .and_then(|th| RhpType::from_type_hint(th, parent.enclosing_class.as_deref()).ok())
            .unwrap_or(RhpType::Dynamic);

        let mut fb = ChunkBuilder::new(
            &arrow_name,
            Some(return_type.clone()),
            parent.enclosing_class.clone(),
            parent.inferred_var_types.clone(),
        );

        let mut param_names = HashSet::new();
        let mut compiled_params = Vec::with_capacity(params.len());
        for param in params {
            let norm = normalize_var_name(param.name.name);
            param_names.insert(norm.clone());
            let param_ty = param
                .type_hint
                .as_ref()
                .and_then(|th| RhpType::from_type_hint(th, parent.enclosing_class.as_deref()).ok())
                .unwrap_or(RhpType::Dynamic);
            let reg = fb.get_or_alloc_local(
                param.name.name,
                Some(param_ty.clone()),
                true,
                param.type_hint.is_some(),
                param.span,
            )?;
            compiled_params.push(CompiledParam {
                name: norm,
                reg,
                rhp_type: param_ty,
                has_default: param.default_value.is_some(),
                is_variadic: param.is_variadic,
                is_readonly: param.is_readonly,
                visibility: param.visibility,
                span: param.span,
            });
        }

        // 1. Emit default parameter initialization prologue & type assertions
        for (param_ast, compiled_param) in params.iter().zip(compiled_params.iter()) {
            if let Some(ref def_expr) = param_ast.default_value {
                let skip_jmp = fb.emit(
                    OpCode::JumpIfNotNull {
                        cond: compiled_param.reg,
                        target: 0,
                    },
                    param_ast.span,
                );
                self.lower_expr(&mut fb, def_expr, Some(compiled_param.reg))?;
                let after_def = fb.current_ip();
                fb.patch_jump(skip_jmp, after_def);
            }

            if !matches!(compiled_param.rhp_type, RhpType::Mixed | RhpType::Dynamic) {
                let type_const = fb.intern_type(compiled_param.rhp_type.clone());
                fb.emit(
                    OpCode::AssertType {
                        reg: compiled_param.reg,
                        type_const,
                    },
                    param_ast.span,
                );
            }
        }

        // Automatically detect outer variables referenced in the arrow function body
        let mut referenced_vars = Vec::new();
        collect_expr_variables(body, &mut referenced_vars);

        let mut upvalues = Vec::new();
        for var_name in referenced_vars {
            if !param_names.contains(&var_name)
                && parent.chunk.local_map.contains_key(&var_name)
                && !upvalues.contains(&var_name)
            {
                fb.get_or_alloc_local(&var_name, None, false, false, span)?;
                upvalues.push(var_name);
            }
        }

        let ret_reg = self.lower_expr(&mut fb, body, None)?;
        if !matches!(
            return_type,
            RhpType::Void | RhpType::Mixed | RhpType::Dynamic | RhpType::Never
        ) {
            let type_const = fb.intern_type(return_type.clone());
            fb.emit(
                OpCode::AssertType {
                    reg: ret_reg,
                    type_const,
                },
                span,
            );
        }
        fb.emit(
            OpCode::Return {
                value: Some(ret_reg),
            },
            span,
        );

        let func_id = FunctionId(self.module.functions.len() as u32);
        self.module.functions.push(CompiledFunction {
            id: func_id,
            name: arrow_name,
            params: compiled_params,
            return_type,
            upvalues: upvalues.clone(),
            is_async: false,
            is_static: false,
            is_abstract: false,
            is_final: false,
            is_closure: true,
            visibility: Visibility::Public,
            enclosing_class: parent.enclosing_class.clone(),
            chunk: fb.chunk,
            span,
        });

        Ok((func_id, upvalues))
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_closure(
        &mut self,
        parent: &ChunkBuilder,
        is_static: bool,
        params: &[Param<'_>],
        upvalues: &[String],
        return_type_hint: Option<&TypeHint<'_>>,
        body: &[Stmt<'_>],
        span: Span,
    ) -> Result<FunctionId, IrError> {
        let closure_name = format!("<closure#{}>", self.synthetic_counter);
        self.synthetic_counter += 1;

        let return_type = return_type_hint
            .and_then(|th| RhpType::from_type_hint(th, parent.enclosing_class.as_deref()).ok())
            .unwrap_or(RhpType::Dynamic);

        let mut fb = ChunkBuilder::new(
            &closure_name,
            Some(return_type.clone()),
            if is_static {
                None
            } else {
                parent.enclosing_class.clone()
            },
            parent.inferred_var_types.clone(),
        );

        // For non-static closures, reserve r0 for `$this` to support instance method receivers and dynamic macros
        if !is_static {
            let _ = fb.get_or_alloc_local("$this", None, false, false, span);
        }

        let mut compiled_params = Vec::with_capacity(params.len());
        for param in params {
            let param_ty = param
                .type_hint
                .as_ref()
                .and_then(|th| RhpType::from_type_hint(th, parent.enclosing_class.as_deref()).ok())
                .unwrap_or(RhpType::Dynamic);
            let reg = fb.get_or_alloc_local(
                param.name.name,
                Some(param_ty.clone()),
                true,
                param.type_hint.is_some(),
                param.span,
            )?;
            compiled_params.push(CompiledParam {
                name: normalize_var_name(param.name.name),
                reg,
                rhp_type: param_ty,
                has_default: param.default_value.is_some(),
                is_variadic: param.is_variadic,
                is_readonly: param.is_readonly,
                visibility: param.visibility,
                span: param.span,
            });
        }

        // 1. Emit default parameter initialization prologue & type assertions
        for (param_ast, compiled_param) in params.iter().zip(compiled_params.iter()) {
            if let Some(ref def_expr) = param_ast.default_value {
                let skip_jmp = fb.emit(
                    OpCode::JumpIfNotNull {
                        cond: compiled_param.reg,
                        target: 0,
                    },
                    param_ast.span,
                );
                self.lower_expr(&mut fb, def_expr, Some(compiled_param.reg))?;
                let after_def = fb.current_ip();
                fb.patch_jump(skip_jmp, after_def);
            }

            if !matches!(compiled_param.rhp_type, RhpType::Mixed | RhpType::Dynamic) {
                let type_const = fb.intern_type(compiled_param.rhp_type.clone());
                fb.emit(
                    OpCode::AssertType {
                        reg: compiled_param.reg,
                        type_const,
                    },
                    param_ast.span,
                );
            }
        }

        // Pre-allocate registers for captured `use ($var)` bindings
        for upval in upvalues {
            fb.get_or_alloc_local(upval, None, false, false, span)?;
        }

        for s in body {
            self.lower_stmt(&mut fb, s)?;
        }

        let needs_terminal_return = fb
            .chunk
            .instructions
            .last()
            .map(|op| !matches!(op, OpCode::Return { .. } | OpCode::Throw { .. }))
            .unwrap_or(true);
        if needs_terminal_return {
            fb.emit(OpCode::Return { value: None }, span);
        }

        let func_id = FunctionId(self.module.functions.len() as u32);
        self.module.functions.push(CompiledFunction {
            id: func_id,
            name: closure_name,
            params: compiled_params,
            return_type,
            upvalues: upvalues.to_vec(),
            is_async: false,
            is_static,
            is_abstract: false,
            is_final: false,
            is_closure: true,
            visibility: Visibility::Public,
            enclosing_class: if is_static {
                None
            } else {
                parent.enclosing_class.clone()
            },
            chunk: fb.chunk,
            span,
        });

        Ok(func_id)
    }

    fn compile_class_decl(&mut self, cls: &ClassDecl<'_>) -> Result<ClassId, IrError> {
        let class_name = cls.name.name.to_string();
        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = HashMap::new();
        let mut traits = Vec::new();

        for member in &cls.members {
            match &member.kind {
                ClassMemberKind::TraitUse(tu) => {
                    for tr in &tu.traits {
                        traits.push(tr.name.to_string());
                    }
                }
                ClassMemberKind::Property(prop) => {
                    let rhp_type = prop
                        .type_hint
                        .as_ref()
                        .and_then(|th| RhpType::from_type_hint(th, Some(&class_name)).ok())
                        .unwrap_or(RhpType::Dynamic);
                    let default_chunk = if let Some(ref def_expr) = prop.default {
                        Some(self.compile_initializer_expr(&class_name, def_expr)?)
                    } else {
                        None
                    };
                    properties.push(CompiledProperty {
                        name: prop.name.bare_name().to_string(),
                        rhp_type,
                        visibility: prop.visibility,
                        is_static: prop.is_static,
                        is_readonly: prop.is_readonly || cls.modifiers.is_readonly,
                        default_chunk,
                        span: member.span,
                    });
                }
                ClassMemberKind::Constant(c) => {
                    let value_chunk = self.compile_initializer_expr(&class_name, &c.value)?;
                    constants.push(CompiledClassConst {
                        name: c.name.name.to_string(),
                        visibility: c.visibility,
                        is_final: c.is_final,
                        value_chunk,
                        span: member.span,
                    });
                }
                ClassMemberKind::Method(m) => {
                    // Register promoted constructor properties on the class descriptor
                    if m.function.name.name == "__construct" {
                        for param in &m.function.params {
                            if let Some(vis) = param.visibility {
                                let prop_name = param.name.bare_name().to_string();
                                if !properties.iter().any(|p| p.name == prop_name) {
                                    let rhp_type = param
                                        .type_hint
                                        .as_ref()
                                        .and_then(|th| {
                                            RhpType::from_type_hint(th, Some(&class_name)).ok()
                                        })
                                        .unwrap_or(RhpType::Dynamic);
                                    properties.push(CompiledProperty {
                                        name: prop_name,
                                        rhp_type,
                                        visibility: vis,
                                        is_static: false,
                                        is_readonly: param.is_readonly || cls.modifiers.is_readonly,
                                        default_chunk: None,
                                        span: param.span,
                                    });
                                }
                            }
                        }
                    }

                    let method_key = format!("{}::{}", class_name, m.function.name.name);
                    let func_id = self.compile_function_decl(
                        &m.function,
                        &method_key,
                        Some(class_name.clone()),
                        m.visibility,
                        m.is_static,
                        m.is_abstract,
                        m.is_final,
                    )?;
                    self.module.function_index.insert(method_key, func_id);
                    methods.insert(m.function.name.name.to_string(), func_id);
                }
            }
        }

        let class_id = ClassId(self.module.classes.len() as u32);
        self.module.classes.push(CompiledClass {
            id: class_id,
            name: class_name.clone(),
            is_trait: false,
            modifiers: cls.modifiers,
            extends: cls.extends.as_ref().map(|e| e.name.to_string()),
            implements: cls.implements.iter().map(|i| i.name.to_string()).collect(),
            traits,
            properties,
            constants,
            methods,
            span: cls.span,
        });
        self.module.class_index.insert(class_name, class_id);
        Ok(class_id)
    }

    fn compile_trait_decl(&mut self, tr: &TraitDecl<'_>) -> Result<ClassId, IrError> {
        let trait_name = tr.name.name.to_string();
        let mut properties = Vec::new();
        let mut constants = Vec::new();
        let mut methods = HashMap::new();
        let mut traits = Vec::new();

        for member in &tr.members {
            match &member.kind {
                ClassMemberKind::TraitUse(tu) => {
                    for sub_tr in &tu.traits {
                        traits.push(sub_tr.name.to_string());
                    }
                }
                ClassMemberKind::Property(prop) => {
                    let rhp_type = prop
                        .type_hint
                        .as_ref()
                        .and_then(|th| RhpType::from_type_hint(th, Some(&trait_name)).ok())
                        .unwrap_or(RhpType::Dynamic);
                    let default_chunk = if let Some(ref def_expr) = prop.default {
                        Some(self.compile_initializer_expr(&trait_name, def_expr)?)
                    } else {
                        None
                    };
                    properties.push(CompiledProperty {
                        name: prop.name.bare_name().to_string(),
                        rhp_type,
                        visibility: prop.visibility,
                        is_static: prop.is_static,
                        is_readonly: prop.is_readonly,
                        default_chunk,
                        span: member.span,
                    });
                }
                ClassMemberKind::Constant(c) => {
                    let value_chunk = self.compile_initializer_expr(&trait_name, &c.value)?;
                    constants.push(CompiledClassConst {
                        name: c.name.name.to_string(),
                        visibility: c.visibility,
                        is_final: c.is_final,
                        value_chunk,
                        span: member.span,
                    });
                }
                ClassMemberKind::Method(m) => {
                    let method_key = format!("{}::{}", trait_name, m.function.name.name);
                    let func_id = self.compile_function_decl(
                        &m.function,
                        &method_key,
                        Some(trait_name.clone()),
                        m.visibility,
                        m.is_static,
                        m.is_abstract,
                        m.is_final,
                    )?;
                    self.module.function_index.insert(method_key, func_id);
                    methods.insert(m.function.name.name.to_string(), func_id);
                }
            }
        }

        let class_id = ClassId(self.module.classes.len() as u32);
        self.module.classes.push(CompiledClass {
            id: class_id,
            name: trait_name.clone(),
            is_trait: true,
            modifiers: Default::default(),
            extends: None,
            implements: Vec::new(),
            traits,
            properties,
            constants,
            methods,
            span: tr.span,
        });
        self.module.class_index.insert(trait_name, class_id);
        Ok(class_id)
    }

    fn compile_interface_decl(&mut self, iface: &InterfaceDecl<'_>) -> Result<(), IrError> {
        let iface_name = iface.name.name.to_string();
        let mut methods = HashMap::new();
        for m in &iface.methods {
            let method_key = format!("{}::{}", iface_name, m.function.name.name);
            let func_id = self.compile_function_decl(
                &m.function,
                &method_key,
                Some(iface_name.clone()),
                m.visibility,
                m.is_static,
                true,
                m.is_final,
            )?;
            self.module.function_index.insert(method_key, func_id);
            methods.insert(m.function.name.name.to_string(), func_id);
        }

        self.module.interfaces.insert(
            iface_name.clone(),
            CompiledInterface {
                name: iface_name,
                extends: iface.extends.iter().map(|e| e.name.to_string()).collect(),
                methods,
                span: iface.span,
            },
        );
        Ok(())
    }

    fn compile_enum_decl(&mut self, enm: &EnumDecl<'_>) -> Result<(), IrError> {
        let enum_name = enm.name.name.to_string();
        let backing_type = enm
            .backing_type
            .as_ref()
            .and_then(|th| RhpType::from_type_hint(th, Some(&enum_name)).ok());

        let mut cases = Vec::with_capacity(enm.cases.len());
        for c in &enm.cases {
            let value_chunk = if let Some(ref val_expr) = c.value {
                Some(self.compile_initializer_expr(&enum_name, val_expr)?)
            } else {
                None
            };
            cases.push(CompiledEnumCase {
                name: c.name.name.to_string(),
                value_chunk,
                span: c.span,
            });
        }

        let mut methods = HashMap::new();
        for m in &enm.methods {
            let method_key = format!("{}::{}", enum_name, m.function.name.name);
            let func_id = self.compile_function_decl(
                &m.function,
                &method_key,
                Some(enum_name.clone()),
                m.visibility,
                m.is_static,
                m.is_abstract,
                m.is_final,
            )?;
            self.module.function_index.insert(method_key, func_id);
            methods.insert(m.function.name.name.to_string(), func_id);
        }

        self.module.enums.insert(
            enum_name.clone(),
            CompiledEnum {
                name: enum_name,
                backing_type,
                implements: enm.implements.iter().map(|i| i.name.to_string()).collect(),
                cases,
                methods,
                span: enm.span,
            },
        );
        Ok(())
    }

    fn compile_initializer_expr(
        &mut self,
        enclosing_class: &str,
        expr: &Expr<'_>,
    ) -> Result<BytecodeChunk, IrError> {
        let mut b = ChunkBuilder::new(
            format!("{}::<init>", enclosing_class),
            None,
            Some(enclosing_class.to_string()),
            HashMap::new(),
        );
        let reg = self.lower_expr(&mut b, expr, None)?;
        b.emit(OpCode::Return { value: Some(reg) }, expr.span);
        Ok(b.chunk)
    }

    fn resolve_class_name(&self, raw_name: &str, enclosing_class: Option<&str>) -> String {
        match raw_name {
            "self" | "static" => enclosing_class.unwrap_or(raw_name).to_string(),
            "parent" => {
                if let Some(cls) = enclosing_class {
                    if let Some(ti) = self.type_info {
                        if let Some(info) = ti.hierarchy.classes.get(cls) {
                            if let Some(ref p) = info.extends {
                                return p.clone();
                            }
                        }
                    }
                }
                "parent".to_string()
            }
            other => {
                if let Some(aliased) = self.use_aliases.get(other) {
                    aliased.clone()
                } else {
                    other.to_string()
                }
            }
        }
    }
}

#[inline]
fn normalize_var_name(name: &str) -> String {
    if name.starts_with('$') {
        name.to_string()
    } else {
        format!("${}", name)
    }
}

#[inline]
fn assign_op_to_binary_op(op: AssignOp) -> BinaryOp {
    match op {
        AssignOp::AddAssign => BinaryOp::Add,
        AssignOp::SubAssign => BinaryOp::Sub,
        AssignOp::MulAssign => BinaryOp::Mul,
        AssignOp::DivAssign => BinaryOp::Div,
        AssignOp::ModAssign => BinaryOp::Mod,
        AssignOp::PowAssign => BinaryOp::Pow,
        AssignOp::ConcatAssign => BinaryOp::Concat,
        AssignOp::BitAndAssign => BinaryOp::BitAnd,
        AssignOp::BitOrAssign => BinaryOp::BitOr,
        AssignOp::BitXorAssign => BinaryOp::BitXor,
        AssignOp::ShlAssign => BinaryOp::Shl,
        AssignOp::ShrAssign => BinaryOp::Shr,
        AssignOp::Assign | AssignOp::CoalesceAssign => unreachable!(),
    }
}

/// Recursively collect variable names referenced inside an expression (used for automatic arrow function capture).
fn collect_expr_variables(expr: &Expr<'_>, out: &mut Vec<String>) {
    match &expr.kind {
        ExprKind::Variable(v) => {
            out.push(normalize_var_name(v.name));
        }
        ExprKind::Literal(Literal::InterpolatedString { parts, .. }) => {
            for part in parts {
                if let StringPart::Expr(sub) = part {
                    collect_expr_variables(sub, out);
                }
            }
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::Clone(inner)
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::Await(inner)
        | ExprKind::Grouping(inner) => {
            collect_expr_variables(inner, out);
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::Assign {
            target: left,
            value: right,
            ..
        }
        | ExprKind::Instanceof {
            expr: left,
            class: right,
        } => {
            collect_expr_variables(left, out);
            collect_expr_variables(right, out);
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            collect_expr_variables(condition, out);
            if let Some(t) = if_true {
                collect_expr_variables(t, out);
            }
            collect_expr_variables(if_false, out);
        }
        ExprKind::Call { callee, args } => {
            collect_expr_variables(callee, out);
            for arg in args {
                collect_expr_variables(&arg.value, out);
            }
        }
        ExprKind::MethodCall { object, args, .. } => {
            collect_expr_variables(object, out);
            for arg in args {
                collect_expr_variables(&arg.value, out);
            }
        }
        ExprKind::StaticCall { target, args, .. } => {
            collect_expr_variables(target, out);
            for arg in args {
                collect_expr_variables(&arg.value, out);
            }
        }
        ExprKind::PropertyFetch { object, .. } => {
            collect_expr_variables(object, out);
        }
        ExprKind::Array(elems) => {
            for elem in elems {
                if let Some(ref k) = elem.key {
                    collect_expr_variables(k, out);
                }
                collect_expr_variables(&elem.value, out);
            }
        }
        ExprKind::ArrayIndex { array, index } => {
            collect_expr_variables(array, out);
            if let Some(idx) = index {
                collect_expr_variables(idx, out);
            }
        }
        ExprKind::ArrowFunction { params, body, .. } => {
            let mut sub_vars = Vec::new();
            collect_expr_variables(body, &mut sub_vars);
            let local_params: HashSet<String> =
                params.iter().map(|p| normalize_var_name(p.name.name)).collect();
            for v in sub_vars {
                if !local_params.contains(&v) {
                    out.push(v);
                }
            }
        }
        ExprKind::Closure { uses, .. } => {
            for u in uses {
                out.push(normalize_var_name(u.var.name));
            }
        }
        ExprKind::Match { condition, arms } => {
            collect_expr_variables(condition, out);
            for arm in arms {
                for pat in &arm.patterns {
                    collect_expr_variables(pat, out);
                }
                collect_expr_variables(&arm.body, out);
            }
        }
        ExprKind::New { class, args } => {
            collect_expr_variables(class, out);
            for arg in args {
                collect_expr_variables(&arg.value, out);
            }
        }
        ExprKind::Yield { key, value, .. } => {
            if let Some(k) = key {
                collect_expr_variables(k, out);
            }
            if let Some(v) = value {
                collect_expr_variables(v, out);
            }
        }
        ExprKind::Literal(_)
        | ExprKind::Ident(_)
        | ExprKind::StaticPropertyFetch { .. }
        | ExprKind::ClassConstFetch { .. }
        | ExprKind::List(_) => {}
    }
}
