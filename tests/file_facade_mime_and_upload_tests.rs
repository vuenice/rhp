use rhp::vm::facade::{clear_request_context, create_uploaded_file_object, init_request_files};
use rhp::{run_source, PhpValue};
use std::fs::File;
use std::io::Write;

#[test]
fn test_file_mime_type_magic_bytes_detection() {
    let pid = std::process::id();
    let dir = std::env::temp_dir();

    // 1. PNG file
    let png_path = dir.join(format!("rhp_test_{}_sample.png", pid));
    {
        let mut f = File::create(&png_path).unwrap();
        f.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00]).unwrap();
    }

    // 2. JPEG file
    let jpg_path = dir.join(format!("rhp_test_{}_sample.jpg", pid));
    {
        let mut f = File::create(&jpg_path).unwrap();
        f.write_all(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46]).unwrap();
    }

    // 3. GIF file
    let gif_path = dir.join(format!("rhp_test_{}_sample.gif", pid));
    {
        let mut f = File::create(&gif_path).unwrap();
        f.write_all(b"GIF89a\x01\x00\x01\x00").unwrap();
    }

    // 4. WebP file
    let webp_path = dir.join(format!("rhp_test_{}_sample.webp", pid));
    {
        let mut f = File::create(&webp_path).unwrap();
        f.write_all(b"RIFF\x20\x00\x00\x00WEBPVP8 ").unwrap();
    }

    // 5. PDF file
    let pdf_path = dir.join(format!("rhp_test_{}_sample.pdf", pid));
    {
        let mut f = File::create(&pdf_path).unwrap();
        f.write_all(b"%PDF-1.7\n1 0 obj\n<<>>\nendobj").unwrap();
    }

    // 6. JSON file
    let json_path = dir.join(format!("rhp_test_{}_sample.json", pid));
    {
        let mut f = File::create(&json_path).unwrap();
        f.write_all(b"{\"status\": \"ok\", \"code\": 200}").unwrap();
    }

    // 7. Text file
    let txt_path = dir.join(format!("rhp_test_{}_sample.txt", pid));
    {
        let mut f = File::create(&txt_path).unwrap();
        f.write_all(b"Hello world from RHP high-performance engine!").unwrap();
    }

    let source = format!(
        r#"
        $png = File::mimeType("{}");
        $jpg = File::mimeType("{}");
        $gif = File::mimeType("{}");
        $webp = File::mimeType("{}");
        $pdf = File::mimeType("{}");
        $json = File::mimeType("{}");
        $txt = File::mimeType("{}");
        $nonExistent = File::mimeType("/path/to/nonexistent/file_12345.xyz");

        return [
            $png,
            $jpg,
            $gif,
            $webp,
            $pdf,
            $json,
            $txt,
            $nonExistent,
        ];
        "#,
        png_path.to_str().unwrap(),
        jpg_path.to_str().unwrap(),
        gif_path.to_str().unwrap(),
        webp_path.to_str().unwrap(),
        pdf_path.to_str().unwrap(),
        json_path.to_str().unwrap(),
        txt_path.to_str().unwrap()
    );

    let res = run_source(&source).expect("Script execution failed");
    if let PhpValue::List(list) | PhpValue::Array(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "image/png");
        assert_eq!(items[1].to_php_string(), "image/jpeg");
        assert_eq!(items[2].to_php_string(), "image/gif");
        assert_eq!(items[3].to_php_string(), "image/webp");
        assert_eq!(items[4].to_php_string(), "application/pdf");
        assert_eq!(items[5].to_php_string(), "application/json");
        assert_eq!(items[6].to_php_string(), "text/plain");
        assert_eq!(items[7], PhpValue::Bool(false));
    } else {
        panic!("Expected array return value");
    }

    // Clean up
    let _ = std::fs::remove_file(png_path);
    let _ = std::fs::remove_file(jpg_path);
    let _ = std::fs::remove_file(gif_path);
    let _ = std::fs::remove_file(webp_path);
    let _ = std::fs::remove_file(pdf_path);
    let _ = std::fs::remove_file(json_path);
    let _ = std::fs::remove_file(txt_path);
}

#[test]
fn test_file_guess_extension_and_helpers() {
    let pid = std::process::id();
    let dir = std::env::temp_dir();

    let png_path = dir.join(format!("rhp_test_guess_{}.png", pid));
    {
        let mut f = File::create(&png_path).unwrap();
        f.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
    }

    let source = format!(
        r#"
        // 1. Guess from existing file
        $extFromFile = File::guessExtension("{}");

        // 2. Guess from MIME strings
        $extPng = File::guessExtension("image/png");
        $extJpg = File::guessExtension("image/jpeg");
        $extJson = File::guessExtension("application/json");
        $extPdf = File::guessExtension("application/pdf");
        $extUnknown = File::guessExtension("unknown/type");

        // 3. String-based extension & path helpers
        $ext = File::extension("/home/user/document.backup.tar.gz");
        $name = File::name("/home/user/document.backup.tar.gz");
        $base = File::basename("/home/user/document.backup.tar.gz");
        $dir = File::dirname("/home/user/document.backup.tar.gz");

        // 4. File inspection
        $isFile = File::isFile("{}");
        $isDir = File::isDirectory("/tmp");
        $size = File::size("{}");
        $type = File::type("{}");

        return [
            $extFromFile,
            $extPng,
            $extJpg,
            $extJson,
            $extPdf,
            $extUnknown,
            $ext,
            $name,
            $base,
            $dir,
            $isFile,
            $isDir,
            $size,
            $type,
        ];
        "#,
        png_path.to_str().unwrap(),
        png_path.to_str().unwrap(),
        png_path.to_str().unwrap(),
        png_path.to_str().unwrap()
    );

    let res = run_source(&source).expect("Script execution failed");
    if let PhpValue::List(list) | PhpValue::Array(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "png");
        assert_eq!(items[1].to_php_string(), "png");
        assert_eq!(items[2].to_php_string(), "jpeg");
        assert_eq!(items[3].to_php_string(), "json");
        assert_eq!(items[4].to_php_string(), "pdf");
        assert_eq!(items[5], PhpValue::Null);
        assert_eq!(items[6].to_php_string(), "gz");
        assert_eq!(items[7].to_php_string(), "document.backup.tar");
        assert_eq!(items[8].to_php_string(), "document.backup.tar.gz");
        assert_eq!(items[9].to_php_string(), "/home/user");
        assert_eq!(items[10], PhpValue::Bool(true));
        assert_eq!(items[11], PhpValue::Bool(true));
        assert_eq!(items[12], PhpValue::Int(8));
        assert_eq!(items[13].to_php_string(), "file");
    } else {
        panic!("Expected array return value");
    }

    let _ = std::fs::remove_file(png_path);
}

#[test]
fn test_legacy_mime_content_type_and_filesize_lowering() {
    let pid = std::process::id();
    let dir = std::env::temp_dir();

    let sample_path = dir.join(format!("rhp_test_legacy_{}.txt", pid));
    {
        let mut f = File::create(&sample_path).unwrap();
        f.write_all(b"sample text content").unwrap();
    }

    let source = format!(
        r#"
        // Legacy PHP builtins lowered to File facade
        $mime = mime_content_type("{}");
        $sz = filesize("{}");
        $isF = is_file("{}");
        $isD = is_dir("/tmp");

        return [
            $mime,
            $sz,
            $isF,
            $isD,
        ];
        "#,
        sample_path.to_str().unwrap(),
        sample_path.to_str().unwrap(),
        sample_path.to_str().unwrap()
    );

    let res = run_source(&source).expect("Script execution failed");
    if let PhpValue::List(list) | PhpValue::Array(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "text/plain");
        assert_eq!(items[1], PhpValue::Int(19));
        assert_eq!(items[2], PhpValue::Bool(true));
        assert_eq!(items[3], PhpValue::Bool(true));
    } else {
        panic!("Expected array return value");
    }

    let _ = std::fs::remove_file(sample_path);
}

#[test]
fn test_uploaded_file_methods_and_file_from_request() {
    clear_request_context();

    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let tmp_upload = dir.join(format!("rhp_upload_{}_tmp.bin", pid));
    {
        let mut f = File::create(&tmp_upload).unwrap();
        // Write PNG bytes to simulate an uploaded avatar
        f.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x01, 0x02, 0x03, 0x04]).unwrap();
    }

    // Create an UploadedFile instance
    let uploaded_obj = create_uploaded_file_object(
        tmp_upload.to_str().unwrap(),
        Some("avatar.png"),
        Some("image/png"),
    );

    // Register into active request files context
    init_request_files(vec![("avatar".to_string(), PhpValue::object(uploaded_obj))]);

    let dest_dir = dir.join(format!("rhp_uploads_{}", pid));

    let source = format!(
        r#"
        // 1. Retrieve uploaded file using File::fromRequest() or Request::file()
        $uploaded = File::fromRequest('avatar');
        $hasFile = Request::hasFile('avatar');
        $reqFile = Request::file('avatar');

        if ($uploaded === null) {{
            return false;
        }}

        // 2. Read metadata and MIME detection
        $origName = $uploaded->clientName();
        $mime = $uploaded->mimeType();
        $clientMime = $uploaded->clientMimeType();
        $ext = $uploaded->extension();
        $guessExt = $uploaded->guessExtension();
        $size = $uploaded->size();
        $isValid = $uploaded->isValid();

        // 3. Move/store to destination directory
        $destDir = "{}";
        $newPath = $uploaded->move($destDir, "avatar_saved.png");
        $newExists = File::exists($newPath);
        $newMime = File::mimeType($newPath);

        return [
            $hasFile,
            $origName,
            $mime,
            $clientMime,
            $ext,
            $guessExt,
            $size,
            $isValid,
            $newExists,
            $newMime,
        ];
        "#,
        dest_dir.to_str().unwrap()
    );

    let res = run_source(&source).expect("Script execution failed");
    if let PhpValue::List(list) | PhpValue::Array(list) = res {
        let items = list.borrow();
        assert_eq!(items[0], PhpValue::Bool(true));
        assert_eq!(items[1].to_php_string(), "avatar.png");
        assert_eq!(items[2].to_php_string(), "image/png");
        assert_eq!(items[3].to_php_string(), "image/png");
        assert_eq!(items[4].to_php_string(), "png");
        assert_eq!(items[5].to_php_string(), "png");
        assert_eq!(items[6], PhpValue::Int(12));
        assert_eq!(items[7], PhpValue::Bool(true));
        assert_eq!(items[8], PhpValue::Bool(true));
        assert_eq!(items[9].to_php_string(), "image/png");
    } else {
        panic!("Expected array return value");
    }

    // Clean up
    let _ = std::fs::remove_dir_all(&dest_dir);
    clear_request_context();
}

#[test]
fn test_afile_mime_type_async() {
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let test_path = dir.join(format!("rhp_test_afile_mime_{}.png", pid));
    {
        let mut f = File::create(&test_path).unwrap();
        f.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
    }

    let source = format!(
        r#"
        $mimeFut = AFile::mimeType("{}");
        $guessFut = AFile::guessExtension("{}");
        $extFut = AFile::extension("{}");
        $typeFut = AFile::type("{}");

        $mime = $mimeFut->wait();
        $guess = $guessFut->wait();
        $ext = $extFut->wait();
        $type = $typeFut->wait();

        return [
            $mime,
            $guess,
            $ext,
            $type,
        ];
        "#,
        test_path.to_str().unwrap(),
        test_path.to_str().unwrap(),
        test_path.to_str().unwrap(),
        test_path.to_str().unwrap()
    );

    let res = run_source(&source).expect("Script execution failed");
    if let PhpValue::List(list) | PhpValue::Array(list) = res {
        let items = list.borrow();
        assert_eq!(items[0].to_php_string(), "image/png");
        assert_eq!(items[1].to_php_string(), "png");
        assert_eq!(items[2].to_php_string(), "png");
        assert_eq!(items[3].to_php_string(), "file");
    } else {
        panic!("Expected array return value");
    }

    let _ = std::fs::remove_file(test_path);
}
