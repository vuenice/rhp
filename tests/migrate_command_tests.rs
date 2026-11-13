use rhp::migrate::{
    comment_ini_set_calls, migrate_source, replace_get_array_access, replace_post_array_access,
    strip_error_reporting, upgrade_php_source,
};
use std::fs;
use std::process::Command;

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
        "        public int $val = 1;\n",
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
    assert!(res.migrated_content.contains("// $data = json_decode($res, true);"));
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
    assert!(res.migrated_content.contains("// You can use Http::pool([...]) / Future::wait([...])"));
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
        "function add(int $a, int $b): int {\n",
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
        "foreach ($rows as [$id, $name]) {\n",
        "    echo $id;\n",
        "}\n",
        "foreach ($data as $key => [$a, $b]) {\n",
        "    echo $key;\n",
        "}\n"
    );
    assert_eq!(res.upgraded_content, expected);
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
        "return json_decode($json, true);\n"
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
    assert!(check_stdout.contains("1 error_reporting(...) call(s) removed"));
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
    assert!(!migrated_content.contains("error_reporting("));
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

