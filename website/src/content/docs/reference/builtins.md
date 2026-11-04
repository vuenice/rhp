---
title: "Built-in Functions"
description: "Reference for intrinsic compiler intrinsics and legacy PHP backward-compatibility function lowering."
section: "Reference"
order: 3
---

## Reserved Compiler Intrinsics

To eliminate global namespace pollution while preserving convenient developer shorthand, the Pratt parser automatically intercepts reserved intrinsics and lowers them to static `Sys::*` facade calls:

| Shorthand Call | Lowered AST Node | Description |
| :--- | :--- | :--- |
| `echo("text", ...$args)` | `Sys::echo("text", ...$args)` | Directly compiles to Rust `print!()` (stdout without trailing newline) |
| `print("text", ...$args)` | `Sys::print("text", ...$args)` | Directly compiles to Rust `println!()` (stdout WITH trailing newline `\n`) |
| `env('KEY', 'default')` | `Sys::env('KEY', 'default')` | Read system environment variable (`$_ENV` is not supported) |
| `dump($val1, $val2)` | `Debug::dump($val1, $val2)` | Pretty print values for human inspection |
| `dd($val1, $val2)` | `Debug::dd($val1, $val2)` | Pretty print values and terminate execution |

> **Formatting Engine & Parameters:**
> Both `Sys::echo` and `Sys::print` directly map to Rust function calls (`print!` and `println!`) with function parameters passed as-is. RHP does **not** support C-style formatting (`%d`, `%s`, `%f`, `%x`, etc.). Use native Rust placeholders such as `{}`, `{:.2}` (floating-point precision), `{:05}` (zero padding), `{:x}` (hexadecimal), and `{:b}` (binary).
>
> **Note on `Sys::println`:**
> There is **no** `Sys::println` function in RHP. Only `echo` (inline, no newline) and `print` (with trailing newline) exist in the language and via the `Sys` facade (`Sys::echo` and `Sys::print`). Calling `Sys::println(...)` produces an `UndefinedFacadeMethod` error.

---

## Helper Classes vs. Unsupported Superglobals

RHP completely drops procedural PHP mutable superglobals in favor of type-safe static helper classes and intrinsics:

| Helper Class / Function | Unsupported Superglobal | Explanation & Idiom |
| :--- | :--- | :--- |
| **`env()`** | **`$_ENV` is not supported** | Read process environment variables with defaults: `env('APP_ENV', 'production')` or `Sys::env()`. |
| **`File::fromRequest()`** | **`$_FILES` is not supported** | Retrieve uploaded files from multi-part HTTP requests: `File::fromRequest('avatar')`. |
| **`Session::get()`** | **`$_SESSION` is not supported** | Access and mutate thread-safe session data: `Session::get('user_id')`, `Session::set('key', $val)`. |
| **`Cookie::get()`** | **`$_COOKIE` is not supported** | Read and manage HTTP cookies: `Cookie::get('token')`, `Cookie::set('name', $val)`. |
| **`Glob::`** | **`$GLOBALS` is also not supported** | Dynamic global helper fallback & class inspection: `Glob::match()`, `Glob::customHelper()`, or lexical `global $var;`. |

---

## Legacy PHP Function Lowering (`phpFun::*`)

For backward compatibility with legacy codebases, standard PHP function invocations are recognized via a compile-time static registry and automatically lowered into corresponding RHP facade methods:

| Legacy Function Call | Lowered Expression | Target Facade Implementation |
| :--- | :--- | :--- |
| `array_merge($a, $b)` | `phpFun::array_merge($a, $b)` | `Arr::merge($a, $b)` |
| `strlen($str)` | `phpFun::strlen($str)` | `Str::byteLength($str)` |
| `strpos($h, $n, $off)` | `phpFun::strpos($h, $n, $off)` | `Str::find($h, $n, $off)` |
| `strcmp($s1, $s2)` | `phpFun::strcmp($s1, $s2)` | `Str::strcmp($s1, $s2)` |
| `strcasecmp($s1, $s2)` | `phpFun::strcasecmp($s1, $s2)` | `Str::strcasecmp($s1, $s2)` |
| `strncmp($s1, $s2, $len)` | `phpFun::strncmp($s1, $s2, $len)` | `Str::strncmp($s1, $s2, $len)` |
| `strncasecmp($s1, $s2, $len)` | `phpFun::strncasecmp($s1, $s2, $len)` | `Str::strncasecmp($s1, $s2, $len)` |
| `implode($glue, $arr)` | `phpFun::implode($glue, $arr)` | `Arr::implode($glue, $arr)` |
| `ctype_digit($str)` | `phpFun::ctype_digit($str)` | `Str::digit($str)` |
| `ctype_alpha($str)` | `phpFun::ctype_alpha($str)` | `Str::alpha($str)` |
| `ctype_alnum($str)` | `phpFun::ctype_alnum($str)` | `Str::alnum($str)` |
| `file_get_contents($path)` | `phpFun::file_get_contents($path)` | `File::read($path)` |
| `file_put_contents($path, $data)` | `phpFun::file_put_contents($path, $data)` | `File::write($path, $data)` |
| `copy($from, $to)` | `phpFun::copy($from, $to)` | `File::copy($from, $to)` |
| `rename($from, $to)` | `phpFun::rename($from, $to)` | `File::rename($from, $to)` |
| `chmod($path, $mode)` | `phpFun::chmod($path, $mode)` | `File::chmod($path, $mode)` (accepts string `"0755"` or `"rwxr-xr-x"`) |
| `chown($path, $user)` | `phpFun::chown($path, $user)` | `File::chown($path, $user)` |
| `mkdir($path, $mode, $recursive)` | `phpFun::mkdir($path, $mode, $recursive)` | `File::mkdir($path, $mode, $recursive)` (accepts string `"0755"` or `"rwxr-xr-x"`) |
| `parse_mode($mode)` | `phpFun::parse_mode($mode)` | `File::parseMode($mode)` |
| `touch($path)` | `phpFun::touch($path)` | `File::touch($path)` |
| `collect($items)` | `phpFun::collect($items)` | Creates an iterable `Collection` instance |
| `md5($str)` | `phpFun::md5($str)` | `Crypt::md5($str)` |
| `sha1($str)` | `phpFun::sha1($str)` | `Crypt::sha1($str)` |
| `hash($algo, $data)` | `phpFun::hash($algo, $data)` | `Crypt::hash($algo, $data)` |
| `hash_hmac($algo, $data, $key)` | `phpFun::hash_hmac($algo, $data, $key)` | `Crypt::hashHmac($algo, $data, $key)` |
| `openssl_encrypt(...)` | `phpFun::openssl_encrypt(...)` | `Crypt::opensslEncrypt(...)` |
| `openssl_decrypt(...)` | `phpFun::openssl_decrypt(...)` | `Crypt::opensslDecrypt(...)` |
| `random_bytes($len)` | `phpFun::random_bytes($len)` | `Crypt::randomBytes($len)` |
| `htmlspecialchars($str)` | `phpFun::htmlspecialchars($str)` | `Html::specialChars($str)` |
| `htmlspecialchars_decode($str)` | `phpFun::htmlspecialchars_decode($str)` | `Html::specialCharsDecode($str)` |
| `strip_tags($str)` | `phpFun::strip_tags($str)` | `Html::stripTags($str)` |
| `htmlentities($str)` | `phpFun::htmlentities($str)` | `Html::entities($str)` |
| `html_entity_decode($str)` | `phpFun::html_entity_decode($str)` | `Html::entityDecode($str)` |
| `parse_url($url)` | `phpFun::parse_url($url)` | `Url::parseUrl($url)` |
| `http_build_query($data)` | `phpFun::http_build_query($data)` | `Url::httpBuildQuery($data)` |
| `base64_encode($str)` | `phpFun::base64_encode($str)` | `Url::base64Encode($str)` |
| `base64_decode($str)` | `phpFun::base64_decode($str)` | `Url::base64Decode($str)` |
| `urlencode($str)` | `phpFun::urlencode($str)` | `Url::encode($str)` |
| `urldecode($str)` | `phpFun::urldecode($str)` | `Url::decode($str)` |
| `rawurlencode($str)` | `phpFun::rawurlencode($str)` | `Url::rawUrlEncode($str)` |
| `rawurldecode($str)` | `phpFun::rawurldecode($str)` | `Url::rawUrlDecode($str)` |
| `var_dump(...$vars)` | `phpFun::var_dump(...)` | `Debug::dump(...$vars)` |
| `dump(...$vars)` | `phpFun::dump(...)` | `Debug::dump(...$vars)` |
| `print_r($val, true)` | `phpFun::print_r(...)` | `Debug::format($val)` |
| `var_export($val, true)` | `phpFun::var_export(...)` | `Debug::format($val)` |
| `json_encode($val)` | `phpFun::json_encode(...)` | `Json::encode($val)` |
| `json_decode($json)` | `phpFun::json_decode(...)` | `Json::decode($json)` |
| `serialize($val)` | `phpFun::serialize(...)` | `Bin::encode($val)` |
| `unserialize($bytes)` | `phpFun::unserialize(...)` | `Bin::decode($bytes)` |
| `is_null($val)` | `phpFun::is_null($val)` | `Validate::null($val)` |
| `is_int($val)` | `phpFun::is_int($val)` | `Validate::int($val)` |
| `is_bool($val)` | `phpFun::is_bool($val)` | `Validate::bool($val)` |
| `is_float($val)` | `phpFun::is_float($val)` | `Validate::float($val)` |
| `is_numeric($val)` | `phpFun::is_numeric($val)` | `Validate::numeric($val)` |
| `is_scalar($val)` | `phpFun::is_scalar($val)` | `Validate::scalar($val)` |
| `is_resource($val)` | `phpFun::is_resource($val)` | `Validate::resource($val)` |
| `is_a($val, $class)` | `phpFun::is_a($val, $class)` | `$val instanceof $class` |
| `gettype($val)` | `phpFun::gettype($val)` | `Debug::type($val)` |
| `getType($val)` | `phpFun::getType($val)` | `Debug::type($val)` |
| `get_debug_type($val)` | `phpFun::get_debug_type($val)` | `Debug::type($val)` |
| `fgetcsv($stream)` | `phpFun::fgetcsv(...)` | `Csv::fgetcsv($stream)` |
| `fputcsv($stream, $fields)` | `phpFun::fputcsv(...)` | `Csv::fputcsv($stream, $fields)` |
| `str_getcsv($str)` | `phpFun::str_getcsv(...)` | `Csv::parseRow($str)` |
| `fopen($path, $mode)` | `phpFun::fopen(...)` | `File::open($path, $mode)` |
| `fclose($stream)` | `phpFun::fclose(...)` | `File::close($stream)` |
| `feof($stream)` | `phpFun::feof(...)` | `File::eof($stream)` |
| `rewind($stream)` | `phpFun::rewind(...)` | `File::rewind($stream)` |
| `tempnam($dir, $prefix)` | `phpFun::tempnam($dir, $prefix)` | `File::tempnam($dir, $prefix)` |
| `tmpfile()` | `phpFun::tmpfile()` | `File::tmpfile()` (auto-deleted on close) |
| `sys_get_temp_dir()` | `phpFun::sys_get_temp_dir()` | `File::sysGetTempDir()` |
| `fwrite($stream, $data)` | `phpFun::fwrite($stream, $data)` | `File::writeStream($stream, $data)` |
| `fread($stream, $len)` | `phpFun::fread($stream, $len)` | `File::readStream($stream, $len)` |
| `clearstatcache(...)` | `phpFun::clearstatcache(...)` | `Sys::clearstatcache(...)` |
| `disk_free_space($path)` | `phpFun::disk_free_space(...)` | `Sys::disk_free_space($path)` |
| `disk_total_space($path)` | `phpFun::disk_total_space(...)` | `Sys::disk_total_space($path)` |
| `uniqid($prefix, $entropy)` | `phpFun::uniqid(...)` | `Uniq::uniqid($prefix, $entropy)` |

> **Architectural Separation: Inspection vs. String Formatting vs. Persistence:**
> RHP deliberately decouples what PHP historically conflated under `var_dump`, `print_r`, and `var_export`:
> - **Human Inspection (`Debug::dump`, `Debug::dd`):** Pretty-prints to console or debug bar.
> - **String Formatting (`Debug::format`):** Returns the diagnostic representation as a string (the `var_export($x, true)` use case). `var_export` is dropped because RHP does not export executable PHP source code.
> - **Data Persistence (`Json::*`, `Bin::*`):** Uses Rust `serde` for readable JSON text or compact, ultra-fast binary cache serialization, with mandatory typed properties automatically deriving `Serialize` without `__set_state` boilerplate.

---

## Dynamic Global Helpers (`Glob::*`)

Legacy `$GLOBALS` is **also not supported** in RHP. Any unrecognized global function call that is not defined in userland is routed through the `Glob` fallback container:

```rust
// Pratt parser lowering fallback:
Expr::static_call("Glob", "customHelper", vec![callee_name, ...args])
```
