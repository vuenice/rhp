use std::collections::HashMap;
use std::fmt;

use crate::ast::{expr::BinaryOp, types::CastType};
use crate::lexer::span::Span;
use crate::type_system::types::{RhpType, RustFloatKind, RustIntKind};

/// Virtual register identifier in the register-based Bytecode VM (`r0`..`r65535`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Reg(pub u16);

impl Reg {
    #[inline]
    pub const fn new(index: u16) -> Self {
        Self(index)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// Constant pool index within a [`BytecodeChunk`] (`c0`, `c1`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ConstantId(pub u32);

impl ConstantId {
    #[inline]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for ConstantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// Compiled function or closure index within an [`IrModule`] (`fn#0`, `fn#1`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FunctionId(pub u32);

impl FunctionId {
    #[inline]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn#{}", self.0)
    }
}

/// Compiled class descriptor index within an [`IrModule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ClassId(pub u32);

impl ClassId {
    #[inline]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for ClassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cls#{}", self.0)
    }
}

/// Explicit Rust-backed collection subtype selector (`Arr::list`, `Arr::dict`, `Arr::set`, `Arr::hashMap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IrCollectionKind {
    /// `Arr::list()` -> `Box<Vec<PhpValue>>`
    List,
    /// `Arr::dict()` -> `Box<IndexMap<String, PhpValue>>`
    Dict,
    /// `Arr::set()` -> `Box<IndexSet<PhpValue>>`
    Set,
    /// `Arr::hashMap()` -> `Box<HashMap<PhpValue, PhpValue>>`
    HashMap,
}

impl fmt::Display for IrCollectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List => write!(f, "Arr::list"),
            Self::Dict => write!(f, "Arr::dict"),
            Self::Set => write!(f, "Arr::set"),
            Self::HashMap => write!(f, "Arr::hashMap"),
        }
    }
}

/// Pure unary operators executed on a single source register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IrUnaryOp {
    /// Unary plus (`+$x`)
    Plus,
    /// Arithmetic negation (`-$x`)
    Neg,
    /// Logical negation (`!$x`)
    Not,
    /// Bitwise NOT (`~$x`)
    BitNot,
    /// Diagnostic suppression (`@$expr`)
    Silence,
}

impl fmt::Display for IrUnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plus => write!(f, "+"),
            Self::Neg => write!(f, "-"),
            Self::Not => write!(f, "!"),
            Self::BitNot => write!(f, "~"),
            Self::Silence => write!(f, "@"),
        }
    }
}

/// Constant pool entry stored inside a [`BytecodeChunk`].
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    /// `null` literal.
    Null,
    /// Boolean literal (`true` / `false`).
    Bool(bool),
    /// Standard 64-bit integer literal or wide integer constant.
    Int(i64),
    /// Standard 64-bit floating-point literal.
    Float(f64),
    /// Explicit native Rust integer value (`i8`..`u128`).
    RustInt { kind: RustIntKind, value: i128 },
    /// Explicit native Rust float value (`f32`, `f64`).
    RustFloat { kind: RustFloatKind, value: f64 },
    /// Interned UTF-8 string literal, identifier, or symbol name.
    String(String),
    /// Semantic RHP type contract used by runtime boundary assertions (`AssertType`).
    Type(RhpType),
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Int(n) => write!(f, "{}", n),
            Self::Float(fl) => write!(f, "{:?}", fl),
            Self::RustInt { kind, value } => write!(f, "{}_{}", value, kind),
            Self::RustFloat { kind, value } => write!(f, "{:?}_{}", value, kind),
            Self::String(s) => write!(f, "{:?}", s),
            Self::Type(ty) => write!(f, "<type:{}>", ty),
        }
    }
}

/// Flat, compact, `Copy`-able linear register-based bytecode instruction.
///
/// Every instruction is self-contained with zero heap indirection so that a
/// [`BytecodeChunk`]'s instruction stream is laid out contiguously in CPU cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    // =========================================================================
    // 1. Constants & Register Data Movement
    // =========================================================================
    /// `dst = null`
    LoadNull { dst: Reg },

    /// `dst = value` (`true` or `false`)
    LoadBool { dst: Reg, value: bool },

    /// `dst = value` (inline 32-bit signed integer immediate)
    LoadInt { dst: Reg, value: i32 },

    /// `dst = constants[const_id]`
    LoadConst { dst: Reg, const_id: ConstantId },

    /// `dst = src` (register copy / COW handle move)
    Move { dst: Reg, src: Reg },

    /// `dst = clone src` (shallow object clone)
    Clone { dst: Reg, src: Reg },

    // =========================================================================
    // 2. Arithmetic, Bitwise, Comparison, Cast, & Type Contracts
    // =========================================================================
    /// `dst = op src` (`+`, `-`, `!`, `~`, `@`)
    Unary { dst: Reg, op: IrUnaryOp, src: Reg },

    /// `dst = lhs op rhs`
    Binary {
        dst: Reg,
        op: BinaryOp,
        lhs: Reg,
        rhs: Reg,
    },

    /// `dst = src + 1` (if `is_inc`) or `src - 1`
    IncDec { dst: Reg, src: Reg, is_inc: bool },

    /// `dst = (kind) src`
    Cast { dst: Reg, kind: CastType, src: Reg },

    /// Enforce a Medium/Gradual Type System contract on `reg` against `constants[type_const]`.
    AssertType { reg: Reg, type_const: ConstantId },

    /// Wrap a register value into an explicit native Rust integer subtype (`Int::i64()`, `Int::u32()`, etc.).
    WrapRustInt {
        dst: Reg,
        src: Option<Reg>,
        kind: RustIntKind,
    },

    /// Wrap a register value into an explicit native Rust float subtype (`Float::f64()`, `Float::f32()`).
    WrapRustFloat {
        dst: Reg,
        src: Option<Reg>,
        kind: RustFloatKind,
    },

    // =========================================================================
    // 3. Control Flow & Linear Branching
    // =========================================================================
    /// Unconditional jump to linear instruction offset `target`.
    Jump { target: u32 },

    /// Jump to `target` if `cond` evaluates to truthy (`true`).
    JumpIfTrue { cond: Reg, target: u32 },

    /// Jump to `target` if `cond` evaluates to falsy (`false`).
    JumpIfFalse { cond: Reg, target: u32 },

    /// Jump to `target` if `cond` is `null` (used by `?->` nullsafe chains).
    JumpIfNull { cond: Reg, target: u32 },

    /// Jump to `target` if `cond` is NOT `null` (used by `??` and `??=` null-coalescing).
    JumpIfNotNull { cond: Reg, target: u32 },

    /// Return from the current function/method/script frame.
    Return { value: Option<Reg> },

    // =========================================================================
    // 4. Standard Arrays & Subtyped Collections (`Arr::list`, `dict`, `set`, `hashMap`)
    // =========================================================================
    /// Allocate a new array in `dst` with pre-allocated `capacity`.
    NewArray { dst: Reg, capacity: u16 },

    /// Instantiate an explicit Rust-backed collection subtype (`Arr::list`, `Arr::dict`, `Arr::set`, `Arr::hashMap`).
    NewCollection {
        dst: Reg,
        kind: IrCollectionKind,
        source: Option<Reg>,
    },

    /// Append `value` to sequential array/collection `array` (`$arr[] = $val`).
    ArrayPush { array: Reg, value: Reg },

    /// Associate `key => value` in `array` (`$arr[$key] = $val`).
    ArraySet { array: Reg, key: Reg, value: Reg },

    /// Unpack elements from `src_array` into `dst_array` (`[...$items]`).
    ArrayUnpack { dst_array: Reg, src_array: Reg },

    /// Read `dst = array[index]`.
    ArrayGet { dst: Reg, array: Reg, index: Reg },

    // =========================================================================
    // 5. Iteration (`foreach`)
    // =========================================================================
    /// Initialize an iterator state in `iter_reg` over `iterable`.
    IterInit { iter_reg: Reg, iterable: Reg },

    /// Advance iterator `iter_reg`. If exhausted, jump to `jump_end`; otherwise populate `key_dst` and `val_dst`.
    IterNext {
        iter_reg: Reg,
        key_dst: Option<Reg>,
        val_dst: Reg,
        jump_end: u32,
    },

    // =========================================================================
    // 6. Function, Facade, Method, & Closure Invocations
    // =========================================================================
    /// Stage an argument on the VM call argument buffer prior to a `Call*`, `NewObject`, or `Interpolate` instruction.
    PushArg {
        src: Reg,
        name: Option<ConstantId>,
        is_unpack: bool,
    },

    /// Invoke a named user function: `dst = func_name(args...)`.
    CallFunction {
        dst: Reg,
        name: ConstantId,
        arg_count: u16,
    },

    /// Invoke a dynamic callable or closure stored in `callee`: `dst = callee(args...)`.
    CallDynamic {
        dst: Reg,
        callee: Reg,
        arg_count: u16,
    },

    /// Invoke a static facade method (`Str::slug`, `Arr::merge`, `Future::wait`, `Sys::print`, etc.).
    FacadeCall {
        dst: Reg,
        facade: ConstantId,
        method: ConstantId,
        arg_count: u16,
    },

    /// Invoke a static method on a user class (`User::fromArray(...)`).
    StaticCall {
        dst: Reg,
        class: ConstantId,
        method: ConstantId,
        arg_count: u16,
    },

    /// Invoke an instance method (`$obj->method(args...)` or fluent wrapper call).
    MethodCall {
        dst: Reg,
        object: Reg,
        method: ConstantId,
        arg_count: u16,
        is_nullsafe: bool,
    },

    /// Concatenate `part_count` staged `PushArg` values into a single UTF-8 string in `dst`.
    Interpolate { dst: Reg, part_count: u16 },

    /// Stage an outer register capture (`use ($var)`) for the next `MakeClosure` instruction.
    CaptureUpvalue { src: Reg, name: ConstantId },

    /// Instantiate a closure/arrow-function object in `dst` referencing `func_id`.
    MakeClosure {
        dst: Reg,
        func_id: FunctionId,
        upvalue_count: u16,
    },

    // =========================================================================
    // 7. Object-Oriented Operations & Properties
    // =========================================================================
    /// Instantiate a class (`dst = new Class(args...)`).
    NewObject {
        dst: Reg,
        class: ConstantId,
        arg_count: u16,
    },

    /// Instantiate a class dynamically (`dst = new $classVar(args...)`).
    NewObjectDynamic {
        dst: Reg,
        class_reg: Reg,
        arg_count: u16,
    },

    /// Read instance property (`dst = object->prop`).
    GetProp {
        dst: Reg,
        object: Reg,
        prop: ConstantId,
    },

    /// Write instance property (`object->prop = value`).
    SetProp {
        object: Reg,
        prop: ConstantId,
        value: Reg,
    },

    /// Read static property (`dst = Class::$prop`).
    GetStaticProp {
        dst: Reg,
        class: ConstantId,
        prop: ConstantId,
    },

    /// Write static property (`Class::$prop = value`).
    SetStaticProp {
        class: ConstantId,
        prop: ConstantId,
        value: Reg,
    },

    /// Read class constant or enum case (`dst = Class::CONST`).
    GetClassConst {
        dst: Reg,
        class: ConstantId,
        const_name: ConstantId,
    },

    /// Check `dst = object instanceof Class`.
    Instanceof {
        dst: Reg,
        object: Reg,
        class: ConstantId,
    },

    // =========================================================================
    // 8. Global Scope, Exceptions, Concurrency, Generators, & Output
    // =========================================================================
    /// Bind or load global variable `name` into local register `dst`.
    LoadGlobal { dst: Reg, name: ConstantId },

    /// Store local register `src` into global variable `name`.
    StoreGlobal { name: ConstantId, src: Reg },

    /// Emit values to standard output (`echo $expr;`).
    Echo { src: Reg },

    /// Register a post-response or scope defer hook (`defer { ... };`).
    Defer { func_id: FunctionId },

    /// Await an asynchronous `Future` handle (`await $future`).
    Await { dst: Reg, future: Reg },

    /// Import a Rust file helper (`import "./Cache.rs";` or `use "./Cache.rs" as Cache;`).
    ImportRust {
        path: ConstantId,
        alias: Option<ConstantId>,
    },

    /// Yield a value (and optional key) from a generator (`yield $k => $v` or `yield from $gen`).
    Yield {
        dst: Reg,
        key: Option<Reg>,
        value: Option<Reg>,
        is_from: bool,
    },

    /// Throw an exception value (`throw $e;`).
    Throw { exception: Reg },

    /// Throw an unhandled `MatchError` when no arm in a `match` expression matches `subject`.
    ThrowMatchError { subject: Reg },

    /// Enter a protected `try` region mapped to `handler_id` in the chunk's exception table.
    EnterTry { handler_id: u16 },

    /// Exit the innermost active `try` region normally.
    LeaveTry { handler_id: u16 },
}

impl fmt::Display for OpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpCode::LoadNull { dst } => write!(f, "LOAD_NULL       {}", dst),
            OpCode::LoadBool { dst, value } => write!(f, "LOAD_BOOL       {}, {}", dst, value),
            OpCode::LoadInt { dst, value } => write!(f, "LOAD_INT        {}, {}", dst, value),
            OpCode::LoadConst { dst, const_id } => {
                write!(f, "LOAD_CONST      {}, {}", dst, const_id)
            }
            OpCode::Move { dst, src } => write!(f, "MOVE            {}, {}", dst, src),
            OpCode::Clone { dst, src } => write!(f, "CLONE           {}, {}", dst, src),
            OpCode::Unary { dst, op, src } => write!(f, "UNARY           {}, {}, {}", dst, op, src),
            OpCode::Binary { dst, op, lhs, rhs } => {
                write!(f, "BINARY          {}, {} {} {}", dst, lhs, op, rhs)
            }
            OpCode::IncDec { dst, src, is_inc } => {
                let op = if *is_inc { "INC" } else { "DEC" };
                write!(f, "{:<15} {}, {}", op, dst, src)
            }
            OpCode::Cast { dst, kind, src } => {
                write!(f, "CAST            {}, ({:?}) {}", dst, kind, src)
            }
            OpCode::AssertType { reg, type_const } => {
                write!(f, "ASSERT_TYPE     {}, {}", reg, type_const)
            }
            OpCode::WrapRustInt { dst, src, kind } => match src {
                Some(s) => write!(f, "WRAP_RUST_INT   {}, {}, {}", dst, s, kind),
                None => write!(f, "WRAP_RUST_INT   {}, {}", dst, kind),
            },
            OpCode::WrapRustFloat { dst, src, kind } => match src {
                Some(s) => write!(f, "WRAP_RUST_FLOAT {}, {}, {}", dst, s, kind),
                None => write!(f, "WRAP_RUST_FLOAT {}, {}", dst, kind),
            },
            OpCode::Jump { target } => write!(f, "JUMP            @{:04}", target),
            OpCode::JumpIfTrue { cond, target } => {
                write!(f, "JUMP_IF_TRUE    {}, @{:04}", cond, target)
            }
            OpCode::JumpIfFalse { cond, target } => {
                write!(f, "JUMP_IF_FALSE   {}, @{:04}", cond, target)
            }
            OpCode::JumpIfNull { cond, target } => {
                write!(f, "JUMP_IF_NULL    {}, @{:04}", cond, target)
            }
            OpCode::JumpIfNotNull { cond, target } => {
                write!(f, "JUMP_IF_NONNULL {}, @{:04}", cond, target)
            }
            OpCode::Return { value } => match value {
                Some(reg) => write!(f, "RETURN          {}", reg),
                None => write!(f, "RETURN          void"),
            },
            OpCode::NewArray { dst, capacity } => {
                write!(f, "NEW_ARRAY       {}, cap={}", dst, capacity)
            }
            OpCode::NewCollection { dst, kind, source } => match source {
                Some(src) => write!(f, "NEW_COLLECTION  {}, {}, src={}", dst, kind, src),
                None => write!(f, "NEW_COLLECTION  {}, {}", dst, kind),
            },
            OpCode::ArrayPush { array, value } => {
                write!(f, "ARRAY_PUSH      {}, {}", array, value)
            }
            OpCode::ArraySet { array, key, value } => {
                write!(f, "ARRAY_SET       {}[{}] = {}", array, key, value)
            }
            OpCode::ArrayUnpack {
                dst_array,
                src_array,
            } => write!(f, "ARRAY_UNPACK    {}, ...{}", dst_array, src_array),
            OpCode::ArrayGet { dst, array, index } => {
                write!(f, "ARRAY_GET       {}, {}[{}]", dst, array, index)
            }
            OpCode::IterInit { iter_reg, iterable } => {
                write!(f, "ITER_INIT       {}, {}", iter_reg, iterable)
            }
            OpCode::IterNext {
                iter_reg,
                key_dst,
                val_dst,
                jump_end,
            } => match key_dst {
                Some(k) => write!(
                    f,
                    "ITER_NEXT       {}, {} => {}, end=@{:04}",
                    iter_reg, k, val_dst, jump_end
                ),
                None => write!(
                    f,
                    "ITER_NEXT       {}, {}, end=@{:04}",
                    iter_reg, val_dst, jump_end
                ),
            },
            OpCode::PushArg {
                src,
                name,
                is_unpack,
            } => {
                let prefix = if *is_unpack { "..." } else { "" };
                match name {
                    Some(n) => write!(f, "PUSH_ARG        {}: {}{}", n, prefix, src),
                    None => write!(f, "PUSH_ARG        {}{}", prefix, src),
                }
            }
            OpCode::CallFunction {
                dst,
                name,
                arg_count,
            } => write!(f, "CALL_FUNC       {}, {}, args={}", dst, name, arg_count),
            OpCode::CallDynamic {
                dst,
                callee,
                arg_count,
            } => write!(f, "CALL_DYNAMIC    {}, {}, args={}", dst, callee, arg_count),
            OpCode::FacadeCall {
                dst,
                facade,
                method,
                arg_count,
            } => write!(
                f,
                "FACADE_CALL     {}, {}::{}, args={}",
                dst, facade, method, arg_count
            ),
            OpCode::StaticCall {
                dst,
                class,
                method,
                arg_count,
            } => write!(
                f,
                "STATIC_CALL     {}, {}::{}, args={}",
                dst, class, method, arg_count
            ),
            OpCode::MethodCall {
                dst,
                object,
                method,
                arg_count,
                is_nullsafe,
            } => {
                let arrow = if *is_nullsafe { "?->" } else { "->" };
                write!(
                    f,
                    "METHOD_CALL     {}, {}{}{}, args={}",
                    dst, object, arrow, method, arg_count
                )
            }
            OpCode::Interpolate { dst, part_count } => {
                write!(f, "INTERPOLATE     {}, parts={}", dst, part_count)
            }
            OpCode::CaptureUpvalue { src, name } => {
                write!(f, "CAPTURE_UPVAL   {}, {}", src, name)
            }
            OpCode::MakeClosure {
                dst,
                func_id,
                upvalue_count,
            } => write!(
                f,
                "MAKE_CLOSURE    {}, {}, upvals={}",
                dst, func_id, upvalue_count
            ),
            OpCode::NewObject {
                dst,
                class,
                arg_count,
            } => write!(f, "NEW_OBJECT      {}, {}, args={}", dst, class, arg_count),
            OpCode::NewObjectDynamic {
                dst,
                class_reg,
                arg_count,
            } => write!(f, "NEW_OBJECT_DYN  {}, {}, args={}", dst, class_reg, arg_count),
            OpCode::GetProp { dst, object, prop } => {
                write!(f, "GET_PROP        {}, {}->{}", dst, object, prop)
            }
            OpCode::SetProp {
                object,
                prop,
                value,
            } => write!(f, "SET_PROP        {}->{} = {}", object, prop, value),
            OpCode::GetStaticProp { dst, class, prop } => {
                write!(f, "GET_STATIC_PROP {}, {}::{}", dst, class, prop)
            }
            OpCode::SetStaticProp { class, prop, value } => {
                write!(f, "SET_STATIC_PROP {}::{} = {}", class, prop, value)
            }
            OpCode::GetClassConst {
                dst,
                class,
                const_name,
            } => write!(f, "GET_CLASS_CONST {}, {}::{}", dst, class, const_name),
            OpCode::Instanceof { dst, object, class } => {
                write!(f, "INSTANCEOF      {}, {}, {}", dst, object, class)
            }
            OpCode::LoadGlobal { dst, name } => write!(f, "LOAD_GLOBAL     {}, {}", dst, name),
            OpCode::StoreGlobal { name, src } => write!(f, "STORE_GLOBAL    {}, {}", name, src),
            OpCode::Echo { src } => write!(f, "ECHO            {}", src),
            OpCode::Defer { func_id } => write!(f, "DEFER           {}", func_id),
            OpCode::Await { dst, future } => write!(f, "AWAIT           {}, {}", dst, future),
            OpCode::ImportRust { path, alias } => {
                write!(f, "IMPORT_RUST     {}, alias={:?}", path, alias)
            }
            OpCode::Yield {
                dst,
                key,
                value,
                is_from,
            } => {
                let kind = if *is_from { "YIELD_FROM" } else { "YIELD" };
                write!(f, "{:<15} {}, key={:?}, val={:?}", kind, dst, key, value)
            }
            OpCode::Throw { exception } => write!(f, "THROW           {}", exception),
            OpCode::ThrowMatchError { subject } => {
                write!(f, "THROW_MATCH_ERR {}", subject)
            }
            OpCode::EnterTry { handler_id } => write!(f, "ENTER_TRY       handler#{}", handler_id),
            OpCode::LeaveTry { handler_id } => write!(f, "LEAVE_TRY       handler#{}", handler_id),
        }
    }
}

/// Metadata describing a virtual register allocated in a [`BytecodeChunk`].
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterInfo {
    pub reg: Reg,
    /// Variable name (`$x`, `$this`, etc.) if this register backs a named variable; `None` for temporaries.
    pub name: Option<String>,
    /// Statically locked or inferred semantic type for JIT specialization and debugging.
    pub rhp_type: RhpType,
    /// True if this register holds a formal parameter.
    pub is_param: bool,
    /// True if this variable has an explicit locked type annotation (`int $x` or parameter type).
    pub is_locked_type: bool,
}

/// Catch clause dispatch descriptor within an [`ExceptionHandler`].
#[derive(Debug, Clone, PartialEq)]
pub struct CatchTarget {
    /// Exception class names matched by this catch block (empty means catch-all).
    pub catch_types: Vec<String>,
    /// Register receiving the caught exception instance (`$e`).
    pub exception_reg: Reg,
    /// Instruction pointer offset of the catch handler body.
    pub target_ip: u32,
}

/// Structured exception table entry for zero-cost `try` / `catch` / `finally` unwinding.
#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionHandler {
    pub handler_id: u16,
    /// Inclusive start instruction offset of the `try` block.
    pub try_start: u32,
    /// Exclusive end instruction offset of the `try` block.
    pub try_end: u32,
    /// Ordered catch clauses for this `try` block.
    pub catches: Vec<CatchTarget>,
    /// Optional instruction offset of the `finally` block.
    pub finally_target: Option<u32>,
}

/// A flat, linear bytecode sequence paired with its constant pool, register metadata,
/// exception handler table, and source span mapping.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BytecodeChunk {
    /// Contiguous flat linear instruction stream.
    pub instructions: Vec<OpCode>,
    /// Parallel source span table (`spans.len() == instructions.len()`) for exact stack traces.
    pub spans: Vec<Span>,
    /// Deduplicated constant pool referenced by [`ConstantId`].
    pub constants: Vec<Constant>,
    /// Total number of virtual registers allocated by this chunk.
    pub num_registers: u16,
    /// Metadata for allocated registers (`register_info.len() == num_registers as usize`).
    pub register_info: Vec<RegisterInfo>,
    /// Map from named local variable (`$name`) to its dedicated virtual register.
    pub local_map: HashMap<String, Reg>,
    /// Structured exception handlers for `try`/`catch`/`finally` regions.
    pub exception_handlers: Vec<ExceptionHandler>,
}

impl BytecodeChunk {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of instructions in this linear bytecode stream.
    #[inline]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Returns true if the instruction stream is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    /// Retrieve the source [`Span`] corresponding to instruction pointer `ip`.
    #[inline]
    pub fn span_at(&self, ip: usize) -> Option<Span> {
        self.spans.get(ip).copied()
    }

    /// Look up a constant in the constant pool by [`ConstantId`].
    #[inline]
    pub fn get_constant(&self, id: ConstantId) -> Option<&Constant> {
        self.constants.get(id.index())
    }

    /// Look up a string constant in the constant pool by [`ConstantId`].
    #[inline]
    pub fn get_string_constant(&self, id: ConstantId) -> Option<&str> {
        match self.get_constant(id)? {
            Constant::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Look up the virtual register assigned to a local variable (`$var` or `var`).
    pub fn register_for_var(&self, var_name: &str) -> Option<Reg> {
        if var_name.starts_with('$') {
            self.local_map.get(var_name).copied()
        } else {
            let normalized = format!("${}", var_name);
            self.local_map.get(&normalized).copied()
        }
    }

    /// Format a human-readable disassembly of this bytecode chunk.
    pub fn disassemble(&self, label: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "=== {} (regs: {}, consts: {}, instrs: {}) ===\n",
            label,
            self.num_registers,
            self.constants.len(),
            self.instructions.len()
        ));

        if !self.constants.is_empty() {
            out.push_str("Constants:\n");
            for (i, c) in self.constants.iter().enumerate() {
                out.push_str(&format!("  c{}: {}\n", i, c));
            }
        }

        if !self.register_info.is_empty() {
            out.push_str("Registers:\n");
            for info in &self.register_info {
                if let Some(ref name) = info.name {
                    out.push_str(&format!(
                        "  {}: {} : {}{}\n",
                        info.reg,
                        name,
                        info.rhp_type,
                        if info.is_param { " (param)" } else { "" }
                    ));
                }
            }
        }

        out.push_str("Bytecode:\n");
        for (ip, instr) in self.instructions.iter().enumerate() {
            out.push_str(&format!("  {:04}: {}\n", ip, instr));
        }

        out
    }
}
