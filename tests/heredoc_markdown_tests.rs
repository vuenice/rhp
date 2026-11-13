use rhp::lexer::{Lexer, Token};
use rhp::lint::format_source;
use rhp::{run_source, PhpValue};

#[test]
fn test_heredoc_lexer_alphabetical_tags_only() {
    // Valid alphabetical tags
    let html = Lexer::tokenize("```html\n<div>hello</div>\n```").expect("Tokenize HTML tag");
    assert_eq!(html[0].token, Token::BacktickString("```html\n<div>hello</div>\n```"));
    assert_eq!(html[0].token.raw_string_content(), Some("<div>hello</div>"));

    let sql = Lexer::tokenize("```sql\nSELECT * FROM users\n```").expect("Tokenize SQL tag");
    assert_eq!(sql[0].token.raw_string_content(), Some("SELECT * FROM users"));

    let empty = Lexer::tokenize("```\nHello world\n```").expect("Tokenize untagged block");
    assert_eq!(empty[0].token.raw_string_content(), Some("Hello world"));

    let mixed_case = Lexer::tokenize("```GraphQL\nquery { user }\n```").expect("Tokenize uppercase tag");
    assert_eq!(mixed_case[0].token.raw_string_content(), Some("query { user }"));

    // Invalid non-alphabetical tags: numbers, hyphens, spaces, quotes
    let digits = Lexer::tokenize("```html123\nhello\n```");
    assert!(digits.is_err() || digits.unwrap()[0].token != Token::BacktickString("```html123\nhello\n```"));

    let special = Lexer::tokenize("```html-css\nhello\n```");
    assert!(special.is_err() || special.unwrap()[0].token != Token::BacktickString("```html-css\nhello\n```"));

    let with_space = Lexer::tokenize("```html \nhello\n```");
    assert!(with_space.is_err() || with_space.unwrap()[0].token != Token::BacktickString("```html \nhello\n```"));

    let with_quote = Lexer::tokenize("```'html'\nhello\n```");
    assert!(with_quote.is_err() || with_quote.unwrap()[0].token != Token::BacktickString("```'html'\nhello\n```"));
}

#[test]
fn test_heredoc_interpolation_and_bare_variables() {
    let source = r#"
        $name = "Alice";
        $age = 30;
        $html = ```html
            <div>
                <h1>Hello, {$name}!</h1>
                <p>Age: { $age }</p>
                <p>Price: $100</p>
                <code>body { color: red; }</code>
                <span>Escaped: \{$name\} and \$age</span>
            </div>
        ```;
        return $html;
    "#;

    let html = run_source(source).expect("Execution should succeed");
    let html_str = match &html {
        PhpValue::String(s) => s.as_str(),
        _ => panic!("Expected string, got {:?}", html),
    };

    // Interpolation inside curly braces works
    assert!(html_str.contains("Hello, Alice!"));
    assert!(html_str.contains("Age: 30"));

    // Bare $100 is not interpolated
    assert!(html_str.contains("Price: $100"));

    // CSS { color: red; } is not treated as interpolation
    assert!(html_str.contains("body { color: red; }"));

    // Escaped \{$name\} becomes {$name} without interpolating
    assert!(html_str.contains("Escaped: {$name} and $age"));
}

#[test]
fn test_heredoc_escape_sequences() {
    let source = r#"
        $val = "escapes";
        $block = ```text
            Line1\nLine2\tTabbed\r\nQuote: \"Backslash: \\ Value: {$val}
        ```;
        return $block;
    "#;

    let block = run_source(source).expect("Execution should succeed");
    let block_str = match &block {
        PhpValue::String(s) => s.as_str(),
        _ => panic!("Expected string, got {:?}", block),
    };

    assert!(block_str.contains("Line1\nLine2\tTabbed\r\nQuote: \"Backslash: \\ Value: escapes"));
}

#[test]
fn test_heredoc_vm_dev_and_build_mode() {
    let source = concat!(
        "$msg = \"Success\";\n",
        "return ```html\n",
        "<b>{ $msg }</b>\n",
        "```;\n"
    );

    let res = run_source(source).expect("Execution should succeed");
    assert_eq!(res.to_php_string(), "<b>Success</b>");
}

#[test]
fn test_linter_adds_tab_and_preserves_newline_separation() {
    let unformatted = concat!(
        "$html = ```html\n",
        "<div class=\"card\">\n",
        "    <h1>Hello, {$name}</h1>\n",
        "</div>\n",
        "```;\n"
    );

    let formatted = format_source(unformatted);
    let expected = concat!(
        "$html = ```html\n",
        "\t<div class=\"card\">\n",
        "\t    <h1>Hello, {$name}</h1>\n",
        "\t</div>\n",
        "```;\n"
    );

    assert_eq!(formatted, expected);
}

#[test]
fn test_linter_splits_inline_heredoc_into_multiline_with_tabs() {
    let inline = "$html = ```html <div>hello, {$name}</div> ```;";
    let formatted = format_source(inline);

    assert!(formatted.starts_with("$html = ```html\n"));
    assert!(formatted.contains("\t<div>hello, {$name}</div>\n"));
    assert!(formatted.ends_with("```;\n"));
}

#[test]
fn test_linter_heredoc_inside_indented_function() {
    let unformatted = concat!(
        "function getCard($name) {\n",
        "    $html = ```html\n",
        "<p>User: {$name}</p>\n",
        "```;\n",
        "    return $html;\n",
        "}\n"
    );

    let formatted = format_source(unformatted);

    assert!(formatted.contains("    $html = ```html\n"));
    assert!(formatted.contains("\t<p>User: {$name}</p>\n"));
    assert!(formatted.contains("    ```;\n"));
    assert!(formatted.contains("    return $html;\n"));
}

#[test]
fn test_upgrade_php_heredoc_blocks() {
    let php = concat!(
        "$str = <<<HTML\n",
        "<div>Hello</div>\n",
        "HTML;\n",
        "$sql = <<<'SQL'\n",
        "SELECT * FROM users\n",
        "SQL;\n",
        "$eof = <<<EOF\n",
        "Simple string\n",
        "EOF;\n"
    );

    let (upgraded, count) = rhp::lint::upgrade_php::upgrade_heredoc_blocks(php);
    assert_eq!(count, 3);
    assert!(upgraded.contains("```html\n<div>Hello</div>\n```;"));
    assert!(upgraded.contains("```sql\nSELECT * FROM users\n```;"));
    assert!(upgraded.contains("```\nSimple string\n```;"));
}

