use rhp::rade::compiler::directives::{
    compile_directive, compile_else, compile_elseif, compile_endfor, compile_endif, compile_for,
    compile_if, is_builtin_directive, DirectiveKind, DirectiveStack,
};
use rhp::rade::{compile, CompileError, Compiler};

#[test]
fn test_if_directive() {
    let input = "@if($active)\n  <span>Active</span>\n@endif";
    let output = compile(input).unwrap();
    assert_eq!(output, "if ($active) {\n  <span>Active</span>\n}");
}

#[test]
fn test_if_else_directive() {
    let input = "@if($active)\n  <span>Active</span>\n@else\n  <span>Inactive</span>\n@endif";
    let output = compile(input).unwrap();
    assert_eq!(
        output,
        "if ($active) {\n  <span>Active</span>\n} else {\n  <span>Inactive</span>\n}"
    );
}

#[test]
fn test_if_elseif_else_directive() {
    let input = r#"@if($score >= 90)
  <span>Grade A</span>
@elseif($score >= 80)
  <span>Grade B</span>
@elseif($score >= 70)
  <span>Grade C</span>
@else
  <span>Grade F</span>
@endif"#;

    let output = compile(input).unwrap();
    let expected = r#"if ($score >= 90) {
  <span>Grade A</span>
} else if ($score >= 80) {
  <span>Grade B</span>
} else if ($score >= 70) {
  <span>Grade C</span>
} else {
  <span>Grade F</span>
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_for_directive() {
    let input = "@for($i = 0; $i < 10; $i++)\n  <li>Item {{ $i }}</li>\n@endfor";
    let output = compile(input).unwrap();
    assert_eq!(
        output,
        "for ($i = 0; $i < 10; $i++) {\n  <li>Item {{ $i }}</li>\n}"
    );
}

#[test]
fn test_nested_if_inside_for() {
    let input = r#"@for($i = 0; $i < 5; $i++)
  @if($i % 2 == 0)
    <span>Even: {{ $i }}</span>
  @else
    <span>Odd: {{ $i }}</span>
  @endif
@endfor"#;

    let output = compile(input).unwrap();
    let expected = r#"for ($i = 0; $i < 5; $i++) {
  if ($i % 2 == 0) {
    <span>Even: {{ $i }}</span>
  } else {
    <span>Odd: {{ $i }}</span>
  }
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_nested_for_inside_if() {
    let input = r#"@if($has_items)
  <ul>
    @for($i = 0; $i < count($items); $i++)
      <li>{{ $items[$i] }}</li>
    @endfor
  </ul>
@endif"#;

    let output = compile(input).unwrap();
    let expected = r#"if ($has_items) {
  <ul>
    for ($i = 0; $i < count($items); $i++) {
      <li>{{ $items[$i] }}</li>
    }
  </ul>
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_deeply_nested_control_flow() {
    let input = r#"@if($auth)
  @for($i = 0; $i < 3; $i++)
    @if($roles[$i] == "admin")
      @for($j = 0; $j < 2; $j++)
        <span>Action {{ $j }}</span>
      @endfor
    @endif
  @endfor
@endif"#;

    let output = compile(input).unwrap();
    let expected = r#"if ($auth) {
  for ($i = 0; $i < 3; $i++) {
    if ($roles[$i] == "admin") {
      for ($j = 0; $j < 2; $j++) {
        <span>Action {{ $j }}</span>
      }
    }
  }
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_raw_rhp_emits_as_is() {
    // Requirement 3: Ensure raw RHP block @rhp ... @endrhp directly emits
    // the inner RHP code as-is without any wrapping or alteration.
    let input = "@rhp\nlet x = 10;\nlet y = 20;\nlet z = x + y;\n@endrhp";
    let output = compile(input).unwrap();
    assert_eq!(output, "let x = 10;\nlet y = 20;\nlet z = x + y;");
}

#[test]
fn test_raw_rhp_inside_control_flow() {
    let input = r#"@if($run_setup)
@rhp
let count = 100;
let initialized = true;
@endrhp
<p>Setup done</p>
@endif"#;

    let output = compile(input).unwrap();
    let expected = r#"if ($run_setup) {
let count = 100;
let initialized = true;
<p>Setup done</p>
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_comments_stripped_during_compilation() {
    let input = "@if($cond)\n  {{-- Ignore this secret --}}\n  <p>Visible</p>\n@endif";
    let output = compile(input).unwrap();
    assert_eq!(output, "if ($cond) {\n  \n  <p>Visible</p>\n}");
}

#[test]
fn test_escaped_verbatim_preserved() {
    let input = "@if($cond)\n  @{{ vueBinding }}\n@endif";
    let output = compile(input).unwrap();
    assert_eq!(output, "if ($cond) {\n  {{ vueBinding }}\n}");
}

#[test]
fn test_error_endif_without_opener() {
    let input = "<div>Hello</div>\n@endif";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedClosingDirective {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "endif");
            assert_eq!(line, 2);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnexpectedClosingDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 2);
    assert_eq!(err.column(), 1);
}

#[test]
fn test_error_endfor_without_opener() {
    let input = "  @endfor";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedClosingDirective {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "endfor");
            assert_eq!(line, 1);
            assert_eq!(column, 3);
        }
        _ => panic!("Expected UnexpectedClosingDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 1);
    assert_eq!(err.column(), 3);
}

#[test]
fn test_error_mismatched_closing_for_with_endif() {
    let input = "@for($i = 0; $i < 10; $i++)\n  <span>Loop</span>\n@endif";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::MismatchedClosingDirective {
            ref expected,
            ref found,
            line,
            column,
        } => {
            assert_eq!(expected, "endfor");
            assert_eq!(found, "endif");
            assert_eq!(line, 3);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected MismatchedClosingDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 3);
    assert_eq!(err.column(), 1);
}

#[test]
fn test_error_mismatched_closing_if_with_endfor() {
    let input = "@if($condition)\n  <span>True</span>\n@endfor";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::MismatchedClosingDirective {
            ref expected,
            ref found,
            line,
            column,
        } => {
            assert_eq!(expected, "endif");
            assert_eq!(found, "endfor");
            assert_eq!(line, 3);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected MismatchedClosingDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 3);
    assert_eq!(err.column(), 1);
}

#[test]
fn test_error_unclosed_if_at_eof() {
    let input = "<div>\n  @if($active)\n    <p>Unclosed</p>\n</div>";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnclosedDirective {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "if");
            assert_eq!(line, 2);
            assert_eq!(column, 3);
        }
        _ => panic!("Expected UnclosedDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 2);
    assert_eq!(err.column(), 3);
}

#[test]
fn test_error_unclosed_for_at_eof() {
    let input = "@for($i = 0; $i < 10; $i++)\n  <p>Loop</p>";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnclosedDirective {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "for");
            assert_eq!(line, 1);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnclosedDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 1);
    assert_eq!(err.column(), 1);
}

#[test]
fn test_error_unclosed_inner_block_at_eof() {
    let input = "@if($cond)\n  @for($i = 0; $i < 5; $i++)\n    <span>Text</span>\n  @endfor";
    // The @if is not closed
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnclosedDirective {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "if");
            assert_eq!(line, 1);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnclosedDirective for 'if', got {err:?}"),
    }
}

#[test]
fn test_error_else_outside_if() {
    let input = "<p>Text</p>\n@else\n<p>More</p>";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedDirective {
            ref directive,
            line,
            column,
            ..
        } => {
            assert_eq!(directive, "else");
            assert_eq!(line, 2);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnexpectedDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 2);
    assert_eq!(err.column(), 1);
}

#[test]
fn test_error_elseif_outside_if() {
    let input = "@elseif($cond)\n<span>Text</span>";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedDirective {
            ref directive,
            line,
            column,
            ..
        } => {
            assert_eq!(directive, "elseif");
            assert_eq!(line, 1);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnexpectedDirective, got {err:?}"),
    }
    assert_eq!(err.line(), 1);
    assert_eq!(err.column(), 1);
}

#[test]
fn test_error_else_inside_for() {
    let input = "@for($i = 0; $i < 10; $i++)\n  @else\n@endfor";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedDirective {
            ref directive,
            line,
            column,
            ..
        } => {
            assert_eq!(directive, "else");
            assert_eq!(line, 2);
            assert_eq!(column, 3);
        }
        _ => panic!("Expected UnexpectedDirective for 'else' inside 'for', got {err:?}"),
    }
}

#[test]
fn test_error_elseif_inside_for() {
    let input = "@for($i = 0; $i < 10; $i++)\n  @elseif($i > 5)\n@endfor";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedDirective {
            ref directive,
            line,
            column,
            ..
        } => {
            assert_eq!(directive, "elseif");
            assert_eq!(line, 2);
            assert_eq!(column, 3);
        }
        _ => panic!("Expected UnexpectedDirective for 'elseif' inside 'for', got {err:?}"),
    }
}

#[test]
fn test_error_multiple_else_in_same_if() {
    let input = "@if($cond)\n  A\n@else\n  B\n@else\n  C\n@endif";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedDirective {
            ref directive,
            line,
            column,
            ..
        } => {
            assert_eq!(directive, "else");
            assert_eq!(line, 5);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnexpectedDirective for duplicate 'else', got {err:?}"),
    }
}

#[test]
fn test_error_elseif_after_else() {
    let input = "@if($cond)\n  A\n@else\n  B\n@elseif($other)\n  C\n@endif";
    let err = compile(input).unwrap_err();
    match err {
        CompileError::UnexpectedDirective {
            ref directive,
            line,
            column,
            ..
        } => {
            assert_eq!(directive, "elseif");
            assert_eq!(line, 5);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected UnexpectedDirective for 'elseif' after 'else', got {err:?}"),
    }
}

#[test]
fn test_error_missing_directive_arguments() {
    // @if without arguments
    let input_if = "@if\n<span>Hello</span>\n@endif";
    let err_if = compile(input_if).unwrap_err();
    match err_if {
        CompileError::MissingDirectiveArgs {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "if");
            assert_eq!(line, 1);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected MissingDirectiveArgs, got {err_if:?}"),
    }

    // @if() with empty arguments
    let input_empty = "@if()\n<span>Hello</span>\n@endif";
    let err_empty = compile(input_empty).unwrap_err();
    match err_empty {
        CompileError::MissingDirectiveArgs { ref directive, .. } => {
            assert_eq!(directive, "if");
        }
        _ => panic!("Expected MissingDirectiveArgs, got {err_empty:?}"),
    }

    // @for without arguments
    let input_for = "@for\n<span>Hello</span>\n@endfor";
    let err_for = compile(input_for).unwrap_err();
    match err_for {
        CompileError::MissingDirectiveArgs {
            ref directive,
            line,
            column,
        } => {
            assert_eq!(directive, "for");
            assert_eq!(line, 1);
            assert_eq!(column, 1);
        }
        _ => panic!("Expected MissingDirectiveArgs, got {err_for:?}"),
    }
}

#[test]
fn test_individual_directive_compiler_functions() {
    assert_eq!(compile_if("$user.is_admin"), "if ($user.is_admin) {");
    assert_eq!(
        compile_elseif("$user.is_moderator"),
        "} else if ($user.is_moderator) {"
    );
    assert_eq!(compile_else(), "} else {");
    assert_eq!(compile_endif(), "}");
    assert_eq!(
        compile_for("$i = 0; $i < 10; $i++"),
        "for ($i = 0; $i < 10; $i++) {"
    );
    assert_eq!(compile_endfor(), "}");

    assert!(is_builtin_directive("if"));
    assert!(is_builtin_directive("elseif"));
    assert!(is_builtin_directive("else"));
    assert!(is_builtin_directive("endif"));
    assert!(is_builtin_directive("for"));
    assert!(is_builtin_directive("endfor"));
    assert!(is_builtin_directive("foreach"));
    assert!(is_builtin_directive("endforeach"));
    assert!(is_builtin_directive("include"));
    assert!(!is_builtin_directive("custom"));
}

#[test]
fn test_directive_stack_unit() {
    let mut stack = DirectiveStack::new();
    assert!(stack.is_empty());
    assert_eq!(stack.depth(), 0);

    stack.handle_if(1, 1);
    assert!(!stack.is_empty());
    assert_eq!(stack.depth(), 1);
    assert_eq!(stack.peek().unwrap().kind, DirectiveKind::If);

    assert!(stack.handle_elseif(2, 1).is_ok());
    assert!(stack.handle_else(3, 1).is_ok());
    // Duplicate else should fail
    assert!(stack.handle_else(4, 1).is_err());
    // Elseif after else should fail
    assert!(stack.handle_elseif(5, 1).is_err());

    assert!(stack.handle_endif(6, 1).is_ok());
    assert!(stack.is_empty());
    assert!(stack.finish().is_ok());
}

#[test]
fn test_compiler_with_custom_directives() {
    let mut compiler = Compiler::new();
    compiler.register_directive("custom_directive", |args| {
        format!("@custom_directive({})", args.unwrap_or(""))
    });
    let input = "@if($cond)\n  @custom_directive('param')\n@endif";
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, "if ($cond) {\n  @custom_directive('param')\n}");
}


#[test]
fn test_comprehensive_template() {
    let template = r#"
<div class="container">
  {{-- Header section --}}
  @if($user.is_authenticated)
    <h2>Welcome, {{ $user.name }}</h2>
    @if($user.is_admin)
      <span class="badge">Admin</span>
    @elseif($user.is_mod)
      <span class="badge">Mod</span>
    @else
      <span class="badge">User</span>
    @endif
  @else
    <h2>Please log in</h2>
  @endif

  {{-- Raw RHP block --}}
  @rhp
  let max_items = 5;
  @endrhp

  <ul>
    @for($i = 0; $i < max_items; $i++)
      <li>Item {{ $i }}</li>
    @endfor
  </ul>
</div>
"#;

    let output = compile(template).unwrap();
    assert!(output.contains("if ($user.is_authenticated) {"));
    assert!(output.contains("if ($user.is_admin) {"));
    assert!(output.contains("} else if ($user.is_mod) {"));
    assert!(output.contains("} else {"));
    assert!(output.contains("for ($i = 0; $i < max_items; $i++) {"));
    assert!(output.contains("let max_items = 5;"));
    assert!(!output.contains("{{-- Header section --}}"));
    assert!(!output.contains("{{-- Raw RHP block --}}"));
}

#[test]
fn test_compiler_struct_and_compile_tokens() {
    use rhp::rade::Token;

    let mut compiler = Compiler::new();
    let tokens = vec![
        Token::Directive {
            name: "if".to_string(),
            args: Some("$is_admin".to_string()),
        },
        Token::Text("Hello Admin".to_string()),
        Token::Directive {
            name: "endif".to_string(),
            args: None,
        },
    ];

    let result = compiler.compile_tokens(&tokens).unwrap();
    assert_eq!(result, "if ($is_admin) {Hello Admin}");

    // Direct compile_directive function test
    let mut stack = DirectiveStack::new();
    let if_compiled = compile_directive("if", Some("$x > 0"), 1, 1, &mut stack).unwrap();
    assert_eq!(if_compiled, "if ($x > 0) {");

    let endif_compiled = compile_directive("endif", None, 2, 1, &mut stack).unwrap();
    assert_eq!(endif_compiled, "}");
    assert!(stack.finish().is_ok());
}

#[test]
fn test_foreach_directive() {
    let input = "@foreach($users as $user)\n  <li>{{ $user.name }}</li>\n@endforeach";
    let output = compile(input).unwrap();
    assert_eq!(
        output,
        "for ($user in $users) {\n  <li>{{ $user.name }}</li>\n}"
    );

    let input_keyed = "@foreach($items as $key => $item)\n  <li>{{ $key }}: {{ $item }}</li>\n@endforeach";
    let output_keyed = compile(input_keyed).unwrap();
    assert_eq!(
        output_keyed,
        "for ($key => $item in $items) {\n  <li>{{ $key }}: {{ $item }}</li>\n}"
    );
}

#[test]
fn test_include_directive() {
    use rhp::rade::compile_to_rhp;
    let input = "<div>\n@include('partials.header')\n</div>";
    let output = compile_to_rhp(input).unwrap();
    assert!(output.contains("$__out += View::render('partials.header');"));
}
