use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use rhp::vm::run_source;
use rhp::PhpValue;

fn setup_test_views_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_views_scratch")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("Failed to create test views dir");
    dir
}

#[test]
fn test_view_function_basic_and_dot_syntax() {
    let base_dir = setup_test_views_dir("basic_dot");
    let views_dir = base_dir.join("views");
    let cache_dir = base_dir.join(".rhp").join("cache").join("views");
    fs::create_dir_all(&views_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    // 1. Root view: views/home.rade.rhp
    let home_template = "<h1>Welcome, <@ $name @>!</h1>\n@if ($show_count)\n<p>Count: <@ $count @></p>\n@endif";
    fs::write(views_dir.join("home.rade.rhp"), home_template).unwrap();

    // 2. Nested view with dot syntax: views/admin/dashboard.rade.rhp
    let admin_dir = views_dir.join("admin");
    fs::create_dir_all(&admin_dir).unwrap();
    let dashboard_template = "<div class=\"dashboard\">Admin: <@ $admin_name @></div>";
    fs::write(admin_dir.join("dashboard.rade.rhp"), dashboard_template).unwrap();

    // 3. Test .rade extension: views/blog/post.rade
    let blog_dir = views_dir.join("blog");
    fs::create_dir_all(&blog_dir).unwrap();
    let blog_template = "<article><h1><@ $title @></h1><p><@ $body @></p></article>";
    fs::write(blog_dir.join("post.rade"), blog_template).unwrap();

    // 4. Test backward compatibility with legacy .blade.rhp: views/legacy.blade.rhp
    let legacy_template = "<div>Legacy Blade View: <@ $tag @></div>";
    fs::write(views_dir.join("legacy.blade.rhp"), legacy_template).unwrap();

    // Test direct ViewEngine resolution
    let engine = rhp::rade::runtime::ViewEngine::new(&views_dir, &cache_dir, rhp::rade::runtime::ViewMode::Development);
    assert!(engine.resolve_view_path("home").is_ok());
    assert!(engine.resolve_view_path("admin.dashboard").is_ok());
    assert!(engine.resolve_view_path("blog.post").is_ok());
    assert!(engine.resolve_view_path("legacy").is_ok());

    let compiled_home = engine.load("home").unwrap();
    assert!(compiled_home.contains("$__out += \"<h1>Welcome, \";"));
    assert!(compiled_home.contains("Html::escape($name)"));

    let compiled_blog = engine.load("blog.post").unwrap();
    assert!(compiled_blog.contains("Html::escape($title)"));

    let compiled_legacy = engine.load("legacy").unwrap();
    assert!(compiled_legacy.contains("Legacy Blade View"));
}

#[test]
fn test_view_on_demand_compilation_mtime_recompile() {
    let base_dir = setup_test_views_dir("mtime_recompile");
    let views_dir = base_dir.join("views");
    let cache_dir = base_dir.join(".rhp").join("cache").join("views");
    fs::create_dir_all(&views_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    let template_path = views_dir.join("dynamic.rade.rhp");
    fs::write(&template_path, "<p>Initial Version</p>").unwrap();

    let engine = rhp::rade::runtime::ViewEngine::new(&views_dir, &cache_dir, rhp::rade::runtime::ViewMode::Development);

    // Initial load: compiles to cache
    let first_compiled = engine.load("dynamic").unwrap();
    assert!(first_compiled.contains("Initial Version"));

    let cached_file = engine.get_cache_path(&template_path);
    assert!(cached_file.exists());

    // Sleep briefly to ensure filesystem mtime difference
    thread::sleep(Duration::from_millis(50));

    // Update the template
    fs::write(&template_path, "<p>Updated Version <@ $tag @></p>").unwrap();

    // Verify cache is recognized as expired
    assert!(engine.is_expired(&template_path).unwrap());

    // Next load automatically re-compiles on-demand without any external watcher!
    let second_compiled = engine.load("dynamic").unwrap();
    assert!(second_compiled.contains("Updated Version"));
    assert!(second_compiled.contains("Html::escape($tag)"));
}

#[test]
fn test_render_inline_template_in_vm() {
    // In RHP, single quotes prevent the outer script parser from interpolating $variables
    let script = r#"
        $out = render('<h1>Hello, <@ $name @>!</h1>', ["name" => "World"]);
        return $out;
    "#;
    let res = run_source(script).expect("Execution failed");
    assert_eq!(res, PhpValue::string("<h1>Hello, World!</h1>"));
}

#[test]
fn test_render_inline_template_with_control_flow() {
    let script = r#"
        $tpl = '<ul>@for ($i = 0; $i < 3; $i++)<li>Item <@ $i @></li>@endfor</ul>';
        $out = render($tpl, []);
        return $out;
    "#;
    let res = run_source(script).expect("Execution failed");
    assert_eq!(res, PhpValue::string("<ul><li>Item 0</li><li>Item 1</li><li>Item 2</li></ul>"));
}

#[test]
fn test_render_raw_and_escaped_entities() {
    let script = r#"
        $tpl = '<div>Escaped: <@ $malicious @> | Raw: <@! $safe_html @></div>';
        $out = render($tpl, [
            "malicious" => "<script>alert('xss')</script>",
            "safe_html" => "<b>Bold</b>"
        ]);
        return $out;
    "#;
    let res = run_source(script).expect("Execution failed");
    assert_eq!(
        res,
        PhpValue::string("<div>Escaped: &lt;script&gt;alert(&#039;xss&#039;)&lt;/script&gt; | Raw: <b>Bold</b></div>")
    );
}

#[test]
fn test_view_from_file_in_vm() {
    let base_dir = setup_test_views_dir("vm_view_file");
    let views_dir = base_dir.join("views");
    fs::create_dir_all(&views_dir).unwrap();

    let template_path = views_dir.join("welcome.rade.rhp");
    fs::write(&template_path, "<h1>Welcome, <@ $user @>!</h1><p>Status: <@ $status @></p>").unwrap();

    // Call view() with explicit relative file path
    let script = format!(
        r#"
            $out = view('{}', ["user" => "Alice", "status" => "Active"]);
            return $out;
        "#,
        template_path.display()
    );

    let res = run_source(&script).expect("Execution failed");
    assert_eq!(res, PhpValue::string("<h1>Welcome, Alice!</h1><p>Status: Active</p>"));
}
