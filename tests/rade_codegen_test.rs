use rhp::rade::compiler::codegen::escape_rhp_string;
use rhp::rade::{
    compile_to_closure, compile_to_rhp, compile_with_options, CompileError, Compiler,
    CompilerOptions, OutputFormat,
};

#[test]
fn test_empty_template_standalone() {
    let input = "";
    let expected = "$__out = \"\";\nreturn $__out;";
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_empty_template_closure() {
    let input = "";
    let expected = r#"return function($__data: dict): string {
    $__out = "";
    return $__out;
};"#;
    let output = compile_to_closure(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_plain_text_standalone() {
    let input = "<h1>Hello World</h1>";
    let expected = r#"$__out = "";
$__out += "<h1>Hello World</h1>";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_plain_text_closure() {
    let input = "<h1>Hello World</h1>";
    let expected = r#"return function($__data: dict): string {
    $__out = "";
    $__out += "<h1>Hello World</h1>";
    return $__out;
};"#;
    let output = compile_to_closure(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_escape_quotes_and_special_characters() {
    let input = r#"<div class="card" id="main">Price: $99 & \path\to\file</div>"#;
    let expected = r#"$__out = "";
$__out += "<div class=\"card\" id=\"main\">Price: $99 & \\path\\to\\file</div>";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_escape_newlines_and_tabs_in_text() {
    let input = "Line 1\n\tLine 2\r\nLine 3";
    let expected = "$__out = \"\";\n$__out += \"Line 1\\n\\tLine 2\\r\\nLine 3\";\nreturn $__out;";
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_merge_consecutive_text_nodes() {
    // When text is broken by comments, the comments are stripped and
    // consecutive text chunks must be merged into a single $__out += "..."; statement
    let input = "Hello {{-- internal comment --}} Beautiful {{-- another comment --}} World!";
    let expected = r#"$__out = "";
$__out += "Hello  Beautiful  World!";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_merge_unregistered_directive_without_args_as_text() {
    // In Blade, @contact without parentheses when unregistered is literal text
    let input = "Please write to @support or @admin for help.";
    let expected = r#"$__out = "";
$__out += "Please write to @support or @admin for help.";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_echo_escaped() {
    let input = "<h1>Welcome, {{ $user.name }}!</h1>";
    let expected = r#"$__out = "";
$__out += "<h1>Welcome, ";
$__out += Html::escape($user.name);
$__out += "!</h1>";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_echo_raw() {
    let input = "<div class=\"body\">{!! $article.content !!}</div>";
    let expected = r#"$__out = "";
$__out += "<div class=\"body\">";
$__out += string($article.content);
$__out += "</div>";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_escaped_verbatim() {
    let input = "Vue binding: @{{ message }} and @{{ $expr }}";
    let expected = r#"$__out = "";
$__out += "Vue binding: ";
$__out += "{{ message }}";
$__out += " and ";
$__out += "{{ $expr }}";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_escaped_verbatim_with_quotes() {
    let input = r#"@{{ "hello " + name }}"#;
    let expected = r#"$__out = "";
$__out += "{{ \"hello \" + name }}";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_if_directive_standalone() {
    let input = "@if($is_active)<span>Active</span>@endif";
    let expected = r#"$__out = "";
if ($is_active) {
    $__out += "<span>Active</span>";
}
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_if_else_directive() {
    let input = "@if($is_admin)\n<span>Admin</span>\n@else\n<span>User</span>\n@endif";
    let expected = r#"$__out = "";
if ($is_admin) {
    $__out += "\n<span>Admin</span>\n";
} else {
    $__out += "\n<span>User</span>\n";
}
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_if_elseif_else_directive() {
    let input = "@if($score >= 90)\nGrade A\n@elseif($score >= 80)\nGrade B\n@else\nGrade C\n@endif";
    let expected = r#"$__out = "";
if ($score >= 90) {
    $__out += "\nGrade A\n";
} else if ($score >= 80) {
    $__out += "\nGrade B\n";
} else {
    $__out += "\nGrade C\n";
}
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_for_directive_standalone() {
    let input = "@for($i = 0; $i < 5; $i++)<li>{{ $i }}</li>@endfor";
    let expected = r#"$__out = "";
for ($i = 0; $i < 5; $i++) {
    $__out += "<li>";
    $__out += Html::escape($i);
    $__out += "</li>";
}
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_for_directive_closure() {
    let input = "@for($i = 0; $i < 5; $i++)<li>{{ $i }}</li>@endfor";
    let expected = r#"return function($__data: dict): string {
    $__out = "";
    for ($i = 0; $i < 5; $i++) {
        $__out += "<li>";
        $__out += Html::escape($i);
        $__out += "</li>";
    }
    return $__out;
};"#;
    let output = compile_to_closure(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_nested_if_inside_for() {
    let input = "@for($i = 0; $i < 3; $i++)\n@if($i == 0)\nFirst\n@else\nOther\n@endif\n@endfor";
    let expected = r#"$__out = "";
for ($i = 0; $i < 3; $i++) {
    $__out += "\n";
    if ($i == 0) {
        $__out += "\nFirst\n";
    } else {
        $__out += "\nOther\n";
    }
    $__out += "\n";
}
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_nested_for_inside_if() {
    let input = "@if($has_items)<ul>@for($i = 0; $i < count($items); $i++)<li>{{ $items[$i] }}</li>@endfor</ul>@endif";
    let expected = r#"$__out = "";
if ($has_items) {
    $__out += "<ul>";
    for ($i = 0; $i < count($items); $i++) {
        $__out += "<li>";
        $__out += Html::escape($items[$i]);
        $__out += "</li>";
    }
    $__out += "</ul>";
}
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_raw_rhp_block() {
    let input = "@rhp\nlet x = 10;\nlet y = 20;\n@endrhp\n<span>{{ $x + $y }}</span>";
    let expected = r#"$__out = "";
let x = 10;
let y = 20;
$__out += "\n<span>";
$__out += Html::escape($x + $y);
$__out += "</span>";
return $__out;"#;
    let output = compile_to_rhp(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_raw_rhp_inside_if_closure() {
    let input = "@if($debug)\n@rhp\nLog::info(\"debug mode enabled\");\n@endrhp\n@endif";
    let expected = r#"return function($__data: dict): string {
    $__out = "";
    if ($debug) {
        $__out += "\n";
        Log::info("debug mode enabled");
        $__out += "\n";
    }
    return $__out;
};"#;
    let output = compile_to_closure(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_custom_directives_codegen() {
    let mut compiler = Compiler::standalone();
    compiler
        .register_directive("uppercase", |args| {
            format!("$__out += Str::upperCase({});", args.unwrap_or("''"))
        })
        .register_directive("timestamp", |_args| {
            "$__out += Sys::timestamp();".to_string()
        });

    let input = "Name: @uppercase($user.name), Time: @timestamp";
    let expected = r#"$__out = "";
$__out += "Name: ";
$__out += Str::upperCase($user.name);
$__out += ", Time: ";
$__out += Sys::timestamp();
return $__out;"#;
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_compiler_with_format_builder() {
    let mut compiler = Compiler::new().with_format(OutputFormat::Closure);
    let input = "Hello {{ $name }}";
    let expected = r#"return function($__data: dict): string {
    $__out = "";
    $__out += "Hello ";
    $__out += Html::escape($name);
    return $__out;
};"#;
    let output = compiler.compile(input).unwrap();
    assert_eq!(output, expected);

    // Switch format dynamically
    compiler.set_format(OutputFormat::Standalone);
    let expected_standalone = r#"$__out = "";
$__out += "Hello ";
$__out += Html::escape($name);
return $__out;"#;
    let output_standalone = compiler.compile(input).unwrap();
    assert_eq!(output_standalone, expected_standalone);
}

#[test]
fn test_compiler_with_options_helper() {
    let input = "<span>Test</span>";
    let output_standalone = compile_with_options(input, CompilerOptions::standalone()).unwrap();
    assert_eq!(
        output_standalone,
        "$__out = \"\";\n$__out += \"<span>Test</span>\";\nreturn $__out;"
    );

    let output_closure = compile_with_options(input, CompilerOptions::closure()).unwrap();
    let expected_closure = "return function($__data: dict): string {\n    $__out = \"\";\n    $__out += \"<span>Test</span>\";\n    return $__out;\n};";
    assert_eq!(output_closure, expected_closure);
}

#[test]
fn test_no_echo_or_ob_functions_anywhere() {
    let template = r#"
<div>
    <h1>{{ $title }}</h1>
    {!! $body !!}
    @{{ vueMsg }}
    @if($show)
        <p>Visible</p>
    @endif
    @for($i = 0; $i < 3; $i++)
        <span>{{ $i }}</span>
    @endfor
</div>
"#;
    let compiled = compile_to_rhp(template).unwrap();
    assert!(!compiled.contains("echo "));
    assert!(!compiled.contains("echo("));
    assert!(!compiled.contains("ob_start"));
    assert!(!compiled.contains("ob_get_clean"));
    assert!(!compiled.contains("ob_end_clean"));
    assert!(!compiled.contains("ob_get_contents"));
    assert!(compiled.starts_with("$__out = \"\";"));
    assert!(compiled.ends_with("return $__out;"));
}

#[test]
fn test_comprehensive_rade_rhp_template_end_to_end() {
    let template = r#"<div class="user-profile">
    {{-- Header with user greetings --}}
    <h1>Welcome, {{ $user.name }}!</h1>

    @if($user.is_admin && $user.level > 5)
        <span class="badge">Admin: {!! $user.badge_html !!}</span>
    @elseif($user.is_moderator)
        <span class="badge">Moderator</span>
    @else
        <span>Member</span>
    @endif

    @rhp
    let count = $user.items.len();
    @endrhp

    <ul>
    @for($i = 0; $i < $count; $i++)
        <li>Item #{{ $i }}: {{ $user.items[$i] }}</li>
    @endfor
    </ul>

    <div id="vue-app">
        <span>@{{ dynamicVueMessage }}</span>
    </div>
</div>"#;

    let expected = r#"$__out = "";
$__out += "<div class=\"user-profile\">\n    \n    <h1>Welcome, ";
$__out += Html::escape($user.name);
$__out += "!</h1>\n\n    ";
if ($user.is_admin && $user.level > 5) {
    $__out += "\n        <span class=\"badge\">Admin: ";
    $__out += string($user.badge_html);
    $__out += "</span>\n    ";
} else if ($user.is_moderator) {
    $__out += "\n        <span class=\"badge\">Moderator</span>\n    ";
} else {
    $__out += "\n        <span>Member</span>\n    ";
}
$__out += "\n\n    ";
let count = $user.items.len();
$__out += "\n\n    <ul>\n    ";
for ($i = 0; $i < $count; $i++) {
    $__out += "\n        <li>Item #";
    $__out += Html::escape($i);
    $__out += ": ";
    $__out += Html::escape($user.items[$i]);
    $__out += "</li>\n    ";
}
$__out += "\n    </ul>\n\n    <div id=\"vue-app\">\n        <span>";
$__out += "{{ dynamicVueMessage }}";
$__out += "</span>\n    </div>\n</div>";
return $__out;"#;

    let compiled = compile_to_rhp(template).unwrap();
    assert_eq!(compiled, expected);
}

#[test]
fn test_codegen_errors_unclosed_if() {
    let input = "@if($cond)<span>Missing endif";
    let err = compile_to_rhp(input).unwrap_err();
    assert_eq!(
        err,
        CompileError::UnclosedDirective {
            directive: "if".to_string(),
            line: 1,
            column: 1,
        }
    );
}

#[test]
fn test_codegen_errors_unexpected_endif() {
    let input = "<span>Hello</span>@endif";
    let err = compile_to_rhp(input).unwrap_err();
    assert_eq!(
        err,
        CompileError::UnexpectedClosingDirective {
            directive: "endif".to_string(),
            line: 1,
            column: 19,
        }
    );
}

#[test]
fn test_escape_rhp_string_unit() {
    assert_eq!(escape_rhp_string("hello"), "hello");
    assert_eq!(escape_rhp_string("a\"b\\c\nd\re\tf"), "a\\\"b\\\\c\\nd\\re\\tf");
}
