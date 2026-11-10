use crate::ir::opcode::FunctionId;
use crate::lexer::span::Span;
use crate::vm::value::PhpValue;

/// Hierarchy relationships for built-in PHP Throwable classes.
/// Maps child class name to immediate parent class name.
pub static BUILTIN_THROWABLE_PARENTS: &[(&str, &str)] = &[
    // Roots directly under Throwable
    ("Error", "Throwable"),
    ("Exception", "Throwable"),

    // Error hierarchy
    ("ArithmeticError", "Error"),
    ("DivisionByZeroError", "ArithmeticError"),
    ("AssertionError", "Error"),
    ("CompileError", "Error"),
    ("ParseError", "CompileError"),
    ("TypeError", "Error"),
    ("ArgumentCountError", "TypeError"),
    ("ArgumentError", "Error"),
    ("ValueError", "Error"),
    ("UnhandledMatchError", "Error"),

    // Exception hierarchy (SPL family & standard)
    ("ErrorException", "Exception"),
    ("InvalidEncodingException", "Exception"),
    ("UnknownEncodingException", "Exception"),
    ("LogicException", "Exception"),
    ("BadFunctionCallException", "LogicException"),
    ("BadMethodCallException", "BadFunctionCallException"),
    ("DomainException", "LogicException"),
    ("InvalidArgumentException", "LogicException"),
    ("LengthException", "LogicException"),
    ("OutOfRangeException", "LogicException"),
    ("RuntimeException", "Exception"),
    ("OutOfBoundsException", "RuntimeException"),
    ("OverflowException", "RuntimeException"),
    ("RangeException", "RuntimeException"),
    ("UnderflowException", "RuntimeException"),
    ("UnexpectedValueException", "RuntimeException"),
    ("PDOException", "RuntimeException"),
];

/// Returns true if `class_name` is recognized as a built-in PHP Throwable (or ends with Exception/Error).
pub fn is_builtin_throwable(class_name: &str) -> bool {
    let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
    if clean.eq_ignore_ascii_case("Throwable")
        || clean.eq_ignore_ascii_case("Exception")
        || clean.eq_ignore_ascii_case("Error")
        || clean.ends_with("Exception")
        || clean.ends_with("Error")
    {
        return true;
    }
    BUILTIN_THROWABLE_PARENTS
        .iter()
        .any(|(child, _)| child.eq_ignore_ascii_case(clean))
}

/// Returns true if `class_name` is an Error or Error subclass in PHP.
pub fn is_builtin_error(class_name: &str) -> bool {
    let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
    if clean.eq_ignore_ascii_case("Error") {
        return true;
    }
    let mut cursor = Some(clean);
    while let Some(curr) = cursor {
        if curr.eq_ignore_ascii_case("Error") {
            return true;
        }
        if curr.eq_ignore_ascii_case("Exception") || curr.eq_ignore_ascii_case("Throwable") {
            return false;
        }
        cursor = get_builtin_throwable_parent(curr);
    }
    false
}

/// Returns true if `class_name` is an Exception or Exception subclass in PHP.
pub fn is_builtin_exception(class_name: &str) -> bool {
    let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
    if clean.eq_ignore_ascii_case("Exception") {
        return true;
    }
    let mut cursor = Some(clean);
    while let Some(curr) = cursor {
        if curr.eq_ignore_ascii_case("Exception") {
            return true;
        }
        if curr.eq_ignore_ascii_case("Error") || curr.eq_ignore_ascii_case("Throwable") {
            return false;
        }
        cursor = get_builtin_throwable_parent(curr);
    }
    false
}

/// Get the immediate built-in parent class name for a built-in Throwable.
pub fn get_builtin_throwable_parent(name: &str) -> Option<&'static str> {
    let clean = name.strip_prefix('\\').unwrap_or(name);
    for &(child, parent) in BUILTIN_THROWABLE_PARENTS {
        if child.eq_ignore_ascii_case(clean) {
            return Some(parent);
        }
    }
    if clean.eq_ignore_ascii_case("Throwable") {
        None
    } else if clean.ends_with("Exception") {
        Some("Exception")
    } else if clean.ends_with("Error") {
        Some("Error")
    } else {
        None
    }
}

/// Check if `actual` is a subtype of `target` in the PHP Throwable hierarchy.
///
/// Follows PHP OOP rules:
/// - Throwable is root interface.
/// - Error and Exception are distinct branches under Throwable.
/// - An Error is NOT an Exception.
/// - An Exception is NOT an Error.
pub fn is_throwable_subtype(actual: &str, target: &str) -> bool {
    let actual_clean = actual.strip_prefix('\\').unwrap_or(actual);
    let target_clean = target.strip_prefix('\\').unwrap_or(target);

    if actual_clean.eq_ignore_ascii_case(target_clean) {
        return true;
    }

    if target_clean.eq_ignore_ascii_case("Throwable") {
        return is_builtin_throwable(actual_clean);
    }

    let mut cursor = Some(actual_clean);
    let mut depth = 0;
    while let Some(curr) = cursor {
        if depth > 32 {
            break;
        }
        depth += 1;

        if curr.eq_ignore_ascii_case(target_clean) {
            return true;
        }

        if curr.eq_ignore_ascii_case("Throwable") {
            break;
        }

        cursor = get_builtin_throwable_parent(curr);
    }

    false
}

/// A fully-symbolicated stack frame representing one function or method invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub file: String,
    pub line: usize,
    pub function: String,
    pub class: Option<String>,
    pub frame_type: Option<String>,
    pub args: Vec<PhpValue>,
}

impl Frame {
    pub fn to_php_value(&self, ignore_args: bool) -> PhpValue {
        let mut map = indexmap::IndexMap::new();
        if !self.file.is_empty() {
            map.insert("file".to_string(), PhpValue::string(&self.file));
            map.insert("line".to_string(), PhpValue::Int(self.line as i64));
        }
        map.insert("function".to_string(), PhpValue::string(&self.function));
        if let Some(ref cls) = self.class {
            map.insert("class".to_string(), PhpValue::string(cls.clone()));
        }
        if let Some(ref ty) = self.frame_type {
            map.insert("type".to_string(), PhpValue::string(ty.clone()));
        }
        if !ignore_args {
            map.insert("args".to_string(), PhpValue::list(self.args.clone()));
        }
        PhpValue::dict_from_map(map)
    }

    pub fn format_as_string(&self, index: usize) -> String {
        let location = if self.file.is_empty() {
            "[internal function]".to_string()
        } else {
            format!("{}({})", self.file, self.line)
        };
        let call = if let Some(ref cls) = self.class {
            let ty = self.frame_type.as_deref().unwrap_or("->");
            format!("{}{}{}()", cls, ty, self.function)
        } else {
            format!("{}()", self.function)
        };
        format!("#{} {}: {}", index, location, call)
    }
}

/// Raw, unsymbolicated frame captured at exception instantiation time.
/// Stores cheap numeric instruction pointers and function handles without
/// eager symbol resolution or string allocations.
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub callee_func_id: Option<FunctionId>,
    pub function_name: String,
    pub class_name: Option<String>,
    pub is_static: bool,
    pub caller_func_id: Option<FunctionId>,
    pub caller_ip: usize,
    pub caller_span: Span,
    pub args: Vec<PhpValue>,
}

/// Backing Rust representation for PHP `Throwable` instances.
///
/// Complies with the specification:
/// `message`, `code`, `file`, `line`, `trace: Vec<Frame>`, `previous: Option<Box<Throwable>>`.
///
/// File, line, and raw trace pointers are captured when the exception is instantiated,
/// matching PHP semantics. Trace symbolication is performed lazily on the first call to `getTrace()`.
#[derive(Debug, Clone)]
pub struct Throwable {
    pub message: String,
    pub code: i64,
    pub file: String,
    pub line: usize,
    pub trace: Vec<Frame>,
    pub previous: Option<Box<Throwable>>,
    pub(crate) raw_trace: Vec<RawFrame>,
    pub(crate) is_symbolicated: bool,
    pub(crate) previous_obj: Option<PhpValue>,
    pub(crate) severity: Option<i32>,
}

impl Throwable {
    pub fn new(message: impl Into<String>, code: i64, file: impl Into<String>, line: usize) -> Self {
        Self {
            message: message.into(),
            code,
            file: file.into(),
            line,
            trace: Vec::new(),
            previous: None,
            raw_trace: Vec::new(),
            is_symbolicated: false,
            previous_obj: None,
            severity: None,
        }
    }

    pub fn with_previous(mut self, prev: Throwable, prev_obj: Option<PhpValue>) -> Self {
        self.previous = Some(Box::new(prev));
        self.previous_obj = prev_obj;
        self
    }

    #[inline]
    pub fn get_message(&self) -> &str {
        &self.message
    }

    #[inline]
    pub fn get_code(&self) -> i64 {
        self.code
    }

    #[inline]
    pub fn get_file(&self) -> &str {
        &self.file
    }

    #[inline]
    pub fn get_line(&self) -> usize {
        self.line
    }

    #[inline]
    pub fn get_trace(&self) -> &[Frame] {
        &self.trace
    }

    /// Primary method returning the previous (cause) Throwable in the chain.
    #[inline]
    pub fn source(&self) -> Option<&Throwable> {
        self.previous.as_deref()
    }

    /// PHP/Laravel compatibility alias for `source()`.
    #[inline]
    pub fn get_previous(&self) -> Option<&Throwable> {
        self.source()
    }

    /// Primary method returning an iterator walking the exception chain from `self` to the root cause.
    pub fn chain(&self) -> impl Iterator<Item = &Throwable> {
        std::iter::successors(Some(self), |e| e.source())
    }

    /// Alias for `chain()`.
    #[inline]
    pub fn get_chain(&self) -> impl Iterator<Item = &Throwable> {
        self.chain()
    }

    /// Primary method returning the deepest (root cause) Throwable in the chain.
    pub fn root_cause(&self) -> &Throwable {
        self.chain().last().unwrap_or(self)
    }

    /// Alias for `root_cause()`.
    #[inline]
    pub fn get_root_cause(&self) -> &Throwable {
        self.root_cause()
    }

    pub fn get_trace_as_string(&self) -> String {
        let mut out = String::new();
        for (i, frame) in self.trace.iter().enumerate() {
            out.push_str(&frame.format_as_string(i));
            out.push('\n');
        }
        out.push_str(&format!("#{} {{main}}", self.trace.len()));
        out
    }

    pub fn to_string_representation(&self, class_name: &str) -> String {
        let mut out = format!(
            "{}: {} in {}:{}\nStack trace:\n",
            class_name, self.message, self.file, self.line
        );
        out.push_str(&self.get_trace_as_string());
        if let Some(ref prev) = self.previous {
            out.push_str("\n\nNext ");
            out.push_str(&prev.to_string_representation("Exception"));
        }
        out
    }

    pub fn to_trace_php_value(&self, ignore_args: bool) -> PhpValue {
        let items: Vec<PhpValue> = self.trace.iter().map(|f| f.to_php_value(ignore_args)).collect();
        PhpValue::list(items)
    }
}

/// Walk the exception chain starting from `e`. Primary free-function helper.
pub fn chain(e: &Throwable) -> impl Iterator<Item = &Throwable> {
    e.chain()
}

/// Return the root cause of the exception. Primary free-function helper.
pub fn root_cause(e: &Throwable) -> &Throwable {
    e.root_cause()
}

impl std::error::Error for Throwable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source().map(|p| p as &(dyn std::error::Error + 'static))
    }
}

impl std::fmt::Display for Throwable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.file.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{} in {}:{}", self.message, self.file, self.line)
        }
    }
}

impl From<std::io::Error> for Throwable {
    fn from(err: std::io::Error) -> Self {
        Throwable::new(err.to_string(), 0, "<native>", 0)
    }
}

impl From<String> for Throwable {
    fn from(msg: String) -> Self {
        Throwable::new(msg, 0, "<native>", 0)
    }
}

impl From<&str> for Throwable {
    fn from(msg: &str) -> Self {
        Throwable::new(msg.to_string(), 0, "<native>", 0)
    }
}

