use rhp::migrate::{
    comment_error_reporting_calls, comment_ini_set_calls,
    migrate_path, migrate_source, migrate_source_with_options, MigrateOptions,
    replace_get_array_access, replace_post_array_access,
    strip_error_reporting, upgrade_php_source,
};
use std::fs;
use std::process::Command;
use serde_json::Value;

#[test]
fn test_migrate_replaces_strict_equality() {
    let input = concat!(
        "<?php\n",
        "if ($a === 1 && $b !== 2) {\n",
        "    $c = $a === $b;\n",
        "    $msg = \"=== stays in string ===\";\n",
        "    // === stays in comment\n",
        "}\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.strict_eq_count, 3);
    assert!(res.migrated_content.contains("if ($a == 1 && $b != 2)"));
    assert!(res.migrated_content.contains("$c = $a == $b;"));
    assert!(res.migrated_content.contains("\"=== stays in string ===\""));
    assert!(res.migrated_content.contains("// === stays in comment"));
    assert!(!res.migrated_content.contains("$a ==="));
    assert!(!res.migrated_content.contains("$b !=="));
}

#[test]
fn test_migrate_get_used() {
    let input = concat!(
        "<?php\n",
        "$params = $_GET;\n",
        "echo $params['id'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_GET = Request::query();"]);
    assert!(res.migrated_content.contains("$_GET = Request::query();\n\n$params = $_GET;"));
}

#[test]
fn test_migrate_get_array_access_replaced() {
    let input = concat!(
        "<?php\n",
        "$id = $_GET['id'];\n",
        "echo $id;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.get_replace_count, 1);
    assert_eq!(res.added_lines, Vec::<String>::new());
    assert_eq!(
        res.migrated_content,
        "$id = Request::query('id');\necho $id;\n"
    );
}

#[test]
fn test_migrate_post_used() {
    let input = concat!(
        "<?php\n",
        "$payload = $_POST;\n",
        "echo $payload;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_POST = Request::body();"]);
    assert!(res.migrated_content.contains("$_POST = Request::body();\n\n$payload = $_POST;"));
}

#[test]
fn test_migrate_post_array_access_replaced() {
    let input = concat!(
        "<?php\n",
        "$title = $_POST['title'];\n",
        "echo $title;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.post_replace_count, 1);
    assert_eq!(res.added_lines, Vec::<String>::new());
    assert_eq!(
        res.migrated_content,
        "$title = Request::body('title');\necho $title;\n"
    );
}

#[test]
fn test_migrate_server_vars_used() {
    let input = concat!(
        "<?php\n",
        "$method = $_SERVER['REQUEST_METHOD'];\n",
        "$ip = $_SERVER['REMOTE_ADDR'];\n",
        "$host = $_SERVER['HTTP_HOST'];\n",
        "echo $method . ' ' . $ip . ' ' . $host;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(
        res.added_lines,
        vec![
            "$_SERVER['REQUEST_METHOD'] = Request::method();",
            "$_SERVER['REMOTE_ADDR'] = Request::ip();",
            "$_SERVER['HTTP_HOST'] = Request::host();"
        ]
    );
    assert!(res.migrated_content.contains("$_SERVER['REQUEST_METHOD'] = Request::method();"));
    assert!(res.migrated_content.contains("$_SERVER['REMOTE_ADDR'] = Request::ip();"));
    assert!(res.migrated_content.contains("$_SERVER['HTTP_HOST'] = Request::host();"));
    assert!(res.migrated_content.contains("echo $method + ' ' + $ip + ' ' + $host;\n"));
    assert_eq!(res.dot_concat_count, 4);
}

#[test]
fn test_migrate_files_used() {
    let input = concat!(
        "<?php\n",
        "$doc = $_FILES['document'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_FILES = Request::files();"]);
}

#[test]
fn test_migrate_cookie_read() {
    let input = concat!(
        "<?php\n",
        "$session = $_COOKIE['session_token'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_COOKIE = Cookie::get();"]);
}

#[test]
fn test_migrate_cookie_set_assignment_and_functions() {
    let input = concat!(
        "<?php\n",
        "$_COOKIE['theme'] = 'dark';\n",
        "$_COOKIE['user'] = $currentUser;\n",
        "setcookie('auth', 'token123');\n",
        "\\setrawcookie('raw', 'val456');\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cookie_set_count, 4);
    assert!(res.migrated_content.contains("Cookie::set('theme', 'dark');"));
    assert!(res.migrated_content.contains("Cookie::set('user', $currentUser);"));
    assert!(res.migrated_content.contains("Cookie::set('auth', 'token123');"));
    assert!(res.migrated_content.contains("Cookie::set('raw', 'val456');"));
    // Since $_COOKIE was only written to and never read, $_COOKIE = Cookie::get(); is NOT added
    assert!(!res.migrated_content.contains("$_COOKIE = Cookie::get();"));
}

#[test]
fn test_migrate_cookie_set_and_read() {
    let input = concat!(
        "<?php\n",
        "$_COOKIE['theme'] = 'dark';\n",
        "echo $_COOKIE['theme'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cookie_set_count, 1);
    assert!(res.migrated_content.contains("Cookie::set('theme', 'dark');"));
    // Because $_COOKIE is still read, $_COOKIE = Cookie::get(); IS added
    assert!(res.migrated_content.contains("$_COOKIE = Cookie::get();"));
}

#[test]
fn test_migrate_globals_replacement() {
    let input = concat!(
        "<?php\n",
        "$conn = $_GLOBAL['db'];\n",
        "$secret = $_GLOBALS['api_key'];\n",
        "$app = $GLOBALS['config'];\n",
        "$dynamic = $GLOBALS[$keyName];\n",
        "$_GLOBAL['counter'] = 42;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.global_replace_count, 5);
    assert!(res.migrated_content.contains("$conn = $db;"));
    assert!(res.migrated_content.contains("$secret = $api_key;"));
    assert!(res.migrated_content.contains("$app = $config;"));
    assert!(res.migrated_content.contains("$dynamic = $$keyName;"));
    assert!(res.migrated_content.contains("$counter = 42;"));
}

#[test]
fn test_migrate_all_variables_combined() {
    let input = concat!(
        "<?php\n",
        "$raw_get = $_GET;\n",
        "$raw_post = $_POST;\n",
        "$q = $_GET['search'];\n",
        "$b = $_POST['payload'];\n",
        "$m = $_SERVER['REQUEST_METHOD'];\n",
        "$ip = $_SERVER['REMOTE_ADDR'];\n",
        "$h = $_SERVER['HTTP_HOST'];\n",
        "$f = $_FILES['avatar'];\n",
        "$c = $_COOKIE['session'];\n",
        "$db = $_GLOBAL['conn'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.get_replace_count, 1);
    assert_eq!(res.post_replace_count, 1);
    assert_eq!(
        res.added_lines,
        vec![
            "$_GET = Request::query();",
            "$_POST = Request::body();",
            "$_SERVER['REQUEST_METHOD'] = Request::method();",
            "$_SERVER['REMOTE_ADDR'] = Request::ip();",
            "$_SERVER['HTTP_HOST'] = Request::host();",
            "$_FILES = Request::files();",
            "$_COOKIE = Cookie::get();"
        ]
    );
    assert!(res.migrated_content.contains("$q = Request::query('search');"));
    assert!(res.migrated_content.contains("$b = Request::body('payload');"));
    assert!(res.migrated_content.contains("$db = $conn;"));
}

#[test]
fn test_migrate_declare_strict_types_insertion() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "$raw = $_GET;\n",
        "$page = $_GET['page'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.declare_count, 1);
    assert_eq!(res.get_replace_count, 1);
    let expected = concat!(
        "$_GET = Request::query();\n\n",
        "$raw = $_GET;\n",
        "$page = Request::query('page');\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_namespace_insertion() {
    let input = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "$raw = $_GET;\n",
        "$val = $_GET['val'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    let expected = concat!(
        "namespace App\\Http\\Controllers;\n",
        "$_GET = Request::query();\n\n",
        "$raw = $_GET;\n",
        "$val = Request::query('val');\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_without_opening_tag() {
    let input = "$params = $_GET;\n$id = $_GET['id'];\n";
    let res = migrate_source(input, "script.rhp");
    assert!(res.modified);
    assert!(!res.php_tag_removed);
    let expected = "$_GET = Request::query();\n\n$params = $_GET;\n$id = Request::query('id');\n";
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_does_not_duplicate_existing_lines() {
    // When input already has no tags and no migration needed
    let input_clean = concat!(
        "$_GET = Request::query();\n",
        "$id = Request::query('id');\n"
    );
    let res_clean = migrate_source(input_clean, "test.rhp");
    assert!(!res_clean.modified);
    assert!(res_clean.added_lines.is_empty());

    // When input has <?php tag, tag is removed without duplicating declarations
    let input_tagged = concat!(
        "<?php\n",
        "$_GET = Request::query();\n",
        "$id = Request::query('id');\n"
    );
    let res_tagged = migrate_source(input_tagged, "test.php");
    assert!(res_tagged.modified);
    assert!(res_tagged.php_tag_removed);
    assert!(res_tagged.added_lines.is_empty());
    assert_eq!(res_tagged.migrated_content, input_clean);
}

#[test]
fn test_migrate_ignores_comments_and_strings() {
    // Without <?php tag: no modifications at all
    let input_clean = concat!(
        "// Don't add $_GET here\n",
        "/* Nor $_POST here */\n",
        "$msg = \"This $_FILES is a string\";\n",
        "$msg2 = 'Single quoted $_COOKIE string';\n"
    );
    let res_clean = migrate_source(input_clean, "test.rhp");
    assert!(!res_clean.modified);
    assert!(res_clean.added_lines.is_empty());

    // With <?php and ?> tags: tags removed, but no superglobals injected
    let input_tagged = concat!(
        "<?php\n",
        "// Don't add $_GET here\n",
        "/* Nor $_POST here */\n",
        "$msg = \"This $_FILES is a string\";\n",
        "$msg2 = 'Single quoted $_COOKIE string';\n",
        "?>\n"
    );
    let res_tagged = migrate_source(input_tagged, "test.php");
    assert!(res_tagged.modified);
    assert!(res_tagged.php_tag_removed);
    assert!(res_tagged.added_lines.is_empty());
    assert_eq!(res_tagged.migrated_content, input_clean);
}

#[test]
fn test_cli_migrate_execution() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("index.php");
    let content = concat!(
        "<?php\n",
        "$raw_get = $_GET;\n",
        "$user = $_GET['user'];\n",
        "$_COOKIE['pref'] = 'light';\n",
        "$db = $GLOBALS['database'];\n",
        "echo $user . ' ' . $db;\n"
    );
    fs::write(&file_path, content).unwrap();

    // 1. Dry run / check mode with `rhp migrate --check`
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("Dry Run"));
    assert!(check_stdout.contains("$_GET = Request::query();"));
    assert!(check_stdout.contains("1 $_GET[...] access(es) replaced with Request::query(...)"));

    // Verify file on disk was not modified yet
    assert_eq!(fs::read_to_string(&file_path).unwrap(), content);

    // 2. Perform actual migration with `rhp migrate`
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .output()
        .expect("failed to run rhp migrate");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(migrated_content.contains("$_GET = Request::query();"));
    assert!(migrated_content.contains("$user = Request::query('user');"));
    assert!(migrated_content.contains("Cookie::set('pref', 'light');"));
    assert!(migrated_content.contains("$db = $database;"));
    assert!(migrated_content.contains("echo $user + ' ' + $db;"));
    assert!(!migrated_content.contains("<?php"));

    // 3. Test backward-compatible alias `rhp upgrade-php`
    let alias_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("upgrade-php")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp upgrade-php alias");
    assert!(alias_output.status.success());

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_dot_concatenation_basic() {
    let input = concat!(
        "<?php\n",
        "$str = 'hello ' . 'world';\n",
        "$msg = \"count: \" . $count . \" items\";\n",
        "$tight = $a.$b;\n",
        "$mixed = $a. $b . $c;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.dot_concat_count, 6);
    assert!(res.migrated_content.contains("$str = 'hello ' + 'world';"));
    assert!(res.migrated_content.contains("$msg = \"count: \" + $count + \" items\";"));
    assert!(res.migrated_content.contains("$tight = $a + $b;"));
    assert!(res.migrated_content.contains("$mixed = $a + $b + $c;"));
}

#[test]
fn test_migrate_dot_assign_operator() {
    let input = concat!(
        "<?php\n",
        "$str = 'init';\n",
        "$str .= ' more';\n",
        "$str .= $extra;\n",
        "$tight.=$b;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.dot_concat_count, 3);
    assert!(res.migrated_content.contains("$str += ' more';"));
    assert!(res.migrated_content.contains("$str += $extra;"));
    assert!(res.migrated_content.contains("$tight += $b;"));
}

#[test]
fn test_migrate_preserves_floats() {
    let input = concat!(
        "<?php\n",
        "$pi = 3.14159;\n",
        "$zero = 0.0;\n",
        "$half = .5;\n",
        "$rate = 10.5;\n",
        "$concat_float = 'pi: ' . 3.14;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    // Only the dot in 'pi: ' . 3.14 should be replaced; the float literals must not be touched
    assert_eq!(res.dot_concat_count, 1);
    assert!(res.migrated_content.contains("$pi = 3.14159;"));
    assert!(res.migrated_content.contains("$zero = 0.0;"));
    assert!(res.migrated_content.contains("$half = .5;"));
    assert!(res.migrated_content.contains("$rate = 10.5;"));
    assert!(res.migrated_content.contains("$concat_float = 'pi: ' + 3.14;"));
}

#[test]
fn test_migrate_preserves_ellipsis_and_range() {
    let input = concat!(
        "<?php\n",
        "function variadic(...$args) {\n",
        "    return [...$args];\n",
        "}\n",
        "$range = 1..10;\n",
        "$str = 'arg: ' . $args[0];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.dot_concat_count, 1);
    assert!(res.migrated_content.contains("function variadic(...$args)"));
    assert!(res.migrated_content.contains("return [...$args];"));
    assert!(res.migrated_content.contains("$range = 1..10;"));
    assert!(res.migrated_content.contains("$str = 'arg: ' + $args[0];"));
}

#[test]
fn test_migrate_preserves_dots_in_strings_and_comments() {
    let input = concat!(
        "<?php\n",
        "// A comment with a dot. And more dots...\n",
        "/* Multi-line comment. with dots. */\n",
        "# Hash comment with dot. here\n",
        "$file = \"archive.tar.gz\";\n",
        "$host = 'example.com';\n",
        "$sql = \"SELECT users.id, users.name FROM users\";\n",
        "$res = $file . ' from ' . $host;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.dot_concat_count, 2);
    assert!(res.migrated_content.contains("// A comment with a dot. And more dots..."));
    assert!(res.migrated_content.contains("/* Multi-line comment. with dots. */"));
    assert!(res.migrated_content.contains("# Hash comment with dot. here"));
    assert!(res.migrated_content.contains("$file = \"archive.tar.gz\";"));
    assert!(res.migrated_content.contains("$host = 'example.com';"));
    assert!(res.migrated_content.contains("$sql = \"SELECT users.id, users.name FROM users\";"));
    assert!(res.migrated_content.contains("$res = $file + ' from ' + $host;"));
}

#[test]
fn test_migrate_multiline_dot_concatenation() {
    let input = concat!(
        "<?php\n",
        "$query = \"SELECT * FROM users \"\n",
        "    . \"WHERE active = 1 \"\n",
        "    . \"ORDER BY id DESC\";\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.dot_concat_count, 2);
    let expected = concat!(
        "$query = \"SELECT * FROM users \"\n",
        "    + \"WHERE active = 1 \"\n",
        "    + \"ORDER BY id DESC\";\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_removes_opening_and_closing_tags() {
    let input = concat!(
        "<?php\n",
        "echo 'Hello, World!';\n",
        "?>\n"
    );
    let res = migrate_source(input, "hello.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "echo 'Hello, World!';\n");
}

#[test]
fn test_migrate_removes_opening_tag_only() {
    let input = concat!(
        "<?php\n",
        "$x = 42;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "$x = 42;\n");
}

#[test]
fn test_migrate_removes_closing_tag_only() {
    let input = concat!(
        "$x = 42;\n",
        "?>\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "$x = 42;\n");
}

#[test]
fn test_migrate_removes_tags_inline() {
    let input = "<?php echo 1; ?>\n";
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "echo 1;\n");
}

#[test]
fn test_migrate_preserves_php_tags_in_strings_and_comments() {
    let input = concat!(
        "<?php\n",
        "$str = '<?php echo \"inside string\"; ?>';\n",
        "// <?php in comment ?>\n",
        "?>\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    let expected = concat!(
        "$str = '<?php echo \"inside string\"; ?>';\n",
        "// <?php in comment ?>\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_removes_case_insensitive_tags() {
    let input = concat!(
        "<?PHP\n",
        "$title = 'Title';\n",
        "?>\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "$title = 'Title';\n");
}

#[test]
fn test_migrate_empty_tags_only() {
    let input = "<?php ?>\n";
    let res = migrate_source(input, "empty.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "");

    let input_multiline = "<?php\n?>\n";
    let res_multiline = migrate_source(input_multiline, "empty.php");
    assert!(res_multiline.modified);
    assert!(res_multiline.php_tag_removed);
    assert_eq!(res_multiline.migrated_content, "");
}

#[test]
fn test_migrate_tags_with_blank_lines_and_spaces() {
    let input = concat!(
        "   <?php   \n",
        "\n",
        "$a = 1;\n",
        "\n",
        "   ?>   \n"
    );
    let res = migrate_source(input, "spaces.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    assert_eq!(res.migrated_content, "$a = 1;\n");
}

#[test]
fn test_migrate_preserves_indentation_after_opening_tag() {
    let input = concat!(
        "<?php\n",
        "    class MyClass {\n",
        "        public int $val = 1;\n",
        "    }\n",
        "?>\n"
    );
    let res = migrate_source(input, "indent.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    let expected = concat!(
        "    class MyClass {\n",
        "        public $val: int = 1;\n",
        "    }\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_curl_basic_get_option_a() {
    let input = concat!(
        "$ch = curl_init();\n",
        "curl_setopt($ch, CURLOPT_URL, \"https://api.example.com/items\");\n",
        "curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);\n",
        "$res = curl_exec($ch);\n",
        "curl_close($ch);\n",
        "echo $res;\n"
    );
    let res = migrate_source(input, "curl_test.php");
    assert!(res.modified);
    assert_eq!(res.curl_migrated_count, 1);
    // Original curl code is commented
    assert!(res.migrated_content.contains("// $ch = curl_init();"));
    assert!(res.migrated_content.contains("// curl_setopt($ch, CURLOPT_URL, \"https://api.example.com/items\");"));
    assert!(res.migrated_content.contains("// $res = curl_exec($ch);"));
    assert!(res.migrated_content.contains("// curl_close($ch);"));
    // Option A generates $response and $res = $response->body()
    assert!(res.migrated_content.contains("$response = Http::get(\"https://api.example.com/items\");"));
    assert!(res.migrated_content.contains("$res = $response->body();"));
    assert!(res.migrated_content.contains("echo $res;"));
}

#[test]
fn test_migrate_curl_option_b_json_decode() {
    let input = concat!(
        "$ch = curl_init();\n",
        "curl_setopt($ch, CURLOPT_URL, \"https://api.example.com/users\");\n",
        "curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);\n",
        "curl_setopt($ch, CURLOPT_TIMEOUT, 10);\n",
        "$res = curl_exec($ch);\n",
        "curl_close($ch);\n",
        "$data = json_decode($res, true);\n"
    );
    let res = migrate_source(input, "curl_test.php");
    assert!(res.modified);
    assert_eq!(res.curl_migrated_count, 1);
    // Original curl code and json_decode are commented
    assert!(res.migrated_content.contains("// $ch = curl_init();"));
    assert!(res.migrated_content.contains("// $res = curl_exec($ch);"));
    assert!(res.migrated_content.contains("// $data = Json::decode($res);"));
    // Option B rewrites directly to ->json()
    assert!(res.migrated_content.contains("$data = Http::timeout(10)->get(\"https://api.example.com/users\")->json();"));
}

#[test]
fn test_migrate_curl_unassigned_future_async() {
    let input = concat!(
        "$ch = curl_init(\"https://api.example.com/webhook\");\n",
        "curl_setopt($ch, CURLOPT_POST, true);\n",
        "curl_setopt($ch, CURLOPT_POSTFIELDS, \"event=ping\");\n",
        "curl_exec($ch);\n",
        "curl_close($ch);\n"
    );
    let res = migrate_source(input, "curl_webhook.php");
    assert!(res.modified);
    assert_eq!(res.curl_migrated_count, 1);
    // Commented old code
    assert!(res.migrated_content.contains("// $ch = curl_init(\"https://api.example.com/webhook\");"));
    assert!(res.migrated_content.contains("// curl_exec($ch);"));
    // Output not saved to variable -> wraps in Future::async(...)
    assert!(res.migrated_content.contains("Future::async(fn() => Http::post(\"https://api.example.com/webhook\", \"event=ping\"));"));
}

#[test]
fn test_migrate_curl_multi_flagged_for_review() {
    let input = concat!(
        "$mh = curl_multi_init();\n",
        "curl_multi_add_handle($mh, $ch1);\n",
        "curl_multi_exec($mh, $active);\n",
        "curl_multi_close($mh);\n"
    );
    let res = migrate_source(input, "curl_multi.php");
    assert!(res.modified);
    assert!(res.curl_multi_count > 0);
    // Flags with guide link and recommendation
    assert!(res.migrated_content.contains("// NOTICE: curl_multi_* requires manual review. See https://rhp.dev/docs/reference/unsupported-php#curl-extension"));
    assert!(res.migrated_content.contains("// You can use Http::pool([...]) / Future::all([...])"));
    // Comments old code
    assert!(res.migrated_content.contains("// $mh = curl_multi_init();"));
    assert!(res.migrated_content.contains("// curl_multi_exec($mh, $active);"));
}

#[test]
fn test_migrate_curl_post_json_with_token() {
    let input = concat!(
        "$ch = curl_init(\"https://api.example.com/orders\");\n",
        "curl_setopt($ch, CURLOPT_POST, 1);\n",
        "curl_setopt($ch, CURLOPT_POSTFIELDS, json_encode(['sku' => 'LAPTOP', 'qty' => 1]));\n",
        "curl_setopt($ch, CURLOPT_HTTPHEADER, [\n",
        "    \"Authorization: Bearer \" . $token,\n",
        "    \"X-Tenant: acme\",\n",
        "]);\n",
        "$res = curl_exec($ch);\n",
        "curl_close($ch);\n"
    );
    let res = migrate_source(input, "orders.php");
    assert!(res.modified);
    assert_eq!(res.curl_migrated_count, 1);
    assert!(res.migrated_content.contains("Http::withToken($token)"));
    assert!(res.migrated_content.contains("withHeaders([\"X-Tenant: acme\"])"));
    assert!(res.migrated_content.contains("post(\"https://api.example.com/orders\", ['sku' => 'LAPTOP', 'qty' => 1])"));
}

#[test]
fn test_migrate_curl_with_status_code() {
    let input = concat!(
        "$ch = curl_init(\"https://api.example.com/health\");\n",
        "curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);\n",
        "$raw = curl_exec($ch);\n",
        "$status = curl_getinfo($ch, CURLINFO_HTTP_CODE);\n",
        "curl_close($ch);\n"
    );
    let res = migrate_source(input, "health.php");
    assert!(res.modified);
    assert_eq!(res.curl_migrated_count, 1);
    assert!(res.migrated_content.contains("$response = Http::get(\"https://api.example.com/health\");"));
    assert!(res.migrated_content.contains("$raw = $response->body();"));
    assert!(res.migrated_content.contains("$status = $response->status();"));
}

#[test]
fn test_migrate_curl_setopt_array() {
    let input = concat!(
        "$ch = curl_init();\n",
        "curl_setopt_array($ch, [\n",
        "    CURLOPT_URL => \"https://api.example.com/data\",\n",
        "    CURLOPT_RETURNTRANSFER => true,\n",
        "    CURLOPT_TIMEOUT => 20,\n",
        "]);\n",
        "$res = curl_exec($ch);\n",
        "curl_close($ch);\n"
    );
    let res = migrate_source(input, "data.php");
    assert!(res.modified);
    assert_eq!(res.curl_migrated_count, 1);
    assert!(res.migrated_content.contains("$response = Http::timeout(20)->get(\"https://api.example.com/data\");"));
    assert!(res.migrated_content.contains("$res = $response->body();"));
}

#[test]
fn test_migrate_heredoc_html() {
    let input = concat!(
        "$html = <<<HTML\n",
        "<div class=\"card\">\n",
        "    <h1>Title</h1>\n",
        "</div>\n",
        "HTML;\n"
    );
    let res = migrate_source(input, "page.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 1);
    let expected = concat!(
        "$html = ```html\n",
        "<div class=\"card\">\n",
        "    <h1>Title</h1>\n",
        "</div>\n",
        "```;\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_heredoc_sql() {
    let input = concat!(
        "$sql = <<<SQL\n",
        "SELECT id, name\n",
        "FROM users\n",
        "WHERE active = 1\n",
        "SQL;\n"
    );
    let res = migrate_source(input, "query.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 1);
    let expected = concat!(
        "$sql = ```sql\n",
        "SELECT id, name\n",
        "FROM users\n",
        "WHERE active = 1\n",
        "```;\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_nowdoc_html_and_sql() {
    let input = concat!(
        "$html = <<<'HTML'\n",
        "<p>$var_not_interpolated</p>\n",
        "HTML;\n",
        "$sql = <<<'SQL'\n",
        "SELECT * FROM logs;\n",
        "SQL;\n"
    );
    let res = migrate_source(input, "views.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 2);
    assert!(res.migrated_content.contains("$html = ```html\n<p>$var_not_interpolated</p>\n```;"));
    assert!(res.migrated_content.contains("$sql = ```sql\nSELECT * FROM logs;\n```;"));
}

#[test]
fn test_migrate_heredoc_generic_eof() {
    let input = concat!(
        "$txt = <<<EOF\n",
        "Plain text string\n",
        "EOF;\n"
    );
    let res = migrate_source(input, "text.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 1);
    let expected = concat!(
        "$txt = ```\n",
        "Plain text string\n",
        "```;\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_heredoc_indented_php73() {
    let input = concat!(
        "function getQuery() {\n",
        "    return <<<SQL\n",
        "        SELECT id, title\n",
        "        FROM posts\n",
        "        SQL;\n",
        "}\n"
    );
    let res = migrate_source(input, "posts.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 1);
    let expected = concat!(
        "function getQuery() {\n",
        "    return ```sql\n",
        "        SELECT id, title\n",
        "        FROM posts\n",
        "        ```;\n",
        "}\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_heredoc_in_function_argument() {
    let input = concat!(
        "render(<<<HTML\n",
        "<button class=\"btn\">Click me</button>\n",
        "HTML, 42);\n"
    );
    let res = migrate_source(input, "render.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 1);
    let expected = concat!(
        "render(```html\n",
        "<button class=\"btn\">Click me</button>\n",
        "```, 42);\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_heredoc_dot_concatenation_preserved_inside() {
    let input = concat!(
        "$query = <<<SQL\n",
        "SELECT users.id, profiles.name\n",
        "FROM users\n",
        "SQL;\n",
        "$extra = \"hello\" . \"world\";\n"
    );
    let res = migrate_source(input, "query_dots.php");
    assert!(res.modified);
    assert_eq!(res.heredoc_count, 1);
    assert_eq!(res.dot_concat_count, 1);
    assert!(res.migrated_content.contains("SELECT users.id, profiles.name"));
    assert!(res.migrated_content.contains("\"hello\" + \"world\""));
}

#[test]
fn test_migrate_heredoc_ignored_inside_comments_and_strings() {
    let input = concat!(
        "// Comment with <<<HTML\n",
        "$msg = \"Here is <<<SQL in a string\";\n"
    );
    let res = migrate_source(input, "comments.php");
    assert_eq!(res.heredoc_count, 0);
    assert_eq!(res.migrated_content, input);
}

#[test]
fn test_migrate_declare_strict_types_removal() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "\n",
        "$x = 42;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    let expected = concat!(
        "\n",
        "$x = 42;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_declare_ticks_removal() {
    let input = concat!(
        "<?php\n",
        "declare(ticks=1);\n",
        "\n",
        "echo \"Processing\";\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    let expected = concat!(
        "\n",
        "echo \"Processing\";\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_declare_encoding_removal() {
    let input = concat!(
        "<?php\n",
        "declare(encoding='ISO-8859-1');\n",
        "\n",
        "$text = \"test\";\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    let expected = concat!(
        "\n",
        "$text = \"test\";\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_multiple_declares_removal() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "declare(ticks=1);\n",
        "declare(encoding=\"UTF-8\");\n",
        "\n",
        "function add(int $a, int $b): int {\n",
        "    return $a + $b;\n",
        "}\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 3);
    let expected = concat!(
        "\n",
        "function add($a: int, $b: int): int {\n",
        "    return $a + $b;\n",
        "}\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_combined_declare_directives() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1, ticks=1);\n",
        "$x = 10;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    let expected = concat!(
        "$x = 10;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_declare_multiline_and_spaces() {
    let input = concat!(
        "<?php\n",
        "declare (\n",
        "    strict_types = 1,\n",
        "    ticks = 1\n",
        ") ;\n",
        "$ready = true;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    let expected = concat!(
        "$ready = true;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_declare_inline_with_php_tag() {
    let input = "<?php declare(strict_types=1);\n$val = 100;\n";
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    assert_eq!(res.upgraded_content, "$val = 100;\n");
}

#[test]
fn test_migrate_declare_block_braces() {
    let input = concat!(
        "<?php\n",
        "declare(ticks=1) {\n",
        "    $i = 0;\n",
        "    $i++;\n",
        "}\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    let expected = concat!(
        "    $i = 0;\n",
        "    $i++;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_declare_in_comments_and_strings_preserved() {
    let input = concat!(
        "// declare(strict_types=1);\n",
        "/* declare(ticks=1); */\n",
        "$str = 'declare(encoding=\"UTF-8\");';\n",
        "$str2 = \"declare(strict_types=1);\";\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.declare_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_cli_migrate_write_removes_declares() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("service.php");
    let content = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "declare(ticks=1);\n",
        "declare(encoding='UTF-8');\n",
        "\n",
        "class Service {\n",
        "    public function run(): void {}\n",
        "}\n"
    );
    fs::write(&file_path, content).unwrap();

    // 1. Dry run via `rhp migrate --check`
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("Dry Run"));
    assert!(check_stdout.contains("3 declare() directive(s) removed"));
    // File on disk unmodified during check
    assert_eq!(fs::read_to_string(&file_path).unwrap(), content);

    // 2. Actual write via `rhp migrate --write`
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(!migrated_content.contains("declare("));
    assert!(migrated_content.contains("class Service {"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_type_casts_basic() {
    let input = concat!(
        "<?php\n",
        "$a = (int)$x;\n",
        "$b = (string)$y;\n",
        "$c = (float)$z;\n",
        "$d = (bool)$w;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cast_count, 4);
    let expected = concat!(
        "$a = int($x);\n",
        "$b = string($y);\n",
        "$c = float($z);\n",
        "$d = bool($w);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_type_casts_with_spaces_and_casing() {
    let input = concat!(
        "<?php\n",
        "$a = (INT) $x;\n",
        "$b = ( string )  $y;\n",
        "$c = (FLOAT)$z;\n",
        "$d = (BOOL) $w;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cast_count, 4);
    let expected = concat!(
        "$a = int($x);\n",
        "$b = string($y);\n",
        "$c = float($z);\n",
        "$d = bool($w);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_type_casts_aliases() {
    let input = concat!(
        "<?php\n",
        "$a = (integer)$x;\n",
        "$b = (boolean)$y;\n",
        "$c = (double)$z;\n",
        "$d = (real)$w;\n",
        "$e = (binary)$v;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cast_count, 5);
    let expected = concat!(
        "$a = int($x);\n",
        "$b = bool($y);\n",
        "$c = float($z);\n",
        "$d = float($w);\n",
        "$e = string($v);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_type_casts_complex_expressions() {
    let input = concat!(
        "<?php\n",
        "$id = (int)$_GET['id'];\n",
        "$name = (string)$user->profile->getName();\n",
        "$total = (float)$item['price'] * $qty;\n",
        "$sum = (int)($a + $b);\n",
        "$cnt = (int)count($list);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cast_count, 5);
    assert_eq!(res.get_replace_count, 1);
    assert!(res.upgraded_content.contains("$id = int(Request::query('id'));"));
    assert!(res.upgraded_content.contains("$name = string($user->profile->getName());"));
    assert!(res.upgraded_content.contains("$total = float($item['price']) * $qty;"));
    assert!(res.upgraded_content.contains("$sum = int($a + $b);"));
    assert!(res.upgraded_content.contains("$cnt = int(count($list));"));
}

#[test]
fn test_migrate_type_casts_chained() {
    let input = concat!(
        "<?php\n",
        "$val = (string)(int)$x;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cast_count, 2);
    let expected = concat!(
        "$val = string(int($x));\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_type_casts_in_comments_and_strings_preserved() {
    let input = concat!(
        "// (int)$x\n",
        "/* (string)$y */\n",
        "$msg = '(float)$z is a string';\n",
        "$msg2 = \"(bool)$w in double quotes\";\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.cast_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_cli_migrate_write_type_casts() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_cast_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("calc.php");
    let content = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "$id = (int)$_GET['id'];\n",
        "$price = (float)$raw_price;\n",
        "$title = (string)$name;\n",
        "$isActive = (bool)$flag;\n"
    );
    fs::write(&file_path, content).unwrap();

    // Dry-run
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("4 type cast(s) converted to functional conversion(s)"));
    assert!(check_stdout.contains("1 declare() directive(s) removed"));
    assert!(check_stdout.contains("1 $_GET[...] access(es) replaced with Request::query(...)"));

    // Write mode
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(!migrated_content.contains("declare("));
    assert!(migrated_content.contains("int(Request::query('id'))"));
    assert!(migrated_content.contains("float($raw_price)"));
    assert!(migrated_content.contains("string($name)"));
    assert!(migrated_content.contains("bool($flag)"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_list_destructuring_basic() {
    let input = concat!(
        "<?php\n",
        "list($a, $b) = $coords;\n",
        "list($x) = $single;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.list_destructure_count, 2);
    let expected = concat!(
        "[$a, $b] = $coords;\n",
        "[$x] = $single;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_list_destructuring_skipping() {
    let input = concat!(
        "<?php\n",
        "list($r, , $b) = getRgbColors();\n",
        "list(, , $z) = $dimensions;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.list_destructure_count, 2);
    let expected = concat!(
        "[$r, , $b] = getRgbColors();\n",
        "[, , $z] = $dimensions;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_list_destructuring_keyed() {
    let input = concat!(
        "<?php\n",
        "list('id' => $id, 'role' => $role) = $user;\n",
        "list(\"name\" => $name, \"age\" => $age) = $profile;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.list_destructure_count, 2);
    let expected = concat!(
        "['id' => $id, 'role' => $role] = $user;\n",
        "[\"name\" => $name, \"age\" => $age] = $profile;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_list_destructuring_nested() {
    let input = concat!(
        "<?php\n",
        "list($a, list($b, $c)) = $nested;\n",
        "list(list($x, $y), list($w, $h)) = $box;\n",
        "list($a, list($b, list($c, $d))) = $deep;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    // 2 in first line, 3 in second line, 3 in third line = 8 list(...) constructs
    assert_eq!(res.list_destructure_count, 8);
    let expected = concat!(
        "[$a, [$b, $c]] = $nested;\n",
        "[[$x, $y], [$w, $h]] = $box;\n",
        "[$a, [$b, [$c, $d]]] = $deep;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_list_destructuring_foreach() {
    let input = concat!(
        "<?php\n",
        "foreach ($rows as list($id, $name)) {\n",
        "    echo $id;\n",
        "}\n",
        "foreach ($data as $key => list($a, $b)) {\n",
        "    echo $key;\n",
        "}\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.list_destructure_count, 2);
    let expected = concat!(
        "for ([$id, $name] in $rows) {\n",
        "    echo $id;\n",
        "}\n",
        "for ($key => [$a, $b] in $data) {\n",
        "    echo $key;\n",
        "}\n"
    );
    assert_eq!(res.foreach_count, 2);
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_foreach_to_for_in() {
    let input = concat!(
        "<?php\n",
        "foreach ($users as $name) {\n",
        "    echo $name;\n",
        "}\n",
        "foreach($rows as $k => $v) {\n",
        "    echo $k;\n",
        "}\n",
        "$keep = \"foreach ($a as $b)\";\n",
        "for ($i = 0; $i < 10; $i++) {\n",
        "    echo $i;\n",
        "}\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert_eq!(res.foreach_count, 2);
    assert!(res.upgraded_content.contains("for ($name in $users) {"));
    assert!(res.upgraded_content.contains("for ($k => $v in $rows) {"));
    assert!(res.upgraded_content.contains("\"foreach ($a as $b)\""));
    assert!(res.upgraded_content.contains("for ($i = 0; $i < 10; $i++) {"));
    assert!(!res.upgraded_content.contains("foreach ($users"));
}

#[test]
fn test_migrate_list_destructuring_multiline_and_spacing() {
    let input = concat!(
        "<?php\n",
        "list(\n",
        "    $first,\n",
        "    $second,\n",
        ") = $pair;\n",
        "list ($m, $n) = $other;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.list_destructure_count, 2);
    let expected = concat!(
        "[\n",
        "    $first,\n",
        "    $second,\n",
        "] = $pair;\n",
        "[$m, $n] = $other;\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_list_preserves_methods_and_static_calls() {
    let input = concat!(
        "$items = Arr::list($a, $b);\n",
        "$collection->list();\n",
        "$obj?->list('all');\n",
        "$list($foo, $bar);\n",
        "class Handler {\n",
        "    public function list($filter) {\n",
        "        return [];\n",
        "    }\n",
        "}\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.list_destructure_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_migrate_list_preserves_comments_and_strings() {
    let input = concat!(
        "// list($a, $b) = $x;\n",
        "/* list($c, $d) = $y; */\n",
        "$msg = 'list($a, $b) in single quotes';\n",
        "$str = \"list($c, $d) in double quotes\";\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.list_destructure_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_cli_migrate_write_list_destructuring() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_list_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("destruct.php");
    let content = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "list($id, $name) = $row;\n",
        "list($a, list($b, $c)) = $coords;\n"
    );
    fs::write(&file_path, content).unwrap();

    // Dry-run
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("3 list(...) destructuring construct(s) converted to [...]"));
    assert!(check_stdout.contains("1 declare() directive(s) removed"));

    // Write mode
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(!migrated_content.contains("declare("));
    assert!(migrated_content.contains("[$id, $name] = $row;"));
    assert!(migrated_content.contains("[$a, [$b, $c]] = $coords;"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_error_suppression_function_calls() {
    let input = concat!(
        "<?php\n",
        "@unlink($file);\n",
        "$content = @file_get_contents($url);\n",
        "@mkdir($dir, 0777, true);\n",
        "@chmod($path, 0755);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.error_suppress_count, 4);
    let expected = concat!(
        "unlink($file);\n",
        "$content = file_get_contents($url);\n",
        "mkdir($dir, 0777, true);\n",
        "chmod($path, 0755);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_error_suppression_variables_and_arrays() {
    let input = concat!(
        "<?php\n",
        "$val = @$arr['key'];\n",
        "$item = @$object->property;\n",
        "$nested = @$data[0]['nested'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.error_suppress_count, 3);
    let expected = concat!(
        "$val = $arr['key'];\n",
        "$item = $object->property;\n",
        "$nested = $data[0]['nested'];\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_error_suppression_multiple_and_spacing() {
    let input = concat!(
        "<?php\n",
        "@@unlink($file);\n",
        "$a = @ unlink($x);\n",
        "$b = @  $arr['k'];\n",
        "foo(@$p1, @ $p2);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.error_suppress_count, 6);
    let expected = concat!(
        "unlink($file);\n",
        "$a = unlink($x);\n",
        "$b = $arr['k'];\n",
        "foo($p1, $p2);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_error_suppression_control_flow_and_keywords() {
    let input = concat!(
        "<?php\n",
        "if (@unlink($file)) {\n",
        "    echo 1;\n",
        "}\n",
        "return @json_decode($json, true);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.error_suppress_count, 2);
    let expected = concat!(
        "if (unlink($file)) {\n",
        "    echo 1;\n",
        "}\n",
        "return Json::decode($json);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_error_suppression_chained_with_casts_and_lists() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "list($a, $b) = @getCoords();\n",
        "$id = (int)@$_GET['id'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    assert_eq!(res.list_destructure_count, 1);
    assert_eq!(res.cast_count, 1);
    assert_eq!(res.get_replace_count, 1);
    assert_eq!(res.error_suppress_count, 2);
    let expected = concat!(
        "[$a, $b] = getCoords();\n",
        "$id = int(Request::query('id'));\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_error_suppression_preserves_strings_and_comments() {
    let input = concat!(
        "// @param int $x\n",
        "/* @var User $user */\n",
        "# @todo fix later\n",
        "$email = 'support@domain.com';\n",
        "$msg = \"contact@example.org\";\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.error_suppress_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_cli_migrate_write_error_suppression() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_suppress_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("suppress.php");
    let content = concat!(
        "<?php\n",
        "@unlink($file);\n",
        "$content = @file_get_contents($url);\n",
        "$id = (int)@$_GET['id'];\n"
    );
    fs::write(&file_path, content).unwrap();

    // Dry-run
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("3 error suppression '@' operator(s) removed"));
    assert!(check_stdout.contains("1 type cast(s) converted to functional conversion(s)"));
    assert!(check_stdout.contains("1 $_GET[...] access(es) replaced with Request::query(...)"));

    // Write mode
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(!migrated_content.contains("@unlink"));
    assert!(!migrated_content.contains("@file_get_contents"));
    assert!(!migrated_content.contains("@$_GET"));
    assert!(migrated_content.contains("unlink($file);"));
    assert!(migrated_content.contains("file_get_contents($url);"));
    assert!(migrated_content.contains("int(Request::query('id'))"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_var_dump_basic() {
    let input = concat!(
        "<?php\n",
        "var_dump($a);\n",
        "var_dump($x, $y, $z);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.var_dump_count, 2);
    let expected = concat!(
        "dd($a);\n",
        "dd($x, $y, $z);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_var_export_basic() {
    let input = concat!(
        "<?php\n",
        "var_export($user);\n",
        "var_export($data, true);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.var_dump_count, 2);
    let expected = concat!(
        "dd($user);\n",
        "dd($data, true);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_var_dump_and_export_casing_and_namespaces() {
    let input = concat!(
        "<?php\n",
        "\\var_dump($foo);\n",
        "\\var_export($bar);\n",
        "VAR_DUMP($baz);\n",
        "Var_Export($qux);\n",
        "var_dump ($spaced);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.var_dump_count, 5);
    let expected = concat!(
        "dd($foo);\n",
        "dd($bar);\n",
        "dd($baz);\n",
        "dd($qux);\n",
        "dd ($spaced);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_migrate_var_dump_preserves_methods_and_custom_names() {
    let input = concat!(
        "$dumper->var_dump($a);\n",
        "$exporter?->var_export($b);\n",
        "Debugger::var_dump($c);\n",
        "$var_dump($d);\n",
        "class Debug {\n",
        "    public function var_dump($e) {}\n",
        "}\n",
        "my_var_dump($f);\n",
        "var_dump_all($g);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.var_dump_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_migrate_var_dump_preserves_strings_and_comments() {
    let input = concat!(
        "// var_dump($x);\n",
        "/* var_export($y); */\n",
        "$msg = \"calling var_dump($z)\";\n",
        "$sql = 'SELECT var_export FROM logs';\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert_eq!(res.var_dump_count, 0);
    assert_eq!(res.upgraded_content, input);
}

#[test]
fn test_cli_migrate_write_var_dump_and_export() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_dump_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("debug.php");
    let content = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "var_dump($user);\n",
        "var_export($config);\n"
    );
    fs::write(&file_path, content).unwrap();

    // Dry-run
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("2 var_dump / var_export call(s) replaced with dd()"));
    assert!(check_stdout.contains("1 declare() directive(s) removed"));

    // Write mode
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(!migrated_content.contains("declare("));
    assert!(!migrated_content.contains("var_dump("));
    assert!(!migrated_content.contains("var_export("));
    assert!(migrated_content.contains("dd($user);"));
    assert!(migrated_content.contains("dd($config);"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_strip_error_reporting_basic() {
    let input = concat!(
        "<?php\n",
        "error_reporting(E_ALL);\n",
        "$x = 1;\n",
        "error_reporting(0);\n",
        "$y = 2;\n",
        "error_reporting(E_ALL & ~E_NOTICE);\n",
        "$z = 3;\n",
        "error_reporting();\n"
    );
    let (cleaned, count) = strip_error_reporting(input);
    assert_eq!(count, 4);
    let expected = concat!(
        "<?php\n",
        "$x = 1;\n",
        "$y = 2;\n",
        "$z = 3;\n"
    );
    assert_eq!(cleaned, expected);
}

#[test]
fn test_strip_error_reporting_with_at_and_slash() {
    let input = concat!(
        "<?php\n",
        "@error_reporting(0);\n",
        "\\error_reporting(E_ALL);\n",
        "@\\error_reporting(E_ALL | E_STRICT);\n",
        "$a = 10;\n"
    );
    let (cleaned, count) = strip_error_reporting(input);
    assert_eq!(count, 3);
    let expected = concat!(
        "<?php\n",
        "$a = 10;\n"
    );
    assert_eq!(cleaned, expected);
}

#[test]
fn test_strip_error_reporting_trailing_comments_and_spacing() {
    let input = concat!(
        "<?php\n",
        "    error_reporting(E_ALL); // enable all warnings\n",
        "    $x = 42;\n",
        "    error_reporting(0); # disable completely\n",
        "    $y = 84;\n"
    );
    let (cleaned, count) = strip_error_reporting(input);
    assert_eq!(count, 2);
    let expected = concat!(
        "<?php\n",
        "    $x = 42;\n",
        "    $y = 84;\n"
    );
    assert_eq!(cleaned, expected);
}

#[test]
fn test_strip_error_reporting_same_line_as_php_tag() {
    let input = "<?php error_reporting(0);\n$x = 1;\n";
    let (cleaned, count) = strip_error_reporting(input);
    assert_eq!(count, 1);
    assert_eq!(cleaned, "<?php\n$x = 1;\n");
}

#[test]
fn test_strip_error_reporting_preserves_methods_and_strings() {
    let input = concat!(
        "<?php\n",
        "// error_reporting(E_ALL);\n",
        "/* error_reporting(0); */\n",
        "$str = \"error_reporting(E_ALL)\";\n",
        "$obj->error_reporting(1);\n",
        "$obj?->error_reporting(2);\n",
        "Config::error_reporting(3);\n",
        "function error_reporting() {\n",
        "    return 0;\n",
        "}\n"
    );
    let (cleaned, count) = strip_error_reporting(input);
    assert_eq!(count, 0);
    assert_eq!(cleaned, input);
}

#[test]
fn test_migrate_error_reporting_in_full_pipeline() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "error_reporting(E_ALL);\n",
        "list($a, $b) = @getCoords();\n",
        "$id = (int)$_GET['id'];\n",
        "var_dump($a, $b);\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.declare_count, 1);
    assert_eq!(res.error_reporting_count, 1);
    assert_eq!(res.list_destructure_count, 1);
    assert_eq!(res.error_suppress_count, 1);
    assert_eq!(res.cast_count, 1);
    assert_eq!(res.get_replace_count, 1);
    assert_eq!(res.var_dump_count, 1);

    let expected = concat!(
        "// error_reporting(E_ALL);\n",
        "[$a, $b] = getCoords();\n",
        "$id = int(Request::query('id'));\n",
        "dd($a, $b);\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_cli_migrate_write_error_reporting() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_err_rep_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_path = temp_dir.join("init.php");
    let content = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "error_reporting(E_ALL & ~E_DEPRECATED);\n",
        "@ini_set('display_errors', '0');\n",
        "$id = (int)$_GET['id'];\n",
        "var_dump($id);\n"
    );
    fs::write(&file_path, content).unwrap();

    // Dry-run
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("1 error_reporting(...) call(s) commented out") || check_stdout.contains("1 error_reporting(...) call(s) removed"));
    assert!(check_stdout.contains("1 declare() directive(s) removed"));
    assert!(check_stdout.contains("1 ini_set(...) call(s) commented out"));
    assert!(check_stdout.contains("1 $_GET[...] access(es) replaced with Request::query(...)"));
    assert!(check_stdout.contains("1 type cast(s) converted to functional conversion(s)"));
    assert!(check_stdout.contains("1 var_dump / var_export call(s) replaced with dd()"));

    // Write mode
    let migrate_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");

    assert!(migrate_output.status.success());
    let migrated_content = fs::read_to_string(&file_path).unwrap();
    assert!(!migrated_content.contains("declare("));
    assert!(migrated_content.contains("// error_reporting(") || !migrated_content.contains("error_reporting("));
    assert!(migrated_content.contains("// ini_set('display_errors', '0');"));
    assert!(migrated_content.contains("int(Request::query('id'))"));
    assert!(migrated_content.contains("dd($id);"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_replace_get_array_access_various_forms() {
    let input = concat!(
        "<?php\n",
        "$id = $_GET['id'];\n",
        "$name = $_GET[\"name\"];\n",
        "$key = $_GET[$varKey];\n",
        "$zero = $_GET[0];\n",
        "$nested = $_GET['filter']['status'];\n",
        "$spaces = $_GET[ 'sort' ];\n"
    );
    let (res, count) = replace_get_array_access(input);
    assert_eq!(count, 6);
    let expected = concat!(
        "<?php\n",
        "$id = Request::query('id');\n",
        "$name = Request::query(\"name\");\n",
        "$key = Request::query($varKey);\n",
        "$zero = Request::query(0);\n",
        "$nested = Request::query('filter')['status'];\n",
        "$spaces = Request::query('sort');\n"
    );
    assert_eq!(res, expected);
}

#[test]
fn test_replace_get_array_access_preserves_strings_and_comments() {
    let input = concat!(
        "<?php\n",
        "// $_GET['commented']\n",
        "/* $_GET['block'] */\n",
        "$msg = \"Hello $_GET['str']\";\n",
        "$msg2 = '$_GET[\"test\"]';\n",
        "$all = $_GET;\n"
    );
    let (res, count) = replace_get_array_access(input);
    assert_eq!(count, 0);
    assert_eq!(res, input);
}

#[test]
fn test_comment_ini_set_basic_and_at_prefix() {
    let input = concat!(
        "<?php\n",
        "ini_set('display_errors', '0');\n",
        "@ini_set('memory_limit', '512M');\n",
        "\\ini_set('max_execution_time', 300);\n",
        "@\\ini_set('log_errors', 1);\n"
    );
    let (res, count) = comment_ini_set_calls(input);
    assert_eq!(count, 4);
    let expected = concat!(
        "<?php\n",
        "// ini_set('display_errors', '0');\n",
        "// ini_set('memory_limit', '512M');\n",
        "// ini_set('max_execution_time', 300);\n",
        "// ini_set('log_errors', 1);\n"
    );
    assert_eq!(res, expected);
}

#[test]
fn test_comment_ini_set_indentation_and_comments() {
    let input = concat!(
        "<?php\n",
        "    ini_set('display_errors', '1'); // debug mode\n",
        "    @ini_set('memory_limit', '1G'); # large jobs\n"
    );
    let (res, count) = comment_ini_set_calls(input);
    assert_eq!(count, 2);
    let expected = concat!(
        "<?php\n",
        "    // ini_set('display_errors', '1'); // debug mode\n",
        "    // ini_set('memory_limit', '1G'); # large jobs\n"
    );
    assert_eq!(res, expected);
}

#[test]
fn test_comment_ini_set_same_line_as_tag() {
    let input = "<?php ini_set('display_errors', '0');\n$x = 1;\n";
    let (res, count) = comment_ini_set_calls(input);
    assert_eq!(count, 1);
    assert_eq!(res, "<?php\n// ini_set('display_errors', '0');\n$x = 1;\n");
}

#[test]
fn test_comment_ini_set_multiline() {
    let input = concat!(
        "<?php\n",
        "ini_set(\n",
        "    'error_reporting',\n",
        "    'E_ALL'\n",
        ");\n"
    );
    let (res, count) = comment_ini_set_calls(input);
    assert_eq!(count, 1);
    let expected = concat!(
        "<?php\n",
        "// ini_set(\n",
        "//     'error_reporting',\n",
        "//     'E_ALL'\n",
        "// );\n"
    );
    assert_eq!(res, expected);
}

#[test]
fn test_comment_ini_set_preserves_methods_and_strings() {
    let input = concat!(
        "<?php\n",
        "// ini_set('foo', 'bar');\n",
        "/* @ini_set('baz', 'qux'); */\n",
        "$str = \"ini_set('a', 'b')\";\n",
        "$obj->ini_set('c', 'd');\n",
        "$obj?->ini_set('e', 'f');\n",
        "Config::ini_set('g', 'h');\n",
        "function ini_set($k, $v) {\n",
        "    return false;\n",
        "}\n"
    );
    let (res, count) = comment_ini_set_calls(input);
    assert_eq!(count, 0);
    assert_eq!(res, input);
}

#[test]
fn test_replace_post_array_access_various_keys() {
    let input = concat!(
        "<?php\n",
        "$id = $_POST['id'];\n",
        "$name = $_POST[\"name\"];\n",
        "$key = $_POST[$varKey];\n",
        "$zero = $_POST[0];\n",
        "$nested = $_POST['filter']['status'];\n",
        "$spaces = $_POST [ 'sort' ] ;\n"
    );
    let (res, count) = replace_post_array_access(input);
    assert_eq!(count, 6);
    let expected = concat!(
        "<?php\n",
        "$id = Request::body('id');\n",
        "$name = Request::body(\"name\");\n",
        "$key = Request::body($varKey);\n",
        "$zero = Request::body(0);\n",
        "$nested = Request::body('filter')['status'];\n",
        "$spaces = Request::body('sort') ;\n"
    );
    assert_eq!(res, expected);
}

#[test]
fn test_replace_post_array_access_preserves_plain_post_and_strings() {
    let input = concat!(
        "<?php\n",
        "$all = $_POST;\n",
        "foreach ($_POST as $k => $v) {}\n",
        "$s1 = \"$_POST['id']\";\n",
        "$s2 = '$_POST[\"name\"]';\n",
        "// $_POST['commented']\n",
        "/* $_POST['block'] */\n"
    );
    let (res, count) = replace_post_array_access(input);
    assert_eq!(count, 0);
    assert_eq!(res, input);
}

#[test]
fn test_request_query_and_body_return_string_type() {
    let script = r#"
        $q = Request::query('non_existent');
        $b = Request::body('non_existent');
        return [
            'q' => $q,
            'b' => $b,
            'q_type' => gettype($q),
            'b_type' => gettype($b),
        ];
    "#;
    let res = rhp::run_source(script).expect("Execution failed");
    if let rhp::vm::PhpValue::Dict(dict) = res {
        let map = dict.borrow();
        assert_eq!(map.get("q"), Some(&rhp::vm::PhpValue::string("")));
        assert_eq!(map.get("b"), Some(&rhp::vm::PhpValue::string("")));
        assert_eq!(map.get("q_type"), Some(&rhp::vm::PhpValue::string("string")));
        assert_eq!(map.get("b_type"), Some(&rhp::vm::PhpValue::string("string")));
    } else {
        panic!("Expected Dict result, got: {:?}", res);
    }
}

#[test]
fn test_migrate_binary_strings_updates_function_arguments() {
    let input = concat!(
        "<?php\n",
        "function handleRawPayload($payload, string $name) {\n",
        "    return Str::byteLength($payload);\n",
        "}\n",
        "class ImageProcessor {\n",
        "    public function process($header, $tag) {\n",
        "        return true;\n",
        "    }\n",
        "}\n",
        "$data = \"\\x89PNG\\r\\n\\x1a\\n\";\n",
        "handleRawPayload($data, \"avatar.png\");\n",
        "$proc = new ImageProcessor();\n",
        "$proc->process(\"\\xFF\\xD8\\xFF\\xE0\", \"jpeg\");\n"
    );

    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.byte_param_count >= 2, "Expected at least 2 byte params updated, got: {}", res.byte_param_count);
    assert!(
        res.migrated_content.contains("function handleRawPayload($payload: byte, $name: string)"),
        "Expected byte $payload in handleRawPayload, got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("public function process($header: byte, $tag)"),
        "Expected byte $header in process, got:\n{}",
        res.migrated_content
    );
}

#[test]
fn test_migrate_named_arguments_reordered_to_proper_declaration_order() {
    let input = concat!(
        "<?php\n",
        "function add(int $a, int $b): int {\n",
        "    return $a + $b;\n",
        "}\n",
        "$val = add(b: 20, a: 5);\n",
        "return $val;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 1);
    assert!(res.migrated_content.contains("$val = add('b' => 20, 'a' => 5);"), "Got:\n{}", res.migrated_content);
    assert!(!res.migrated_content.contains("b: 20"));
    assert!(!res.migrated_content.contains("a: 5"));

    // Verify it executes cleanly in RHP VM
    let vm_res = rhp::run_source(&res.migrated_content).expect("Migrated code must execute in RHP");
    assert_eq!(vm_res, rhp::vm::PhpValue::Int(25));
}

#[test]
fn test_migrate_named_arguments_with_skipped_optional_arguments() {
    let input = concat!(
        "<?php\n",
        "function configureApp(string $name, string $env = 'production', string $port = '8080', bool $debug = false, string $logLevel = 'info'): string {\n",
        "    return $name + ':' + $env + ':' + $port + ':' + ($debug ? '1' : '0') + ':' + $logLevel;\n",
        "}\n",
        "$cfg1 = configureApp(name: 'myApp', debug: true);\n",
        "$cfg2 = configureApp(logLevel: 'debug', name: 'apiServer', env: 'staging');\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 2);
    assert!(
        res.migrated_content.contains("$cfg1 = configureApp('name' => 'myApp', 'debug' => true);"),
        "Expected arrow names, got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("$cfg2 = configureApp('logLevel' => 'debug', 'name' => 'apiServer', 'env' => 'staging');"),
        "Expected arrow names in source order, got:\n{}",
        res.migrated_content
    );

    // Verify it executes cleanly in RHP VM
    let (vm, _) = rhp::run_source_with_vm(&res.migrated_content).expect("Must run");
    assert_eq!(
        vm.get_main_variable("$cfg1"),
        Some(&rhp::vm::PhpValue::string("myApp:production:8080:1:info"))
    );
    assert_eq!(
        vm.get_main_variable("$cfg2"),
        Some(&rhp::vm::PhpValue::string("apiServer:staging:8080:0:debug"))
    );
}

#[test]
fn test_migrate_named_arguments_leading_skipped_parameters() {
    let input = concat!(
        "<?php\n",
        "function coords(string $x = '0', string $y = '0', string $z = '0'): string {\n",
        "    return $x + ',' + $y + ',' + $z;\n",
        "}\n",
        "$c1 = coords(y: '50');\n",
        "$c2 = coords(z: '100');\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 2);
    assert!(
        res.migrated_content.contains("$c1 = coords('y' => '50');"),
        "Expected coords('y' => '50'), got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("$c2 = coords('z' => '100');"),
        "Expected coords('z' => '100'), got:\n{}",
        res.migrated_content
    );

    let (vm, _) = rhp::run_source_with_vm(&res.migrated_content).expect("Must run");
    assert_eq!(vm.get_main_variable("$c1"), Some(&rhp::vm::PhpValue::string("0,50,0")));
    assert_eq!(vm.get_main_variable("$c2"), Some(&rhp::vm::PhpValue::string("0,0,100")));
}

#[test]
fn test_migrate_named_arguments_class_methods_and_constructors() {
    let input = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function __construct(public string $host, public string $port = '3306', public string $user = 'root', public string $database = 'main') {\n",
        "    }\n",
        "    public function query(string $sql, string $timeout = '30', bool $fetchAssoc = true): string {\n",
        "        return $this->host + ':' + $this->port + '|' + $sql + '|' + $timeout + '|' + ($fetchAssoc ? 'assoc' : 'list');\n",
        "    }\n",
        "}\n",
        "$svc = new Service(database: 'ecommerce', host: 'db.local');\n",
        "$res = $svc->query(sql: 'SELECT 1', fetchAssoc: false);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 2);
    assert!(
        res.migrated_content.contains("$svc = new Service('database' => 'ecommerce', 'host' => 'db.local');"),
        "Constructor call should keep named arguments: got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("$res = $svc->query('sql' => 'SELECT 1', 'fetchAssoc' => false);"),
        "Method call should keep named arguments: got:\n{}",
        res.migrated_content
    );

    let (vm, _) = rhp::run_source_with_vm(&res.migrated_content).expect("Must run");
    assert_eq!(
        vm.get_main_variable("$res"),
        Some(&rhp::vm::PhpValue::string("db.local:3306|SELECT 1|30|list"))
    );
}

#[test]
fn test_migrate_named_arguments_static_calls() {
    let input = concat!(
        "<?php\n",
        "class MathHelper {\n",
        "    public static function clamp(int $value, int $min, int $max): int {\n",
        "        if ($value < $min) { return $min; }\n",
        "        if ($value > $max) { return $max; }\n",
        "        return $value;\n",
        "    }\n",
        "}\n",
        "$clamped = MathHelper::clamp(max: 100, min: 0, value: 50);\n",
        "return $clamped;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 1);
    assert!(
        res.migrated_content.contains("$clamped = MathHelper::clamp('max' => 100, 'min' => 0, 'value' => 50);"),
        "Got:\n{}",
        res.migrated_content
    );

    let vm_res = rhp::run_source(&res.migrated_content).expect("Must run");
    assert_eq!(vm_res, rhp::vm::PhpValue::Int(50));
}

#[test]
fn test_migrate_named_arguments_builtins() {
    let input = concat!(
        "<?php\n",
        "$items = [10, 20, 30, 40, 50];\n",
        "$slice = array_slice($items, length: 2, offset: 1);\n",
        "$html = htmlspecialchars('hello & world', double_encode: false);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 2);
    assert!(
        res.migrated_content.contains("$slice = array_slice($items, 'length' => 2, 'offset' => 1);"),
        "array_slice should keep named arguments in source order: got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("$html = htmlspecialchars('hello & world', 'double_encode' => false);"),
        "htmlspecialchars should rewrite the named flag: got:\n{}",
        res.migrated_content
    );
}

#[test]
fn test_migrate_named_arguments_unknown_function_preserves_order_and_removes_names() {
    let input = concat!(
        "<?php\n",
        "$res = customExternalFunc(foo: 1, bar: 2, baz: 3);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 1);
    assert_eq!(
        res.migrated_content,
        "$res = customExternalFunc('foo' => 1, 'bar' => 2, 'baz' => 3);\n"
    );
}

#[test]
fn test_migrate_named_arguments_pass_by_reference() {
    let input = concat!(
        "<?php\n",
        "function applyBonus(int &$score, int $bonus): void {\n",
        "    $score = $score + $bonus;\n",
        "}\n",
        "$playerScore = 100;\n",
        "applyBonus(bonus: 50, score: &$playerScore);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 1);
    assert!(
        res.migrated_content.contains("applyBonus('bonus' => 50, 'score' => &$playerScore);"),
        "Pass-by-reference must be preserved: got:\n{}",
        res.migrated_content
    );

    let (vm, _) = rhp::run_source_with_vm(&res.migrated_content).expect("Must run");
    assert_eq!(vm.get_main_variable("$playerScore"), Some(&rhp::vm::PhpValue::Int(150)));
}

#[test]
fn test_migrate_named_arguments_nested_calls() {
    let input = concat!(
        "<?php\n",
        "function add(int $a, int $b): int { return $a + $b; }\n",
        "function mul(int $x, int $y): int { return $x * $y; }\n",
        "$val = add(b: mul(y: 4, x: 5), a: 10);\n",
        "return $val;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.named_arg_count, 2);
    assert!(
        res.migrated_content.contains("$val = add('b' => mul('y' => 4, 'x' => 5), 'a' => 10);"),
        "Nested calls must both be reordered: got:\n{}",
        res.migrated_content
    );

    let vm_res = rhp::run_source(&res.migrated_content).expect("Must run");
    assert_eq!(vm_res, rhp::vm::PhpValue::Int(30));
}

#[test]
fn test_migrate_named_arguments_preserves_attributes() {
    let input = concat!(
        "<?php\n",
        "#[Route(path: '/api/v1', methods: ['GET'])]\n",
        "class ApiController {\n",
        "    public function handle($req) {\n",
        "        return true;\n",
        "    }\n",
        "}\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(
        res.migrated_content.contains("#[Route(path: '/api/v1', methods: ['GET'])]"),
        "Attribute annotations must preserve their named arguments: got:\n{}",
        res.migrated_content
    );
}

#[test]
fn test_migrate_fixes_function_parameter_order_and_named_arguments() {
    let input = concat!(
        "<?php\n",
        "function badSignature(int $opt = 10, int $req): int {\n",
        "    return $req - $opt;\n",
        "}\n",
        "$ans = badSignature(opt: 10, req: 50);\n",
        "return $ans;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.named_arg_count >= 1);
    assert!(res.param_order_count >= 1);
    assert!(
        res.migrated_content.contains("function badSignature($req: int, $opt: int = 10): int"),
        "Function definition parameters must have required before optional: got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("$ans = badSignature('opt' => 10, 'req' => 50);"),
        "Call site must keep named arguments: got:\n{}",
        res.migrated_content
    );

    let vm_res = rhp::run_source(&res.migrated_content).expect("Must run");
    assert_eq!(vm_res, rhp::vm::PhpValue::Int(40));
}

#[test]
fn test_migrate_byte_parameters_automatically_added() {
    let input = concat!(
        "<?php\n",
        "function parsePacket(binary $header, $payload = b\"\\x00\\x01\\x02\") {\n",
        "    return true;\n",
        "}\n",
        "$fn = function($raw = \"\\xFF\\xFE\") {\n",
        "    return true;\n",
        "};\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.byte_param_count >= 3, "Expected at least 3 byte parameters updated, got: {}", res.byte_param_count);
    assert!(
        res.migrated_content.contains("function parsePacket($header: byte, $payload: byte = b\"\\x00\\x01\\x02\")"),
        "Expected byte $header and byte $payload in parsePacket, got:\n{}",
        res.migrated_content
    );
    assert!(
        res.migrated_content.contains("function($raw: byte = \"\\xFF\\xFE\")"),
        "Expected byte $raw in closure, got:\n{}",
        res.migrated_content
    );
}

#[test]
fn test_migrate_php8_mode_only_swaps_named_arguments_and_preserves_declarations() {
    let input = concat!(
        "<?php\n",
        "function myfun(?$x, $y = '', $z) {\n",
        "    return $z;\n",
        "}\n",
        "$res = myfun(z: 'hello', x: 42, y: 'world');\n",
        "$normal = myfun(1, 'two', 3);\n"
    );
    let options = MigrateOptions {
        fix: true,
        php_version: 8,
        ..Default::default()
    };
    let res = migrate_source_with_options(input, "test.php", options);

    assert!(res.modified);
    // In PHP 8 mode: definition is untouched, no //before: comment added
    assert!(
        res.migrated_content.contains("function myfun($x: mixed?, $y = '', $z)"),
        "Function definition must be preserved in PHP 8 mode: got:\n{}",
        res.migrated_content
    );
    assert!(
        !res.migrated_content.contains("//before:"),
        "No //before: comment should be added in PHP 8 mode: got:\n{}",
        res.migrated_content
    );
    // Named arguments swapped to declaration order (x, y, z) and names removed
    assert!(
        res.migrated_content.contains("$res = myfun('z' => 'hello', 'x' => 42, 'y' => 'world');"),
        "Named arguments must be rewritten to arrow form in source order: got:\n{}",
        res.migrated_content
    );
    // Normal positional calls untouched
    assert!(
        res.migrated_content.contains("$normal = myfun(1, 'two', 3);"),
        "Normal positional call must be untouched in PHP 8 mode: got:\n{}",
        res.migrated_content
    );
    // No warnings emitted
    assert!(res.warnings.is_empty(), "No warnings should be emitted in PHP 8 mode: got: {:?}", res.warnings);
}

#[test]
fn test_migrate_rewrites_prefix_nullable_everywhere_except_ternary_and_strings() {
    let input = concat!(
        "function find(?int $id, ?int &$score, ?\\App\\User $user): ?User {\n",
        "    $flag ? int($id) : 0;\n",
        "    $url = \"https://example.test/posts?user=1\";\n",
        "    return null;\n",
        "}\n",
        "class Box { public ?string $name = null; }\n",
        "function bare(?$x) { return $x; }\n",
    );
    let res = migrate_source(input, "nullable.php");
    let migrated = &res.migrated_content;
    assert!(migrated.contains("function find($id: int?, &$score: int?, $user: \\App\\User?): User?"));
    assert!(migrated.contains("$flag ? int($id) : 0;"));
    assert!(migrated.contains("posts?user=1"));
    assert!(migrated.contains("public $name: string? = null;"));
    assert!(migrated.contains("function bare($x: mixed?)"));
}

#[test]
fn test_migrate_php7_mode_reorders_definitions_adds_before_comment_and_swaps_positional_calls() {
    let input = concat!(
        "<?php\n",
        "function myfun(?$x, $y = '', $z) {\n",
        "    return [$z, $x, $y];\n",
        "}\n",
        "$call1 = myfun(1, 'foo', 2);\n",
        "$call2 = myfun(10, , 20);\n"
    );
    let options = MigrateOptions {
        fix: true,
        php_version: 7,
        ..Default::default()
    };
    let res = migrate_source_with_options(input, "test.php", options);

    assert!(res.modified);
    // Definition must be reordered with required parameters ($z) first
    assert!(
        res.migrated_content.contains("//before: function myfun($x: mixed?, $y = '', $z)\nfunction myfun($z, $x: mixed?, $y = '')"),
        "Old signature comment and reordered definition must be present: got:\n{}",
        res.migrated_content
    );
    // Normal positional call myfun(1, 'foo', 2) where original was (x, y, z) must become (z, x, y) -> (2, 1, 'foo')
    assert!(
        res.migrated_content.contains("$call1 = myfun(2, 1, 'foo');"),
        "Positional call site must be reordered: got:\n{}",
        res.migrated_content
    );
}

#[test]
fn test_migrate_php7_mode_warns_on_tricky_call_sites() {
    let input = concat!(
        "<?php\n",
        "function myfun(?$x, $y = '', $z) {\n",
        "    return $z;\n",
        "}\n",
        "$args = [1, 'val', 2];\n",
        "$call1 = myfun(...$args);\n",
        "$call2 = call_user_func('myfun', 1, 2, 3);\n"
    );
    let options = MigrateOptions {
        fix: true,
        php_version: 7,
        ..Default::default()
    };
    let res = migrate_source_with_options(input, "test.php", options);

    assert!(res.modified);
    // Tricky call with ...$args is not reordered
    assert!(
        res.migrated_content.contains("myfun(...$args)"),
        "Unpacked call site should not be corrupted: got:\n{}",
        res.migrated_content
    );
    // Warnings must be emitted for tricky calls and parameter order
    assert!(!res.warnings.is_empty(), "Warnings must be emitted for tricky call sites");
    let warnings_str = res.warnings.join("\n");
    assert!(
        warnings_str.contains("Tricky call site using argument unpacking (...) cannot be automatically reordered at line 6"),
        "Expected unpacking warning, got:\n{}",
        warnings_str
    );
    assert!(
        warnings_str.contains("Tricky call site using callback (call_user_func) cannot be automatically reordered at line 7"),
        "Expected callback dispatcher warning, got:\n{}",
        warnings_str
    );
    assert!(
        warnings_str.contains("Parameter order changed for 'function myfun($x: mixed?, $y = '', $z)'"),
        "Expected general notice for myfun, got:\n{}",
        warnings_str
    );
}

#[test]
fn test_migrate_removes_explicit_void_return_types() {
    let input = concat!(
        "function ping(): void {\n",
        "}\n",
        "function titled(): Void {\n",
        "}\n",
        "function keep(): int {\n",
        "    return 1;\n",
        "}\n",
        "function packed():void {\n",
        "}\n",
        "$f = function(): void {};\n",
        "$g = fn(): Void => 1;\n",
    );
    let res = migrate_source(input, "void.rhp");
    assert_eq!(res.void_return_count, 5, "content:\n{}", res.migrated_content);
    assert!(res.migrated_content.contains("function ping() {"), "{}", res.migrated_content);
    assert!(res.migrated_content.contains("function titled() {"), "{}", res.migrated_content);
    assert!(res.migrated_content.contains("function packed() {"), "{}", res.migrated_content);
    assert!(res.migrated_content.contains("function keep(): int"), "{}", res.migrated_content);
    assert!(res.migrated_content.contains("function() {}"), "{}", res.migrated_content);
    assert!(res.migrated_content.contains("fn() => 1"), "{}", res.migrated_content);
    assert!(!res.migrated_content.to_ascii_lowercase().contains(": void"), "{}", res.migrated_content);
}

#[test]
fn test_migrate_strip_legacy_includes() {
    let input = concat!(
        "<?php\n",
        "require_once __DIR__ . '/vendor/autoload.php';\n",
        "require_once 'Models/User.php';\n",
        "include_once 'config.php';\n",
        "require 'bootstrap.php';\n",
        "$msg = 'keep require_once inside string';\n",
        "// require_once inside comment\n",
        "$user = new User();\n"
    );
    let res = migrate_source(input, "app.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 4);
    assert!(!res.migrated_content.contains("require_once __DIR__"));
    assert!(!res.migrated_content.contains("require_once 'Models/User.php'"));
    assert!(!res.migrated_content.contains("include_once 'config.php'"));
    assert!(!res.migrated_content.contains("require 'bootstrap.php'"));
    assert!(res.migrated_content.contains("$msg = 'keep require_once inside string';"));
    assert!(res.migrated_content.contains("// require_once inside comment"));
    assert!(res.migrated_content.contains("$user = new User();"));
}

#[test]
fn test_migrate_simple_output_buffering_echo() {
    let input = concat!(
        "<?php\n",
        "ob_start();\n",
        "echo \"Hello, \";\n",
        "echo $name;\n",
        "$greeting = ob_get_clean();\n"
    );
    let res = migrate_source(input, "greet.php");
    assert!(res.modified);
    assert_eq!(res.ob_rewritten_count, 1);
    assert!(!res.migrated_content.contains("ob_start()"));
    assert!(!res.migrated_content.contains("ob_get_clean()"));
    assert!(res.migrated_content.contains("$greeting = \"Hello, \" + $name;"));
}

#[test]
fn test_migrate_simple_output_buffering_include_view() {
    let input = concat!(
        "<?php\n",
        "ob_start();\n",
        "include __DIR__ . '/partials/user_card.php';\n",
        "$html = ob_get_clean();\n"
    );
    let res = migrate_source(input, "view.php");
    assert!(res.modified);
    assert_eq!(res.ob_rewritten_count, 1);
    assert!(!res.migrated_content.contains("ob_start()"));
    assert!(!res.migrated_content.contains("ob_get_clean()"));
    assert!(res.migrated_content.contains("$html = View::render('partials.user_card');"));
}

#[test]
fn test_migrate_simple_output_buffering_discard() {
    let input = concat!(
        "<?php\n",
        "ob_start();\n",
        "ob_end_clean();\n",
        "$x = 10;\n"
    );
    let res = migrate_source(input, "discard.php");
    assert!(res.modified);
    assert_eq!(res.ob_rewritten_count, 1);
    assert!(!res.migrated_content.contains("ob_start()"));
    assert!(!res.migrated_content.contains("ob_end_clean()"));
    assert!(res.migrated_content.contains("$x = 10;"));
}

#[test]
fn test_migrate_controller_echo() {
    let input = concat!(
        "<?php\n",
        "echo View::render('home.index');\n",
        "echo json_encode($data);\n",
        "echo Response::html($content);\n"
    );
    let res = migrate_source(input, "controller.php");
    assert!(res.modified);
    assert_eq!(res.echo_response_count, 3);
    assert!(res.migrated_content.contains("return Response::html(View::render('home.index'));"));
    assert!(res.migrated_content.contains("return Response::json($data);"));
    assert!(res.migrated_content.contains("return Response::html($content);"));
}

#[test]
fn test_transpile_php_template_to_rade() {
    let input = concat!(
        "<div class=\"user-card\">\n",
        "    <h2><?= htmlspecialchars($user['name'], ENT_QUOTES, 'UTF-8') ?></h2>\n",
        "    <span>ID: <?= $user['id'] ?></span>\n",
        "    <?php if ($user['is_admin']): ?>\n",
        "        <span class=\"badge\">Admin</span>\n",
        "    <?php elseif ($user['is_mod']): ?>\n",
        "        <span class=\"badge\">Mod</span>\n",
        "    <?php else: ?>\n",
        "        <span class=\"badge\">User</span>\n",
        "    <?php endif; ?>\n",
        "    <ul>\n",
        "    <?php foreach ($items as $item): ?>\n",
        "        <li><?= $item ?></li>\n",
        "    <?php endforeach; ?>\n",
        "    </ul>\n",
        "    <?php include 'partials/footer.php'; ?>\n",
        "</div>\n"
    );
    let res = migrate_source(input, "card.blade.php");
    assert!(res.modified);
    assert!(res.rade_directive_count >= 8);
    assert!(res.migrated_content.contains("<h2>{{ $user['name'] }}</h2>"));
    assert!(res.migrated_content.contains("<span>ID: {{ $user['id'] }}</span>"));
    assert!(res.migrated_content.contains("@if ($user['is_admin'])"));
    assert!(res.migrated_content.contains("@elseif ($user['is_mod'])"));
    assert!(res.migrated_content.contains("@else"));
    assert!(res.migrated_content.contains("@endif"));
    assert!(res.migrated_content.contains("@foreach ($items as $item)"));
    assert!(res.migrated_content.contains("@endforeach"));
    assert!(res.migrated_content.contains("@include('partials.footer')"), "Output was:\n{}", res.migrated_content);
}

#[test]
fn test_migrate_json_encode_flags_to_options_dict() {
    let input = concat!(
        "<?php\n",
        "$pretty = json_encode($v, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR, 32);\n",
        "$num = json_encode($v, 128);\n",
        "$html = json_encode($v, JSON_HEX_TAG | JSON_HEX_AMP);\n",
        "$dyn = json_encode($v, $flags);\n",
        "$named = json_encode(value: $v, flags: JSON_PRETTY_PRINT);\n",
        "$data = json_decode($s, true);\n",
        "$ok = json_validate($raw);\n",
        "$err = json_last_error();\n",
    );
    let res = migrate_source(input, "json.php");
    assert!(res.migrated_content.contains(
        "Json::encode($v, [\"pretty\" => true, \"depth\" => 32])"
    ));
    assert!(
        res.migrated_content
            .matches("Json::encode($v, [\"pretty\" => true])")
            .count()
            >= 2
    );
    assert!(res.migrated_content.contains("Json::encode($v, [\"html\" => true])"));
    assert!(res.migrated_content.contains("$dyn = Json::encode($v);"));
    assert!(res.migrated_content.contains("Json::decode($s)"));
    assert!(res.migrated_content.contains("Json::validate($raw)"));
    assert!(res.migrated_content.contains("json_last_error()"));
    assert!(res.warnings.iter().any(|w| w.contains("$flags")));
    assert!(res.warnings.iter().any(|w| w.contains("json_last_error")));
}

#[test]
fn test_migrate_echo_json_encode_drops_flags() {
    let input = concat!(
        "<?php\n",
        "echo json_encode($data, JSON_PRETTY_PRINT);\n",
    );
    let res = migrate_source(input, "controller.php");
    assert!(res.migrated_content.contains("return Response::json($data);"));
    assert!(!res.migrated_content.contains("JSON_PRETTY_PRINT"));
    assert!(res.warnings.iter().any(|w| w.contains("echo json_encode flags")));
}

#[test]
fn test_cli_migrate_single_file_flag_variants() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_migrate_file_flag_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let file_a = temp_dir.join("file_a.php");
    let file_b = temp_dir.join("file_b.php");

    let legacy_code_a = concat!(
        "<?php\n",
        "error_reporting(E_ALL);\n",
        "ini_set('display_errors', 1);\n",
        "$id = (int)$_GET['id'];\n"
    );
    let legacy_code_b = concat!(
        "<?php\n",
        "error_reporting(E_ALL);\n",
        "ini_set('display_errors', 1);\n",
        "$name = (string)$_GET['name'];\n"
    );

    fs::write(&file_a, legacy_code_a).unwrap();
    fs::write(&file_b, legacy_code_b).unwrap();

    // 1. Dry run on file_a with --file="path"
    let dry_run = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(format!("--file={}", file_a.display()))
        .arg("--check")
        .output()
        .expect("failed to run rhp migrate --file=... --check");

    assert!(dry_run.status.success());
    let dry_stdout = String::from_utf8_lossy(&dry_run.stdout);
    assert!(dry_stdout.contains("Files Scanned:  1"));
    assert!(dry_stdout.contains("1 error_reporting(...) call(s) commented out") || dry_stdout.contains("1 error_reporting(...) call(s) removed"));

    // File A should not have changed yet because of --check
    assert!(fs::read_to_string(&file_a).unwrap().contains("error_reporting(E_ALL);"));

    // 2. Migrate only file_a using --file="path"
    let run_a = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(format!("--file=\"{}\"", file_a.display()))
        .output()
        .expect("failed to run rhp migrate --file=...");

    assert!(run_a.status.success());
    let content_a = fs::read_to_string(&file_a).unwrap();
    assert!(content_a.contains("// error_reporting(") || !content_a.contains("error_reporting("));
    assert!(content_a.contains("// ini_set('display_errors', 1);"));
    assert!(content_a.contains("int(Request::query('id'))"));

    // File B must remain completely untouched!
    let content_b = fs::read_to_string(&file_b).unwrap();
    assert_eq!(content_b, legacy_code_b);

    // 3. Migrate file_b using shorthand -f
    let run_b = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg("-f")
        .arg(&file_b)
        .output()
        .expect("failed to run rhp migrate -f");

    assert!(run_b.status.success());
    let migrated_b = fs::read_to_string(&file_b).unwrap();
    assert!(migrated_b.contains("// error_reporting(") || !migrated_b.contains("error_reporting("));
    assert!(migrated_b.contains("string(Request::query('name'))"));

    // 4. Passing a directory to --file should fail with helpful error
    let fail_dir = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(format!("--file={}", temp_dir.display()))
        .output()
        .expect("failed to run rhp migrate with dir for --file");

    assert!(!fail_dir.status.success());
    let err_str = String::from_utf8_lossy(&fail_dir.stderr);
    assert!(err_str.contains("is not a file"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_require_to_use_simple() {
    let input = concat!(
        "<?php\n",
        "require_once __DIR__ . '/../Models/User.php';\n\n",
        "class PaymentService {\n",
        "    public function pay(User $user) {}\n",
        "}\n"
    );
    let res = migrate_source(input, "app/Services/PaymentService.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 1);
    assert!(!res.migrated_content.contains("require_once"));
    assert!(res.migrated_content.contains("namespace App\\Services;\n\nuse App\\Models\\User;\n\nclass PaymentService"));
}

#[test]
fn test_migrate_require_to_use_with_alias_on_collision() {
    let input = concat!(
        "<?php\n",
        "require_once __DIR__ . '/../Models/User.php';\n\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n"
    );
    let res = migrate_source(input, "app/Services/OrderService.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 1);
    assert!(!res.migrated_content.contains("require_once"));
    assert!(res.migrated_content.contains("namespace App\\Services;\n\nuse App\\Models\\User as UserModel;\n\nclass User"));
}

#[test]
fn test_migrate_require_with_existing_namespace_and_use() {
    let input = concat!(
        "<?php\n",
        "namespace App\\Services;\n\n",
        "use Str;\n\n",
        "require_once 'Models/User.php';\n\n",
        "class AuthService {}\n"
    );
    let res = migrate_source(input, "app/Services/AuthService.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 1);
    assert!(!res.migrated_content.contains("require_once"));
    assert!(res.migrated_content.contains("namespace App\\Services;\n\nuse App\\Models\\User;\nuse Str;\n\nclass AuthService"));
}

#[test]
fn test_migrate_require_same_namespace_omits_use() {
    let input = concat!(
        "<?php\n",
        "namespace App\\Models;\n\n",
        "require_once 'Models/User.php';\n\n",
        "class Order {}\n"
    );
    let res = migrate_source(input, "app/Models/Order.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 1);
    assert!(!res.migrated_content.contains("require_once"));
    assert!(!res.migrated_content.contains("use App\\Models\\User;"));
    assert!(res.migrated_content.contains("namespace App\\Models;\n\nclass Order"));
}

#[test]
fn test_migrate_injects_namespace_for_classes_lacking_namespace() {
    let input = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n"
    );
    let res = migrate_source(input, "app/Models/User.php");
    assert!(res.modified);
    assert!(res.migrated_content.contains("namespace App\\Models;\n\nclass User"));
}

#[test]
fn test_migrate_psr4_resolution_with_rhp_toml() {
    let temp_dir = std::env::temp_dir().join(format!("rhp_test_psr4_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(temp_dir.join("src/Billing")).unwrap();
    fs::create_dir_all(temp_dir.join("src/Auth")).unwrap();

    let rhp_toml = r#"[package]
name = "acme-test"
version = "0.1.0"

[autoload]
psr-4 = { "Acme\\Core\\" = "src/" }
"#;
    fs::write(temp_dir.join("rhp.toml"), rhp_toml).unwrap();

    let auth_file = temp_dir.join("src/Auth/User.php");
    fs::write(&auth_file, "<?php\nclass User {}\n").unwrap();

    let billing_file = temp_dir.join("src/Billing/Invoice.php");
    fs::write(&billing_file, "<?php\nrequire_once __DIR__ . '/../Auth/User.php';\nclass Invoice {}\n").unwrap();

    let res = rhp::migrate::migrate_file(&billing_file, rhp::migrate::MigrateOptions::default()).unwrap();
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 1);
    let content = fs::read_to_string(&billing_file).unwrap();
    assert!(!content.contains("require_once"));
    assert!(content.contains("namespace Acme\\Core\\Billing;\n\nuse Acme\\Core\\Auth\\User;\n\nclass Invoice"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_comment_error_reporting_calls_standalone_and_methods() {
    let input = concat!(
        "<?php\n",
        "error_reporting(E_ALL);\n",
        "@error_reporting(0);\n",
        "\\error_reporting(E_ALL & ~E_NOTICE);\n",
        "$obj->error_reporting(1);\n",
        "Config::error_reporting(2);\n",
        "$msg = \"error_reporting(E_ALL)\";\n",
        "// error_reporting(0);\n",
    );
    let (res, count) = comment_error_reporting_calls(input);
    assert_eq!(count, 3);
    assert!(res.contains("// error_reporting(E_ALL);"));
    assert!(res.contains("// error_reporting(0);"));
    assert!(res.contains("// error_reporting(E_ALL & ~E_NOTICE);"));
    assert!(res.contains("$obj->error_reporting(1);"));
    assert!(res.contains("Config::error_reporting(2);"));
    assert!(res.contains("\"error_reporting(E_ALL)\""));
}

#[test]
fn test_project_wide_manifest_auto_initialization() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_init_manifest_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(temp_dir.join("src")).unwrap();

    let composer_json = r#"{
        "name": "acme/sample-app",
        "description": "Sample legacy application",
        "autoload": {
            "psr-4": {
                "Acme\\App\\": "src/"
            }
        }
    }"#;
    fs::write(temp_dir.join("composer.json"), composer_json).unwrap();
    fs::write(temp_dir.join("src/Index.php"), "<?php\nfunction hello(): void {}\n").unwrap();

    let report = migrate_path(&temp_dir, MigrateOptions::default()).unwrap();
    assert_eq!(report.files_scanned, 1);
    assert_eq!(report.files_modified, 1);

    let rhp_toml_path = temp_dir.join("rhp.toml");
    assert!(rhp_toml_path.exists(), "rhp.toml must be automatically initialized");
    let toml_content = fs::read_to_string(&rhp_toml_path).unwrap();
    assert!(toml_content.contains("name = \"sample-app\""));
    assert!(toml_content.contains("Acme\\\\App\\\\"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_wide_cross_file_signature_and_call_migration() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_staged_cross_file_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    // File A defines a function with optional before required
    let file_a = temp_dir.join("calc.php");
    fs::write(
        &file_a,
        concat!(
            "<?php\n",
            "function computeTotal(int $discount = 0, int $price): int {\n",
            "    return $price - $discount;\n",
            "}\n"
        ),
    )
    .unwrap();

    // File B calls that function, uses typecasts, and error_reporting
    let file_b = temp_dir.join("main.php");
    fs::write(
        &file_b,
        concat!(
            "<?php\n",
            "error_reporting(E_ALL);\n",
            "$raw = (int)$_GET['val'];\n",
            "$total = computeTotal(10, 100);\n"
        ),
    )
    .unwrap();

    let report = migrate_path(&temp_dir, MigrateOptions::default()).unwrap();
    assert_eq!(report.files_scanned, 2);
    assert_eq!(report.files_modified, 2);

    let content_a = fs::read_to_string(&file_a).unwrap();
    // In Stage 1: signature parameter types transformed and order normalized ($price before $discount)
    assert!(content_a.contains("function computeTotal($price: int, $discount: int = 0): int"));

    let content_b = fs::read_to_string(&file_b).unwrap();
    // In Stage 2: call site reordered using global signature registry ($price=100 first, $discount=10 second)
    assert!(content_b.contains("computeTotal(100, 10)"));
    // In Stage 3: typecast transformed
    assert!(content_b.contains("int(Request::query('val'))"));
    // In Stage 4: error_reporting commented out
    assert!(content_b.contains("// error_reporting(E_ALL);"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_wide_non_blocking_unsupported_features() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_non_blocking_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    // File 1 has unsupported dynamic variable syntax $$foo and eval()
    let file_unsupported = temp_dir.join("legacy_dynamic.php");
    fs::write(
        &file_unsupported,
        concat!(
            "<?php\n",
            "$name = 'bar';\n",
            "$$name = 42;\n",
            "eval('echo 1;');\n",
            "goto finish;\n",
            "finish:\n"
        ),
    )
    .unwrap();

    // File 2 has standard PHP code that should still migrate successfully
    let file_valid = temp_dir.join("valid.php");
    fs::write(
        &file_valid,
        concat!(
            "<?php\n",
            "error_reporting(0);\n",
            "$id = (int)$_GET['id'];\n"
        ),
    )
    .unwrap();

    let report = migrate_path(&temp_dir, MigrateOptions::default()).unwrap();
    assert_eq!(report.files_scanned, 2);

    // Warnings must be emitted for unsupported features
    assert!(
        report.warnings.iter().any(|w| w.contains("Dynamic variable variable '$$'")),
        "Report should contain warning for $$"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("'eval()' is unsupported")),
        "Report should contain warning for eval()"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("'goto' statement is unsupported")),
        "Report should contain warning for goto"
    );

    // Crucially: Migration did NOT halt, file_valid was fully migrated!
    let valid_migrated = fs::read_to_string(&file_valid).unwrap();
    assert!(valid_migrated.contains("// error_reporting(0);"));
    assert!(valid_migrated.contains("int(Request::query('id'))"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_pdo_prepared_query_with_parameters() {
    let input = concat!(
        "<?php\n",
        "$stmt = $pdo->prepare('SELECT id, total_amount, status FROM orders WHERE user_id = ? ORDER BY id DESC');\n",
        "$stmt->execute([$userId]);\n",
        "$orders = $stmt->fetchAll(PDO::FETCH_ASSOC);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert_eq!(
        res.migrated_content,
        "$orders = DB::select('SELECT id, total_amount, status FROM orders WHERE user_id = ? ORDER BY id DESC', [$userId]);\n"
    );
}

#[test]
fn test_migrate_pdo_prepared_query_empty_execute() {
    let input = concat!(
        "<?php\n",
        "$stmt = $pdo->prepare('SELECT * FROM orders');\n",
        "$stmt->execute();\n",
        "$orders = $stmt->fetchAll(PDO::FETCH_ASSOC);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert_eq!(
        res.migrated_content,
        "$orders = DB::select('SELECT * FROM orders');\n"
    );
}

#[test]
fn test_migrate_pdo_prepared_query_inside_function_with_indentation() {
    let input = concat!(
        "<?php\n",
        "function getOrders($pdo, $userId) {\n",
        "    $stmt = $pdo->prepare('SELECT id, total_amount, status FROM orders WHERE user_id = ? ORDER BY id DESC');\n",
        "    $stmt->execute([$userId]);\n",
        "    $orders = $stmt->fetchAll(PDO::FETCH_ASSOC);\n",
        "    return $orders;\n",
        "}\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert!(res.migrated_content.contains("    $orders = DB::select('SELECT id, total_amount, status FROM orders WHERE user_id = ? ORDER BY id DESC', [$userId]);"));
    assert!(res.migrated_content.contains("    return $orders;"));
}

#[test]
fn test_migrate_pdo_prepared_query_with_return() {
    let input = concat!(
        "<?php\n",
        "function fetchAllUsers($pdo) {\n",
        "    $stmt = $pdo->prepare('SELECT * FROM users');\n",
        "    $stmt->execute();\n",
        "    return $stmt->fetchAll(PDO::FETCH_ASSOC);\n",
        "}\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert!(res.migrated_content.contains("    return DB::select('SELECT * FROM users');"));
}

#[test]
fn test_migrate_pdo_prepared_query_fetch_single_row() {
    let input = concat!(
        "<?php\n",
        "$stmt = $pdo->prepare('SELECT * FROM users WHERE id = ?');\n",
        "$stmt->execute([$id]);\n",
        "$user = $stmt->fetch(PDO::FETCH_ASSOC);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert_eq!(
        res.migrated_content,
        "$user = DB::select('SELECT * FROM users WHERE id = ?', [$id])[0] ?? null;\n"
    );
}

#[test]
fn test_migrate_pdo_prepared_query_bind_value() {
    let input = concat!(
        "<?php\n",
        "$stmt = $pdo->prepare('SELECT * FROM users WHERE email = :email');\n",
        "$stmt->bindValue(':email', $email);\n",
        "$stmt->execute();\n",
        "$users = $stmt->fetchAll(PDO::FETCH_ASSOC);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert_eq!(
        res.migrated_content,
        "$users = DB::select('SELECT * FROM users WHERE email = :email', [':email' => $email]);\n"
    );
}

#[test]
fn test_migrate_pdo_query_chained() {
    let input = concat!(
        "<?php\n",
        "$orders = $pdo->query('SELECT * FROM orders')->fetchAll(PDO::FETCH_ASSOC);\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.pdo_migrated_count, 1);
    assert_eq!(
        res.migrated_content,
        "$orders = DB::select('SELECT * FROM orders');\n"
    );
}

#[test]
fn test_cli_migrate_stream_multi_file_and_dependency_order() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_migrate_stream_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    // Create 3 files: BaseModel, User (extends BaseModel), and template.blade.php
    let base_file = temp_dir.join("BaseModel.php");
    fs::write(
        &base_file,
        concat!(
            "<?php\n",
            "class BaseModel {\n",
            "    public function id(): int {\n",
            "        return 1;\n",
            "    }\n",
            "}\n"
        ),
    )
    .unwrap();

    let user_file = temp_dir.join("User.php");
    fs::write(
        &user_file,
        concat!(
            "<?php\n",
            "class User extends BaseModel {\n",
            "    public function greet(): string {\n",
            "        $user = 'World';\n",
            "        return 'Hello ' . $user;\n",
            "    }\n",
            "}\n"
        ),
    )
    .unwrap();

    let view_file = temp_dir.join("index.blade.php");
    fs::write(
        &view_file,
        concat!(
            "<?php\n",
            "$u = new User();\n",
            "echo $u->greet();\n"
        ),
    )
    .unwrap();

    // Run rhp migrate --stream
    let output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--stream")
        .output()
        .expect("failed to run rhp migrate --stream");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty(), "expected NDJSON stream output");

    let events: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("each line must be valid JSON"))
        .collect();

    // Check Phase 1 start
    assert_eq!(events[0]["phase"], "project_regex");
    assert_eq!(events[0]["event"], "start");
    assert_eq!(events[0]["total_files"], 3);

    // Find Phase 1 complete
    let p1_complete_idx = events
        .iter()
        .position(|e| e["phase"] == "project_regex" && e["event"] == "complete")
        .expect("must emit project_regex complete event");
    assert!(events[p1_complete_idx]["modified_count"].as_u64().unwrap() >= 1);

    // Verify Phase 2 order: BaseModel must start verify before User
    let mut base_verify_idx = None;
    let mut user_verify_idx = None;

    for (i, e) in events.iter().enumerate() {
        if e["phase"] == "file_verify" && e["event"] == "file_start" {
            let file = e["file"].as_str().unwrap_or("");
            if file.contains("BaseModel.php") {
                base_verify_idx = Some(i);
            } else if file.contains("User.php") {
                user_verify_idx = Some(i);
            }
        }
    }

    assert!(
        base_verify_idx.is_some() && user_verify_idx.is_some(),
        "both BaseModel and User must be verified"
    );
    assert!(
        base_verify_idx.unwrap() < user_verify_idx.unwrap(),
        "BaseModel (dependency) must be verified before User (dependent)"
    );

    // Check final complete event
    let final_event = events.last().unwrap();
    assert_eq!(final_event["phase"], "all_done");
    assert_eq!(final_event["event"], "complete");
    assert_eq!(final_event["summary"]["migrated"], 3);
    assert_eq!(final_event["summary"]["warnings"], 0);

    // Verify files on disk had tags stripped and changes applied
    let base_content = fs::read_to_string(&base_file).unwrap();
    assert!(!base_content.contains("<?php"));

    let user_content = fs::read_to_string(&user_file).unwrap();
    assert!(!user_content.contains("<?php"));
    assert!(user_content.contains("+"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_migrate_format_ndjson_flag_and_diagnostics() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_migrate_ndjson_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let ok_file = temp_dir.join("ok.phtml");
    fs::write(
        &ok_file,
        concat!(
            "<?php\n",
            "$title = $_GET['title'];\n",
            "echo $title;\n"
        ),
    )
    .unwrap();

    let broken_file = temp_dir.join("broken.php");
    fs::write(
        &broken_file,
        concat!(
            "<?php\n",
            "function broken( {\n"
        ),
    )
    .unwrap();

    // Test with --format=ndjson
    let output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--format=ndjson")
        .output()
        .expect("failed to run rhp migrate --format=ndjson");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("valid JSON"))
        .collect();

    assert!(events.iter().any(|e| e["phase"] == "project_regex" && e["event"] == "start"));

    // Check for diagnostic event on broken.php
    let diag_event = events
        .iter()
        .find(|e| e["phase"] == "file_verify" && e["event"] == "diagnostic");
    assert!(diag_event.is_some(), "expected diagnostic event for syntax error");
    let diag = diag_event.unwrap();
    assert_eq!(diag["level"], "error");
    assert!(diag["file"].as_str().unwrap().contains("broken.php"));

    // Check broken.php file_complete has status "warning"
    let broken_complete = events.iter().find(|e| {
        e["phase"] == "file_verify"
            && e["event"] == "file_complete"
            && e["file"].as_str().unwrap_or("").contains("broken.php")
    });
    assert!(broken_complete.is_some());
    assert_eq!(broken_complete.unwrap()["status"], "warning");

    // Final summary has 1 warning and 1 migrated
    let final_event = events.last().unwrap();
    assert_eq!(final_event["phase"], "all_done");
    assert_eq!(final_event["summary"]["warnings"], 1);
    assert_eq!(final_event["summary"]["migrated"], 1);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_migrate_single_file_stream() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_migrate_single_stream_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let single_file = temp_dir.join("single.rade.rhp");
    fs::write(
        &single_file,
        concat!(
            "<?php\n",
            "$v = $_POST['item'];\n",
            "echo $v;\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&single_file)
        .arg("--stream")
        .output()
        .expect("failed to run rhp migrate single file --stream");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("valid JSON"))
        .collect();

    assert_eq!(events[0]["phase"], "project_regex");
    assert_eq!(events[0]["event"], "start");
    assert_eq!(events[0]["total_files"], 1);

    let final_event = events.last().unwrap();
    assert_eq!(final_event["phase"], "all_done");
    assert_eq!(final_event["summary"]["migrated"], 1);
    assert_eq!(final_event["summary"]["warnings"], 0);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_migrate_view_includes_top_level_and_inside_function() {
    let input = concat!(
        "<?php\n",
        "require __DIR__ . '/views/header.phtml';\n",
        "include __DIR__ . '/views/orders-list.phtml';\n",
        "require_once __DIR__ . '/views/footer.phtml';\n",
        "include_once __DIR__ . '/views/footer.phtml';\n",
        "\n",
        "function renderOrderPage() {\n",
        "    require __DIR__ . '/views/header.phtml';\n",
        "    include __DIR__ . '/views/orders-list.phtml';\n",
        "    require_once __DIR__ . '/views/footer.phtml';\n",
        "    include_once __DIR__ . '/views/footer.phtml';\n",
        "}\n"
    );

    let res = migrate_source(input, "order_controller.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 8);

    let expected = concat!(
        "View::render('./views/header');\n",
        "View::render('./views/orders-list');\n",
        "View::render('./views/footer');\n",
        "View::render('./views/footer');\n",
        "\n",
        "function renderOrderPage() {\n",
        "    View::render('./views/header');\n",
        "    View::render('./views/orders-list');\n",
        "    View::render('./views/footer');\n",
        "    View::render('./views/footer');\n",
        "}\n"
    );

    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_transpile_phtml_braced_syntax_to_rade() {
    let input = concat!(
        "<div class=\"dashboard\">\n",
        "    <?php /* Header banner */ ?>\n",
        "    <?php require __DIR__ . '/views/header.phtml'; ?>\n",
        "    <?php if ($user['is_admin']) { ?>\n",
        "        <h1>Admin Panel: <?= htmlspecialchars($user['name']) ?></h1>\n",
        "        <?php $debug_info = true; ?>\n",
        "    <?php } else { ?>\n",
        "        <h1>Welcome <?= $user['name'] ?></h1>\n",
        "    <?php } ?>\n",
        "    <ul>\n",
        "    <?php foreach ($orders as $order) { ?>\n",
        "        <li>Order #<?= $order['id'] ?></li>\n",
        "    <?php } ?>\n",
        "    </ul>\n",
        "    <?php include_once __DIR__ . '/views/footer.phtml'; ?>\n",
        "</div>\n"
    );

    let res = migrate_source(input, "dashboard.phtml");
    assert!(res.modified);
    assert!(res.migrated_content.contains("{{-- Header banner --}}"));
    assert!(res.migrated_content.contains("@include('header')"));
    assert!(res.migrated_content.contains("@if ($user['is_admin'])"));
    assert!(res.migrated_content.contains("<h1>Admin Panel: {{ $user['name'] }}</h1>"));
    assert!(res.migrated_content.contains("@rhp\n$debug_info = true;\n@endrhp"));
    assert!(res.migrated_content.contains("@else"));
    assert!(res.migrated_content.contains("<h1>Welcome {{ $user['name'] }}</h1>"));
    assert!(res.migrated_content.contains("@endif"));
    assert!(res.migrated_content.contains("@foreach ($orders as $order)"));
    assert!(res.migrated_content.contains("<li>Order #{{ $order['id'] }}</li>"));
    assert!(res.migrated_content.contains("@endforeach"));
    assert!(res.migrated_content.contains("@include('footer')"));
    assert_eq!(res.target_file_path.as_deref(), Some("dashboard.rade.rhp"));
}

#[test]
fn test_phtml_file_migrated_and_renamed_to_rade_rhp_on_disk() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_migrate_phtml_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let phtml_file = temp_dir.join("order-details.phtml");
    fs::write(
        &phtml_file,
        concat!(
            "<div class=\"order\">\n",
            "    <?php /* Order info */ ?>\n",
            "    <h3>Order #<?= $order_id ?></h3>\n",
            "    <?php foreach ($items as $item) { ?>\n",
            "        <p><?= $item['name'] ?></p>\n",
            "    <?php } ?>\n",
            "</div>\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");
    assert!(output.status.success());

    // Old .phtml should be deleted
    assert!(!phtml_file.exists(), "Old .phtml file should have been removed");

    // New .rade.rhp should exist
    let target_file = temp_dir.join("order-details.rade.rhp");
    assert!(target_file.exists(), "New .rade.rhp file should exist on disk");

    let contents = fs::read_to_string(&target_file).unwrap();
    assert!(contents.contains("{{-- Order info --}}"));
    assert!(contents.contains("<h3>Order #{{ $order_id }}</h3>"));
    assert!(contents.contains("@foreach ($items as $item)"));
    assert!(contents.contains("<p>{{ $item['name'] }}</p>"));
    assert!(contents.contains("@endforeach"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_php_file_with_doctype_and_html_tags_migrated_to_rade_rhp() {
    let input = concat!(
        "<!DOCTYPE html>\n",
        "<html lang=\"en\">\n",
        "<head>\n",
        "    <meta charset=\"UTF-8\">\n",
        "    <title><?= $page_title ?></title>\n",
        "</head>\n",
        "<body>\n",
        "    <h1>Welcome to <?= $site_name ?></h1>\n",
        "</body>\n",
        "</html>\n"
    );

    let res = migrate_source(input, "home.php");
    assert!(res.modified);
    assert_eq!(res.target_file_path.as_deref(), Some("home.rade.rhp"));
    assert!(res.migrated_content.contains("<title>{{ $page_title }}</title>"));
    assert!(res.migrated_content.contains("<h1>Welcome to {{ $site_name }}</h1>"));
    assert!(res.migrated_content.contains("</html>"));
}

#[test]
fn test_php_file_with_preamble_and_html_migrated_to_rade_rhp() {
    let input = concat!(
        "<?php\n",
        "$title = \"Dashboard\";\n",
        "$user = Request::query('user');\n",
        "?>\n",
        "<!DOCTYPE html>\n",
        "<html>\n",
        "<head><title><?= $title ?></title></head>\n",
        "<body>\n",
        "    <h1>Hello <?= $user ?></h1>\n",
        "</body>\n",
        "</html>\n"
    );

    let res = migrate_source(input, "index.php");
    assert!(res.modified);
    assert_eq!(res.target_file_path.as_deref(), Some("index.rade.rhp"));
    assert!(res.migrated_content.contains("@rhp\n$title = \"Dashboard\";\n$user = Request::query('user');\n@endrhp"));
    assert!(res.migrated_content.contains("<title>{{ $title }}</title>"));
    assert!(res.migrated_content.contains("<h1>Hello {{ $user }}</h1>"));
    assert!(res.migrated_content.contains("</html>"));
}

#[test]
fn test_php_partial_view_with_multiple_tags_migrated_to_rade_rhp() {
    let input = concat!(
        "<div class=\"user-card\">\n",
        "    <?php if ($user['is_active']): ?>\n",
        "        <p>Active user: <?= $user['name'] ?></p>\n",
        "    <?php else: ?>\n",
        "        <p>Inactive user</p>\n",
        "    <?php endif; ?>\n",
        "</div>\n"
    );

    let res = migrate_source(input, "user_card.php");
    assert!(res.modified);
    assert_eq!(res.target_file_path.as_deref(), Some("user_card.rade.rhp"));
    assert!(res.migrated_content.contains("@if ($user['is_active'])"));
    assert!(res.migrated_content.contains("<p>Active user: {{ $user['name'] }}</p>"));
    assert!(res.migrated_content.contains("@else"));
    assert!(res.migrated_content.contains("@endif"));
    assert!(res.migrated_content.contains("</div>"));
}

#[test]
fn test_pure_php_class_not_converted_to_rade_rhp() {
    let input = concat!(
        "<?php\n",
        "\n",
        "namespace App\\Controllers;\n",
        "\n",
        "class UserController {\n",
        "    public function index() {\n",
        "        return \"hello\";\n",
        "    }\n",
        "}\n"
    );

    let res = migrate_source(input, "UserController.php");
    assert_eq!(res.target_file_path, None);
}

#[test]
fn test_pure_php_script_covering_entire_file_not_converted_to_rade_rhp() {
    let input = concat!(
        "<?php\n",
        "\n",
        "return [\n",
        "    'app_name' => 'Store',\n",
        "    'debug' => true,\n",
        "];\n",
        "?>\n"
    );

    let res = migrate_source(input, "config.php");
    assert_eq!(res.target_file_path, None);
}

#[test]
fn test_cli_shebang_script_not_converted_to_rade_rhp() {
    let input = concat!(
        "#!/usr/bin/env php\n",
        "<?php\n",
        "\n",
        "echo \"Running CLI batch\\n\";\n"
    );

    let res = migrate_source(input, "cli_worker.php");
    assert_eq!(res.target_file_path, None);
}

#[test]
fn test_php_file_migrated_and_renamed_to_rade_rhp_on_disk() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_migrate_php_view_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).unwrap();

    let php_file = temp_dir.join("landing.php");
    fs::write(
        &php_file,
        concat!(
            "<!DOCTYPE html>\n",
            "<html>\n",
            "<head><title><?= $page_title ?></title></head>\n",
            "<body>\n",
            "    <h3>Welcome <?= $user_name ?></h3>\n",
            "</body>\n",
            "</html>\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("migrate")
        .arg(&temp_dir)
        .arg("--write")
        .output()
        .expect("failed to run rhp migrate --write");
    assert!(output.status.success());

    // Old .php view should be deleted
    assert!(!php_file.exists(), "Old .php view file should have been removed");

    // New .rade.rhp should exist
    let target_file = temp_dir.join("landing.rade.rhp");
    assert!(target_file.exists(), "New .rade.rhp file should exist on disk");

    let contents = fs::read_to_string(&target_file).unwrap();
    assert!(contents.contains("<title>{{ $page_title }}</title>"));
    assert!(contents.contains("<h3>Welcome {{ $user_name }}</h3>"));
    assert!(contents.contains("</html>"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_include_view_php_migrated_to_view_render() {
    let input = concat!(
        "<?php\n",
        "require __DIR__ . '/header.php';\n",
        "include __DIR__ . '/views/sidebar.php';\n",
        "require_once __DIR__ . '/footer.php';\n"
    );

    let res = migrate_source(input, "controller.php");
    assert!(res.modified);
    assert_eq!(res.legacy_include_count, 3);
    assert!(res.migrated_content.contains("View::render('./header');"));
    assert!(res.migrated_content.contains("View::render('./views/sidebar');"));
    assert!(res.migrated_content.contains("View::render('./footer');"));
}


