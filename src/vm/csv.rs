use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex};

use crate::vm::error::RuntimeErrorKind;
use crate::vm::facade::{extract_array_elements, FacadeResult};
use crate::vm::value::PhpValue;

pub enum StreamSource {
    File {
        file: Option<File>,
        buffer: Vec<u8>,
        buf_pos: usize,
    },
    Memory(std::io::Cursor<Vec<u8>>),
    Socket {
        listener: Option<std::net::TcpListener>,
        local_addr: String,
        peer_addr: Option<String>,
    },
}

impl StreamSource {
    pub fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        match self {
            StreamSource::Socket { .. } => {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "Cannot read bytes directly from listening socket"))
            }
            StreamSource::Memory(cursor) => {
                let mut buf = [0u8; 1];
                let n = cursor.read(&mut buf)?;
                if n == 0 {
                    Ok(None)
                } else {
                    Ok(Some(buf[0]))
                }
            }
            StreamSource::File {
                file,
                buffer,
                buf_pos,
            } => {
                if *buf_pos >= buffer.len() {
                    buffer.resize(8192, 0);
                    let f = match file.as_mut() {
                        Some(f) => f,
                        None => return Ok(None),
                    };
                    let n = f.read(buffer)?;
                    buffer.truncate(n);
                    *buf_pos = 0;
                    if n == 0 {
                        return Ok(None);
                    }
                }
                let byte = buffer[*buf_pos];
                *buf_pos += 1;
                Ok(Some(byte))
            }
        }
    }

    pub fn read_bytes(&mut self, len: usize) -> std::io::Result<Vec<u8>> {
        let mut result = Vec::with_capacity(len.min(65536));
        for _ in 0..len {
            match self.read_byte()? {
                Some(b) => result.push(b),
                None => break,
            }
        }
        Ok(result)
    }

    pub fn unread_byte(&mut self, byte: u8) {
        match self {
            StreamSource::Memory(cursor) => {
                let pos = cursor.position();
                if pos > 0 {
                    cursor.set_position(pos - 1);
                }
            }
            StreamSource::File {
                buffer,
                buf_pos,
                ..
            } => {
                if *buf_pos > 0 {
                    *buf_pos -= 1;
                } else {
                    buffer.insert(0, byte);
                }
            }
            StreamSource::Socket { .. } => {}
        }
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            StreamSource::Socket { .. } => {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "Cannot write to listening socket"))
            }
            StreamSource::Memory(cursor) => cursor.write_all(bytes),
            StreamSource::File {
                file,
                buffer,
                buf_pos,
            } => {
                let f = match file.as_mut() {
                    Some(f) => f,
                    None => return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Stream closed")),
                };
                if *buf_pos < buffer.len() {
                    let unread = (buffer.len() - *buf_pos) as i64;
                    f.seek(SeekFrom::Current(-unread))?;
                    buffer.clear();
                    *buf_pos = 0;
                }
                f.write_all(bytes)
            }
        }
    }

    pub fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            StreamSource::Socket { .. } => {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "Cannot seek listening socket"))
            }
            StreamSource::Memory(cursor) => cursor.seek(pos),
            StreamSource::File {
                file,
                buffer,
                buf_pos,
            } => {
                let f = match file.as_mut() {
                    Some(f) => f,
                    None => return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Stream closed")),
                };
                if *buf_pos < buffer.len() {
                    let unread = (buffer.len() - *buf_pos) as i64;
                    f.seek(SeekFrom::Current(-unread))?;
                    buffer.clear();
                    *buf_pos = 0;
                }
                f.seek(pos)
            }
        }
    }
}

pub struct StreamResource {
    pub source: Mutex<StreamSource>,
    pub closed: Mutex<bool>,
    pub delete_on_close: Option<std::path::PathBuf>,
}

impl Drop for StreamResource {
    fn drop(&mut self) {
        if let Ok(mut src) = self.source.lock() {
            match &mut *src {
                StreamSource::File { file, .. } => {
                    *file = None;
                }
                StreamSource::Socket { listener, .. } => {
                    *listener = None;
                }
                _ => {}
            }
        }
        if let Some(ref path) = self.delete_on_close {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn get_stream(val: &PhpValue) -> Result<&StreamResource, RuntimeErrorKind> {
    match val {
        PhpValue::Resource(arc) => {
            let any_ref: &(dyn std::any::Any + Send + Sync) = &**arc;
            any_ref.downcast_ref::<StreamResource>().ok_or_else(|| {
                RuntimeErrorKind::TypeError {
                    message: "Supplied resource is not a valid stream resource".to_string(),
                }
            })
        }
        other => Err(RuntimeErrorKind::TypeError {
            message: format!(
                "Argument must be of type resource, {} given",
                other.type_name()
            ),
        }),
    }
}

pub fn open_stream(path: &str, mode: &str) -> Result<PhpValue, RuntimeErrorKind> {
    let clean_mode = mode.replace('b', "").replace('t', "");
    if path == "php://memory" || path == "php://temp" {
        let cursor = std::io::Cursor::new(Vec::new());
        let stream = StreamResource {
            source: Mutex::new(StreamSource::Memory(cursor)),
            closed: Mutex::new(false),
            delete_on_close: None,
        };
        return Ok(PhpValue::Resource(Arc::new(stream)));
    }

    let mut options = std::fs::OpenOptions::new();
    match clean_mode.as_str() {
        "r" => {
            options.read(true);
        }
        "r+" => {
            options.read(true).write(true);
        }
        "w" => {
            options.write(true).create(true).truncate(true);
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
        }
        "a" => {
            options.write(true).create(true).append(true);
        }
        "a+" => {
            options.read(true).write(true).create(true).append(true);
        }
        "x" => {
            options.write(true).create_new(true);
        }
        "x+" => {
            options.read(true).write(true).create_new(true);
        }
        "c" => {
            options.write(true).create(true);
        }
        "c+" => {
            options.read(true).write(true).create(true);
        }
        _ => {
            options.read(true);
        }
    }

    match options.open(path) {
        Ok(file) => {
            let stream = StreamResource {
                source: Mutex::new(StreamSource::File {
                    file: Some(file),
                    buffer: Vec::new(),
                    buf_pos: 0,
                }),
                closed: Mutex::new(false),
                delete_on_close: None,
            };
            Ok(PhpValue::Resource(Arc::new(stream)))
        }
        Err(_) => Ok(PhpValue::Bool(false)),
    }
}

pub fn close_stream(val: &PhpValue) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    let mut closed = stream.closed.lock().unwrap();
    if *closed {
        return Ok(PhpValue::Bool(false));
    }
    *closed = true;
    let mut src = stream.source.lock().unwrap();
    match &mut *src {
        StreamSource::File { file, buffer, .. } => {
            buffer.clear();
            *file = None;
        }
        StreamSource::Memory(cursor) => {
            cursor.get_mut().clear();
        }
        StreamSource::Socket { listener, .. } => {
            *listener = None;
        }
    }
    drop(src);
    if let Some(ref path) = stream.delete_on_close {
        let _ = std::fs::remove_file(path);
    }
    Ok(PhpValue::Bool(true))
}

pub fn create_socket_stream(listener: std::net::TcpListener, local_addr: String) -> PhpValue {
    let stream = StreamResource {
        source: Mutex::new(StreamSource::Socket {
            listener: Some(listener),
            local_addr,
            peer_addr: None,
        }),
        closed: Mutex::new(false),
        delete_on_close: None,
    };
    PhpValue::Resource(Arc::new(stream))
}

pub fn get_socket_addr(val: &PhpValue, want_peer: bool) -> Result<Option<String>, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    let closed = *stream.closed.lock().unwrap();
    if closed {
        return Ok(None);
    }
    let src = stream.source.lock().unwrap();
    if let StreamSource::Socket { local_addr, peer_addr, .. } = &*src {
        if want_peer {
            Ok(peer_addr.clone())
        } else {
            Ok(Some(local_addr.clone()))
        }
    } else {
        Err(RuntimeErrorKind::TypeError {
            message: "Resource is not a valid socket resource".to_string(),
        })
    }
}

pub fn feof_stream(val: &PhpValue) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    if *stream.closed.lock().unwrap() {
        return Ok(PhpValue::Bool(true));
    }
    let mut src = stream.source.lock().unwrap();
    let is_eof = match &mut *src {
        StreamSource::Memory(cursor) => cursor.position() >= cursor.get_ref().len() as u64,
        StreamSource::File {
            file,
            buffer,
            buf_pos,
        } => {
            if *buf_pos < buffer.len() {
                false
            } else {
                buffer.resize(8192, 0);
                let f = match file.as_mut() {
                    Some(f) => f,
                    None => return Ok(PhpValue::Bool(true)),
                };
                match f.read(buffer) {
                    Ok(0) => {
                        buffer.clear();
                        *buf_pos = 0;
                        true
                    }
                    Ok(n) => {
                        buffer.truncate(n);
                        *buf_pos = 0;
                        false
                    }
                    Err(_) => true,
                }
            }
        }
        StreamSource::Socket { .. } => true,
    };
    Ok(PhpValue::Bool(is_eof))
}

pub fn rewind_stream(val: &PhpValue) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    if *stream.closed.lock().unwrap() {
        return Ok(PhpValue::Bool(false));
    }
    let mut src = stream.source.lock().unwrap();
    match src.seek(SeekFrom::Start(0)) {
        Ok(_) => Ok(PhpValue::Bool(true)),
        Err(_) => Ok(PhpValue::Bool(false)),
    }
}

pub fn write_stream(val: &PhpValue, data: &[u8]) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    if *stream.closed.lock().unwrap() {
        return Ok(PhpValue::Bool(false));
    }
    let mut src = stream.source.lock().unwrap();
    match src.write_all(data) {
        Ok(_) => Ok(PhpValue::Int(data.len() as i64)),
        Err(_) => Ok(PhpValue::Bool(false)),
    }
}

pub fn read_stream(val: &PhpValue, length: usize) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    if *stream.closed.lock().unwrap() {
        return Ok(PhpValue::Bool(false));
    }
    let mut src = stream.source.lock().unwrap();
    match src.read_bytes(length) {
        Ok(bytes) => Ok(PhpValue::string(String::from_utf8_lossy(&bytes).to_string())),
        Err(_) => Ok(PhpValue::Bool(false)),
    }
}

pub fn create_tmpfile_stream() -> Result<PhpValue, RuntimeErrorKind> {
    let (file, path) = match crate::vm::facade::create_unique_temp_file_handle(None, Some("tmp")) {
        Ok(pair) => pair,
        Err(_) => return Ok(PhpValue::Bool(false)),
    };
    let stream = StreamResource {
        source: Mutex::new(StreamSource::File {
            file: Some(file),
            buffer: Vec::new(),
            buf_pos: 0,
        }),
        closed: Mutex::new(false),
        delete_on_close: Some(path),
    };
    Ok(PhpValue::Resource(Arc::new(stream)))
}

pub fn create_memory_stream(initial: &[u8]) -> PhpValue {
    let cursor = std::io::Cursor::new(initial.to_vec());
    let stream = StreamResource {
        source: Mutex::new(StreamSource::Memory(cursor)),
        closed: Mutex::new(false),
        delete_on_close: None,
    };
    PhpValue::Resource(Arc::new(stream))
}

pub fn csv_fgetcsv(
    val: &PhpValue,
    length: Option<usize>,
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    if *stream.closed.lock().unwrap() {
        return Ok(PhpValue::Bool(false));
    }
    let mut src = stream.source.lock().unwrap();

    let mut fields: Vec<PhpValue> = Vec::new();
    let mut current_field: Vec<u8> = Vec::new();
    let mut in_quotes = false;
    let mut field_started = false;
    let mut bytes_read: usize = 0;
    let mut has_read_any_byte = false;

    loop {
        let byte_res = src.read_byte();
        let b = match byte_res {
            Ok(Some(b)) => b,
            Ok(None) => {
                if !has_read_any_byte {
                    return Ok(PhpValue::Bool(false));
                }
                break;
            }
            Err(err) => {
                return Err(RuntimeErrorKind::Custom {
                    message: format!("fgetcsv read error: {}", err),
                });
            }
        };

        has_read_any_byte = true;
        bytes_read += 1;
        let c = b as char;

        if in_quotes {
            if Some(c) == escape && escape.is_some() {
                match src.read_byte() {
                    Ok(Some(next_b)) => {
                        bytes_read += 1;
                        let next_c = next_b as char;
                        if Some(next_c) == enclosure || Some(next_c) == escape {
                            current_field.push(next_b);
                        } else {
                            current_field.push(b);
                            current_field.push(next_b);
                        }
                    }
                    Ok(None) => {
                        current_field.push(b);
                        break;
                    }
                    Err(err) => {
                        return Err(RuntimeErrorKind::Custom {
                            message: format!("fgetcsv read error: {}", err),
                        });
                    }
                }
            } else if Some(c) == enclosure {
                match src.read_byte() {
                    Ok(Some(next_b)) => {
                        let next_c = next_b as char;
                        if Some(next_c) == enclosure {
                            bytes_read += 1;
                            current_field.push(b);
                        } else {
                            in_quotes = false;
                            src.unread_byte(next_b);
                        }
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(err) => {
                        return Err(RuntimeErrorKind::Custom {
                            message: format!("fgetcsv read error: {}", err),
                        });
                    }
                }
            } else {
                current_field.push(b);
            }
        } else {
            if c == '\r' || c == '\n' {
                if c == '\r' {
                    match src.read_byte() {
                        Ok(Some(b'\n')) => {}
                        Ok(Some(other)) => {
                            src.unread_byte(other);
                        }
                        Ok(None) => {}
                        Err(_) => {}
                    }
                }

                if fields.is_empty() && current_field.is_empty() && !field_started {
                    return Ok(PhpValue::list(vec![PhpValue::Null]));
                }

                fields.push(PhpValue::string(
                    String::from_utf8_lossy(&current_field).to_string(),
                ));
                return Ok(PhpValue::list(fields));
            } else if c == separator {
                fields.push(PhpValue::string(
                    String::from_utf8_lossy(&current_field).to_string(),
                ));
                current_field.clear();
                field_started = false;
            } else if Some(c) == enclosure {
                if current_field.is_empty() {
                    in_quotes = true;
                    field_started = true;
                } else {
                    current_field.push(b);
                    field_started = true;
                }
            } else {
                current_field.push(b);
                field_started = true;
            }
        }

        if let Some(limit) = length {
            if limit > 0 && bytes_read >= limit {
                break;
            }
        }
    }

    fields.push(PhpValue::string(
        String::from_utf8_lossy(&current_field).to_string(),
    ));
    Ok(PhpValue::list(fields))
}

pub fn format_csv_line(
    fields: &[PhpValue],
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
    eol: &str,
) -> String {
    let mut out = String::new();
    let enc_char = enclosure.unwrap_or('"');

    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(separator);
        }

        let s = match field {
            PhpValue::Null => String::new(),
            other => other.to_php_string(),
        };

        let needs_enclosure = enclosure.is_some()
            && (s.contains(separator)
                || s.contains(enc_char)
                || s.contains('\n')
                || s.contains('\r')
                || s.contains(' ')
                || s.contains('\t')
                || (escape.is_some() && s.contains(escape.unwrap())));

        if needs_enclosure {
            out.push(enc_char);
            let enc_str = enc_char.to_string();
            let escaped = s.replace(&enc_str, &format!("{}{}", enc_str, enc_str));
            out.push_str(&escaped);
            out.push(enc_char);
        } else {
            out.push_str(&s);
        }
    }

    out.push_str(eol);
    out
}

pub fn csv_fputcsv(
    val: &PhpValue,
    fields_val: &PhpValue,
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
    eol: &str,
) -> Result<PhpValue, RuntimeErrorKind> {
    let stream = get_stream(val)?;
    if *stream.closed.lock().unwrap() {
        return Ok(PhpValue::Bool(false));
    }
    let items = extract_array_elements(fields_val);
    let line = format_csv_line(&items, separator, enclosure, escape, eol);
    let bytes = line.as_bytes();
    let mut src = stream.source.lock().unwrap();
    match src.write_all(bytes) {
        Ok(_) => Ok(PhpValue::Int(bytes.len() as i64)),
        Err(err) => Err(RuntimeErrorKind::Custom {
            message: format!("fputcsv write error: {}", err),
        }),
    }
}

pub fn parse_csv_string(
    csv: &str,
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
) -> Result<PhpValue, RuntimeErrorKind> {
    let stream_val = create_memory_stream(csv.as_bytes());
    let mut rows: Vec<PhpValue> = Vec::new();
    loop {
        let row = csv_fgetcsv(&stream_val, None, separator, enclosure, escape)?;
        match row {
            PhpValue::Bool(false) => break,
            other => rows.push(other),
        }
    }
    Ok(PhpValue::list(rows))
}

pub fn parse_csv_row(
    line: &str,
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
) -> Result<PhpValue, RuntimeErrorKind> {
    let stream_val = create_memory_stream(line.as_bytes());
    let row = csv_fgetcsv(&stream_val, None, separator, enclosure, escape)?;
    match row {
        PhpValue::Bool(false) => Ok(PhpValue::list(Vec::new())),
        other => Ok(other),
    }
}

pub fn generate_csv_string(
    rows: &[PhpValue],
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
    eol: &str,
) -> String {
    let mut out = String::new();
    for row in rows {
        let items = extract_array_elements(row);
        let line = format_csv_line(&items, separator, enclosure, escape, eol);
        out.push_str(&line);
    }
    out
}

pub fn read_csv_file(
    path: &str,
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
) -> Result<PhpValue, RuntimeErrorKind> {
    let stream_val = open_stream(path, "r")?;
    if let PhpValue::Bool(false) = stream_val {
        return Err(RuntimeErrorKind::Custom {
            message: format!("Csv::readFile failed: cannot open file '{}'", path),
        });
    }
    let mut rows: Vec<PhpValue> = Vec::new();
    loop {
        let row = csv_fgetcsv(&stream_val, None, separator, enclosure, escape)?;
        match row {
            PhpValue::Bool(false) => break,
            other => rows.push(other),
        }
    }
    close_stream(&stream_val)?;
    Ok(PhpValue::list(rows))
}

pub fn write_csv_file(
    path: &str,
    rows: &[PhpValue],
    separator: char,
    enclosure: Option<char>,
    escape: Option<char>,
    eol: &str,
) -> Result<PhpValue, RuntimeErrorKind> {
    let stream_val = open_stream(path, "w")?;
    if let PhpValue::Bool(false) = stream_val {
        return Err(RuntimeErrorKind::Custom {
            message: format!("Csv::writeFile failed: cannot open file '{}'", path),
        });
    }
    for row in rows {
        csv_fputcsv(&stream_val, row, separator, enclosure, escape, eol)?;
    }
    close_stream(&stream_val)?;
    Ok(PhpValue::Bool(true))
}

fn parse_sep(val: Option<&PhpValue>) -> char {
    val.and_then(|v| {
        let s = v.to_php_string();
        s.chars().next()
    })
    .unwrap_or(',')
}

fn parse_enc(val: Option<&PhpValue>) -> Option<char> {
    match val {
        None => Some('"'),
        Some(v) => {
            if v.is_null() {
                None
            } else {
                let s = v.to_php_string();
                if s.is_empty() {
                    None
                } else {
                    s.chars().next()
                }
            }
        }
    }
}

fn parse_esc(val: Option<&PhpValue>) -> Option<char> {
    match val {
        None => Some('\\'),
        Some(v) => {
            if v.is_null() {
                None
            } else {
                let s = v.to_php_string();
                if s.is_empty() {
                    None
                } else {
                    s.chars().next()
                }
            }
        }
    }
}

fn parse_eol(val: Option<&PhpValue>) -> String {
    val.map(|v| v.to_php_string()).unwrap_or_else(|| "\n".to_string())
}

pub fn dispatch_csv(method: &str, args: &[PhpValue]) -> FacadeResult {
    match method {
        "fgetcsv" | "readRow" | "getcsv" | "get" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: format!("Csv::{}", method),
                passed: args.len(),
                expected: 1,
            })?;
            let length = args.get(1).and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    v.to_i64().and_then(|n| if n > 0 { Some(n as usize) } else { None })
                }
            });
            let separator = parse_sep(args.get(2));
            let enclosure = parse_enc(args.get(3));
            let escape = parse_esc(args.get(4));
            csv_fgetcsv(stream_val, length, separator, enclosure, escape)
        }
        "fputcsv" | "writeRow" | "putcsv" | "put" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: format!("Csv::{}", method),
                passed: args.len(),
                expected: 2,
            })?;
            let fields_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: format!("Csv::{}", method),
                passed: args.len(),
                expected: 2,
            })?;
            let separator = parse_sep(args.get(2));
            let enclosure = parse_enc(args.get(3));
            let escape = parse_esc(args.get(4));
            let eol = parse_eol(args.get(5));
            csv_fputcsv(stream_val, fields_val, separator, enclosure, escape, &eol)
        }
        "open" => {
            let path_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::open".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let path = path_val.to_php_string();
            let mode = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| "r".to_string());
            open_stream(&path, &mode)
        }
        "close" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::close".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            close_stream(stream_val)
        }
        "feof" | "eof" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::feof".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            feof_stream(stream_val)
        }
        "rewind" => {
            let stream_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::rewind".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            rewind_stream(stream_val)
        }
        "memory" => {
            let initial = args.first().map(|v| v.to_php_string()).unwrap_or_default();
            Ok(create_memory_stream(initial.as_bytes()))
        }
        "parse" | "parseAll" => {
            let content_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::parse".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let content = content_val.to_php_string();
            let separator = parse_sep(args.get(1));
            let enclosure = parse_enc(args.get(2));
            let escape = parse_esc(args.get(3));
            parse_csv_string(&content, separator, enclosure, escape)
        }
        "parseRow" | "str_getcsv" => {
            let line_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::parseRow".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let line = line_val.to_php_string();
            let separator = parse_sep(args.get(1));
            let enclosure = parse_enc(args.get(2));
            let escape = parse_esc(args.get(3));
            parse_csv_row(&line, separator, enclosure, escape)
        }
        "generate" | "stringify" => {
            let rows_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::generate".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let rows = extract_array_elements(rows_val);
            let separator = parse_sep(args.get(1));
            let enclosure = parse_enc(args.get(2));
            let escape = parse_esc(args.get(3));
            let eol = parse_eol(args.get(4));
            let result = generate_csv_string(&rows, separator, enclosure, escape, &eol);
            Ok(PhpValue::string(result))
        }
        "formatRow" => {
            let fields_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::formatRow".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let fields = extract_array_elements(fields_val);
            let separator = parse_sep(args.get(1));
            let enclosure = parse_enc(args.get(2));
            let escape = parse_esc(args.get(3));
            let eol = parse_eol(args.get(4));
            let result = format_csv_line(&fields, separator, enclosure, escape, &eol);
            Ok(PhpValue::string(result))
        }
        "readFile" => {
            let path_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::readFile".to_string(),
                passed: args.len(),
                expected: 1,
            })?;
            let path = path_val.to_php_string();
            let separator = parse_sep(args.get(1));
            let enclosure = parse_enc(args.get(2));
            let escape = parse_esc(args.get(3));
            read_csv_file(&path, separator, enclosure, escape)
        }
        "writeFile" => {
            let path_val = args.first().ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::writeFile".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let rows_val = args.get(1).ok_or_else(|| RuntimeErrorKind::TooFewArguments {
                callable: "Csv::writeFile".to_string(),
                passed: args.len(),
                expected: 2,
            })?;
            let path = path_val.to_php_string();
            let rows = extract_array_elements(rows_val);
            let separator = parse_sep(args.get(2));
            let enclosure = parse_enc(args.get(3));
            let escape = parse_esc(args.get(4));
            let eol = parse_eol(args.get(5));
            write_csv_file(&path, &rows, separator, enclosure, escape, &eol)
        }
        _ => Err(RuntimeErrorKind::UndefinedFacadeMethod {
            facade: "Csv".to_string(),
            method: method.to_string(),
        }),
    }
}
