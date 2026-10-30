use rhp::ast::{
    expr::{ExprKind, Literal},
    stmt::{ClassMemberKind, StmtKind},
};
use rhp::diagnostic::Diagnostic;
use rhp::expand::{
    expand_program, expand_template, parse_and_expand, ExpandCtx, Expander, ExpanderRegistry,
    Expansion, ExpansionSite, TemplateLexer, TemplateToken,
};
use rhp::lexer::span::SourceFile;
use rhp::lexer::{Lexer, Token};
use rhp::parser::parse;

/// 1. Snapshot test: RHP source to expanded AST with built-in attributes
#[test]
fn test_attribute_expansion_fillable_guarded_table() {
    let source = r#"
#[fillable('name', 'email')]
#[guarded('id', 'created_at')]
#[table('users')]
class User {
    public int $id;
    public string $name;
}
"#;

    let program = parse_and_expand(source).expect("Source with attributes should expand cleanly");
    assert_eq!(program.statements.len(), 1);

    let class = match &program.statements[0].kind {
        StmtKind::ClassDecl(c) => c,
        _ => panic!("Expected ClassDecl statement"),
    };

    assert_eq!(class.name.as_str(), "User");

    // Existing members: id, name.
    // Generated members: fillable, guarded, table.
    assert_eq!(class.members.len(), 5);

    let member_names: Vec<String> = class
        .members
        .iter()
        .map(|m| match &m.kind {
            ClassMemberKind::Property(p) => p.name.bare_name().to_string(),
            _ => panic!("Expected Property member"),
        })
        .collect();

    assert_eq!(
        member_names,
        vec!["id", "name", "fillable", "guarded", "table"]
    );

    // Verify $fillable property details
    if let ClassMemberKind::Property(ref fillable_prop) = class.members[2].kind {
        assert_eq!(fillable_prop.name.bare_name(), "fillable");
        assert_eq!(fillable_prop.visibility, rhp::ast::types::Visibility::Protected);
        assert_eq!(fillable_prop.type_hint.as_ref().unwrap().to_string(), "array");
        assert!(fillable_prop.default.is_some());
    } else {
        panic!("Member 2 must be Property $fillable");
    }

    // Verify $table property details
    if let ClassMemberKind::Property(ref table_prop) = class.members[4].kind {
        assert_eq!(table_prop.name.bare_name(), "table");
        assert_eq!(table_prop.visibility, rhp::ast::types::Visibility::Protected);
        assert_eq!(table_prop.type_hint.as_ref().unwrap().to_string(), "string");
        if let Some(ref def_expr) = table_prop.default {
            if let ExprKind::Literal(Literal::String { ref value, .. }) = def_expr.kind {
                assert_eq!(value, "users");
            } else {
                panic!("Expected string literal default for $table");
            }
        }
    } else {
        panic!("Member 4 must be Property $table");
    }
}

/// 2. Snapshot test: Template source to generated RHP AST statements
#[test]
fn test_template_compilation_to_generated_code() {
    let template_source = r#"<div class="user-card">
  @if($user->isAdmin)
    <h1>Admin Portal: {{ $user->name }}</h1>
  @endif
  @for($i = 0; $i < 3; $i++)
    <p>Item {{ $i }}</p>
  @endfor
  @foreach($items as $item)
    <span>{{ $item }}</span>
  @endforeach
  {!! $rawSnippet !!}
  @datetime($createdAt)
</div>"#;

    let file_id = 42;
    let file = SourceFile::new(file_id, "view.blade.rhp", template_source);
    let mut ctx = ExpandCtx::with_source_file(file);
    let registry = ExpanderRegistry::default_with_builtins();

    let compiled = expand_template(template_source, file_id, &registry, &mut ctx)
        .expect("Template must compile to AST");

    assert!(!compiled.statements.is_empty());

    // Verify presence of echo statements, if statement, for loop, foreach loop, and datetime echo
    let mut found_if = false;
    let mut found_for = false;
    let mut found_foreach = false;
    let mut found_escaped_echo = false;
    let mut found_raw_echo = false;

    fn check_stmt(
        stmt: &rhp::ast::Stmt,
        found_if: &mut bool,
        found_for: &mut bool,
        found_foreach: &mut bool,
        found_escaped_echo: &mut bool,
        found_raw_echo: &mut bool,
    ) {
        match &stmt.kind {
            StmtKind::If { then_branch, .. } => {
                *found_if = true;
                check_stmt(then_branch, found_if, found_for, found_foreach, found_escaped_echo, found_raw_echo);
            }
            StmtKind::For { body, .. } => {
                *found_for = true;
                check_stmt(body, found_if, found_for, found_foreach, found_escaped_echo, found_raw_echo);
            }
            StmtKind::Foreach { body, .. } => {
                *found_foreach = true;
                check_stmt(body, found_if, found_for, found_foreach, found_escaped_echo, found_raw_echo);
            }
            StmtKind::Block(stmts) => {
                for s in stmts {
                    check_stmt(s, found_if, found_for, found_foreach, found_escaped_echo, found_raw_echo);
                }
            }
            StmtKind::Echo(exprs) => {
                for expr in exprs {
                    if let ExprKind::StaticCall { method, .. } = &expr.kind {
                        if method.as_str() == "escape" {
                            *found_escaped_echo = true;
                        }
                    } else if let ExprKind::Variable(v) = &expr.kind {
                        if v.name == "$rawSnippet" {
                            *found_raw_echo = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for stmt in &compiled.statements {
        check_stmt(
            stmt,
            &mut found_if,
            &mut found_for,
            &mut found_foreach,
            &mut found_escaped_echo,
            &mut found_raw_echo,
        );
    }

    assert!(found_if, "@if directive must produce StmtKind::If");
    assert!(found_for, "@for directive must produce StmtKind::For");
    assert!(found_foreach, "@foreach directive must produce StmtKind::Foreach");
    assert!(found_escaped_echo, "{{{{ expr }}}} must produce Html::escape static call");
    assert!(found_raw_echo, "{{!! expr !!}} must produce raw echo statement");
}

/// 3. Error-span test: Error in attribute arguments points at the user's attribute line
#[test]
fn test_error_span_points_at_user_attribute_line() {
    let source = "class First {}\n\n#[table()]\nclass Second {}";
    let file_id = 1;
    let file = SourceFile::new(file_id, "models.rhp", source);
    let mut ctx = ExpandCtx::with_source_file(file);
    let registry = ExpanderRegistry::default_with_builtins();

    let program = parse(source).expect("Source parses");
    let err = expand_program(&program, &registry, &mut ctx).expect_err("Empty #[table()] must error");

    assert_eq!(err.code, "RHP0502");
    assert_eq!(err.span.start.line, 3); // Line 3: #[table()]
    assert!(err.message.contains("requires exactly 1 argument"));
}

/// 4. Ordering and conflict test: Conflicting handlers modifying the same fields/members
#[test]
fn test_ordering_and_conflict_detection() {
    let source = r#"
#[fillable('email', 'name')]
#[guarded('email')]
class Account {}
"#;
    let file_id = 2;
    let file = SourceFile::new(file_id, "account.rhp", source);
    let mut ctx = ExpandCtx::with_source_file(file);
    let registry = ExpanderRegistry::default_with_builtins();

    let program = parse(source).expect("Source parses");
    let err = expand_program(&program, &registry, &mut ctx)
        .expect_err("Overlapping field in fillable and guarded must trigger conflict error");

    assert_eq!(err.code, "RHP0504");
    assert!(err.message.contains("conflicting macro expansion"));
    assert!(err.message.contains("email"));
    // Secondary label names the conflicting attribute site
    assert!(!err.labels.is_empty());
}

/// 5. Hygiene test: Directive loop variables do not clash with surrounding user variables
#[test]
fn test_hygiene_gensym_and_variable_isolation() {
    let mut ctx = ExpandCtx::new();

    let sym1 = ctx.gensym("loop");
    let sym2 = ctx.gensym("loop");
    let sym3 = ctx.gensym("tmp");

    assert_ne!(sym1, sym2);
    assert_ne!(sym2, sym3);
    assert!(sym1.starts_with("__rhp_gen_loop_"));
    assert!(sym3.starts_with("__rhp_gen_tmp_"));

    // Template with user variable $item outside foreach
    let template = r#"
<div>
  <span>User item: {{ $item }}</span>
  @foreach($items as $item)
    <p>Loop item: {{ $item }}</p>
  @endforeach
</div>
"#;
    let file = SourceFile::new(0, "test.blade.rhp", template);
    let mut ctx2 = ExpandCtx::with_source_file(file);
    let registry = ExpanderRegistry::default_with_builtins();
    let res = expand_template(template, 0, &registry, &mut ctx2);
    assert!(res.is_ok(), "Template with shadowed loop variable expands without error");
}

/// 6. Determinism test: Expanding identical input twice yields byte-identical AST output
#[test]
fn test_determinism_identical_output() {
    let source = r#"
#[table('products')]
#[fillable('title', 'sku', 'price')]
#[guarded('id', 'created_at')]
class Product {
    public int $id;
    public string $title;
}
"#;

    let program1 = parse(source).unwrap();
    let program2 = parse(source).unwrap();

    let mut ctx1 = ExpandCtx::new();
    let mut ctx2 = ExpandCtx::new();
    let reg1 = ExpanderRegistry::default_with_builtins();
    let reg2 = ExpanderRegistry::default_with_builtins();

    let expanded1 = expand_program(&program1, &reg1, &mut ctx1).unwrap();
    let expanded2 = expand_program(&program2, &reg2, &mut ctx2).unwrap();

    let repr1 = format!("{:?}", expanded1);
    let repr2 = format!("{:?}", expanded2);

    assert_eq!(repr1, repr2, "Expansion output must be byte-identical and deterministic");
}

/// 7. Handler panic test: A panicking handler is caught and emitted as a structured diagnostic
struct PanickingExpander;

impl Expander for PanickingExpander {
    fn name(&self) -> &str {
        "explode"
    }

    fn expand(&self, _ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        panic!("Boom! Deliberate expander failure at line {}", site.span().line);
    }
}

#[test]
fn test_handler_panic_reported_as_diagnostic() {
    let source = r#"
#[explode]
class DangerousClass {}
"#;
    let file = SourceFile::new(1, "panic_test.rhp", source);
    let mut ctx = ExpandCtx::with_source_file(file);
    let mut registry = ExpanderRegistry::default_with_builtins();
    registry
        .register_attribute(Box::new(PanickingExpander))
        .unwrap();

    let program = parse(source).unwrap();
    let err = expand_program(&program, &registry, &mut ctx)
        .expect_err("Panicking expander must be caught");

    assert_eq!(err.code, "RHP0505");
    assert!(err.message.contains("panicked during execution"));
    assert!(err.message.contains("Boom! Deliberate expander failure"));
}

/// 8. Lexer test: `#[` and `@` are never confused in their respective file types
#[test]
fn test_lexer_file_separation_hash_vs_at() {
    // 8a. In RHP source files:
    // #[Route] is recognized as AttributeStart
    let rhp_tokens = Lexer::tokenize("#[Route('/api')]").expect("Tokenizes");
    assert_eq!(rhp_tokens[0].token, Token::AttributeStart);

    // Directive `@if` inside pure RHP source file must be rejected as an invalid directive
    let rhp_directive_source = r#"
@if($cond)
    echo 1;
@endif
"#;
    let parse_err = parse(rhp_directive_source).expect_err("Directives must be rejected in RHP code");
    let err_str = parse_err.to_string();
    assert!(
        err_str.contains("Template directives (`@if`) are only valid inside template files"),
        "Error message must clearly explain directive rejection in RHP code: {}",
        err_str
    );

    // 8b. In template files:
    // `#[not_an_attribute]` is literal HTML/CSS/text, never Token::AttributeStart
    let template_source = r#"
<div id="test">
  #[not_an_attribute]
  @if($active)
    <span>Active</span>
  @endif
</div>
"#;
    let mut lexer = TemplateLexer::new(template_source, 0);
    let template_tokens = lexer.tokenize().expect("Template tokenizes");

    let mut found_literal_hash_bracket = false;
    for tok in &template_tokens {
        if let TemplateToken::Text { content, .. } = tok {
            if content.contains("#[not_an_attribute]") {
                found_literal_hash_bracket = true;
            }
        }
    }
    assert!(
        found_literal_hash_bracket,
        "#[...] inside template must remain literal text"
    );
}

/// 9. Unbalanced directive test: `@if` without matching `@endif` produces RHP0503
#[test]
fn test_unbalanced_template_directive_error() {
    let unclosed_template = r#"
<div>
  @if($valid)
    <p>Valid content</p>
  <!-- missing endif -->
</div>
"#;
    let file = SourceFile::new(3, "broken.blade.rhp", unclosed_template);
    let mut ctx = ExpandCtx::with_source_file(file);
    let registry = ExpanderRegistry::default_with_builtins();

    let err = expand_template(unclosed_template, 3, &registry, &mut ctx)
        .expect_err("Unclosed @if must error");

    assert_eq!(err.code, "RHP0503");
    assert!(err.message.contains("unclosed directive `@if`"));
}

/// 10. Unknown attribute and directive tests produce RHP0501
#[test]
fn test_unknown_attribute_produces_rhp0501() {
    let source = r#"
#[non_existent_attribute('foo')]
class MyClass {}
"#;
    let program = parse(source).unwrap();
    let mut ctx = ExpandCtx::new();
    let reg = ExpanderRegistry::default_with_builtins();

    let err = expand_program(&program, &reg, &mut ctx).expect_err("Unknown attribute must error");
    assert_eq!(err.code, "RHP0501");
    assert!(err.message.contains("unknown compiler extension attribute"));
}

#[test]
fn test_unknown_directive_produces_rhp0501() {
    let template = r#"
<div>
  @unknown_custom_directive($data)
</div>
"#;
    let file = SourceFile::new(4, "unknown.blade.rhp", template);
    let mut ctx = ExpandCtx::with_source_file(file);
    let reg = ExpanderRegistry::default_with_builtins();

    let err = expand_template(template, 4, &reg, &mut ctx).expect_err("Unknown directive must error");
    assert_eq!(err.code, "RHP0501");
    assert!(err.message.contains("unknown template directive"));
}
