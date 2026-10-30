use std::collections::HashMap;

use crate::ast::{
    expr::{Expr, ExprKind, Literal, StringPart},
    stmt::{
        ClassDecl, ClassMemberKind, EnumDecl, FunctionDecl, InterfaceDecl, Param, Stmt,
        StmtKind, TraitDecl,
    },
    types::{TypeHint, TypeHintKind, Variable},
    Program,
};
use crate::lexer::span::Span;
use crate::lint::reporter::{Issue, IssueLevel};
use crate::parser::parse;

/// Split an identifier into component words.
/// Handles camelCase, snake_case, PascalCase, and acronyms (e.g. `XMLParser`).
pub fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let c = chars[i];
        if c == '_' || c == '-' || c.is_whitespace() {
            if !current.is_empty() {
                words.push(current);
                current = String::new();
            }
            i += 1;
            continue;
        }

        if c.is_uppercase() {
            if !current.is_empty() {
                let prev = chars[i - 1];
                let next_is_lower = (i + 1 < len) && chars[i + 1].is_lowercase();
                if prev.is_lowercase() || (prev.is_uppercase() && next_is_lower) {
                    words.push(current);
                    current = String::new();
                }
            }
        }

        current.push(c);
        i += 1;
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

/// Convert identifier to `snake_case`.
pub fn to_snake_case(s: &str) -> String {
    let leading = s.chars().take_while(|&c| c == '_').count();
    let trailing = s.chars().rev().take_while(|&c| c == '_').count();
    if leading == s.len() {
        return s.to_string();
    }
    let inner = &s[leading..s.len() - trailing];
    let words = split_words(inner);
    if words.is_empty() {
        return s.to_string();
    }
    let snake = words
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_");
    format!("{}{}{}", "_".repeat(leading), snake, "_".repeat(trailing))
}

/// Convert identifier to `PascalCase`.
pub fn to_pascal_case(s: &str) -> String {
    if s.contains('\\') {
        let has_leading = s.starts_with('\\');
        let parts: Vec<String> = s
            .split('\\')
            .filter(|p| !p.is_empty())
            .map(|p| to_pascal_case_single(p))
            .collect();
        let joined = parts.join("\\");
        if has_leading {
            format!("\\{}", joined)
        } else {
            joined
        }
    } else {
        to_pascal_case_single(s)
    }
}

fn to_pascal_case_single(s: &str) -> String {
    let words = split_words(s);
    if words.is_empty() {
        return s.to_string();
    }
    let mut result = String::new();
    for word in words {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            for c in chars {
                result.extend(c.to_lowercase());
            }
        }
    }
    result
}

/// Convert identifier to `camelCase`.
pub fn to_camel_case(s: &str) -> String {
    let words = split_words(s);
    if words.is_empty() {
        return s.to_string();
    }
    let mut result = String::new();
    for (i, word) in words.into_iter().enumerate() {
        if i == 0 {
            result.push_str(&word.to_lowercase());
        } else {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                for c in chars {
                    result.extend(c.to_lowercase());
                }
            }
        }
    }
    result
}

/// Returns true if identifier is already in `snake_case`.
pub fn is_snake_case(s: &str) -> bool {
    s == to_snake_case(s)
}

/// Returns true if identifier is already in `PascalCase`.
pub fn is_pascal_case(s: &str) -> bool {
    s == to_pascal_case(s)
}

/// Returns true if identifier is already in `camelCase`.
pub fn is_camel_case(s: &str) -> bool {
    s == to_camel_case(s)
}

/// Returns true if identifier is a PHP superglobal or special variable ($this).
pub fn is_special_var(bare: &str) -> bool {
    matches!(
        bare,
        "this"
            | "_GET"
            | "_POST"
            | "_SERVER"
            | "_ENV"
            | "_FILES"
            | "_COOKIE"
            | "_SESSION"
            | "_REQUEST"
            | "GLOBALS"
    )
}

/// Returns true if function name is a PHP magic method (e.g. `__construct`).
pub fn is_magic_method(name: &str) -> bool {
    name.starts_with("__")
}

/// Returns true if function name starts with "test" (case-insensitive).
pub fn is_test_function(name: &str) -> bool {
    name.to_lowercase().starts_with("test")
}

/// Returns true if identifier is a builtin type keyword or core facade.
pub fn is_builtin_type_or_facade(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "string"
            | "bool"
            | "void"
            | "never"
            | "mixed"
            | "array"
            | "iterable"
            | "object"
            | "callable"
            | "self"
            | "static"
            | "parent"
            | "list"
            | "dict"
            | "set"
            | "hashmap"
            | "hashMap"
            | "HashMap"
            | "Future"
            | "Arr"
            | "Str"
            | "Crypt"
            | "Html"
            | "Carbon"
            | "Url"
            | "URL"
            | "File"
            | "AFile"
            | "Http"
            | "HTTP"
            | "AHttp"
            | "AHTTP"
            | "Db"
            | "DB"
            | "ADb"
            | "ADB"
            | "Cache"
            | "Log"
            | "Reflect"
            | "Env"
            | "Task"
            | "Channel"
            | "Exception"
            | "Error"
            | "Throwable"
            | "Closure"
            | "Fiber"
            | "Generator"
            | "stdClass"
            | "Traversable"
            | "IteratorAggregate"
            | "Iterator"
            | "ArrayIterator"
            | "Countable"
            | "ArrayAccess"
            | "LengthAwarePaginator"
            | "Paginator"
            | "Collection"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    )
}

/// Renames all variable names to snake_case, class names to PascalCase,
/// and function names to camelCase (except test* functions which are in snake_case).
///
/// Returns the updated source code along with any casing diagnostic issues.
pub fn apply_casing(source: &str, file_name: &str) -> (String, Vec<Issue>) {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return (source.to_string(), Vec::new()),
    };

    let mut collector = CasingCollector::new(file_name);
    collector.collect(&program);

    if collector.edits.is_empty() {
        return (source.to_string(), collector.issues);
    }

    // Sort edits by byte offset descending so modifying the string doesn't invalidate earlier offsets
    collector.edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    collector.edits.dedup_by(|a, b| a.0.start == b.0.start && a.0.end == b.0.end);

    let mut result = source.to_string();
    for (span, replacement) in collector.edits {
        if span.end <= result.len() && span.start <= span.end {
            result.replace_range(span.start..span.end, &replacement);
        }
    }

    (result, collector.issues)
}

/// Collect casing issues from source without modifying it.
pub fn check_casing(source: &str, file_name: &str) -> Vec<Issue> {
    let program = match parse(source) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut collector = CasingCollector::new(file_name);
    collector.collect(&program);
    collector.issues
}

struct CasingCollector<'a> {
    file_name: &'a str,
    edits: Vec<(Span, String)>,
    issues: Vec<Issue>,
    declared_classes: HashMap<String, String>,
    declared_functions: HashMap<String, String>,
    declared_methods: HashMap<String, String>,
    declared_properties: HashMap<String, String>,
}

impl<'a> CasingCollector<'a> {
    fn new(file_name: &'a str) -> Self {
        Self {
            file_name,
            edits: Vec::new(),
            issues: Vec::new(),
            declared_classes: HashMap::new(),
            declared_functions: HashMap::new(),
            declared_methods: HashMap::new(),
            declared_properties: HashMap::new(),
        }
    }

    fn collect(&mut self, program: &Program<'_>) {
        // Pass 1: Pre-populate registry of declared classes, functions, methods, properties
        self.register_declarations(&program.statements);

        // Pass 2: Traverse AST and generate edits and diagnostic warnings
        for stmt in &program.statements {
            self.visit_stmt(stmt);
        }

        // Deduplicate issues by span start and code
        self.issues.sort_by_key(|i| i.span.start);
        self.issues.dedup_by(|a, b| a.span.start == b.span.start && a.code == b.code);
    }

    fn register_declarations(&mut self, stmts: &[Stmt<'_>]) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::ClassDecl(cls) => {
                    let orig = cls.name.name;
                    let target = to_pascal_case(orig);
                    self.declared_classes.insert(orig.to_string(), target);

                    for member in &cls.members {
                        match &member.kind {
                            ClassMemberKind::Property(prop) => {
                                let bare = prop.name.bare_name();
                                let target_prop = to_snake_case(bare);
                                self.declared_properties.insert(bare.to_string(), target_prop);
                            }
                            ClassMemberKind::Method(method) => {
                                let orig_m = method.function.name.name;
                                if !is_magic_method(orig_m) {
                                    let target_m = if is_test_function(orig_m) {
                                        to_snake_case(orig_m)
                                    } else {
                                        to_camel_case(orig_m)
                                    };
                                    self.declared_methods.insert(orig_m.to_string(), target_m);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                StmtKind::InterfaceDecl(iface) => {
                    let orig = iface.name.name;
                    let target = to_pascal_case(orig);
                    self.declared_classes.insert(orig.to_string(), target);
                    for method in &iface.methods {
                        let orig_m = method.function.name.name;
                        if !is_magic_method(orig_m) {
                            let target_m = if is_test_function(orig_m) {
                                to_snake_case(orig_m)
                            } else {
                                to_camel_case(orig_m)
                            };
                            self.declared_methods.insert(orig_m.to_string(), target_m);
                        }
                    }
                }
                StmtKind::TraitDecl(tr) => {
                    let orig = tr.name.name;
                    let target = to_pascal_case(orig);
                    self.declared_classes.insert(orig.to_string(), target);
                }
                StmtKind::EnumDecl(en) => {
                    let orig = en.name.name;
                    let target = to_pascal_case(orig);
                    self.declared_classes.insert(orig.to_string(), target);
                    for method in &en.methods {
                        let orig_m = method.function.name.name;
                        if !is_magic_method(orig_m) {
                            let target_m = if is_test_function(orig_m) {
                                to_snake_case(orig_m)
                            } else {
                                to_camel_case(orig_m)
                            };
                            self.declared_methods.insert(orig_m.to_string(), target_m);
                        }
                    }
                }
                StmtKind::FunctionDecl(func) => {
                    let orig_f = func.name.name;
                    if !is_magic_method(orig_f) {
                        let target_f = if is_test_function(orig_f) {
                            to_snake_case(orig_f)
                        } else {
                            to_camel_case(orig_f)
                        };
                        self.declared_functions.insert(orig_f.to_string(), target_f);
                    }
                }
                StmtKind::Namespace { body: Some(b), .. } | StmtKind::Block(b) => {
                    self.register_declarations(b);
                }
                _ => {}
            }
        }
    }

    fn visit_var(&mut self, var: &Variable<'_>) {
        let bare = var.bare_name();
        if is_special_var(bare) {
            return;
        }
        let target_bare = to_snake_case(bare);
        if bare != target_bare {
            let target_var = format!("${}", target_bare);
            self.edits.push((var.span, target_var.clone()));
            self.issues.push(Issue::new(
                IssueLevel::Warning,
                "variable-casing",
                format!("Variable '{}' should be in snake_case ('{}').", var.name, target_var),
                self.file_name,
                var.span.line,
                var.span.col,
                var.span.start,
                var.span.end,
            ));
        }
    }

    fn visit_type_hint(&mut self, hint: &TypeHint<'_>) {
        match &hint.kind {
            TypeHintKind::Named(id) => {
                if !is_builtin_type_or_facade(id.name) {
                    let target = to_pascal_case(id.name);
                    if id.name != target {
                        self.edits.push((id.span, target.clone()));
                        self.issues.push(Issue::new(
                            IssueLevel::Warning,
                            "class-casing",
                            format!("Class '{}' should be in PascalCase ('{}').", id.name, target),
                            self.file_name,
                            id.span.line,
                            id.span.col,
                            id.span.start,
                            id.span.end,
                        ));
                    }
                }
            }
            TypeHintKind::Nullable(inner) => self.visit_type_hint(inner),
            TypeHintKind::Union(types) | TypeHintKind::Intersection(types) => {
                for t in types {
                    self.visit_type_hint(t);
                }
            }
            _ => {}
        }
    }

    fn visit_param(&mut self, param: &Param<'_>) {
        self.visit_var(&param.name);
        if let Some(h) = &param.type_hint {
            self.visit_type_hint(h);
        }
        if let Some(d) = &param.default_value {
            self.visit_expr(d);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt<'_>) {
        match &stmt.kind {
            StmtKind::Expr(e) => self.visit_expr(e),
            StmtKind::VarDecl { var, type_hint, init } => {
                self.visit_var(var);
                if let Some(h) = type_hint {
                    self.visit_type_hint(h);
                }
                if let Some(i) = init {
                    self.visit_expr(i);
                }
            }
            StmtKind::Block(stmts) => {
                for s in stmts {
                    self.visit_stmt(s);
                }
            }
            StmtKind::If {
                condition,
                then_branch,
                elseif_branches,
                else_branch,
            } => {
                self.visit_expr(condition);
                self.visit_stmt(then_branch);
                for b in elseif_branches {
                    self.visit_expr(&b.condition);
                    self.visit_stmt(&b.body);
                }
                if let Some(e) = else_branch {
                    self.visit_stmt(e);
                }
            }
            StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
                self.visit_expr(condition);
                self.visit_stmt(body);
            }
            StmtKind::For { init, condition, update, body } => {
                for e in init {
                    self.visit_expr(e);
                }
                for e in condition {
                    self.visit_expr(e);
                }
                for e in update {
                    self.visit_expr(e);
                }
                self.visit_stmt(body);
            }
            StmtKind::Foreach { expr, key_var, value_var, body }
            | StmtKind::ForIn { expr, key_var, value_var, body } => {
                self.visit_expr(expr);
                if let Some(k) = key_var {
                    self.visit_var(k);
                }
                self.visit_expr(value_var);
                self.visit_stmt(body);
            }
            StmtKind::Loop { body } => {
                self.visit_stmt(body);
            }
            StmtKind::Switch { condition, cases } => {
                self.visit_expr(condition);
                for c in cases {
                    if let Some(v) = &c.value {
                        self.visit_expr(v);
                    }
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            StmtKind::Break(opt) | StmtKind::Continue(opt) | StmtKind::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            StmtKind::Throw(e) => self.visit_expr(e),
            StmtKind::TryCatch { try_block, catches, finally_block } => {
                for s in try_block {
                    self.visit_stmt(s);
                }
                for c in catches {
                    for t in &c.types {
                        if !is_builtin_type_or_facade(t.name) {
                            let target = to_pascal_case(t.name);
                            if t.name != target {
                                self.edits.push((t.span, target.clone()));
                                self.issues.push(Issue::new(
                                    IssueLevel::Warning,
                                    "class-casing",
                                    format!("Class '{}' should be in PascalCase ('{}').", t.name, target),
                                    self.file_name,
                                    t.span.line,
                                    t.span.col,
                                    t.span.start,
                                    t.span.end,
                                ));
                            }
                        }
                    }
                    self.visit_var(&c.var);
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(fin) = finally_block {
                    for s in fin {
                        self.visit_stmt(s);
                    }
                }
            }
            StmtKind::FunctionDecl(func) => {
                self.handle_function_decl(func);
            }
            StmtKind::ClassDecl(cls) => {
                self.handle_class_decl(cls);
            }
            StmtKind::InterfaceDecl(iface) => {
                self.handle_interface_decl(iface);
            }
            StmtKind::TraitDecl(tr) => {
                self.handle_trait_decl(tr);
            }
            StmtKind::EnumDecl(en) => {
                self.handle_enum_decl(en);
            }
            StmtKind::Namespace { body, .. } => {
                if let Some(b) = body {
                    for s in b {
                        self.visit_stmt(s);
                    }
                }
            }
            StmtKind::Global(vars) => {
                for v in vars {
                    self.visit_var(v);
                }
            }
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.visit_expr(e);
                }
            }
            StmtKind::Defer(s) => self.visit_stmt(s),
            StmtKind::Empty | StmtKind::Use(_) | StmtKind::RustImport { .. } => {}
        }
    }

    fn handle_function_decl(&mut self, func: &FunctionDecl<'_>) {
        let name = func.name.name;
        if !is_magic_method(name) {
            let target = if is_test_function(name) {
                to_snake_case(name)
            } else {
                to_camel_case(name)
            };
            if name != target {
                self.edits.push((func.name.span, target.clone()));
                let msg = if is_test_function(name) {
                    format!("Test function '{}' should be in snake_case ('{}').", name, target)
                } else {
                    format!("Function '{}' should be in camelCase ('{}').", name, target)
                };
                self.issues.push(Issue::new(
                    IssueLevel::Warning,
                    "function-casing",
                    msg,
                    self.file_name,
                    func.name.span.line,
                    func.name.span.col,
                    func.name.span.start,
                    func.name.span.end,
                ));
            }
        }

        for p in &func.params {
            self.visit_param(p);
        }
        if let Some(h) = &func.return_type {
            self.visit_type_hint(h);
        }
        if let Some(body) = &func.body {
            for s in body {
                self.visit_stmt(s);
            }
        }
    }

    fn handle_class_decl(&mut self, cls: &ClassDecl<'_>) {
        let name = cls.name.name;
        if name.starts_with("class@anonymous") {
            return;
        }
        let target = to_pascal_case(name);
        if name != target {
            self.edits.push((cls.name.span, target.clone()));
            self.issues.push(Issue::new(
                IssueLevel::Warning,
                "class-casing",
                format!("Class '{}' should be in PascalCase ('{}').", name, target),
                self.file_name,
                cls.name.span.line,
                cls.name.span.col,
                cls.name.span.start,
                cls.name.span.end,
            ));
        }

        if let Some(ext) = &cls.extends {
            if !is_builtin_type_or_facade(ext.name) {
                let target_ext = to_pascal_case(ext.name);
                if ext.name != target_ext {
                    self.edits.push((ext.span, target_ext));
                }
            }
        }
        for imp in &cls.implements {
            if !is_builtin_type_or_facade(imp.name) {
                let target_imp = to_pascal_case(imp.name);
                if imp.name != target_imp {
                    self.edits.push((imp.span, target_imp));
                }
            }
        }

        for member in &cls.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    self.visit_var(&prop.name);
                    if let Some(h) = &prop.type_hint {
                        self.visit_type_hint(h);
                    }
                    if let Some(d) = &prop.default {
                        self.visit_expr(d);
                    }
                }
                ClassMemberKind::Method(method) => {
                    let m_name = method.function.name.name;
                    if !is_magic_method(m_name) {
                        let target_m = if is_test_function(m_name) {
                            to_snake_case(m_name)
                        } else {
                            to_camel_case(m_name)
                        };
                        if m_name != target_m {
                            self.edits.push((method.function.name.span, target_m.clone()));
                            let msg = if is_test_function(m_name) {
                                format!("Test function '{}' in method '{}' should be in snake_case ('{}').", m_name, method.function.name.name, target_m)
                            } else {
                                format!("Method '{}' should be in camelCase ('{}').", m_name, target_m)
                            };
                            self.issues.push(Issue::new(
                                IssueLevel::Warning,
                                "function-casing",
                                msg,
                                self.file_name,
                                method.function.name.span.line,
                                method.function.name.span.col,
                                method.function.name.span.start,
                                method.function.name.span.end,
                            ));
                        }
                    }

                    for p in &method.function.params {
                        self.visit_param(p);
                    }
                    if let Some(h) = &method.function.return_type {
                        self.visit_type_hint(h);
                    }
                    if let Some(body) = &method.function.body {
                        for s in body {
                            self.visit_stmt(s);
                        }
                    }
                }
                ClassMemberKind::Constant(c) => {
                    self.visit_expr(&c.value);
                }
                ClassMemberKind::TraitUse(tu) => {
                    for tr in &tu.traits {
                        if !is_builtin_type_or_facade(tr.name) {
                            let target_tr = to_pascal_case(tr.name);
                            if tr.name != target_tr {
                                self.edits.push((tr.span, target_tr));
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_interface_decl(&mut self, iface: &InterfaceDecl<'_>) {
        let name = iface.name.name;
        let target = to_pascal_case(name);
        if name != target {
            self.edits.push((iface.name.span, target.clone()));
            self.issues.push(Issue::new(
                IssueLevel::Warning,
                "class-casing",
                format!("Interface '{}' should be in PascalCase ('{}').", name, target),
                self.file_name,
                iface.name.span.line,
                iface.name.span.col,
                iface.name.span.start,
                iface.name.span.end,
            ));
        }

        for ext in &iface.extends {
            if !is_builtin_type_or_facade(ext.name) {
                let target_ext = to_pascal_case(ext.name);
                if ext.name != target_ext {
                    self.edits.push((ext.span, target_ext));
                }
            }
        }

        for method in &iface.methods {
            let m_name = method.function.name.name;
            if !is_magic_method(m_name) {
                let target_m = if is_test_function(m_name) {
                    to_snake_case(m_name)
                } else {
                    to_camel_case(m_name)
                };
                if m_name != target_m {
                    self.edits.push((method.function.name.span, target_m.clone()));
                    self.issues.push(Issue::new(
                        IssueLevel::Warning,
                        "function-casing",
                        format!("Method '{}' should be in camelCase ('{}').", m_name, target_m),
                        self.file_name,
                        method.function.name.span.line,
                        method.function.name.span.col,
                        method.function.name.span.start,
                        method.function.name.span.end,
                    ));
                }
            }
            for p in &method.function.params {
                self.visit_param(p);
            }
            if let Some(h) = &method.function.return_type {
                self.visit_type_hint(h);
            }
        }
    }

    fn handle_trait_decl(&mut self, tr: &TraitDecl<'_>) {
        let name = tr.name.name;
        let target = to_pascal_case(name);
        if name != target {
            self.edits.push((tr.name.span, target.clone()));
            self.issues.push(Issue::new(
                IssueLevel::Warning,
                "class-casing",
                format!("Trait '{}' should be in PascalCase ('{}').", name, target),
                self.file_name,
                tr.name.span.line,
                tr.name.span.col,
                tr.name.span.start,
                tr.name.span.end,
            ));
        }

        for member in &tr.members {
            if let ClassMemberKind::Method(method) = &member.kind {
                let m_name = method.function.name.name;
                if !is_magic_method(m_name) {
                    let target_m = if is_test_function(m_name) {
                        to_snake_case(m_name)
                    } else {
                        to_camel_case(m_name)
                    };
                    if m_name != target_m {
                        self.edits.push((method.function.name.span, target_m));
                    }
                }
                for p in &method.function.params {
                    self.visit_param(p);
                }
                if let Some(h) = &method.function.return_type {
                    self.visit_type_hint(h);
                }
                if let Some(body) = &method.function.body {
                    for s in body {
                        self.visit_stmt(s);
                    }
                }
            }
        }
    }

    fn handle_enum_decl(&mut self, en: &EnumDecl<'_>) {
        let name = en.name.name;
        let target = to_pascal_case(name);
        if name != target {
            self.edits.push((en.name.span, target.clone()));
            self.issues.push(Issue::new(
                IssueLevel::Warning,
                "class-casing",
                format!("Enum '{}' should be in PascalCase ('{}').", name, target),
                self.file_name,
                en.name.span.line,
                en.name.span.col,
                en.name.span.start,
                en.name.span.end,
            ));
        }

        for imp in &en.implements {
            if !is_builtin_type_or_facade(imp.name) {
                let target_imp = to_pascal_case(imp.name);
                if imp.name != target_imp {
                    self.edits.push((imp.span, target_imp));
                }
            }
        }

        for case in &en.cases {
            if let Some(v) = &case.value {
                self.visit_expr(v);
            }
        }

        for method in &en.methods {
            let m_name = method.function.name.name;
            if !is_magic_method(m_name) {
                let target_m = if is_test_function(m_name) {
                    to_snake_case(m_name)
                } else {
                    to_camel_case(m_name)
                };
                if m_name != target_m {
                    self.edits.push((method.function.name.span, target_m));
                }
            }
            for p in &method.function.params {
                self.visit_param(p);
            }
            if let Some(h) = &method.function.return_type {
                self.visit_type_hint(h);
            }
            if let Some(body) = &method.function.body {
                for s in body {
                    self.visit_stmt(s);
                }
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expr<'_>) {
        match &expr.kind {
            ExprKind::Literal(lit) => {
                if let Literal::InterpolatedString { parts, .. } = lit {
                    for part in parts {
                        if let StringPart::Expr(e) = part {
                            self.visit_expr(e);
                        }
                    }
                }
            }
            ExprKind::Variable(var) => self.visit_var(var),
            ExprKind::Ident(_) => {}
            ExprKind::Unary { expr, .. } | ExprKind::Clone(expr) | ExprKind::Cast { expr, .. } => {
                self.visit_expr(expr);
            }
            ExprKind::Binary { left, right, .. } | ExprKind::Assign { target: left, value: right, .. } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExprKind::Ternary { condition, if_true, if_false } => {
                self.visit_expr(condition);
                if let Some(t) = if_true {
                    self.visit_expr(t);
                }
                self.visit_expr(if_false);
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Ident(id) = &callee.kind {
                    if let Some(renamed) = self.declared_functions.get(id.name) {
                        if id.name != renamed {
                            self.edits.push((id.span, renamed.clone()));
                        }
                    }
                } else {
                    self.visit_expr(callee);
                }
                for a in args {
                    self.visit_expr(&a.value);
                }
            }
            ExprKind::MethodCall { object, method, args, .. } => {
                self.visit_expr(object);
                if let Some(renamed) = self.declared_methods.get(method.name) {
                    if method.name != renamed {
                        self.edits.push((method.span, renamed.clone()));
                    }
                }
                for a in args {
                    self.visit_expr(&a.value);
                }
            }
            ExprKind::StaticCall { target, method, args } => {
                if let ExprKind::Ident(target_id) = &target.kind {
                    if !is_builtin_type_or_facade(target_id.name) {
                        let t = to_pascal_case(target_id.name);
                        if target_id.name != t {
                            self.edits.push((target_id.span, t));
                        }
                    }
                } else {
                    self.visit_expr(target);
                }
                if let Some(renamed) = self.declared_methods.get(method.name) {
                    if method.name != renamed {
                        self.edits.push((method.span, renamed.clone()));
                    }
                }
                for a in args {
                    self.visit_expr(&a.value);
                }
            }
            ExprKind::PropertyFetch { object, property, .. } => {
                self.visit_expr(object);
                if let Some(renamed) = self.declared_properties.get(property.name) {
                    if property.name != renamed {
                        self.edits.push((property.span, renamed.clone()));
                    }
                }
            }
            ExprKind::StaticPropertyFetch { class, property } => {
                if !is_builtin_type_or_facade(class.name) {
                    let t = to_pascal_case(class.name);
                    if class.name != t {
                        self.edits.push((class.span, t));
                    }
                }
                self.visit_var(property);
            }
            ExprKind::ClassConstFetch { class, .. } => {
                if !is_builtin_type_or_facade(class.name) {
                    let t = to_pascal_case(class.name);
                    if class.name != t {
                        self.edits.push((class.span, t));
                    }
                }
            }
            ExprKind::Array(elements) => {
                for el in elements {
                    if let Some(k) = &el.key {
                        self.visit_expr(k);
                    }
                    self.visit_expr(&el.value);
                }
            }
            ExprKind::ArrayIndex { array, index } => {
                self.visit_expr(array);
                if let Some(i) = index {
                    self.visit_expr(i);
                }
            }
            ExprKind::List(elements) => {
                for el in elements {
                    if let Some(item) = el {
                        if let Some(k) = &item.key {
                            self.visit_expr(k);
                        }
                        self.visit_expr(&item.value);
                    }
                }
            }
            ExprKind::ArrowFunction { params, return_type, body } => {
                for p in params {
                    self.visit_param(p);
                }
                if let Some(h) = return_type {
                    self.visit_type_hint(h);
                }
                self.visit_expr(body);
            }
            ExprKind::Closure { params, uses, return_type, body, .. } => {
                for p in params {
                    self.visit_param(p);
                }
                for u in uses {
                    self.visit_var(&u.var);
                }
                if let Some(h) = return_type {
                    self.visit_type_hint(h);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            ExprKind::Match { condition, arms } => {
                self.visit_expr(condition);
                for arm in arms {
                    for cond in &arm.patterns {
                        self.visit_expr(cond);
                    }
                    self.visit_expr(&arm.body);
                }
            }
            ExprKind::New { class, args } => {
                if let ExprKind::Ident(id) = &class.kind {
                    if !is_builtin_type_or_facade(id.name) {
                        let t = to_pascal_case(id.name);
                        if id.name != t {
                            self.edits.push((id.span, t));
                        }
                    }
                } else {
                    self.visit_expr(class);
                }
                for a in args {
                    self.visit_expr(&a.value);
                }
            }
            ExprKind::AnonymousClass { class_decl, args } => {
                self.handle_class_decl(class_decl);
                for a in args {
                    self.visit_expr(&a.value);
                }
            }
            ExprKind::Instanceof { expr, class } => {
                self.visit_expr(expr);
                if let ExprKind::Ident(id) = &class.kind {
                    if !is_builtin_type_or_facade(id.name) {
                        let t = to_pascal_case(id.name);
                        if id.name != t {
                            self.edits.push((id.span, t));
                        }
                    }
                } else {
                    self.visit_expr(class);
                }
            }
            ExprKind::Yield { key, value, .. } => {
                if let Some(k) = key {
                    self.visit_expr(k);
                }
                if let Some(v) = value {
                    self.visit_expr(v);
                }
            }
            ExprKind::Await(expr) | ExprKind::Grouping(expr) => {
                self.visit_expr(expr);
            }
        }
    }
}
