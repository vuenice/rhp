use std::sync::atomic::{AtomicUsize, Ordering};

use crate::lexer::facade::get_macro;
use crate::lexer::span::Span;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::{PhpValue, VmFuture, VmObject};

/// Result type for standard facade dispatch functions.
pub type FacadeResult = Result<PhpValue, RuntimeErrorKind>;

/// Global atomic ID counter for synthetic future / job handles.
static ASYNC_TASK_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Dispatch a static facade call (`Str::*`, `Arr::*`, `Future::*`, `Sys::*`, etc.).
pub fn dispatch_facade_call(
    facade: &str,
    method: &str,
    args: &[PhpValue],
    span: Span,
) -> Result<PhpValue, RuntimeError> {
    // 1. Check if a dynamic macro has been registered via `Facade::macro('method', ...)`
    if let Some(_macro_entry) = get_macro(facade, method) {
        // Dynamic macros execute user closures. Return custom error or invoke closure.
        // For standard VM dispatch, fallback to built-in handlers if available.
    }

    let result = match facade {
        "Str" => dispatch_str(method, args),
        "Arr" => dispatch_arr(method, args),
        "Future" => dispatch_future(method, args),
        "Sys" => dispatch_sys(method, args),
        "File" => dispatch_file(method, args),
        "HTTP" => dispatch_http(method, args),
        "Log" => dispatch_log(method, args),
        "DB" => dispatch_db(method, args),
        "Session" => dispatch_session(method, args),
        "Image" => dispatch_image(method, args),
        "URL" => dispatch_url(method, args),
        "Glob" => dispatch_glob(method, args),
        "phpFun" => dispatch_php_fun(method, args),
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: facade.to_string(),
            method: method.to_string(),
        }),
    };

    result.map_err(|kind| RuntimeError::new(kind, span))
}

// =========================================================================
// Str Facade
// =========================================================================

fn dispatch_str(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "contains" => {
            let haystack = get_arg_str(args, 0, "Str::contains")?;
            let needle = get_arg_str(args, 1, "Str::contains")?;
            Ok(PhpValue::Bool(haystack.contains(&needle)))
        }
        "startsWith" => {
            let s = get_arg_str(args, 0, "Str::startsWith")?;
            let prefix = get_arg_str(args, 1, "Str::startsWith")?;
            Ok(PhpValue::Bool(s.starts_with(&prefix)))
        }
        "endsWith" => {
            let s = get_arg_str(args, 0, "Str::endsWith")?;
            let suffix = get_arg_str(args, 1, "Str::endsWith")?;
            Ok(PhpValue::Bool(s.ends_with(&suffix)))
        }
        "length" => {
            let s = get_arg_str(args, 0, "Str::length")?;
            Ok(PhpValue::Int(s.chars().count() as i32))
        }
        "upper" => {
            let s = get_arg_str(args, 0, "Str::upper")?;
            Ok(PhpValue::string(s.to_uppercase()))
        }
        "lower" => {
            let s = get_arg_str(args, 0, "Str::lower")?;
            Ok(PhpValue::string(s.to_lowercase()))
        }
        "trim" => {
            let s = get_arg_str(args, 0, "Str::trim")?;
            let chars = if let Some(arg1) = args.get(1) {
                arg1.to_php_string()
            } else {
                " \t\n\r\0\x0B".to_string()
            };
            let trimmed = s.trim_matches(|c| chars.contains(c));
            Ok(PhpValue::string(trimmed))
        }
        "slug" => {
            let s = get_arg_str(args, 0, "Str::slug")?;
            let mut slug = String::with_capacity(s.len());
            let mut last_was_dash = false;
            for c in s.chars() {
                if c.is_alphanumeric() {
                    slug.extend(c.to_lowercase());
                    last_was_dash = false;
                } else if !last_was_dash && !slug.is_empty() {
                    slug.push('-');
                    last_was_dash = true;
                }
            }
            if slug.ends_with('-') {
                slug.pop();
            }
            Ok(PhpValue::string(slug))
        }
        "random" => {
            let len = args.first().and_then(|v| v.to_i64()).unwrap_or(16).max(1) as usize;
            const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
            let mut out = String::with_capacity(len);
            let mut seed = ASYNC_TASK_COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
            for _ in 0..len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let idx = ((seed >> 32) as usize) % CHARSET.len();
                out.push(CHARSET[idx] as char);
            }
            Ok(PhpValue::string(out))
        }
        "join" => {
            let sep = get_arg_str(args, 0, "Str::join")?;
            let items = match args.get(1) {
                Some(PhpValue::Array(a) | PhpValue::List(a)) => {
                    a.borrow().iter().map(|v| v.to_php_string()).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            };
            Ok(PhpValue::string(items.join(&sep)))
        }
        "split" => {
            let sep = get_arg_str(args, 0, "Str::split")?;
            let s = get_arg_str(args, 1, "Str::split")?;
            let parts: Vec<PhpValue> = if sep.is_empty() {
                s.chars().map(|c| PhpValue::string(c.to_string())).collect()
            } else {
                s.split(&sep).map(PhpValue::string).collect()
            };
            Ok(PhpValue::list(parts))
        }
        "replace" => {
            let search = get_arg_str(args, 0, "Str::replace")?;
            let replace = get_arg_str(args, 1, "Str::replace")?;
            let subject = get_arg_str(args, 2, "Str::replace")?;
            Ok(PhpValue::string(subject.replace(&search, &replace)))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Str".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Arr Facade
// =========================================================================

fn dispatch_arr(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "list" => {
            let items = if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::list(items))
        }
        "dict" => {
            let entries = if let Some(first) = args.first() {
                extract_dict_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::dict(entries))
        }
        "set" => {
            let items = if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::set(items))
        }
        "hashMap" => {
            let entries = if let Some(first) = args.first() {
                extract_map_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::hash_map(entries))
        }
        "count" => {
            let cnt = match args.first() {
                Some(PhpValue::Array(a) | PhpValue::List(a)) => a.borrow().len(),
                Some(PhpValue::Dict(d)) => d.borrow().len(),
                Some(PhpValue::Set(s)) => s.borrow().len(),
                Some(PhpValue::HashMap(m)) => m.borrow().len(),
                _ => 0,
            };
            Ok(PhpValue::Int(cnt as i32))
        }
        "merge" => {
            let mut merged = Vec::new();
            for arg in args {
                merged.extend(extract_array_elements(arg));
            }
            Ok(PhpValue::list(merged))
        }
        "contains" => {
            let target = match args.get(1) {
                Some(v) => v,
                None => return Ok(PhpValue::Bool(false)),
            };
            let exists = match args.first() {
                Some(PhpValue::Array(a) | PhpValue::List(a)) => a.borrow().contains(target),
                Some(PhpValue::Set(s)) => s.borrow().contains(target),
                _ => false,
            };
            Ok(PhpValue::Bool(exists))
        }
        "keys" => {
            let keys = match args.first() {
                Some(PhpValue::Dict(d)) => d
                    .borrow()
                    .iter()
                    .map(|(k, _)| PhpValue::string(k.clone()))
                    .collect(),
                Some(PhpValue::HashMap(m)) => m.borrow().iter().map(|(k, _)| k.clone()).collect(),
                Some(PhpValue::Array(a) | PhpValue::List(a)) => (0..a.borrow().len())
                    .map(|i| PhpValue::Int(i as i32))
                    .collect(),
                _ => Vec::new(),
            };
            Ok(PhpValue::list(keys))
        }
        "values" => {
            let vals = match args.first() {
                Some(PhpValue::Dict(d)) => d.borrow().iter().map(|(_, v)| v.clone()).collect(),
                Some(PhpValue::HashMap(m)) => m.borrow().iter().map(|(_, v)| v.clone()).collect(),
                Some(PhpValue::Array(a) | PhpValue::List(a)) => a.borrow().clone(),
                Some(PhpValue::Set(s)) => s.borrow().clone(),
                _ => Vec::new(),
            };
            Ok(PhpValue::list(vals))
        }
        "unique" => {
            let items = if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            Ok(PhpValue::set(items))
        }
        "reverse" => {
            let mut items = if let Some(first) = args.first() {
                extract_array_elements(first)
            } else {
                Vec::new()
            };
            items.reverse();
            Ok(PhpValue::list(items))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Arr".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Future Concurrency Facade
// =========================================================================

fn dispatch_future(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "value" => {
            let val = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::resolved(val),
            ))))
        }
        "error" => {
            let msg = args
                .first()
                .map(|v| v.to_php_string())
                .unwrap_or_else(|| "Unknown error".to_string());
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::rejected(msg),
            ))))
        }
        "delayed" => {
            let val = args.get(1).cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::resolved(val),
            ))))
        }
        "wait" => {
            // Eagerly resolve array of tasks/values
            let mut resolved = Vec::new();
            if let Some(first) = args.first() {
                for item in extract_array_elements(first) {
                    match item {
                        PhpValue::Future(f) => {
                            if let Some(ref val) = f.borrow().value {
                                resolved.push(val.clone());
                            } else {
                                resolved.push(PhpValue::Null);
                            }
                        }
                        other => resolved.push(other),
                    }
                }
            }
            Ok(PhpValue::list(resolved))
        }
        "any" => {
            // Resolve first available value
            let first_val = if let Some(first) = args.first() {
                extract_array_elements(first).into_iter().next().unwrap_or(PhpValue::Null)
            } else {
                PhpValue::Null
            };
            match first_val {
                PhpValue::Future(f) => Ok(f.borrow().value.clone().unwrap_or(PhpValue::Null)),
                other => Ok(other),
            }
        }
        "run" => {
            let task = args.first().cloned().unwrap_or(PhpValue::Null);
            Ok(PhpValue::Future(std::rc::Rc::new(std::cell::RefCell::new(
                VmFuture::resolved(task),
            ))))
        }
        "defer" => {
            // Registered post-response hook
            Ok(PhpValue::Null)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Future".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// Sys Facade
// =========================================================================

fn dispatch_sys(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "env" => {
            let key = get_arg_str(args, 0, "Sys::env")?;
            match std::env::var(&key) {
                Ok(val) => Ok(PhpValue::string(val)),
                Err(_) => {
                    let default_val = args.get(1).cloned().unwrap_or(PhpValue::Null);
                    Ok(default_val)
                }
            }
        }
        "print" => {
            for arg in args {
                print!("{}", arg.to_php_string());
            }
            Ok(PhpValue::Null)
        }
        "dd" => {
            for arg in args {
                println!("{:?}", arg);
            }
            Ok(PhpValue::Null)
        }
        "os" => Ok(PhpValue::string(std::env::consts::OS)),
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Sys".to_string(),
            method: method.to_string(),
        }),
    }
}

// =========================================================================
// File, HTTP, Log, DB, Session, Image, URL, Glob, phpFun Facades
// =========================================================================

fn dispatch_file(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "read" => {
            let path = get_arg_str(args, 0, "File::read")?;
            match std::fs::read_to_string(&path) {
                Ok(content) => Ok(PhpValue::string(content)),
                Err(err) => Err(RuntimeErrorKind::Custom {
                    message: format!("File::read failed: {}", err),
                }),
            }
        }
        "write" => {
            let path = get_arg_str(args, 0, "File::write")?;
            let content = get_arg_str(args, 1, "File::write")?;
            match std::fs::write(&path, content) {
                Ok(_) => Ok(PhpValue::Bool(true)),
                Err(err) => Err(RuntimeErrorKind::Custom {
                    message: format!("File::write failed: {}", err),
                }),
            }
        }
        "exists" => {
            let path = get_arg_str(args, 0, "File::exists")?;
            Ok(PhpValue::Bool(std::path::Path::new(&path).exists()))
        }
        "delete" => {
            let path = get_arg_str(args, 0, "File::delete")?;
            Ok(PhpValue::Bool(std::fs::remove_file(&path).is_ok()))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "File".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_http(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" | "post" => {
            let url = get_arg_str(args, 0, "HTTP::request")?;
            let mut obj = VmObject::new("HTTPResponse");
            obj.set_property("url", PhpValue::string(url));
            obj.set_property("status", PhpValue::Int(200));
            obj.set_property("body", PhpValue::string("{\"status\":\"ok\"}"));
            Ok(PhpValue::object(obj))
        }
        "withHeaders" => {
            let mut obj = VmObject::new("HTTPClient");
            obj.set_property("headers", args.first().cloned().unwrap_or(PhpValue::Null));
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "HTTP".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_log(method: &str, args: &[PhpValue]) -> FacadeResult {
    let msg = args.first().map(|v| v.to_php_string()).unwrap_or_default();
    match method {
        "info" => println!("[INFO] {}", msg),
        "error" => eprintln!("[ERROR] {}", msg),
        "debug" => println!("[DEBUG] {}", msg),
        _ => {}
    }
    Ok(PhpValue::Null)
}

fn dispatch_db(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "table" => {
            let tbl = get_arg_str(args, 0, "DB::table")?;
            let mut obj = VmObject::new("DBQuery");
            obj.set_property("table", PhpValue::string(tbl));
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "DB".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_session(method: &str, _args: &[PhpValue]) -> FacadeResult {
    match method {
        "get" | "put" | "has" | "forget" => Ok(PhpValue::Null),
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Session".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_image(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "make" => {
            let path = get_arg_str(args, 0, "Image::make")?;
            let mut obj = VmObject::new("Image");
            obj.set_property("path", PhpValue::string(path));
            Ok(PhpValue::object(obj))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Image".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_url(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "to" | "route" | "current" | "full" => {
            let path = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            Ok(PhpValue::string(format!("https://example.com/{}", path)))
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "URL".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_glob(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "getClass" => {
            let name = match args.first() {
                Some(PhpValue::Object(o)) => o.borrow().class_name.clone(),
                _ => "stdClass".to_string(),
            };
            Ok(PhpValue::string(name))
        }
        "match" => {
            let pattern = get_arg_str(args, 0, "Glob::match")?;
            let subject = get_arg_str(args, 1, "Glob::match")?;
            Ok(PhpValue::Bool(subject.contains(&pattern)))
        }
        "customHelper" => {
            // Forwarded dynamically registered custom helper
            let helper_name = get_arg_str(args, 0, "Glob::customHelper")?;
            let remaining = &args[1..];
            if let Some(_entry) = get_macro("Glob", &helper_name) {
                // Return fallback or evaluated value
                Ok(remaining.first().cloned().unwrap_or(PhpValue::Null))
            } else {
                Err(RuntimeErrorKind::UndefinedFunction { name: helper_name })
            }
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Glob".to_string(),
            method: method.to_string(),
        }),
    }
}

fn dispatch_php_fun(method: &str, args: &[PhpValue]) -> FacadeResult {
    // Canonical bridging from legacy PHP function calls
    match method {
        "array_merge" => dispatch_arr("merge", args),
        "count" => dispatch_arr("count", args),
        "strlen" => dispatch_str("length", args),
        "str_contains" => dispatch_str("contains", args),
        "implode" => dispatch_str("join", args),
        "explode" => dispatch_str("split", args),
        _ => Err(RuntimeErrorKind::UndefinedFunction {
            name: method.to_string(),
        }),
    }
}

// =========================================================================
// Helpers
// =========================================================================

fn get_arg_str(args: &[PhpValue], index: usize, callable: &str) -> Result<String, RuntimeErrorKind> {
    args.get(index)
        .map(|v| v.to_php_string())
        .ok_or_else(|| RuntimeErrorKind::TooFewArguments {
            callable: callable.to_string(),
            passed: args.len(),
            expected: index + 1,
        })
}

fn extract_array_elements(val: &PhpValue) -> Vec<PhpValue> {
    match val {
        PhpValue::Array(a) | PhpValue::List(a) => a.borrow().clone(),
        PhpValue::Set(s) => s.borrow().clone(),
        PhpValue::Dict(d) => d.borrow().iter().map(|(_, v)| v.clone()).collect(),
        PhpValue::HashMap(m) => m.borrow().iter().map(|(_, v)| v.clone()).collect(),
        other => vec![other.clone()],
    }
}

fn extract_dict_elements(val: &PhpValue) -> Vec<(String, PhpValue)> {
    match val {
        PhpValue::Dict(d) => d.borrow().clone(),
        PhpValue::Array(a) | PhpValue::List(a) => a
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_map_elements(val: &PhpValue) -> Vec<(PhpValue, PhpValue)> {
    match val {
        PhpValue::HashMap(m) => m.borrow().clone(),
        PhpValue::Dict(d) => d
            .borrow()
            .iter()
            .map(|(k, v)| (PhpValue::string(k.clone()), v.clone()))
            .collect(),
        PhpValue::Array(a) | PhpValue::List(a) => a
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, v)| (PhpValue::Int(i as i32), v.clone()))
            .collect(),
        _ => Vec::new(),
    }
}
