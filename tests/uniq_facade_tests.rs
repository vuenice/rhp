use rhp::{
    is_legacy_php_function, is_static_facade, is_static_only, run_source_with_vm, PhpValue,
};

#[test]
fn test_uniq_facade_registration_and_static_only_guard() {
    // 1. Verify Uniq is registered as a native static facade
    assert!(is_static_facade("Uniq"), "Uniq should be registered as static facade");
    assert!(is_static_only("Uniq"), "Uniq should be marked static-only");

    // 2. Verify legacy PHP functions mapping for uniqid
    assert!(
        is_legacy_php_function("uniqid"),
        "uniqid should be recognized as legacy PHP function"
    );
    let mapping = rhp::get_legacy_php_function("uniqid")
        .expect("Mapping for uniqid should exist");
    assert_eq!(mapping.php_name, "uniqid");
    assert_eq!(mapping.rhp_facade, "Uniq");
    assert_eq!(mapping.rhp_method, "uniqid");

    // 3. Verify instantiation via new is strictly forbidden
    let bad_instantiate = r#"
        $u = new Uniq();
    "#;
    let res = run_source_with_vm(bad_instantiate);
    assert!(res.is_err(), "Instantiating Uniq with 'new' must fail");

    // 4. Verify inheritance via extends is strictly forbidden
    let bad_extend = r#"
        class CustomUniq extends Uniq {}
    "#;
    let res2 = run_source_with_vm(bad_extend);
    assert!(res2.is_err(), "Extending Uniq with 'extends' must fail");
}

#[test]
fn test_uniqid_default_and_entropy_modes() {
    let source = r#"
        // Default mode: 13-character hex string
        $id1 = Uniq::uniqid();
        $id2 = Uniq::uniqid();
        $len1 = strlen($id1);
        $len2 = strlen($id2);

        // With prefix: prefix length + 13
        $pfx_id = Uniq::uniqid("order_");
        $pfx_len = strlen($pfx_id);

        // With entropy: 23-character string (13 time + dot + 9 random digits)
        $ent_id = Uniq::uniqid("", true);
        $ent_len = strlen($ent_id);

        // With prefix and entropy: prefix length + 23
        $pfx_ent_id = Uniq::uniqid("item_", true);
        $pfx_ent_len = strlen($pfx_ent_id);

        // Alias: Uniq::id()
        $alias_id = Uniq::id();
        $alias_len = strlen($alias_id);

        // Legacy procedural PHP function call: uniqid()
        $legacy1 = uniqid();
        $legacy1_len = strlen($legacy1);
        $legacy2 = uniqid("usr_", true);
        $legacy2_len = strlen($legacy2);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Uniq::uniqid execution should succeed");

    assert_eq!(vm.get_main_variable("$len1"), Some(&PhpValue::Int(13)));
    assert_eq!(vm.get_main_variable("$len2"), Some(&PhpValue::Int(13)));

    let id1_val = vm.get_main_variable("$id1").unwrap().to_php_string();
    let id2_val = vm.get_main_variable("$id2").unwrap().to_php_string();
    assert_ne!(id1_val, id2_val, "Sequential uniqid calls must be unique");
    assert!(id1_val.chars().all(|c| c.is_ascii_hexdigit()));

    // Prefix check
    assert_eq!(vm.get_main_variable("$pfx_len"), Some(&PhpValue::Int(6 + 13)));
    let pfx_id_val = vm.get_main_variable("$pfx_id").unwrap().to_php_string();
    assert!(pfx_id_val.starts_with("order_"));

    // Entropy check: exactly 23 chars and contains dot
    assert_eq!(vm.get_main_variable("$ent_len"), Some(&PhpValue::Int(23)));
    let ent_id_val = vm.get_main_variable("$ent_id").unwrap().to_php_string();
    assert!(ent_id_val.contains('.'), "With entropy must include a dot");

    // Prefix with entropy: 5 + 23 = 28
    assert_eq!(vm.get_main_variable("$pfx_ent_len"), Some(&PhpValue::Int(5 + 23)));
    let pfx_ent_val = vm.get_main_variable("$pfx_ent_id").unwrap().to_php_string();
    assert!(pfx_ent_val.starts_with("item_"));

    // Alias check
    assert_eq!(vm.get_main_variable("$alias_len"), Some(&PhpValue::Int(13)));

    // Legacy procedural calls
    assert_eq!(vm.get_main_variable("$legacy1_len"), Some(&PhpValue::Int(13)));
    assert_eq!(vm.get_main_variable("$legacy2_len"), Some(&PhpValue::Int(4 + 23)));
    let legacy2_val = vm.get_main_variable("$legacy2").unwrap().to_php_string();
    assert!(legacy2_val.starts_with("usr_"));
    assert!(legacy2_val.contains('.'));
}

#[test]
fn test_uuid4_generation_and_validation() {
    let source = r#"
        $u4_1 = Uniq::uuid4();
        $u4_2 = Uniq::v4();
        $u4_3 = Uniq::uuid();

        $valid1 = Uniq::isValid($u4_1);
        $valid2 = Uniq::isValid($u4_2);
        $valid_bad = Uniq::isValid("not-a-valid-uuid");

        $ver1 = Uniq::version($u4_1);
        $ver2 = Uniq::version($u4_2);

        $len = strlen($u4_1);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Uniq::uuid4 execution should succeed");

    assert_eq!(vm.get_main_variable("$len"), Some(&PhpValue::Int(36)));
    assert_eq!(vm.get_main_variable("$valid1"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$valid2"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$valid_bad"), Some(&PhpValue::Bool(false)));

    assert_eq!(vm.get_main_variable("$ver1"), Some(&PhpValue::Int(4)));
    assert_eq!(vm.get_main_variable("$ver2"), Some(&PhpValue::Int(4)));

    let u1 = vm.get_main_variable("$u4_1").unwrap().to_php_string();
    let u2 = vm.get_main_variable("$u4_2").unwrap().to_php_string();
    assert_ne!(u1, u2, "Random UUIDv4 instances must be unique");
}

#[test]
fn test_uuid5_name_based_rfc4122_vector() {
    let source = r#"
        // Standard RFC 4122 test vector: namespace DNS, name "python.org"
        $u5 = Uniq::uuid5("dns", "python.org");
        $u5_alias = Uniq::v5("dns", "python.org");

        // Custom namespace UUID
        $ns_custom = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
        $u5_custom = Uniq::uuid5($ns_custom, "python.org");

        // URL namespace
        $u5_url = Uniq::uuid5("url", "https://rhp.dev");

        $ver = Uniq::version($u5);
        $valid = Uniq::isValid($u5);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Uniq::uuid5 execution should succeed");

    let u5_val = vm.get_main_variable("$u5").unwrap().to_php_string();
    let u5_alias = vm.get_main_variable("$u5_alias").unwrap().to_php_string();
    let u5_custom = vm.get_main_variable("$u5_custom").unwrap().to_php_string();

    assert_eq!(u5_val, "886313e1-3b8a-5372-9b90-0c9aee199e5d");
    assert_eq!(u5_alias, "886313e1-3b8a-5372-9b90-0c9aee199e5d");
    assert_eq!(u5_custom, "886313e1-3b8a-5372-9b90-0c9aee199e5d");

    assert_eq!(vm.get_main_variable("$ver"), Some(&PhpValue::Int(5)));
    assert_eq!(vm.get_main_variable("$valid"), Some(&PhpValue::Bool(true)));
}

#[test]
fn test_uuid7_time_ordered_and_sorting() {
    let source = r#"
        $u7_1 = Uniq::uuid7();
        $u7_2 = Uniq::v7();

        // Deterministic timestamps in milliseconds
        $t1 = Uniq::uuid7(1730706000000);
        $t2 = Uniq::uuid7(1730706001000);

        $valid = Uniq::isValid($u7_1);
        $ver = Uniq::version($u7_1);
        $ver_t1 = Uniq::version($t1);

        $is_ordered = ($t1 < $t2);
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Uniq::uuid7 execution should succeed");

    assert_eq!(vm.get_main_variable("$valid"), Some(&PhpValue::Bool(true)));
    assert_eq!(vm.get_main_variable("$ver"), Some(&PhpValue::Int(7)));
    assert_eq!(vm.get_main_variable("$ver_t1"), Some(&PhpValue::Int(7)));
    assert_eq!(vm.get_main_variable("$is_ordered"), Some(&PhpValue::Bool(true)));

    let u1 = vm.get_main_variable("$u7_1").unwrap().to_php_string();
    let u2 = vm.get_main_variable("$u7_2").unwrap().to_php_string();
    assert_ne!(u1, u2);
}

#[test]
fn test_uuid3_and_special_uuids_and_parsing() {
    let source = r#"
        // UUIDv3 MD5 vector
        $u3 = Uniq::uuid3("dns", "python.org");
        $u3_alias = Uniq::v3("dns", "python.org");
        $ver3 = Uniq::version($u3);

        // Special constants
        $nil = Uniq::nil();
        $max = Uniq::max();

        // Parsing
        $info = Uniq::parse($u3);
        $hex = $info["hex"];
        $parsed_ver = $info["version"];
        $parsed_var = $info["variant"];

        // Parse v7 includes timestamp_ms
        $v7_item = Uniq::uuid7(1730706000000);
        $info_v7 = Uniq::parse($v7_item);
        $v7_ts = $info_v7["timestamp_ms"];
    "#;

    let (vm, _) = run_source_with_vm(source).expect("Uniq helpers should succeed");

    let u3_val = vm.get_main_variable("$u3").unwrap().to_php_string();
    let u3_alias = vm.get_main_variable("$u3_alias").unwrap().to_php_string();
    assert_eq!(u3_val, "6fa459ea-ee8a-3ca4-894e-db77e160355e");
    assert_eq!(u3_alias, "6fa459ea-ee8a-3ca4-894e-db77e160355e");
    assert_eq!(vm.get_main_variable("$ver3"), Some(&PhpValue::Int(3)));

    assert_eq!(
        vm.get_main_variable("$nil").unwrap().to_php_string(),
        "00000000-0000-0000-0000-000000000000"
    );
    assert_eq!(
        vm.get_main_variable("$max").unwrap().to_php_string(),
        "ffffffff-ffff-ffff-ffff-ffffffffffff"
    );

    assert_eq!(vm.get_main_variable("$parsed_ver"), Some(&PhpValue::Int(3)));
    assert_eq!(
        vm.get_main_variable("$parsed_var"),
        Some(&PhpValue::string("rfc4122"))
    );

    let v7_ts_val = vm.get_main_variable("$v7_ts").unwrap();
    assert_eq!(v7_ts_val.to_i64(), Some(1730706000000));
}
