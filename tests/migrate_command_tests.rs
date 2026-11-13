use rhp::migrate::migrate_source;
use std::fs;
use std::process::Command;

#[test]
fn test_migrate_get_used() {
    let input = concat!(
        "<?php\n",
        "$id = $_GET['id'];\n",
        "echo $id;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_GET = Request::query();"]);
    assert!(res.migrated_content.contains("$_GET = Request::query();\n\n$id = $_GET['id'];"));
}

#[test]
fn test_migrate_post_used() {
    let input = concat!(
        "<?php\n",
        "$title = $_POST['title'];\n",
        "echo $title;\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_POST = Request::body();"]);
    assert!(res.migrated_content.contains("$_POST = Request::body();\n\n$title = $_POST['title'];"));
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
    assert!(res.migrated_content.contains("$db = $conn;"));
}

#[test]
fn test_migrate_declare_strict_types_insertion() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "$page = $_GET['page'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    let expected = concat!(
        "declare(strict_types=1);\n",
        "$_GET = Request::query();\n\n",
        "$page = $_GET['page'];\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_namespace_insertion() {
    let input = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "$val = $_GET['val'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(res.modified);
    assert!(res.php_tag_removed);
    let expected = concat!(
        "namespace App\\Http\\Controllers;\n",
        "$_GET = Request::query();\n\n",
        "$val = $_GET['val'];\n"
    );
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_without_opening_tag() {
    let input = "$id = $_GET['id'];\n";
    let res = migrate_source(input, "script.rhp");
    assert!(res.modified);
    assert!(!res.php_tag_removed);
    let expected = "$_GET = Request::query();\n\n$id = $_GET['id'];\n";
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_does_not_duplicate_existing_lines() {
    // When input already has no tags and no migration needed
    let input_clean = concat!(
        "$_GET = Request::query();\n",
        "$id = $_GET['id'];\n"
    );
    let res_clean = migrate_source(input_clean, "test.rhp");
    assert!(!res_clean.modified);
    assert!(res_clean.added_lines.is_empty());

    // When input has <?php tag, tag is removed without duplicating declarations
    let input_tagged = concat!(
        "<?php\n",
        "$_GET = Request::query();\n",
        "$id = $_GET['id'];\n"
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


