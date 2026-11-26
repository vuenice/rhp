use std::sync::Arc;
use std::thread;

use rhp::rade::compiler::registry::{register_directive, DirectiveRegistry};
use rhp::rade::{compile, CompileError, Compiler};

#[test]
fn test_single_line_helper_uppercase() {
    let mut compiler = Compiler::new();
    compiler.register_directive("uppercase", |args| {
        format!("$__out += Str::upperCase({});", args.unwrap_or(""))
    });

    let input = "@uppercase($str)";
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, "$__out += Str::upperCase($str);");
}

#[test]
fn test_date_formatter_datetime() {
    let mut compiler = Compiler::new();
    compiler.register_directive("datetime", |args| {
        format!("$__out += {}->format(\"Y-m-d\");", args.unwrap_or(""))
    });

    let input = "@datetime($date)";
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, r#"$__out += $date->format("Y-m-d");"#);
}

#[test]
fn test_parameterless_directive_timestamp() {
    let mut compiler = Compiler::new();
    compiler.register_directive("timestamp", |_args| {
        "$__out += Sys::timestamp();".to_string()
    });

    let input = "@timestamp";
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, "$__out += Sys::timestamp();");
}

#[test]
fn test_parameterless_directive_with_empty_parens() {
    let mut compiler = Compiler::new();
    compiler.register_directive("timestamp", |_args| {
        "$__out += Sys::timestamp();".to_string()
    });

    let input = "@timestamp()";
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, "$__out += Sys::timestamp();");
}

#[test]
fn test_custom_directives_inside_if_control_flow() {
    let mut compiler = Compiler::new();
    compiler
        .register_directive("uppercase", |args| {
            format!("$__out += Str::upperCase({});", args.unwrap_or(""))
        })
        .register_directive("timestamp", |_args| {
            "$__out += Sys::timestamp();".to_string()
        });

    let input = r#"@if($user.is_logged_in)
  <h1>Welcome, @uppercase($user.name)</h1>
  <p>Login time: @timestamp</p>
@else
  <p>Please log in</p>
@endif"#;

    let output = compiler.compile(input).unwrap();
    let expected = r#"if ($user.is_logged_in) {
  <h1>Welcome, $__out += Str::upperCase($user.name);</h1>
  <p>Login time: $__out += Sys::timestamp();</p>
} else {
  <p>Please log in</p>
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_custom_directives_inside_for_loop() {
    let mut compiler = Compiler::new();
    compiler
        .register_directive("datetime", |args| {
            format!("$__out += {}->format(\"Y-m-d\");", args.unwrap_or(""))
        })
        .register_directive("uppercase", |args| {
            format!("$__out += Str::upperCase({});", args.unwrap_or(""))
        });

    let input = r#"@for($i = 0; $i < count($events); $i++)
  <div class="event">
    <span>@datetime($events[$i]->date)</span>
    <span>@uppercase($events[$i]->title)</span>
  </div>
@endfor"#;

    let output = compiler.compile(input).unwrap();
    let expected = r#"for ($i = 0; $i < count($events); $i++) {
  <div class="event">
    <span>$__out += $events[$i]->date->format("Y-m-d");</span>
    <span>$__out += Str::upperCase($events[$i]->title);</span>
  </div>
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_nested_if_and_for_with_all_custom_directives() {
    let mut compiler = Compiler::new();
    compiler
        .register_directive("uppercase", |args| {
            format!("$__out += Str::upperCase({});", args.unwrap_or(""))
        })
        .register_directive("datetime", |args| {
            format!("$__out += {}->format(\"Y-m-d\");", args.unwrap_or(""))
        })
        .register_directive("timestamp", |_args| {
            "$__out += Sys::timestamp();".to_string()
        });

    let input = r#"@if($active)
  <div>Rendered at: @timestamp</div>
  @for($i = 0; $i < 3; $i++)
    @if($i == 0)
      <span>First: @uppercase($names[$i])</span>
    @elseif($i == 1)
      <span>Second: @datetime($dates[$i])</span>
    @else
      <span>Other: @uppercase($names[$i])</span>
    @endif
  @endfor
@endif"#;

    let output = compiler.compile(input).unwrap();
    let expected = r#"if ($active) {
  <div>Rendered at: $__out += Sys::timestamp();</div>
  for ($i = 0; $i < 3; $i++) {
    if ($i == 0) {
      <span>First: $__out += Str::upperCase($names[$i]);</span>
    } else if ($i == 1) {
      <span>Second: $__out += $dates[$i]->format("Y-m-d");</span>
    } else {
      <span>Other: $__out += Str::upperCase($names[$i]);</span>
    }
  }
}"#;

    assert_eq!(output, expected);
}

#[test]
fn test_unregistered_directive_with_args_errors() {
    let mut compiler = Compiler::new();
    let input = "@if($cond)\n  @unknown_helper($arg)\n@endif";

    let err = compiler.compile(input).unwrap_err();
    let err_msg = err.to_string();
    match err {
        CompileError::UnknownDirective {
            directive,
            line,
            column,
        } => {
            assert_eq!(directive, "unknown_helper");
            assert_eq!(line, 2);
            assert_eq!(column, 3);
            assert_eq!(
                err_msg,
                "Unknown directive '@unknown_helper' at line 2, column 3"
            );
        }
        other => panic!("Expected UnknownDirective, got: {other:?}"),
    }
}


#[test]
fn test_unregistered_directive_with_empty_args_errors() {
    let mut compiler = Compiler::new();
    let input = "@nonexistent()";

    let err = compiler.compile(input).unwrap_err();
    match err {
        CompileError::UnknownDirective {
            directive,
            line,
            column,
        } => {
            assert_eq!(directive, "nonexistent");
            assert_eq!(line, 1);
            assert_eq!(column, 1);
        }
        other => panic!("Expected UnknownDirective, got: {other:?}"),
    }
}

#[test]
fn test_unregistered_directive_without_args_treated_as_plain_text() {
    let mut compiler = Compiler::new();
    let input = "@media screen {\n  body { color: red; }\n}";
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, input);

    // Multiple parameterless unregistered identifiers
    let text_input = "Follow @username on @twitter or join our @discord";
    let text_output = compiler.compile(text_input).unwrap();
    assert_eq!(text_output, text_input);
}


#[test]
fn test_global_directive_registration() {
    // Register globally with a unique name for this test
    register_directive("global_test_directive", |args| {
        format!("/* global: {} */", args.unwrap_or("none"))
    });

    let input = "@global_test_directive('hello')";
    // Global compile helper should recognize it
    let output1 = compile(input).unwrap();
    assert_eq!(output1, "/* global: 'hello' */");

    // Fresh compiler instance should also recognize it via global fallback
    let mut compiler = Compiler::new();
    let output2 = compiler.compile(input).unwrap();
    assert_eq!(output2, "/* global: 'hello' */");
}

#[test]
fn test_per_compiler_instance_isolation() {
    let mut compiler_a = Compiler::new();
    compiler_a.register_directive("only_in_a", |_| "code_a".to_string());

    let mut compiler_b = Compiler::new();
    compiler_b.register_directive("only_in_b", |_| "code_b".to_string());

    // compiler_a compiles only_in_a
    assert_eq!(compiler_a.compile("@only_in_a").unwrap(), "code_a");
    // compiler_a fails on only_in_b with parens
    assert!(matches!(
        compiler_a.compile("@only_in_b('param')").unwrap_err(),
        CompileError::UnknownDirective { .. }
    ));

    // compiler_b compiles only_in_b
    assert_eq!(compiler_b.compile("@only_in_b").unwrap(), "code_b");
    // compiler_b fails on only_in_a with parens
    assert!(matches!(
        compiler_b.compile("@only_in_a('param')").unwrap_err(),
        CompileError::UnknownDirective { .. }
    ));
}

#[test]
fn test_per_compiler_override_global() {
    register_directive("overridable_directive", |_| "global_version".to_string());

    let mut compiler = Compiler::new();
    compiler.register_directive("overridable_directive", |_| "local_version".to_string());

    let output = compiler.compile("@overridable_directive").unwrap();
    assert_eq!(output, "local_version");
}

#[test]
fn test_with_registry_constructor() {
    let registry = DirectiveRegistry::new();
    registry.register("custom_reg", |args| {
        format!("custom: {}", args.unwrap_or(""))
    });

    let mut compiler = Compiler::with_registry(registry);
    let output = compiler.compile("@custom_reg('test')").unwrap();
    assert_eq!(output, "custom: 'test'");
}

#[test]
fn test_directive_registry_unit_methods() {
    let reg = DirectiveRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);

    reg.register("test_dir", |args| format!("test: {}", args.unwrap_or("")));
    assert!(!reg.is_empty());
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("test_dir"));
    assert!(reg.has("test_dir"));
    assert!(!reg.contains("unknown"));

    assert_eq!(
        reg.call("test_dir", Some("val")),
        Some("test: val".to_string())
    );
    assert_eq!(reg.call("unknown", None), None);

    let names = reg.names();
    assert_eq!(names, vec!["test_dir".to_string()]);

    // Test clone_isolated
    let isolated = reg.clone_isolated();
    assert!(isolated.contains("test_dir"));
    isolated.register("second", |_| "second".to_string());
    assert_eq!(isolated.len(), 2);
    assert_eq!(reg.len(), 1); // original remains 1

    // Test remove
    assert!(reg.remove("test_dir"));
    assert!(!reg.contains("test_dir"));
    assert_eq!(reg.len(), 0);

    // Test clear
    isolated.clear();
    assert!(isolated.is_empty());
}

#[test]
fn test_thread_safe_concurrent_compilation() {
    let registry = DirectiveRegistry::new();
    registry.register("thread_upper", |args| {
        format!("UPPER({})", args.unwrap_or(""))
    });
    registry.register("thread_lower", |args| {
        format!("LOWER({})", args.unwrap_or(""))
    });

    let reg_arc = Arc::new(registry);
    let mut handles = vec![];

    for i in 0..10 {
        let reg = Arc::clone(&reg_arc);
        let handle = thread::spawn(move || {
            let mut compiler = Compiler::with_registry((*reg).clone());
            let template = format!("@thread_upper('user_{i}') | @thread_lower('ROLE_{i}')");
            let compiled = compiler.compile(&template).unwrap();
            assert_eq!(compiled, format!("UPPER('user_{i}') | LOWER('ROLE_{i}')"));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_custom_directives_with_multiple_and_complex_args() {
    let mut compiler = Compiler::new();
    compiler.register_directive("format_currency", |args| {
        format!("$__out += Currency::format({});", args.unwrap_or(""))
    });

    let input = r#"@format_currency($item->price, "USD", 2)"#;
    let output = compiler.compile(input).unwrap();
    assert_eq!(
        output,
        r#"$__out += Currency::format($item->price, "USD", 2);"#
    );
}

#[test]
fn test_mixed_template_echos_rhp_and_custom_directives() {
    let mut compiler = Compiler::new();
    compiler
        .register_directive("uppercase", |args| {
            format!("$__out += Str::upperCase({});", args.unwrap_or(""))
        })
        .register_directive("timestamp", |_| {
            "$__out += Sys::timestamp();".to_string()
        });

    let input = r#"<div>
  <@?
    let count = 42;
  @>
  @if(count > 0)
    <p>User: <@ $user @></p>
    <p>Title: @uppercase($title)</p>
    <p>Raw: <@! $raw_html @></p>
    <p>Generated at: @timestamp</p>
  @endif
</div>"#;

    let output = compiler.compile(input).unwrap();
    let expected = r#"<div>
  let count = 42;
  if (count > 0) {
    <p>User: <@ $user @></p>
    <p>Title: $__out += Str::upperCase($title);</p>
    <p>Raw: <@! $raw_html @></p>
    <p>Generated at: $__out += Sys::timestamp();</p>
  }
</div>"#;

    assert_eq!(output, expected);
}

