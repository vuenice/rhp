use rhp::lexer::{
    error::LexError,
    span::SourceMap,
    token::Token,
    Lexer,
};

#[test]
fn test_zero_copy_string_slicing() {
    let source = "function computeUserData(Int $userId, String $name): Float { return $userId * 1.5; }";
    let tokens = Lexer::tokenize(source).expect("Tokenization must succeed");

    let source_start = source.as_ptr() as usize;
    let source_end = source_start + source.len();

    for spanned in &tokens {
        let slice = match spanned.token {
            Token::Ident(s)
            | Token::Variable(s)
            | Token::IntLiteral(s)
            | Token::FloatLiteral(s)
            | Token::SingleQuotedString(s)
            | Token::DoubleQuotedString(s) => Some(s),
            _ => None,
        };

        if let Some(s) = slice {
            let s_ptr = s.as_ptr() as usize;
            assert!(
                s_ptr >= source_start && s_ptr + s.len() <= source_end,
                "Token slice {:?} must borrow directly from source buffer without allocation",
                s
            );
        }
    }
}

#[test]
fn test_spec_arr_facade_chaining() {
    let source = r#"
        $numbers = Arr::list([1, 2, 3, 4, 5])
            ->map(fn($n) => $n * 2)
            ->filter(fn($n) => $n > 4);
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize Arr facade");
    let kinds: Vec<_> = tokens.into_iter().map(|t| t.token).collect();

    assert_eq!(
        kinds,
        vec![
            Token::Variable("$numbers"),
            Token::Assign,
            Token::Ident("Arr"),
            Token::DoubleColon,
            Token::TypeList,
            Token::OpenParen,
            Token::OpenBracket,
            Token::IntLiteral("1"),
            Token::Comma,
            Token::IntLiteral("2"),
            Token::Comma,
            Token::IntLiteral("3"),
            Token::Comma,
            Token::IntLiteral("4"),
            Token::Comma,
            Token::IntLiteral("5"),
            Token::CloseBracket,
            Token::CloseParen,
            Token::Arrow,
            Token::Ident("map"),
            Token::OpenParen,
            Token::Fn,
            Token::OpenParen,
            Token::Variable("$n"),
            Token::CloseParen,
            Token::DoubleArrow,
            Token::Variable("$n"),
            Token::Star,
            Token::IntLiteral("2"),
            Token::CloseParen,
            Token::Arrow,
            Token::Ident("filter"),
            Token::OpenParen,
            Token::Fn,
            Token::OpenParen,
            Token::Variable("$n"),
            Token::CloseParen,
            Token::DoubleArrow,
            Token::Variable("$n"),
            Token::GreaterThan,
            Token::IntLiteral("4"),
            Token::CloseParen,
            Token::SemiColon,
        ]
    );
}

#[test]
fn test_spec_str_facade() {
    let source = r#"
        $slug = Str::slug('Laravel Style Facades');
        $token = Str::random(32);
        $hasWord = Str::contains('Runtime engine', 'engine');
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize Str facade");
    assert_eq!(tokens[0].token, Token::Variable("$slug"));
    assert_eq!(tokens[1].token, Token::Assign);
    assert_eq!(tokens[2].token, Token::Ident("Str"));
    assert_eq!(tokens[3].token, Token::DoubleColon);
    assert_eq!(tokens[4].token, Token::Ident("slug"));
    assert_eq!(
        tokens[6].token,
        Token::SingleQuotedString("'Laravel Style Facades'")
    );
    assert_eq!(
        tokens[6].token.raw_string_content(),
        Some("Laravel Style Facades")
    );
}

#[test]
fn test_spec_http_facade() {
    let source = r#"
        $response = HTTP::withHeaders(['Authorization' => 'Bearer token'])
            ->post('https://api.example.com/data', ['name' => 'Runtime']);
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize HTTP facade");
    assert_eq!(tokens[0].token, Token::Variable("$response"));
    assert_eq!(tokens[2].token, Token::Ident("HTTP"));
    assert_eq!(tokens[4].token, Token::Ident("withHeaders"));
    assert_eq!(tokens[8].token, Token::DoubleArrow);
    assert_eq!(tokens[12].token, Token::Arrow);
    assert_eq!(tokens[13].token, Token::Ident("post"));
}

#[test]
fn test_spec_file_and_image_facade() {
    let source = r#"
        File::read('app.log');
        Future::run(fn() => File::read('test.txt'));
        Image::make('photo.jpg')->resize(300, 200)->save('thumbnail.jpg');
        URL::route('dashboard');
        Session::put('user_id', 42);
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize facades");
    assert_eq!(tokens[0].token, Token::Ident("File"));
    assert_eq!(tokens[2].token, Token::Ident("read"));
    assert_eq!(tokens[7].token, Token::Ident("Future"));
    assert_eq!(tokens[9].token, Token::Ident("run"));
    assert_eq!(tokens[11].token, Token::Fn);
    assert_eq!(tokens[23].token, Token::Ident("Image"));
    assert_eq!(tokens[25].token, Token::Ident("make"));
    assert_eq!(tokens[29].token, Token::Arrow);
    assert_eq!(tokens[30].token, Token::Ident("resize"));
    assert_eq!(tokens[42].token, Token::Ident("URL"));
    assert_eq!(tokens[49].token, Token::Ident("Session"));
}

#[test]
fn test_spec_async_concurrent_core() {
    let source = r#"
        [$user, $posts] = Future::wait([
            fn() => DB::table('users')->find($id),
            fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
        ]);

        $fastestResponse = Future::any([
            fn() => HTTP::get('https://primary-mirror.com/data')->json(),
            fn() => HTTP::get('https://secondary-mirror.com/data')->json()
        ]);

        $delayedResult = Future::delayed(500, fn() => 'Execution resumed after 500ms');

        Future::defer(fn() => Log::info('Request processed successfully'));
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize async concurrent core");
    assert_eq!(tokens[0].token, Token::OpenBracket);
    assert_eq!(tokens[1].token, Token::Variable("$user"));
    assert_eq!(tokens[3].token, Token::Variable("$posts"));
    assert_eq!(tokens[4].token, Token::CloseBracket);
    assert_eq!(tokens[5].token, Token::Assign);
    assert_eq!(tokens[6].token, Token::Ident("Future"));
    assert_eq!(tokens[8].token, Token::Ident("wait"));

    // Check string interpolation detection
    let interpolation_tok = tokens
        .iter()
        .find(|t| matches!(t.token, Token::DoubleQuotedString(_)))
        .expect("Double quoted string not found");
    assert!(interpolation_tok.token.has_interpolation());
}

#[test]
fn test_spec_numeric_types_and_fluent_apis() {
    let source = r#"
        $id = Int::i64(9223372036854775807);
        $price = Float::f64(99.99)->toDecimal(2);
        Int::i64()->random()->add()->subtract();
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize numeric types");
    // Supporting classes Int and Float are identifiers (facades)
    assert_eq!(tokens[2].token, Token::Ident("Int"));
    assert_eq!(tokens[3].token, Token::DoubleColon);
    assert_eq!(tokens[4].token, Token::Ident("i64"));
    assert_eq!(tokens[5].token, Token::OpenParen);
    assert_eq!(tokens[6].token, Token::IntLiteral("9223372036854775807"));
    assert_eq!(tokens[7].token, Token::CloseParen);

    assert_eq!(tokens[11].token, Token::Ident("Float"));
    assert_eq!(tokens[12].token, Token::DoubleColon);
    assert_eq!(tokens[13].token, Token::Ident("f64"));
    assert_eq!(tokens[14].token, Token::OpenParen);
    assert_eq!(tokens[15].token, Token::FloatLiteral("99.99"));
    assert_eq!(tokens[16].token, Token::CloseParen);
    assert_eq!(tokens[17].token, Token::Arrow);
    assert_eq!(tokens[18].token, Token::Ident("toDecimal"));
}

#[test]
fn test_spec_surface_level_gradual_typing() {
    let source = r#"
        function calculateTotal(int $quantity, float $unitPrice, string $currency): float {
            $subtotal = $quantity * $unitPrice;
            $tax = $subtotal * 0.18;
            return $subtotal + $tax;
        }
    "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize function");
    assert_eq!(tokens[0].token, Token::Function);
    assert_eq!(tokens[1].token, Token::Ident("calculateTotal"));
    assert_eq!(tokens[3].token, Token::TypeInt);
    assert_eq!(tokens[4].token, Token::Variable("$quantity"));
    assert_eq!(tokens[6].token, Token::TypeFloat);
    assert_eq!(tokens[7].token, Token::Variable("$unitPrice"));
    assert_eq!(tokens[9].token, Token::TypeString);
    assert_eq!(tokens[10].token, Token::Variable("$currency"));
    assert_eq!(tokens[12].token, Token::Colon);
    assert_eq!(tokens[13].token, Token::TypeFloat);
}

#[test]
fn test_supporting_classes_vs_base_datatypes() {
    let source = r#"
        // int, float, string are base datatypes
        int $x = 10;
        float $y = 3.14;
        string $s = "hello";

        // Int and Float are supporting facade classes
        $boxedInt = Int::i64();
        $boxedFloat = Float::f64();
    "#;
    let tokens = Lexer::tokenize(source).expect("Tokenization failed");

    // Primitive keywords
    assert!(tokens.iter().any(|t| t.token == Token::TypeInt));
    assert!(tokens.iter().any(|t| t.token == Token::TypeFloat));
    assert!(tokens.iter().any(|t| t.token == Token::TypeString));

    // Supporting classes are identifiers (not type keywords)
    assert!(tokens.iter().any(|t| t.token == Token::Ident("Int")));
    assert!(tokens.iter().any(|t| t.token == Token::Ident("Float")));

    // Methods on facades are identifiers
    assert!(tokens.iter().any(|t| t.token == Token::Ident("i64")));
    assert!(tokens.iter().any(|t| t.token == Token::Ident("f64")));

    // Confirm is_type behavior
    assert!(Token::TypeInt.is_type());
    assert!(Token::TypeFloat.is_type());
    assert!(!Token::Ident("Int").is_type());
    assert!(!Token::Ident("Float").is_type());
    assert!(!Token::Ident("i64").is_type());
    assert!(!Token::Ident("f64").is_type());
}

#[test]
fn test_pdf_spec_facade_instantiation_forbidden() {
    use rhp::lexer::facade::{is_static_facade, validate_instantiation};

    // Confirm static facades are identified
    assert!(is_static_facade("Int"));
    assert!(is_static_facade("Float"));
    assert!(is_static_facade("Arr"));
    assert!(is_static_facade("Str"));
    assert!(is_static_facade("Glob"));

    // Confirm instantiation via new is strictly forbidden as required by PDF page 2 & 3
    assert_eq!(
        validate_instantiation("Int"),
        Err("Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly.".to_string())
    );
    assert_eq!(
        validate_instantiation("Float"),
        Err("Fatal Error: Cannot instantiate static facade class 'Float'. Use static methods directly.".to_string())
    );
    assert_eq!(
        validate_instantiation("Arr"),
        Err("Fatal Error: Cannot instantiate static facade class 'Arr'. Use static methods directly.".to_string())
    );
    assert_eq!(
        validate_instantiation("Str"),
        Err("Fatal Error: Cannot instantiate static facade class 'Str'. Use static methods directly.".to_string())
    );

    // Normal user classes allowed
    assert!(validate_instantiation("User").is_ok());
}

#[test]
fn test_numeric_literal_radixes_and_parsing() {
    let source = "42 1_000_000 0x1A_2F 0b1010_0101 0o755 3.14159 1.5e-3 0.75";
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize numbers");

    assert_eq!(tokens[0].token.parse_int(), Some(42));
    assert_eq!(tokens[1].token.parse_int(), Some(1_000_000));
    assert_eq!(tokens[2].token.parse_int(), Some(0x1A2F));
    assert_eq!(tokens[3].token.parse_int(), Some(0b1010_0101));
    assert_eq!(tokens[4].token.parse_int(), Some(0o755));

    assert_eq!(tokens[5].token.parse_float(), Some(3.14159));
    assert_eq!(tokens[6].token.parse_float(), Some(1.5e-3));
    assert_eq!(tokens[7].token.parse_float(), Some(0.75));
}

#[test]
fn test_string_unescaping() {
    let source = r#" 'simple' "hello\nworld\t\"quotes\"\\\$var" "#;
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize strings");

    assert_eq!(
        tokens[0].token.unescape_string(),
        Some("simple".to_string())
    );
    assert_eq!(
        tokens[1].token.unescape_string(),
        Some("hello\nworld\t\"quotes\"\\$var".to_string())
    );
}

#[test]
fn test_operators_and_delimiters() {
    let source = "+ - * / % ** ++ -- = += -= *= /= %= **= .= ??= &= |= ^= <<= >>= == === != <> !== < > <= >= <=> && || ! & | ^ ~ << >> ? : ?? ?: -> ?-> => ... .. #[ @";
    let tokens = Lexer::tokenize(source).expect("Failed to tokenize operators");
    let kinds: Vec<_> = tokens.into_iter().map(|t| t.token).collect();

    assert_eq!(
        kinds,
        vec![
            Token::Plus,
            Token::Minus,
            Token::Star,
            Token::Slash,
            Token::Percent,
            Token::StarStar,
            Token::PlusPlus,
            Token::MinusMinus,
            Token::Assign,
            Token::PlusAssign,
            Token::MinusAssign,
            Token::StarAssign,
            Token::SlashAssign,
            Token::PercentAssign,
            Token::StarStarAssign,
            Token::DotAssign,
            Token::NullCoalesceAssign,
            Token::AmpersandAssign,
            Token::PipeAssign,
            Token::CaretAssign,
            Token::ShiftLeftAssign,
            Token::ShiftRightAssign,
            Token::Equal,
            Token::Identical,
            Token::NotEqual,
            Token::NotEqualAlt,
            Token::NotIdentical,
            Token::LessThan,
            Token::GreaterThan,
            Token::LessThanOrEqual,
            Token::GreaterThanOrEqual,
            Token::Spaceship,
            Token::AmpersandAmpersand,
            Token::PipePipe,
            Token::Bang,
            Token::Ampersand,
            Token::Pipe,
            Token::Caret,
            Token::Tilde,
            Token::ShiftLeft,
            Token::ShiftRight,
            Token::Question,
            Token::Colon,
            Token::NullCoalesce,
            Token::Elvis,
            Token::Arrow,
            Token::NullsafeArrow,
            Token::DoubleArrow,
            Token::Ellipsis,
            Token::DotDot,
            Token::AttributeStart,
            Token::At,
        ]
    );
}

#[test]
fn test_source_map_and_diagnostics() {
    let source = "class UserService {\n    public function find(): User {\n        return $invalid;\n    }\n}";
    let mut sm = SourceMap::new();
    let file_id = sm.add_file("UserService.rhp", source);

    let tokens = Lexer::tokenize_with_file_id(source, file_id).expect("Tokenization failed");
    let invalid_var = tokens
        .iter()
        .find(|t| t.token == Token::Variable("$invalid"))
        .expect("Variable not found");

    assert_eq!(invalid_var.span.file_id, file_id);
    assert_eq!(invalid_var.span.line, 3);
    assert_eq!(invalid_var.span.col, 16);

    let diagnostic = sm.format_diagnostic(&invalid_var.span, "undefined variable $invalid");
    assert!(diagnostic.contains("--> UserService.rhp:3:16"));
    assert!(diagnostic.contains("return $invalid;"));
    assert!(diagnostic.contains("^^^^^^^^"));
    assert!(diagnostic.contains("undefined variable $invalid"));
}

#[test]
fn test_error_handling_unterminated_tokens() {
    let unclosed_str = " $valid = 10; 'unclosed string ";
    let result = Lexer::tokenize(unclosed_str);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.error, LexError::UnterminatedString);

    let unclosed_comment = "/* this comment never closes ";
    let result2 = Lexer::tokenize(unclosed_comment);
    assert!(result2.is_ok()); // Block comments at EOF skip to end safely

    let unclosed_doc = "/** doc comment without closing ";
    let result3 = Lexer::tokenize(unclosed_doc);
    assert!(result3.is_err());
    assert_eq!(result3.unwrap_err().error, LexError::UnterminatedComment);
}

#[test]
fn test_numeric_literal_strict_validation() {
    // Trailing underscores and dots must be rejected with exact error message
    for invalid in ["1_", "1_000_", "1.", "1_000.", "0x1A_", "0b10_"] {
        let err = Lexer::tokenize(invalid).expect_err(&format!("Expected error for {}", invalid));
        assert_eq!(
            err.error,
            LexError::InvalidNumericLiteral(
                "Numeric literal cannot end with a separator or decimal point.".to_string()
            )
        );
    }

    // Leading dots and incomplete prefixes/exponents must trigger SyntaxError: invalid syntax.
    for invalid in [".5", "0x", "0b", "1e"] {
        let err = Lexer::tokenize(invalid).expect_err(&format!("Expected error for {}", invalid));
        assert_eq!(
            err.error,
            LexError::InvalidNumericLiteral("SyntaxError: invalid syntax.".to_string())
        );
    }

    // Valid separator must succeed
    let valid = Lexer::tokenize("1_2").expect("Valid numeric separator must succeed");
    assert_eq!(valid[0].token, Token::IntLiteral("1_2"));
}

#[test]
fn test_string_quoting_and_backticks() {
    let single_esc = Lexer::tokenize(r"'it\'s'").expect("Single escaped quote");
    assert_eq!(single_esc[0].token.unescape_string(), Some("it's".to_string()));

    let nested_in_single = Lexer::tokenize(r#"'"nested"'"#).expect("Double inside single");
    assert_eq!(nested_in_single[0].token.unescape_string(), Some("\"nested\"".to_string()));

    let nested_in_double = Lexer::tokenize(r#""'nested'""#).expect("Single inside double");
    assert_eq!(nested_in_double[0].token.unescape_string(), Some("'nested'".to_string()));

    let backtick = Lexer::tokenize(r#"`raw "double" and 'single' quotes`"#).expect("Backtick string");
    assert_eq!(
        backtick[0].token,
        Token::BacktickString(r#"`raw "double" and 'single' quotes`"#)
    );
    assert_eq!(
        backtick[0].token.unescape_string(),
        Some(r#"raw "double" and 'single' quotes"#.to_string())
    );
}

#[test]
fn test_strict_case_sensitivity_and_type_keywords() {
    let toks = Lexer::tokenize("TRUE == true FALSE == false NULL == null string String bool boolean")
        .expect("Tokenize case sensitivity");
    let kinds: Vec<_> = toks.into_iter().map(|t| t.token).collect();
    assert_eq!(
        kinds,
        vec![
            Token::Ident("TRUE"),
            Token::Equal,
            Token::True,
            Token::Ident("FALSE"),
            Token::Equal,
            Token::False,
            Token::Ident("NULL"),
            Token::Equal,
            Token::Null,
            Token::TypeString,
            Token::Ident("String"),
            Token::TypeBool,
            Token::Ident("boolean"),
        ]
    );
}

#[test]
fn test_comments_and_attribute_boundaries() {
    let attr = Lexer::tokenize("#[Route]").expect("Attribute must tokenize");
    assert_eq!(attr[0].token, Token::AttributeStart);
    assert_eq!(attr[1].token, Token::Ident("Route"));
    assert_eq!(attr[2].token, Token::CloseBracket);

    // '#' is strictly reserved for attributes and never used for comments
    assert!(Lexer::tokenize("# line comment").is_err());
    assert!(Lexer::tokenize("#").is_err());

    // Standard comments work as expected
    assert!(Lexer::tokenize("// line comment").unwrap().is_empty());
    let block = Lexer::tokenize("/* block */ code").unwrap();
    assert_eq!(block[0].token, Token::Ident("code"));
}

#[test]
fn test_variable_identifier_validation() {
    for invalid in ["$$", "$$var", "$", "$0", "$9abc"] {
        let err = Lexer::tokenize(invalid).expect_err(&format!("Expected error for {}", invalid));
        assert_eq!(
            err.error,
            LexError::InvalidVariableIdentifier(
                "Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName)."
                    .to_string()
            )
        );
    }
}

#[test]
fn test_static_facade_inheritance_forbidden_and_macro_registry() {
    use rhp::lexer::facade::{validate_inheritance, MacroEntry, MacroRegistry};
    use std::sync::Arc;
    use std::thread;

    // Extending Arr, Str, Int, Float via inheritance is strictly forbidden
    assert_eq!(
        validate_inheritance("Arr"),
        Err("Fatal Error: Cannot extend static facade class 'Arr'. Use 'Arr::macro()' to register custom methods dynamically.".to_string())
    );
    assert_eq!(
        validate_inheritance("Str"),
        Err("Fatal Error: Cannot extend static facade class 'Str'. Use 'Str::macro()' to register custom methods dynamically.".to_string())
    );
    assert!(validate_inheritance("BaseController").is_ok());

    // Thread-safe runtime macro registration (parent bootstrap -> concurrent worker threads)
    let registry = Arc::new(MacroRegistry::new());
    registry
        .register(MacroEntry::new(
            "Arr",
            "toCsv",
            vec![("$delimiter".to_string(), Some("','".to_string()))],
            "return implode($delimiter, $this->all());",
        ))
        .expect("Failed to register Arr::toCsv macro");

    let mut handles = Vec::new();
    for _ in 0..4 {
        let reg = Arc::clone(&registry);
        handles.push(thread::spawn(move || {
            assert!(reg.has("Arr", "toCsv"));
            let entry = reg.get("Arr", "toCsv").expect("Macro must be accessible in worker thread");
            assert_eq!(entry.facade, "Arr");
            assert_eq!(entry.method_name, "toCsv");
            assert_eq!(entry.params[0].0, "$delimiter");
        }));
    }
    for h in handles {
        h.join().expect("Worker thread panicked");
    }

    // Reject macro registration on non-facade classes
    assert!(registry
        .register(MacroEntry::new("CustomUserClass", "foo", vec![], ""))
        .is_err());
}


