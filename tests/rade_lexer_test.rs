use rhp::rade::{tokenize, LexerError, Scanner, Token};

#[test]
fn test_plain_text() {
    let input = "<h1>Hello, World!</h1>\n<p>Welcome to Rade.</p>";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens, vec![Token::Text(input.to_string())]);
}

#[test]
fn test_empty_input() {
    let tokens = tokenize("").unwrap();
    assert_eq!(tokens, vec![]);
}

#[test]
fn test_echo_escaped() {
    let input = "Hello, <@ $name @>!";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Text("Hello, ".into()),
            Token::EchoEscaped("$name".into()),
            Token::Text("!".into()),
        ]
    );

    // Without spaces
    let tokens_nospace = tokenize("<@$name@>").unwrap();
    assert_eq!(tokens_nospace, vec![Token::EchoEscaped("$name".into())]);

    // Expressions
    let tokens_expr = tokenize("<@ 1 + 2 * 3 @>").unwrap();
    assert_eq!(tokens_expr, vec![Token::EchoEscaped("1 + 2 * 3".into())]);
}

#[test]
fn test_echo_raw() {
    let input = "Raw HTML: <@! $content @>";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Text("Raw HTML: ".into()),
            Token::EchoRaw("$content".into()),
        ]
    );

    let tokens_nospace = tokenize("<@!$content@>").unwrap();
    assert_eq!(tokens_nospace, vec![Token::EchoRaw("$content".into())]);
}

#[test]
fn test_escaped_verbatim() {
    // Double braces {{ ... }} now need no escape and are emitted as literal text
    let input = "Vue says: {{ message }}";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![Token::Text("Vue says: {{ message }}".into())]
    );

    let input_mixed = "{{ vue_data }} and <@ rade_data @>";
    let tokens_mixed = tokenize(input_mixed).unwrap();
    assert_eq!(
        tokens_mixed,
        vec![
            Token::Text("{{ vue_data }} and ".into()),
            Token::EchoEscaped("rade_data".into()),
        ]
    );

    // <@@ escapes to literal <@
    let escaped_at = "Literal: <@@ foo";
    assert_eq!(
        tokenize(escaped_at).unwrap(),
        vec![Token::Text("Literal: <@ foo".into())]
    );
}

#[test]
fn test_comments() {
    let input = "Before <@-- This is a comment @> After";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Text("Before ".into()),
            Token::Comment("This is a comment".into()),
            Token::Text(" After".into()),
        ]
    );

    // Comment containing other syntax inside
    let nested_comment = "<@-- {{ $ignored }} @if(ignored) <@ $x @>";
    let tokens_nested = tokenize(nested_comment).unwrap();
    assert_eq!(
        tokens_nested,
        vec![Token::Comment(
            "{{ $ignored }} @if(ignored) <@ $x".into()
        )]
    );
}

#[test]
fn test_directives_with_and_without_args() {
    let input = "@if($active)\n  <span>Active</span>\n@else\n  <span>Inactive</span>\n@endif";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Directive {
                name: "if".into(),
                args: Some("$active".into()),
            },
            Token::Text("\n  <span>Active</span>\n".into()),
            Token::Directive {
                name: "else".into(),
                args: None,
            },
            Token::Text("\n  <span>Inactive</span>\n".into()),
            Token::Directive {
                name: "endif".into(),
                args: None,
            },
        ]
    );

    // Directive with spaces before opening paren
    let tokens_spaced = tokenize("@if   ($a > 1)").unwrap();
    assert_eq!(
        tokens_spaced,
        vec![Token::Directive {
            name: "if".into(),
            args: Some("$a > 1".into()),
        }]
    );

    // Directive with empty arguments
    let tokens_empty_args = tokenize("@csrf()").unwrap();
    assert_eq!(
        tokens_empty_args,
        vec![Token::Directive {
            name: "csrf".into(),
            args: Some("".into()),
        }]
    );
}

#[test]
fn test_directive_nested_parentheses() {
    // Nested parentheses as specified in requirement 3
    let input = "@if(count($items) > 0 && ($a == 1))";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![Token::Directive {
            name: "if".into(),
            args: Some("count($items) > 0 && ($a == 1)".into()),
        }]
    );

    // Deeply nested parentheses
    let deeply_nested = "@foo((((1 + 2) * (3 + 4))))";
    let tokens_nested = tokenize(deeply_nested).unwrap();
    assert_eq!(
        tokens_nested,
        vec![Token::Directive {
            name: "foo".into(),
            args: Some("(((1 + 2) * (3 + 4)))".into()),
        }]
    );

    // Parentheses inside string literals within arguments
    let with_strings = r#"@if($name == "hello (world)" && ($val == ')'))"#;
    let tokens_strings = tokenize(with_strings).unwrap();
    assert_eq!(
        tokens_strings,
        vec![Token::Directive {
            name: "if".into(),
            args: Some(r#"$name == "hello (world)" && ($val == ')') "#.trim().into()),
        }]
    );

    // Escaped quotes inside strings
    let with_escaped = r#"@directive("foo \"(bar)\" baz")"#;
    let tokens_escaped = tokenize(with_escaped).unwrap();
    assert_eq!(
        tokens_escaped,
        vec![Token::Directive {
            name: "directive".into(),
            args: Some(r#""foo \"(bar)\" baz""#.into()),
        }]
    );
}

#[test]
fn test_escaped_at_and_emails() {
    // @@directive should not trigger directive
    let input = "@@directive";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens, vec![Token::Text("@directive".into())]);

    // @@if(true) should output @if(true) as text
    let input_if = "@@if(true)";
    let tokens_if = tokenize(input_if).unwrap();
    assert_eq!(tokens_if, vec![Token::Text("@if(true)".into())]);

    // Vue event listener @@click in Blade
    let vue_button = r#"<button @@click="handleClick">Submit</button>"#;
    let tokens_button = tokenize(vue_button).unwrap();
    assert_eq!(
        tokens_button,
        vec![Token::Text(
            r#"<button @click="handleClick">Submit</button>"#.into()
        )]
    );

    // Email addresses should not trigger directives
    let email_input = "Contact us at test@example.com or support@rade.dev.";
    let tokens_email = tokenize(email_input).unwrap();
    assert_eq!(tokens_email, vec![Token::Text(email_input.into())]);

    let email_underscores = "Send to user_name@domain.com now.";
    let tokens_under = tokenize(email_underscores).unwrap();
    assert_eq!(tokens_under, vec![Token::Text(email_underscores.into())]);

    // Stray @ symbols
    let stray = "Price is @ $5 or 123@abc";
    let tokens_stray = tokenize(stray).unwrap();
    assert_eq!(tokens_stray, vec![Token::Text(stray.into())]);

    // Tripled @@@directive -> "@" (text) followed by @directive
    let triple = "@@@foo(1)";
    let tokens_triple = tokenize(triple).unwrap();
    assert_eq!(
        tokens_triple,
        vec![
            Token::Text("@".into()),
            Token::Directive {
                name: "foo".into(),
                args: Some("1".into()),
            },
        ]
    );
}

#[test]
fn test_raw_rhp_blocks() {
    let input = "<@?\n  let x = 10;\n  let y = 20;\n@>";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![Token::RawRhp("let x = 10;\n  let y = 20;".into())]
    );

    // Single-line <@? ... @>
    let single = "<@? let count = 0; @>";
    let tokens_single = tokenize(single).unwrap();
    assert_eq!(tokens_single, vec![Token::RawRhp("let count = 0;".into())]);

    // Directive named rhp_custom is a normal directive
    let custom = "@rhp_custom(123)";
    let tokens_custom = tokenize(custom).unwrap();
    assert_eq!(
        tokens_custom,
        vec![Token::Directive {
            name: "rhp_custom".into(),
            args: Some("123".into()),
        }]
    );
}

#[test]
fn test_multiline_support() {
    // Multi-line comment
    let ml_comment = "<@--\n  Line 1\n  Line 2\n@>";
    let tokens_comment = tokenize(ml_comment).unwrap();
    assert_eq!(
        tokens_comment,
        vec![Token::Comment("Line 1\n  Line 2".into())]
    );

    // Multi-line echo
    let ml_echo = "<@\n  $items\n    .filter(|x| x > 0)\n@>";
    let tokens_echo = tokenize(ml_echo).unwrap();
    assert_eq!(
        tokens_echo,
        vec![Token::EchoEscaped("$items\n    .filter(|x| x > 0)".into())]
    );

    // Multi-line raw echo
    let ml_raw = "<@!\n  $rich_html\n@>";
    let tokens_raw = tokenize(ml_raw).unwrap();
    assert_eq!(tokens_raw, vec![Token::EchoRaw("$rich_html".into())]);

    // Multi-line directive args
    let ml_directive = "@if(\n  $a > 0 &&\n  $b < 10\n)";
    let tokens_directive = tokenize(ml_directive).unwrap();
    assert_eq!(
        tokens_directive,
        vec![Token::Directive {
            name: "if".into(),
            args: Some("$a > 0 &&\n  $b < 10".into()),
        }]
    );
}

#[test]
fn test_directive_followed_by_newline_and_parens() {
    // In Blade, @else followed by newline and parenthesized text should NOT parse
    // the parentheses on the next line as directive arguments.
    let input = "@else\n(plain text in parentheses)";
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![
            Token::Directive {
                name: "else".into(),
                args: None,
            },
            Token::Text("\n(plain text in parentheses)".into()),
        ]
    );
}

#[test]
fn test_echo_with_string_containing_delimiters() {
    // String inside echo containing @>
    let input = r#"<@ "hello @> world" @>"#;
    let tokens = tokenize(input).unwrap();
    assert_eq!(
        tokens,
        vec![Token::EchoEscaped(r#""hello @> world""#.into())]
    );

    // Unclosed raw echo containing @> in string
    let input_raw = r#"<@! "content @> here" @"#;
    assert!(tokenize(input_raw).is_err());

    let valid_raw = r#"<@! "hello @> world" @>"#;
    let tokens_raw = tokenize(valid_raw).unwrap();
    assert_eq!(
        tokens_raw,
        vec![Token::EchoRaw(r#""hello @> world""#.into())]
    );
}

#[test]
fn test_complex_template_end_to_end() {
    let template = r#"
<div class="user-profile">
    <@-- User header section @>
    <h1>Welcome, <@ $user.name @>!</h1>

    @if($user.is_admin && ($user.level > 5))
        <span class="badge">Admin: <@! $user.badge_html @></span>
    @elseif($user.is_moderator)
        <span class="badge">Moderator</span>
    @else
        <span>Member</span>
    @endif

    <@?
        let items_count = user.items.len();
    @>

    <p>Items count: <@ items_count @></p>

    <@-- Vue.js client-side template bindings @>
    <div id="vue-app">
        <span>{{ dynamicVueMessage }}</span>
        <button @@click="doSomething">Click</button>
    </div>

    <footer>Contact support@example.com</footer>
</div>
"#;

    let tokens = Scanner::scan(template).unwrap();
    assert!(!tokens.is_empty());

    // Verify presence of expected token types
    assert!(tokens.iter().any(|t| t.is_comment()));
    assert!(tokens.iter().any(|t| t.is_echo_escaped()));
    assert!(tokens.iter().any(|t| t.is_echo_raw()));
    assert!(tokens.iter().any(|t| matches!(t, Token::Text(s) if s.contains("{{ dynamicVueMessage }}"))));
    assert!(tokens.iter().any(|t| t.is_raw_rhp()));
    assert!(tokens.iter().any(|t| matches!(t, Token::Directive { name, .. } if name == "if")));
    assert!(tokens.iter().any(|t| matches!(t, Token::Directive { name, .. } if name == "elseif")));
    assert!(tokens.iter().any(|t| matches!(t, Token::Directive { name, .. } if name == "else")));
    assert!(tokens.iter().any(|t| matches!(t, Token::Directive { name, .. } if name == "endif")));

    // Verify email was not parsed as directive
    assert!(!tokens.iter().any(|t| matches!(t, Token::Directive { name, .. } if name == "example")));

    // Verify @@click was treated as text
    assert!(!tokens.iter().any(|t| matches!(t, Token::Directive { name, .. } if name == "click")));
}

#[test]
fn test_lexer_errors() {
    // Unclosed comment
    let unclosed_comment = "Hello <@-- missing end";
    assert_eq!(
        tokenize(unclosed_comment),
        Err(LexerError::UnclosedComment { line: 1, column: 7 })
    );

    // Unclosed echo
    let unclosed_echo = "Hello <@ $name missing end";
    assert_eq!(
        tokenize(unclosed_echo),
        Err(LexerError::UnclosedEcho { line: 1, column: 7 })
    );

    // Unclosed raw echo
    let unclosed_raw = "Hello <@! $raw missing end";
    assert_eq!(
        tokenize(unclosed_raw),
        Err(LexerError::UnclosedRawEcho { line: 1, column: 7 })
    );

    // Unclosed <@?
    let unclosed_rhp = "<@? let a = 1;";
    assert_eq!(
        tokenize(unclosed_rhp),
        Err(LexerError::UnclosedRawRhp { line: 1, column: 1 })
    );

    // Unknown <@ directive syntax
    let err_triple = tokenize("Hello <@@@ world").unwrap_err();
    assert_eq!(
        err_triple,
        LexerError::UnknownDirective {
            syntax: "<@@@".into(),
            line: 1,
            column: 7,
        }
    );
    assert!(err_triple.to_string().contains("unknown directive found"));

    let err_percent = tokenize("<@% foo @>").unwrap_err();
    assert_eq!(
        err_percent,
        LexerError::UnknownDirective {
            syntax: "<@%".into(),
            line: 1,
            column: 1,
        }
    );
    assert!(err_percent.to_string().contains("unknown directive found"));

    // Unclosed directive arguments
    let unclosed_directive = "@if(count($x) > 0";
    assert_eq!(
        tokenize(unclosed_directive),
        Err(LexerError::UnclosedDirectiveArgs {
            directive: "if".into(),
            line: 1,
            column: 1,
        })
    );
}

#[test]
fn test_token_helper_methods() {
    let t_text = Token::Text("hello".into());
    assert!(t_text.is_text());
    assert_eq!(t_text.inner_content(), Some("hello"));

    let t_echo = Token::EchoEscaped("$x".into());
    assert!(t_echo.is_echo_escaped());
    assert_eq!(t_echo.inner_content(), Some("$x"));

    let t_raw = Token::EchoRaw("<b>ok</b>".into());
    assert!(t_raw.is_echo_raw());
    assert_eq!(t_raw.inner_content(), Some("<b>ok</b>"));

    let t_verb = Token::EscapedVerbatim("{{ vue }}".into());
    assert!(t_verb.is_escaped_verbatim());
    assert_eq!(t_verb.inner_content(), Some("vue"));

    let t_comment = Token::Comment("secret".into());
    assert!(t_comment.is_comment());
    assert_eq!(t_comment.inner_content(), Some("secret"));

    let t_dir = Token::Directive {
        name: "if".into(),
        args: Some("true".into()),
    };
    assert!(t_dir.is_directive());
    assert_eq!(t_dir.as_directive(), Some(("if", Some("true"))));
    assert_eq!(t_dir.inner_content(), Some("true"));

    let t_rhp = Token::RawRhp("let x = 1;".into());
    assert!(t_rhp.is_raw_rhp());
    assert_eq!(t_rhp.inner_content(), Some("let x = 1;"));
}
