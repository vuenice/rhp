use rhp::run_source;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_crypt_helper_functions() {
    clear_runtime_closure_macros();

    let code = r#"
        $md5 = Crypt::md5("hello world");
        $sha1 = Crypt::sha1("hello world");
        $sha256 = Crypt::sha256("hello world");
        $sha512 = Crypt::sha512("hello world");

        $hash_sha256 = Crypt::hash("sha256", "hello world");
        $hash_md5 = Crypt::hash("md5", "hello world");

        $hmac_sha256 = Crypt::hashHmac("sha256", "message", "secret-key");
        $hmac_md5 = Crypt::hashHmac("md5", "message", "secret-key");

        $bytes = Crypt::randomBytes(16);
        $hex = Crypt::randomHex(32);

        $algos = Crypt::hashAlgos();
        $ciphers = Crypt::opensslCipherMethods();

        return [
            $md5,
            $sha1,
            $sha256,
            $sha512,
            $hash_sha256,
            $hash_md5,
            $hmac_sha256,
            $hmac_md5,
            Str::byteLength($bytes),
            strlen($hex),
            $algos,
            $ciphers,
        ];
    "#;

    let res = run_source(code).expect("Crypt helper execution failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        // md5("hello world")
        assert_eq!(b[0].to_php_string(), "5eb63bbbe01eeed093cb22bb8f5acdc3");
        // sha1("hello world")
        assert_eq!(b[1].to_php_string(), "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
        // sha256("hello world")
        assert_eq!(b[2].to_php_string(), "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
        // sha512("hello world")
        assert_eq!(
            b[3].to_php_string(),
            "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f"
        );
        // hash matches sha256 and md5
        assert_eq!(b[4].to_php_string(), b[2].to_php_string());
        assert_eq!(b[5].to_php_string(), b[0].to_php_string());
        // hmac sha256
        assert_eq!(
            b[6].to_php_string(),
            "287a3bd8a4fc7731a94c722079055323644d8798bd291bf9878abc9b8fd4b1d0"
        );
        // randomBytes(16) -> len 16
        assert_eq!(b[8].to_i64(), Some(16));
        // randomHex(32) -> len 32
        assert_eq!(b[9].to_i64(), Some(32));
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_crypt_openssl_encrypt_decrypt() {
    clear_runtime_closure_macros();

    let code = r#"
        $key = "01234567890123456789012345678901"; // 32-byte key
        $iv = "1234567890123456"; // 16-byte IV
        $plaintext = "Sensitive user credit card data";

        $encrypted = Crypt::opensslEncrypt($plaintext, "aes-256-cbc", $key, 0, $iv);
        $decrypted = Crypt::opensslDecrypt($encrypted, "aes-256-cbc", $key, 0, $iv);

        $enc_ctr = Crypt::opensslEncrypt($plaintext, "aes-128-ctr", "16bytessecretkey", 0, $iv);
        $dec_ctr = Crypt::opensslDecrypt($enc_ctr, "aes-128-ctr", "16bytessecretkey", 0, $iv);

        return [$encrypted, $decrypted, $dec_ctr];
    "#;

    let res = run_source(code).expect("OpenSSL encryption/decryption failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_ne!(b[0].to_php_string(), "Sensitive user credit card data");
        assert_eq!(b[1].to_php_string(), "Sensitive user credit card data");
        assert_eq!(b[2].to_php_string(), "Sensitive user credit card data");
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_html_helper_functions() {
    clear_runtime_closure_macros();

    let code = r#"
        $orig = '<script>alert("XSS & \'quote\'");</script>';
        $escaped = Html::specialChars($orig);
        $unescaped = Html::specialCharsDecode($escaped);

        $tagged = '<p>Hello <b>World</b>! <a href="https://example.com">Visit</a></p>';
        $stripped = Html::stripTags($tagged);
        $stripped_allowed = Html::stripTags($tagged, ['b']);

        $accented = "Café & Bakery © 2026";
        $entities = Html::entities($accented);
        $decoded_entities = Html::entityDecode($entities);

        $tag_html = Html::tag('button', ['class' => 'btn btn-primary', 'disabled' => true], 'Submit');
        $tag_void = Html::tag('img', ['src' => '/logo.png', 'alt' => 'Logo']);

        return [
            $escaped,
            $unescaped,
            $stripped,
            $stripped_allowed,
            $entities,
            $decoded_entities,
            $tag_html,
            $tag_void,
        ];
    "#;

    let res = run_source(code).expect("Html helper execution failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(
            b[0].to_php_string(),
            "&lt;script&gt;alert(&quot;XSS &amp; &#039;quote&#039;&quot;);&lt;/script&gt;"
        );
        assert_eq!(
            b[1].to_php_string(),
            "<script>alert(\"XSS & 'quote'\");</script>"
        );
        assert_eq!(b[2].to_php_string(), "Hello World! Visit");
        assert_eq!(b[3].to_php_string(), "Hello <b>World</b>! Visit");
        assert!(b[4].to_php_string().contains("&copy;"));
        assert!(b[4].to_php_string().contains("&eacute;"));
        assert_eq!(b[5].to_php_string(), "Café & Bakery © 2026");
        assert_eq!(
            b[6].to_php_string(),
            "<button class=\"btn btn-primary\" disabled>Submit</button>"
        );
        assert_eq!(b[7].to_php_string(), "<img src=\"/logo.png\" alt=\"Logo\" />");
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_url_helper_functions() {
    clear_runtime_closure_macros();

    let code = r#"
        $u = "https://admin:pass123@rhp.dev:8080/docs/helpers?version=2.0&lang=en#section-crypt";
        $parsed = Url::parseUrl($u);

        $host = Url::parseUrl($u, 1); // PHP_URL_HOST
        $port = Url::parseUrl($u, 2); // PHP_URL_PORT
        $path = Url::parseUrl($u, 5); // PHP_URL_PATH

        $query_data = [
            'search' => 'crypt & html',
            'page' => 1,
            'filters' => ['status' => 'active', 'tags' => 'php,rust'],
        ];
        $query_str = Url::httpBuildQuery($query_data);

        $raw = "Hello from RHP!";
        $b64 = Url::base64Encode($raw);
        $b64_dec = Url::base64Decode($b64);

        $url_enc = Url::encode("hello world & foo=bar");
        $url_dec = Url::decode($url_enc);

        $route_to = Url::to("dashboard");

        return [
            $parsed,
            $host,
            $port,
            $path,
            $query_str,
            $b64,
            $b64_dec,
            $url_enc,
            $url_dec,
            $route_to,
        ];
    "#;

    let res = run_source(code).expect("Url helper execution failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        if let PhpValue::Dict(d) = &b[0] {
            let m = d.borrow();
            assert_eq!(m.get("scheme").unwrap().to_php_string(), "https");
            assert_eq!(m.get("host").unwrap().to_php_string(), "rhp.dev");
            assert_eq!(m.get("port").unwrap().to_i64(), Some(8080));
            assert_eq!(m.get("user").unwrap().to_php_string(), "admin");
            assert_eq!(m.get("pass").unwrap().to_php_string(), "pass123");
            assert_eq!(m.get("path").unwrap().to_php_string(), "/docs/helpers");
            assert_eq!(m.get("query").unwrap().to_php_string(), "version=2.0&lang=en");
            assert_eq!(m.get("fragment").unwrap().to_php_string(), "section-crypt");
        } else {
            panic!("Expected dict for parsed URL");
        }

        assert_eq!(b[1].to_php_string(), "rhp.dev");
        assert_eq!(b[2].to_i64(), Some(8080));
        assert_eq!(b[3].to_php_string(), "/docs/helpers");

        assert!(b[4].to_php_string().contains("search=crypt+%26+html"));
        assert!(b[4].to_php_string().contains("filters%5Bstatus%5D=active"));

        assert_eq!(b[5].to_php_string(), "SGVsbG8gZnJvbSBSSFAh");
        assert_eq!(b[6].to_php_string(), "Hello from RHP!");

        assert_eq!(b[7].to_php_string(), "hello+world+%26+foo%3Dbar");
        assert_eq!(b[8].to_php_string(), "hello world & foo=bar");
        assert_eq!(b[9].to_php_string(), "https://example.com/dashboard");
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_legacy_php_functions_bridge() {
    clear_runtime_closure_macros();

    let code = r#"
        $h = md5("test");
        $s = sha1("test");
        $sh = hash("sha256", "test");
        $sh_hmac = hash_hmac("sha256", "test", "key");

        $html = htmlspecialchars("<hello>");
        $plain = strip_tags("<p>content</p>");
        $b64 = base64_encode("test");
        $b64_d = base64_decode($b64);
        $q = http_build_query(['a' => 1, 'b' => 'two']);

        return [$h, $s, $sh, $sh_hmac, $html, $plain, $b64, $b64_d, $q];
    "#;

    let res = run_source(code).expect("Legacy PHP function execution failed");
    if let PhpValue::Array(arr) | PhpValue::List(arr) = res {
        let b = arr.borrow();
        assert_eq!(b[0].to_php_string(), "098f6bcd4621d373cade4e832627b4f6");
        assert_eq!(b[1].to_php_string(), "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3");
        assert_eq!(b[2].to_php_string(), "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08");
        assert_eq!(
            b[3].to_php_string(),
            "02afb56304902c656fcb737cdd03de6205bb6d401da2812efd9b2d36a08af159"
        );
        assert_eq!(b[4].to_php_string(), "&lt;hello&gt;");
        assert_eq!(b[5].to_php_string(), "content");
        assert_eq!(b[6].to_php_string(), "dGVzdA==");
        assert_eq!(b[7].to_php_string(), "test");
        assert_eq!(b[8].to_php_string(), "a=1&b=two");
    } else {
        panic!("Expected array result");
    }
}

#[test]
fn test_facade_instantiation_and_inheritance_forbidden() {
    // Cannot instantiate Crypt
    assert!(run_source("$c = new Crypt();").is_err());
    // Cannot instantiate Html
    assert!(run_source("$h = new Html();").is_err());
    // Cannot instantiate Url
    assert!(run_source("$u = new Url();").is_err());

    // Cannot extend Crypt
    assert!(run_source("class SubCrypt extends Crypt {}").is_err());
    // Cannot extend Html
    assert!(run_source("class SubHtml extends Html {}").is_err());
    // Cannot extend Url
    assert!(run_source("class SubUrl extends Url {}").is_err());
}
