use rhp::lexer::{Lexer, token::Token};

fn probe(label: &str, src: &str) {
    print!("{:45} => ", label);
    match Lexer::tokenize(src) {
        Ok(toks) => {
            let names: Vec<String> = toks.iter().map(|t| format!("{:?}", t.token)).collect();
            println!("{}", names.join("  |  "));
        }
        Err(e) => println!("ERROR: {:?}", e),
    }
}

fn main() {
    println!("=== BUG PROBE: Keyword-as-method-name ===");
    probe("$obj->match()",         "$obj->match()");
    probe("$obj->default()",       "$obj->default()");
    probe("$obj->from()",          "$obj->from()");
    probe("$obj->for()",           "$obj->for()");
    probe("$obj->class",           "$obj->class");
    probe("$obj->new()",           "$obj->new()");
    probe("$obj->static()",        "$obj->static()");
    probe("$obj->clone()",         "$obj->clone()");

    println!("\n=== BUG PROBE: Numeric edge cases ===");
    probe("1_",                    "1_");
    probe("1_000_",                "1_000_");
    probe("0x (incomplete)",       "0x");
    probe("0b (incomplete)",       "0b");
    probe(".5 (leading dot)",      ".5");
    probe("1. (trailing dot)",     "1.");
    probe("1e (incomplete exp)",   "1e");
    probe("1_2 (valid separator)", "1_2");

    println!("\n=== BUG PROBE: String edge cases ===");
    probe("single escape quote",   r"'it\'s'");
    probe("CRLF tracking",         "line1\r\n$var");
    probe("CR-only tracking",      "line1\r$var");
    probe("nested quotes",         r#""hello \"world\"""#);

    println!("\n=== BUG PROBE: Operator ambiguity ===");
    probe("... (ellipsis)",        "...");
    probe(".. (dotdot)",           "..");
    probe(". (dot)",               ".");
    probe("... vs ..",             "...");
    probe("??= (null-coalesce=)",  "??=");
    probe("?? (null-coalesce)",    "??");
    probe("?: (elvis)",            "?:");

    println!("\n=== BUG PROBE: Type keywords ===");
    probe("TRUE == true (case)",   "TRUE == true");
    probe("FALSE == false",        "FALSE == false");
    probe("NULL == null",          "NULL == null");
    probe("string vs String",      "string String");
    probe("bool vs boolean",       "bool boolean");

    println!("\n=== BUG PROBE: Comment & Attribute ambiguity ===");
    probe("#[Route]",              "#[Route]");
    probe("# line comment",        "# line comment");
    probe("#",                     "#");
    probe("// line comment",       "// line comment");
    probe("/* block */ code",      "/* block */ code");
    probe("/** doc */ code",       "/** doc */ code");

    println!("\n=== BUG PROBE: elseif vs else if ===");
    probe("elseif (keyword)",      "elseif");
    probe("else if (two tokens)",  "else if");

    println!("\n=== BUG PROBE: Double-dollar / invalid vars ===");
    probe("$$ (invalid var)",      "$$");
    probe("$ (bare dollar)",       "$");
    probe("$0 (digit start)",      "$0");

    println!("\n=== BUG PROBE: col position tracking ===");
    let src = "ab\n   $var";
    match Lexer::tokenize(src) {
        Ok(toks) => {
            for t in &toks {
                println!("  {:?} line={} col={}", t.token, t.span.line, t.span.col);
            }
        }
        Err(e) => println!("ERROR: {:?}", e),
    }
}
