//! Diagnostic code catalogue and registry for RHP compiler errors.
//!
//! Error codes are stable and never renumbered. Each error code is backed by
//! detailed explanations, code examples, and instructions on how to resolve it.

use serde::{Deserialize, Serialize};

/// Detailed explanation and documentation for a stable diagnostic code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticCodeInfo {
    pub code: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub explanation: &'static str,
    pub example: Option<&'static str>,
    pub fix: Option<&'static str>,
}

pub static ALL_DIAGNOSTIC_CODES: &[DiagnosticCodeInfo] = &[
    // --- 0001 - 0099: Lexer errors ---
    DiagnosticCodeInfo {
        code: "RHP0001",
        title: "Unexpected character",
        summary: "An unrecognized or invalid character was encountered by the scanner.",
        explanation: "The lexer found a character or byte sequence that does not form any valid token in the RHP grammar.",
        example: Some("$foo = `invalid`;"),
        fix: Some("Replace or remove the invalid character with valid RHP syntax."),
    },
    DiagnosticCodeInfo {
        code: "RHP0002",
        title: "Unclosed string literal",
        summary: "A string literal was opened but not closed before the end of input or newline.",
        explanation: "Strings opened with single quotes `'` or double quotes `\"` must be terminated with a matching quote.",
        example: Some("$str = \"hello world;"),
        fix: Some("Add the missing closing quote to terminate the string literal."),
    },
    DiagnosticCodeInfo {
        code: "RHP0003",
        title: "Invalid numeric literal",
        summary: "A numeric literal is malformed or out of representable bounds.",
        explanation: "Floating-point numbers and integer literals must conform to standard numeric formats.",
        example: Some("$num = 12.34.56;"),
        fix: Some("Ensure the number has only a single decimal point and valid digits."),
    },

    // --- 0100 - 0199: Parser errors ---
    DiagnosticCodeInfo {
        code: "RHP0101",
        title: "Unexpected end of file",
        summary: "The parser reached the end of the input stream unexpectedly.",
        explanation: "The source file terminated while a statement, block, or expression was still open.",
        example: Some("fn add($a, $b) {\n    return $a + $b"),
        fix: Some("Ensure all opening braces, parentheses, and statements are properly closed."),
    },
    DiagnosticCodeInfo {
        code: "RHP0102",
        title: "Unexpected token",
        summary: "The parser encountered a token that does not fit the grammar at this position.",
        explanation: "A token was encountered where another grammar construct (e.g. an expression, identifier, or delimiter) was expected.\n\nCommon Causes:\n- Trailing comma in an expression or call list where not permitted.\n- Misplaced punctuation or operator.\n- Incomplete expressions.",
        example: Some("fn total($a, $b) {\n    return add($a, );\n}"),
        fix: Some("Remove the unexpected token, or provide the missing expression or identifier:\nfn total($a, $b) {\n    return add($a, $b);\n}"),
    },
    DiagnosticCodeInfo {
        code: "RHP0103",
        title: "Unclosed delimiter",
        summary: "An opening delimiter ('(', '{', '[') was not matched with a closing delimiter.",
        explanation: "Every opening parenthesis, bracket, or brace must have a corresponding closing delimiter within the same scope.",
        example: Some("$items = [1, 2, 3;"),
        fix: Some("Add the matching closing delimiter (e.g. ']', ')', or '}')."),
    },
    DiagnosticCodeInfo {
        code: "RHP0104",
        title: "Missing statement terminator",
        summary: "A statement is missing its terminating semicolon ';'.",
        explanation: "Statements in RHP must be terminated with a semicolon, except for block structures.",
        example: Some("$x = 42\n$y = 10;"),
        fix: Some("Add a semicolon ';' to terminate the statement."),
    },
    DiagnosticCodeInfo {
        code: "RHP0105",
        title: "Invalid assignment target",
        summary: "The left-hand side of an assignment is not a writable lvalue.",
        explanation: "Assignments require a valid variable, array element, or object property as their target.",
        example: Some("1 + 2 = $x;"),
        fix: Some("Assign values only to variables or assignable object/array properties."),
    },
    DiagnosticCodeInfo {
        code: "RHP0106",
        title: "Syntax error",
        summary: "General syntactic error violating RHP grammar rules.",
        explanation: "The syntax used does not match any valid syntactic construct in the language specification.",
        example: None,
        fix: Some("Review the surrounding syntax against language specification."),
    },
    DiagnosticCodeInfo {
        code: "RHP0110",
        title: "Static facade instantiation forbidden",
        summary: "Static facade classes (such as Arr, Str, Sys) cannot be instantiated.",
        explanation: "Facades provide high-performance static utility methods and cannot be instantiated with 'new'.",
        example: Some("$arr = new Arr();"),
        fix: Some("Call static methods directly, e.g. 'Arr::list(...)' or 'Arr::dict(...)', instead of using 'new'."),
    },
    DiagnosticCodeInfo {
        code: "RHP0111",
        title: "Static facade inheritance forbidden",
        summary: "Static facade classes cannot be extended via inheritance.",
        explanation: "Static facades are sealed intrinsic utilities. Use facade macro registration (e.g. 'Arr::macro(...)') to add methods dynamically.",
        example: Some("class MyArr extends Arr {}"),
        fix: Some("Use macro extension, e.g. 'Arr::macro(\"custom\", fn(...) => ...)', or define a separate helper class."),
    },
    DiagnosticCodeInfo {
        code: "RHP0112",
        title: "Invalid facade macro registration",
        summary: "A macro registered on a static facade is invalid.",
        explanation: "Macros must specify a valid method name and a callable closure.",
        example: None,
        fix: Some("Ensure the macro name is an identifier and the handler is a valid callable."),
    },
    DiagnosticCodeInfo {
        code: "RHP0113",
        title: "Singleton instantiation forbidden",
        summary: "Singleton classes cannot be instantiated with 'new'.",
        explanation: "Singleton classes manage their own instance and require static accessors.",
        example: Some("$db = new Database();"),
        fix: Some("Access singleton instances via static methods, e.g. 'Database::getInstance()'."),
    },
    DiagnosticCodeInfo {
        code: "RHP0114",
        title: "Non-static singleton method call forbidden",
        summary: "Non-static methods on singletons cannot be invoked from non-singleton classes.",
        explanation: "Only static methods of singleton classes can be called from outside.",
        example: None,
        fix: Some("Use static singleton methods or declare the caller as a singleton."),
    },
    DiagnosticCodeInfo {
        code: "RHP0115",
        title: "Non-singleton extends singleton forbidden",
        summary: "A non-singleton class cannot inherit from a singleton class.",
        explanation: "Singletons enforce strict single-instance semantics that cannot be sub-classed by non-singletons.",
        example: Some("class User extends SingletonBase {}"),
        fix: Some("Mark the child class as singleton or use composition instead of inheritance."),
    },

    // --- 0200 - 0299: Type system & boundary errors ---
    DiagnosticCodeInfo {
        code: "RHP0201",
        title: "Missing parameter type annotation",
        summary: "Function or method parameters across module boundaries must declare explicit types.",
        explanation: "RHP enforces explicit types on public functions, methods, and interface signatures across boundaries to guarantee zero-cost runtime contracts.",
        example: Some("function greet($name) {\n    return \"Hello, \" . $name;\n}"),
        fix: Some("Add an explicit type hint to each parameter:\nfunction greet(string $name): string {\n    return \"Hello, \" . $name;\n}"),
    },
    DiagnosticCodeInfo {
        code: "RHP0202",
        title: "Missing return type annotation",
        summary: "Function or method declarations across module boundaries must declare explicit return types.",
        explanation: "RHP requires explicit return type annotations on boundary functions to guarantee strict contract enforcement and enable Cranelift JIT optimizations.",
        example: Some("function add(int $a, int $b) {\n    return $a + $b;\n}"),
        fix: Some("Add an explicit return type hint, e.g. ': int' or ': void':\nfunction add(int $a, int $b): int {\n    return $a + $b;\n}"),
    },
    DiagnosticCodeInfo {
        code: "RHP0203",
        title: "Return type mismatch",
        summary: "The returned value does not match the declared return type hint.",
        explanation: "A return expression produced a value of an incompatible type with the declared function or method return signature.",
        example: Some("function count(): int {\n    return \"invalid\";\n}"),
        fix: Some("Return a value matching the declared return type, or update the return type signature."),
    },
    DiagnosticCodeInfo {
        code: "RHP0204",
        title: "Argument type mismatch",
        summary: "An argument passed to a function or method is incompatible with the parameter type hint.",
        explanation: "The caller passed a value whose type does not conform to the function signature contract.",
        example: Some("function display(string $msg): void {}\ndisplay(123);"),
        fix: Some("Pass an argument matching the expected parameter type, or cast explicitly if intended."),
    },
    DiagnosticCodeInfo {
        code: "RHP0205",
        title: "Argument count mismatch (arity mismatch)",
        summary: "A callable received too few or too many arguments.",
        explanation: "The number of provided positional arguments is outside the [min_args, max_args] range accepted by the function.",
        example: Some("function sum(int $a, int $b): int { return $a + $b; }\nsum(1);"),
        fix: Some("Provide all required arguments according to the function's declaration."),
    },
    DiagnosticCodeInfo {
        code: "RHP0206",
        title: "Unknown named argument",
        summary: "A named argument passed to a function does not match any declared parameter.",
        explanation: "When using named arguments (e.g. 'fn(param: $val)'), the parameter name must match a declared parameter.",
        example: Some("function greet(string $name): void {}\ngreet(user: \"Alice\");"),
        fix: Some("Ensure named arguments match the exact parameter names declared on the function."),
    },
    DiagnosticCodeInfo {
        code: "RHP0207",
        title: "Variable type mismatch",
        summary: "Cannot assign a value of an incompatible type to a statically typed variable.",
        explanation: "Variables declared with explicit type hints lock their type and reject incompatible reassignment.",
        example: Some("int $count = 10;\n$count = \"hello\";"),
        fix: Some("Assign only values that conform to the variable's declared type, or use 'mixed'."),
    },
    DiagnosticCodeInfo {
        code: "RHP0208",
        title: "Parameter default type mismatch",
        summary: "The default value expression for a parameter is incompatible with its declared type hint.",
        explanation: "Default parameter values must be assignable to the parameter's explicit type annotation.",
        example: Some("function render(string $tag = 123): void {}"),
        fix: Some("Provide a default value that matches the parameter type hint."),
    },
    DiagnosticCodeInfo {
        code: "RHP0209",
        title: "Property default type mismatch",
        summary: "The default value assigned to a class property is incompatible with its declared type.",
        explanation: "Class property initializers must evaluate to a type compatible with the property's explicit type hint.",
        example: Some("class User {\n    public int $age = \"none\";\n}"),
        fix: Some("Ensure property initializers match the declared property type."),
    },
    DiagnosticCodeInfo {
        code: "RHP0210",
        title: "Property assignment type mismatch",
        summary: "Cannot assign an incompatible value to a typed property.",
        explanation: "Assigning to a typed property requires a value compatible with the property's declared type.",
        example: Some("$user->age = \"old\";"),
        fix: Some("Assign values compatible with the declared property type."),
    },
    DiagnosticCodeInfo {
        code: "RHP0211",
        title: "Readonly property mutation forbidden",
        summary: "Readonly properties cannot be modified outside of the constructor.",
        explanation: "Properties marked 'readonly' are immutable once initialized and can only be assigned in '__construct'.",
        example: Some("$user->id = 99; // id is readonly"),
        fix: Some("Assign readonly properties only within the class's '__construct' method."),
    },
    DiagnosticCodeInfo {
        code: "RHP0212",
        title: "Void function returned value",
        summary: "Functions declared with 'void' return type cannot return a value.",
        explanation: "A 'void' function must only return without an expression (e.g. 'return;').",
        example: Some("function logMessage(): void {\n    return 42;\n}"),
        fix: Some("Remove the return value or update the return type hint to non-void."),
    },
    DiagnosticCodeInfo {
        code: "RHP0213",
        title: "Never-returning function returned",
        summary: "A function with 'never' return type must not return.",
        explanation: "Functions marked 'never' must terminate the process, throw an exception, or loop infinitely.",
        example: Some("function fail(): never {\n    return;\n}"),
        fix: Some("Ensure 'never' functions throw or exit, and do not return."),
    },
    DiagnosticCodeInfo {
        code: "RHP0214",
        title: "Missing return statement",
        summary: "Not all code paths return a value in a non-void function.",
        explanation: "When a function declares a return type other than 'void', every possible execution branch must terminate in a return statement.",
        example: Some("function check(bool $flag): int {\n    if ($flag) { return 1; }\n}"),
        fix: Some("Ensure every code branch terminates with a return statement."),
    },
    DiagnosticCodeInfo {
        code: "RHP0215",
        title: "Invalid binary operation",
        summary: "A binary operator cannot be applied between the given operand types.",
        explanation: "The specified binary operator (e.g. '+', '-', '*') is not supported between these operand types.",
        example: Some("$res = \"abc\" * 5;"),
        fix: Some("Cast or convert operands to types that support the operation."),
    },
    DiagnosticCodeInfo {
        code: "RHP0216",
        title: "Invalid unary operation",
        summary: "A unary operator cannot be applied to the operand type.",
        explanation: "The unary operator is incompatible with the type of expression provided.",
        example: Some("$res = !123;"),
        fix: Some("Ensure the operand type is valid for the unary operator."),
    },
    DiagnosticCodeInfo {
        code: "RHP0217",
        title: "Invalid type annotation",
        summary: "The specified type annotation is unknown or malformed.",
        explanation: "A type hint references an unknown class, interface, or invalid composite type.",
        example: Some("function process(UnknownType $arg): void {}"),
        fix: Some("Verify the type name spelling or ensure the class/interface is imported or declared."),
    },
    DiagnosticCodeInfo {
        code: "RHP0220",
        title: "Pass-by-reference forbidden",
        summary: "Explicit pass-by-reference ('&') in parameter signatures is forbidden in RHP.",
        explanation: "RHP eliminates hidden pointer aliasing. Objects are passed by reference handles, arrays/strings use Copy-On-Write (COW), and explicit returns or mutable collection wrappers must be used instead of '&'.",
        example: Some("function increment(&$val) {\n    $val++;\n}"),
        fix: Some("Return updated values explicitly, or pass a mutable collection wrapper:\nfunction increment(int $val): int {\n    return $val + 1;\n}"),
    },
    DiagnosticCodeInfo {
        code: "RHP0221",
        title: "Missing reference argument",
        summary: "Argument declared as reference must be passed as reference.",
        explanation: "A legacy function requiring reference arguments was called with a non-reference value.",
        example: None,
        fix: Some("Pass a variable as reference or update the function signature."),
    },
    DiagnosticCodeInfo {
        code: "RHP0222",
        title: "Unexpected reference argument",
        summary: "Passed reference argument to a function that does not accept references.",
        explanation: "The caller passed '&' on an argument where the parameter does not accept references.",
        example: Some("sin(&$x);"),
        fix: Some("Remove the '&' reference indicator at the call site."),
    },
    DiagnosticCodeInfo {
        code: "RHP0223",
        title: "Reference argument must be variable",
        summary: "A reference argument must be an assignable variable, not a literal or expression.",
        explanation: "Only variables can be passed to reference parameters.",
        example: Some("call(&42);"),
        fix: Some("Store the literal in a variable and pass the variable instead."),
    },
    DiagnosticCodeInfo {
        code: "RHP0224",
        title: "Closure reference capture forbidden",
        summary: "Closure capture-by-reference ('use (&$var)') is forbidden.",
        explanation: "Reference captures create unmanaged stack/heap lifetime leaks. RHP uses value copy for scalars and handles for objects.",
        example: Some("$fn = function() use (&$counter) {};"),
        fix: Some("Use return values, object handles, or mutable collections instead of reference capture."),
    },
    DiagnosticCodeInfo {
        code: "RHP0225",
        title: "Cyclic closure capture forbidden",
        summary: "Capturing '$this' into a property closure creates a cyclic reference risk.",
        explanation: "Storing closures that capture '$this' directly on properties creates cyclic reference graphs. Use 'Weak::to($this)' or a static closure.",
        example: Some("$this->callback = fn() => $this->action();"),
        fix: Some("Use 'Weak::to($this)' or a static closure:\n$self = Weak::to($this);\n$this->callback = static fn() => $self->get()?->action();"),
    },
    DiagnosticCodeInfo {
        code: "RHP0226",
        title: "Cyclic reference risk",
        summary: "Bidirectional object reference links risk memory leaks.",
        explanation: "Circular parent-child object links prevent automatic resource cleanup. Use explicit 'Weak::to(...)' wrappers for back-references.",
        example: Some("$child->parent = $this;"),
        fix: Some("Wrap the back-reference with Weak::to:\n$child->parent = Weak::to($this);"),
    },

    // --- 0300 - 0399: OOP & Trait errors ---
    DiagnosticCodeInfo {
        code: "RHP0301",
        title: "Interface contract violation",
        summary: "A class does not implement all required methods of an interface.",
        explanation: "Classes implementing an interface must provide compatible concrete implementations for every method in the interface.",
        example: Some("class Service implements Runnable {} // missing run()"),
        fix: Some("Implement all missing interface methods with compatible signatures."),
    },
    DiagnosticCodeInfo {
        code: "RHP0302",
        title: "Method override contract violation",
        summary: "A child class method signature is incompatible with the parent method.",
        explanation: "Method overrides must satisfy LSP (Liskov Substitution Principle): parameter types must be contravariant and return types covariant.",
        example: None,
        fix: Some("Ensure parameter and return types match or are compatible with parent method."),
    },
    DiagnosticCodeInfo {
        code: "RHP0303",
        title: "Constructor cannot declare return type",
        summary: "Class constructors '__construct' and destructors '__destruct' cannot declare return types.",
        explanation: "Constructors always return the instantiated object implicitly and destructors return nothing.",
        example: Some("class User {\n    public function __construct(): int {}\n}"),
        fix: Some("Remove the return type declaration from '__construct' or '__destruct'."),
    },
    DiagnosticCodeInfo {
        code: "RHP0304",
        title: "Trait method collision",
        summary: "Multiple traits provide conflicting methods without resolution.",
        explanation: "When two traits define a method with the same name, the conflict must be resolved using 'insteadof'.",
        example: Some("class App {\n    use TraitA, TraitB;\n}"),
        fix: Some("Use 'insteadof' in the trait use block to resolve the collision:\nuse TraitA, TraitB {\n    TraitA::method insteadof TraitB;\n}"),
    },
    DiagnosticCodeInfo {
        code: "RHP0305",
        title: "Trait adaptation error",
        summary: "An error occurred while adapting trait methods with 'insteadof' or 'as'.",
        explanation: "Trait adaptation clauses must reference valid trait names and method names.",
        example: None,
        fix: Some("Verify trait and method names in trait adaptation clauses."),
    },

    // --- 0400 - 0499: Lint & Best-practice warnings ---
    DiagnosticCodeInfo {
        code: "RHP0401",
        title: "Deprecated direct superglobal usage",
        summary: "Direct access to legacy superglobals ($_GET, $_POST, $_COOKIE, $_SESSION) is discouraged.",
        explanation: "RHP provides structured, isolated static facades ('Request::get()', 'Session::get()') that guarantee fiber isolation and thread safety.",
        example: Some("$user = $_GET['user'];"),
        fix: Some("Replace '$_GET' with 'Request::get()' or 'Request::all()':\n$user = Request::get('user');"),
    },
    DiagnosticCodeInfo {
        code: "RHP0402",
        title: "Identifier naming convention violation",
        summary: "Identifier does not follow RHP naming conventions.",
        explanation: "Variables must use snake_case, classes/interfaces/traits PascalCase, and methods/functions camelCase.",
        example: Some("$MyVariable = 10;"),
        fix: Some("Rename the identifier according to convention (e.g. '$my_variable')."),
    },

    // --- 0500 - 0599: Compiler Extension & Macro Expansion errors ---
    DiagnosticCodeInfo {
        code: "RHP0501",
        title: "Unknown compiler extension",
        summary: "An unrecognized attribute or template directive was encountered.",
        explanation: "The attribute `#[name]` or directive `@name` is not registered in the expander registry. Check for typos or ensure the defining package is registered in rhp.toml.",
        example: Some("#[unknown_attr]\nclass User {}"),
        fix: Some("Register the expander handler or correct the attribute/directive name."),
    },
    DiagnosticCodeInfo {
        code: "RHP0502",
        title: "Extension argument mismatch",
        summary: "Invalid argument count or type passed to an attribute or directive.",
        explanation: "The arguments provided to `#[attr(...)]` or `@directive(...)` do not match what the handler expects.",
        example: Some("#[table()]\nclass User {}"),
        fix: Some("Provide the expected arguments as documented by the extension."),
    },
    DiagnosticCodeInfo {
        code: "RHP0503",
        title: "Unbalanced template directive",
        summary: "A template directive block was opened but not properly closed or matched.",
        explanation: "Block directives such as `@if`, `@for`, and `@foreach` must be terminated with matching `@endif`, `@endfor`, or `@endforeach` directives.",
        example: Some("@if($user->isAdmin)\n  <p>Admin</p>\n<!-- missing @endif -->"),
        fix: Some("Add the corresponding closing directive (e.g. `@endif`)."),
    },
    DiagnosticCodeInfo {
        code: "RHP0504",
        title: "Conflicting macro expansion",
        summary: "Two or more expanders conflict when modifying the same AST node or class member.",
        explanation: "Multiple attribute handlers attempted to modify or generate the same class member or state with incompatible definitions.",
        example: Some("#[fillable('email')]\n#[guarded('email')]\nclass User {}"),
        fix: Some("Remove or reconcile the conflicting attributes."),
    },
    DiagnosticCodeInfo {
        code: "RHP0505",
        title: "Extension handler execution failure",
        summary: "An error or panic occurred while executing an extension expander handler.",
        explanation: "The expander handler threw an error or panicked during AST transformation. The compiler caught the panic to preserve build integrity.",
        example: None,
        fix: Some("Inspect the handler implementation or report the issue to the package author."),
    },
    DiagnosticCodeInfo {
        code: "RHP0506",
        title: "Invalid extension placement",
        summary: "An attribute or directive was placed in an invalid context.",
        explanation: "Attributes `#[...]` are valid only in RHP source code, while directives `@name` are valid only within template files.",
        example: Some("@if($x) echo 1; // inside a pure .rhp file"),
        fix: Some("Use standard RHP control flow in .rhp files and directives inside template files."),
    },
    DiagnosticCodeInfo {
        code: "RHP0507",
        title: "Duplicate extension registration",
        summary: "An attribute or directive handler with the same name was already registered.",
        explanation: "Each attribute or directive name can only have one active handler registered in the compiler expansion registry.",
        example: None,
        fix: Some("Ensure unique handler names across packages and configuration."),
    },

    // --- 9000+: Internal compiler errors ---
    DiagnosticCodeInfo {
        code: "RHP9999",
        title: "Internal compiler error",
        summary: "An unexpected internal error or panic occurred within the RHP compiler.",
        explanation: "This is a bug in rhp. The compiler failed unexpectedly while processing input.\n\nPlease report this issue with the source code that caused it at:\nhttps://github.com/vuenice/rhp/issues",
        example: None,
        fix: Some("This is a compiler defect, not an error in your code. Please report it with your rhp version and minimal repro."),
    },
];

/// Look up detailed diagnostic explanation by code string (case-insensitive, optional "RHP" prefix).
pub fn get_code_info(code: &str) -> Option<&'static DiagnosticCodeInfo> {
    let clean = code.trim().to_ascii_uppercase();
    let normalized = if clean.starts_with("RHP") {
        clean
    } else {
        format!("RHP{}", clean)
    };

    ALL_DIAGNOSTIC_CODES.iter().find(|c| c.code == normalized)
}

/// Return all registered diagnostic codes.
pub fn all_diagnostic_codes() -> &'static [DiagnosticCodeInfo] {
    ALL_DIAGNOSTIC_CODES
}

/// Render code explanation in human-readable terminal format.
pub fn format_code_explanation(info: &DiagnosticCodeInfo) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}: {}\n", info.code, info.title));
    out.push_str(&"=".repeat(info.code.len() + info.title.len() + 2));
    out.push_str("\n\n");
    out.push_str(info.summary);
    out.push_str("\n\n");
    out.push_str(info.explanation);
    out.push('\n');

    if let Some(ex) = info.example {
        out.push_str("\nExample triggering this error:\n");
        for line in ex.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }

    if let Some(fix) = info.fix {
        out.push_str("\nHow to fix:\n");
        for line in fix.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}
