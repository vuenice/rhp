//! # RHP: High-Performance PHP-Family Programming Language Engine
//!
//! RHP implements a high-throughput runtime engine with dual execution modes:
//! - **Development Mode**: Direct AST tree-walking interpreter with sub-millisecond hot-reloading and exact source mapping.
//! - **Production Mode**: Linear IR opcode register VM with Cranelift JIT compilation.

pub mod ast;
pub mod ir;
pub mod jit;
pub mod lexer;
pub mod parser;
pub mod type_system;
pub mod vm;

pub use ast::{
    walk_expr, walk_program, walk_stmt, Argument, ArrayElement, AssignOp, AstVisitor, BinaryOp,
    ClassDecl, ClassMember, ClassMemberKind, ClassModifiers, ClosureUse, ElseIfBranch, EnumDecl,
    Expr, ExprKind, FunctionDecl, Ident, InterfaceDecl, Literal, MatchArm, MethodDecl, Param,
    Program, PropertyDecl, Stmt, StmtKind, StringPart, SwitchCase, TraitDecl, TypeHint,
    TypeHintKind, UnaryOp, Variable, Visibility, ASTKind, ASTNode,
};
pub use ir::{
    compile_source, compile_source_with_file_id, compile_to_bytecode, lower_program,
    lower_typed_program, lower_typed_program_with_base, BytecodeChunk, CatchTarget, ClassId,
    CompilationError, CompiledClass, CompiledClassConst, CompiledEnum, CompiledEnumCase,
    CompiledFunction, CompiledInterface, CompiledParam, CompiledProperty, Constant, ConstantId,
    ExceptionHandler, FunctionId, IrCollectionKind, IrError, IrErrorKind, IrLowerer, IrModule,
    IrUnaryOp, OpCode, Reg, RegisterInfo,
};
pub use lexer::{
    error::{LexError, SpannedLexError},
    facade::{
        get_custom_helper, get_facades_by_tier, get_legacy_php_function, get_macro,
        get_static_facade, has_custom_helper, has_macro, is_legacy_php_function,
        is_native_or_plugin_facade, is_reserved_intrinsic, is_static_facade, is_static_only,
        php_function_signature_map, register_custom_helper, register_macro, register_tier2_facade,
        register_tier3_facade, validate_inheritance, validate_instantiation, FacadeInfo,
        FacadeRegistry, FacadeTier, LegacyPhpFunctionMapping, MacroEntry, MacroRegistry,
        LEGACY_PHP_FUNCTIONS, NATIVE_FACADES, RESERVED_INTRINSICS,
    },
    span::{SourceFile, SourceMap, Span},
    token::Token,
    Lexer, SpannedToken,
};
pub use parser::{
    error::{ParseError, ParseErrorKind},
    parse, parse_expr, parse_stmt, parse_with_file_id, CombinatorStream, Parser,
};
pub use type_system::{
    check_program, check_program_with_hierarchy, parse_and_check, parse_and_check_with_file_id,
    parse_and_check_with_hierarchy, ClassTypeInfo, CollectionSubtype, EnumTypeInfo, FrontendError,
    FunctionSignature, InterfaceTypeInfo, ParamSignature, PropertyTypeInfo, RhpType, RustFloatKind,
    RustIntKind, TypeCheckResult, TypeChecker, TypeEnvironment, TypeError, TypeErrorKind,
    TypeHierarchy, TypedProgram, VariableBinding,
};
pub use jit::{
    CompiledFunctionFn, CompiledLoopFn, JitCompiler, JitConfig, JitEngine, JitError, NativeFrame,
    NativeSlot, TAG_BOOL, TAG_FLOAT, TAG_INT, TAG_NULL, TAG_OTHER,
};
pub use vm::{
    append_log, clear_log_history, clear_runtime_closure_macros, clear_tier2_plugins,
    dispatch_facade_call, eval_expr, execute, extract_array_elements, extract_dict_elements,
    extract_dict_map, extract_map_elements, flatten_elements, get_recorded_logs,
    get_runtime_closure_macro, has_runtime_closure_macro, has_tier2_plugin,
    register_runtime_closure_macro, register_tier2_plugin, register_tier2_plugin_handler, run_expr,
    run_module, run_source, run_source_with_file_id, run_source_with_vm, CallFrame, EngineState,
    Environment, ExecutionError, FacadeResult, IndexMap, LogRecord, ParentProcess, PhpValue,
    PluginFacade, RunMode, RuntimeError, RuntimeErrorKind, StackTraceEntry, Tier2PluginHandler,
    Value, VmClosure, VmFuture, VmObject, DEFAULT_MAX_CALL_DEPTH, VM,
};
