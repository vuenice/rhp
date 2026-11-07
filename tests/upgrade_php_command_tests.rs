use rhp::upgrade_php::upgrade_php_source;
use std::fs;
use std::process::Command;

#[test]
fn test_upgrade_php_get_used() {
    let input = concat!(
        "<?php\n",
        "$id = $_GET['id'];\n",
        "echo $id;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_GET = Request::query();"]);
    assert!(res.upgraded_content.contains("$_GET = Request::query();\n\n$id = $_GET['id'];"));
}

#[test]
fn test_upgrade_php_post_used() {
    let input = concat!(
        "<?php\n",
        "$title = $_POST['title'];\n",
        "echo $title;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_POST = Request::body();"]);
    assert!(res.upgraded_content.contains("$_POST = Request::body();\n\n$title = $_POST['title'];"));
}

#[test]
fn test_upgrade_php_server_vars_used() {
    let input = concat!(
        "<?php\n",
        "$method = $_SERVER['REQUEST_METHOD'];\n",
        "$ip = $_SERVER['REMOTE_ADDR'];\n",
        "$host = $_SERVER['HTTP_HOST'];\n",
        "echo $method . ' ' . $ip . ' ' . $host;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(
        res.added_lines,
        vec![
            "$_SERVER['REQUEST_METHOD'] = Request::method();",
            "$_SERVER['REMOTE_ADDR'] = Request::ip();",
            "$_SERVER['HTTP_HOST'] = Request::host();"
        ]
    );
    assert!(res.upgraded_content.contains("$_SERVER['REQUEST_METHOD'] = Request::method();"));
    assert!(res.upgraded_content.contains("$_SERVER['REMOTE_ADDR'] = Request::ip();"));
    assert!(res.upgraded_content.contains("$_SERVER['HTTP_HOST'] = Request::host();"));
}

#[test]
fn test_upgrade_php_files_used() {
    let input = concat!(
        "<?php\n",
        "$doc = $_FILES['document'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_FILES = Request::files();"]);
}

#[test]
fn test_upgrade_php_cookie_read() {
    let input = concat!(
        "<?php\n",
        "$session = $_COOKIE['session_token'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.added_lines, vec!["$_COOKIE = Cookie::get();"]);
}

#[test]
fn test_upgrade_php_cookie_set_assignment_and_functions() {
    let input = concat!(
        "<?php\n",
        "$_COOKIE['theme'] = 'dark';\n",
        "$_COOKIE['user'] = $currentUser;\n",
        "setcookie('auth', 'token123');\n",
        "\\setrawcookie('raw', 'val456');\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cookie_set_count, 4);
    assert!(res.upgraded_content.contains("Cookie::set('theme', 'dark');"));
    assert!(res.upgraded_content.contains("Cookie::set('user', $currentUser);"));
    assert!(res.upgraded_content.contains("Cookie::set('auth', 'token123');"));
    assert!(res.upgraded_content.contains("Cookie::set('raw', 'val456');"));
    // Since $_COOKIE was only written to and never read, $_COOKIE = Cookie::get(); is NOT added
    assert!(!res.upgraded_content.contains("$_COOKIE = Cookie::get();"));
}

#[test]
fn test_upgrade_php_cookie_set_and_read() {
    let input = concat!(
        "<?php\n",
        "$_COOKIE['theme'] = 'dark';\n",
        "echo $_COOKIE['theme'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.cookie_set_count, 1);
    assert!(res.upgraded_content.contains("Cookie::set('theme', 'dark');"));
    // Because $_COOKIE is still read, $_COOKIE = Cookie::get(); IS added
    assert!(res.upgraded_content.contains("$_COOKIE = Cookie::get();"));
}

#[test]
fn test_upgrade_php_globals_replacement() {
    let input = concat!(
        "<?php\n",
        "$conn = $_GLOBAL['db'];\n",
        "$secret = $_GLOBALS['api_key'];\n",
        "$app = $GLOBALS['config'];\n",
        "$dynamic = $GLOBALS[$keyName];\n",
        "$_GLOBAL['counter'] = 42;\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    assert_eq!(res.global_replace_count, 5);
    assert!(res.upgraded_content.contains("$conn = $db;"));
    assert!(res.upgraded_content.contains("$secret = $api_key;"));
    assert!(res.upgraded_content.contains("$app = $config;"));
    assert!(res.upgraded_content.contains("$dynamic = $$keyName;"));
    assert!(res.upgraded_content.contains("$counter = 42;"));
}

#[test]
fn test_upgrade_php_all_variables_combined() {
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
    let res = upgrade_php_source(input, "test.php");
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
    assert!(res.upgraded_content.contains("$db = $conn;"));
}

#[test]
fn test_upgrade_php_declare_strict_types_insertion() {
    let input = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "$page = $_GET['page'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    let expected = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "$_GET = Request::query();\n\n",
        "$page = $_GET['page'];\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_upgrade_php_namespace_insertion() {
    let input = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "$val = $_GET['val'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(res.modified);
    let expected = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "$_GET = Request::query();\n\n",
        "$val = $_GET['val'];\n"
    );
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_upgrade_php_without_opening_tag() {
    let input = "$id = $_GET['id'];\n";
    let res = upgrade_php_source(input, "script.rhp");
    assert!(res.modified);
    let expected = "$_GET = Request::query();\n\n$id = $_GET['id'];\n";
    assert_eq!(res.upgraded_content, expected);
}

#[test]
fn test_upgrade_php_does_not_duplicate_existing_lines() {
    let input = concat!(
        "<?php\n",
        "$_GET = Request::query();\n",
        "$id = $_GET['id'];\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert!(res.added_lines.is_empty());
}

#[test]
fn test_upgrade_php_ignores_comments_and_strings() {
    let input = concat!(
        "<?php\n",
        "// Don't add $_GET here\n",
        "/* Nor $_POST here */\n",
        "$msg = \"This $_FILES is a string\";\n",
        "$msg2 = 'Single quoted $_COOKIE string';\n"
    );
    let res = upgrade_php_source(input, "test.php");
    assert!(!res.modified);
    assert!(res.added_lines.is_empty());
}

#[test]
fn test_cli_upgrade_php_execution() {
    let temp_dir = std::env::temp_dir().join(format!(
        "rhp_test_upgrade_{}",
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

    // 1. Dry run / check mode
    let check_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("upgrade-php")
        .arg(&temp_dir)
        .arg("--check")
        .output()
        .expect("failed to run rhp upgrade-php --check");

    assert!(check_output.status.success());
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("Dry Run"));
    assert!(check_stdout.contains("$_GET = Request::query();"));

    // Verify file on disk was not modified yet
    assert_eq!(fs::read_to_string(&file_path).unwrap(), content);

    // 2. Perform actual upgrade
    let upgrade_output = Command::new(env!("CARGO_BIN_EXE_rhp"))
        .arg("upgrade-php")
        .arg(&temp_dir)
        .output()
        .expect("failed to run rhp upgrade-php");

    assert!(upgrade_output.status.success());
    let upgraded_content = fs::read_to_string(&file_path).unwrap();
    assert!(upgraded_content.contains("$_GET = Request::query();"));
    assert!(upgraded_content.contains("Cookie::set('pref', 'light');"));
    assert!(upgraded_content.contains("$db = $database;"));

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}
