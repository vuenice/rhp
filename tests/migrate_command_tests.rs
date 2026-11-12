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
    let expected = concat!(
        "<?php\n",
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
    let expected = concat!(
        "<?php\n",
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
    let expected = "$_GET = Request::query();\n\n$id = $_GET['id'];\n";
    assert_eq!(res.migrated_content, expected);
}

#[test]
fn test_migrate_does_not_duplicate_existing_lines() {
    let input = concat!(
        "<?php\n",
        "$_GET = Request::query();\n",
        "$id = $_GET['id'];\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(!res.modified);
    assert!(res.added_lines.is_empty());
}

#[test]
fn test_migrate_ignores_comments_and_strings() {
    let input = concat!(
        "<?php\n",
        "// Don't add $_GET here\n",
        "/* Nor $_POST here */\n",
        "$msg = \"This $_FILES is a string\";\n",
        "$msg2 = 'Single quoted $_COOKIE string';\n"
    );
    let res = migrate_source(input, "test.php");
    assert!(!res.modified);
    assert!(res.added_lines.is_empty());
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
    assert_eq!(res.dot_concat_count, 2);
    let expected = concat!(
        "<?php\n",
        "$query = \"SELECT * FROM users \"\n",
        "    + \"WHERE active = 1 \"\n",
        "    + \"ORDER BY id DESC\";\n"
    );
    assert_eq!(res.migrated_content, expected);
}

