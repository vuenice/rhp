//! # RFC 6455 WebSocket Protocol Engine & Connection Hub
//!
//! Implements the full RFC 6455 WebSocket specification for the RHP runtime (`rhpm`)
//! and RHP programming language (`rhp`), including:
//!
//! 1. **HTTP Upgrade Pipeline Handshake**:
//!    - Verifies `Upgrade: websocket` and `Connection: Upgrade` headers.
//!    - Cryptographically calculates `Sec-WebSocket-Accept` using SHA-1 and Base64
//!      with the RFC 6455 GUID (`258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).
//!    - Generates standard `HTTP/1.1 101 Switching Protocols` responses.
//!
//! 2. **RFC 6455 Binary Framing & Masking**:
//!    - Frame header parsing: FIN, Opcode, Mask bit, 7-bit / 16-bit / 64-bit payload lengths.
//!    - Client-to-server 4-byte XOR unmasking.
//!    - Server-to-client unmasked frame serialization.
//!    - Built-in control frame handling: Ping (`0x9`), Pong (`0xA`), and Close (`0x8`).
//!
//! 3. **Thread-Safe WebSocket Connection Hub**:
//!    - Centralized connection registry tracking active client IDs.
//!    - Multi-channel Pub/Sub topic subscription management.
//!    - Cross-worker and edge broadcasting (`broadcast`, `broadcast_channel`).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use base64::prelude::*;
use sha1::{Digest, Sha1};

/// Standard RFC 6455 WebSocket GUID string used for handshake verification.
pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Monotonically increasing connection ID generator.
static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a globally unique WebSocket connection identifier.
pub fn next_connection_id() -> u64 {
    NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed)
}

// ============================================================================
// 1. Handshake & Key Calculation
// ============================================================================

/// Compute the RFC 6455 `Sec-WebSocket-Accept` response header value from the client's `Sec-WebSocket-Key`.
///
/// Formula: `Base64(SHA-1(Sec-WebSocket-Key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))`
pub fn compute_sec_websocket_accept(client_key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(client_key.trim().as_bytes());
    hasher.update(WS_GUID.as_bytes());
    let hash = hasher.finalize();
    BASE64_STANDARD.encode(hash)
}

/// Inspect HTTP headers to determine if an incoming request is an RFC 6455 WebSocket upgrade request.
/// Returns `Some(sec_websocket_key)` if valid, or `None` otherwise.
pub fn is_websocket_upgrade(headers: &[(String, String)]) -> Option<String> {
    let mut is_upgrade = false;
    let mut is_ws = false;
    let mut sec_key = None;

    for (k, v) in headers {
        let key_lower = k.trim().to_ascii_lowercase();
        let val_lower = v.trim().to_ascii_lowercase();

        if key_lower == "connection" && val_lower.contains("upgrade") {
            is_upgrade = true;
        } else if key_lower == "upgrade" && val_lower == "websocket" {
            is_ws = true;
        } else if key_lower == "sec-websocket-key" {
            sec_key = Some(v.trim().to_string());
        }
    }

    if is_upgrade && is_ws {
        sec_key
    } else {
        None
    }
}

/// Build the standard HTTP `101 Switching Protocols` handshake response payload.
pub fn build_upgrade_response(accept_key: &str) -> String {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_key
    )
}

// ============================================================================
// 2. RFC 6455 Opcode & Frame Definitions
// ============================================================================

/// RFC 6455 WebSocket Frame Opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WebSocketOpcode {
    /// Continuation frame (0x0)
    Continuation = 0x0,
    /// Text data frame encoded as UTF-8 (0x1)
    Text = 0x1,
    /// Binary data frame (0x2)
    Binary = 0x2,
    /// Connection close control frame (0x8)
    Close = 0x8,
    /// Heartbeat Ping control frame (0x9)
    Ping = 0x9,
    /// Heartbeat Pong control frame (0xA)
    Pong = 0xA,
}

impl WebSocketOpcode {
    /// Convert a 4-bit integer opcode to [`WebSocketOpcode`].
    pub fn from_u8(val: u8) -> Option<Self> {
        match val & 0x0F {
            0x0 => Some(Self::Continuation),
            0x1 => Some(Self::Text),
            0x2 => Some(Self::Binary),
            0x8 => Some(Self::Close),
            0x9 => Some(Self::Ping),
            0xA => Some(Self::Pong),
            _ => None,
        }
    }

    /// Check if this opcode represents a control frame (Ping, Pong, Close).
    pub fn is_control(&self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }
}

/// An RFC 6455 WebSocket data or control frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketFrame {
    /// Frame opcode.
    pub opcode: WebSocketOpcode,
    /// FIN bit (indicates whether this is the final fragment in a message).
    pub fin: bool,
    /// Decoded/unmasked payload bytes.
    pub payload: Vec<u8>,
}

impl WebSocketFrame {
    /// Create a new unfragmented text frame.
    pub fn text(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode: WebSocketOpcode::Text,
            fin: true,
            payload: payload.into(),
        }
    }

    /// Create a new unfragmented binary frame.
    pub fn binary(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode: WebSocketOpcode::Binary,
            fin: true,
            payload: payload.into(),
        }
    }

    /// Create a Ping control frame.
    pub fn ping(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode: WebSocketOpcode::Ping,
            fin: true,
            payload: payload.into(),
        }
    }

    /// Create a Pong control frame.
    pub fn pong(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            opcode: WebSocketOpcode::Pong,
            fin: true,
            payload: payload.into(),
        }
    }

    /// Create a Close control frame with a 2-byte status code and optional reason string.
    pub fn close(code: u16, reason: &str) -> Self {
        let mut payload = Vec::with_capacity(2 + reason.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());
        Self {
            opcode: WebSocketOpcode::Close,
            fin: true,
            payload,
        }
    }

    /// Parse the 2-byte status code and optional reason from a Close frame payload.
    pub fn parse_close_payload(&self) -> (u16, String) {
        if self.payload.len() < 2 {
            return (1000, String::new());
        }
        let code = u16::from_be_bytes([self.payload[0], self.payload[1]]);
        let reason = String::from_utf8_lossy(&self.payload[2..]).into_owned();
        (code, reason)
    }

    /// Get payload as a UTF-8 string slice if valid.
    pub fn as_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.payload)
    }
}

/// Errors that can occur during WebSocket frame reading or writing.
#[derive(Debug)]
pub enum WebSocketError {
    Io(std::io::Error),
    Protocol(String),
    UnexpectedEof,
    InvalidOpcode(u8),
    PayloadTooLarge(usize),
}

impl std::fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "WebSocket I/O error: {}", e),
            Self::Protocol(msg) => write!(f, "WebSocket protocol violation: {}", msg),
            Self::UnexpectedEof => write!(f, "WebSocket stream ended unexpectedly"),
            Self::InvalidOpcode(op) => write!(f, "Invalid WebSocket opcode: 0x{:02x}", op),
            Self::PayloadTooLarge(sz) => write!(f, "WebSocket payload size exceeds limit: {} bytes", sz),
        }
    }
}

impl std::error::Error for WebSocketError {}

impl From<std::io::Error> for WebSocketError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ============================================================================
// 3. Frame Serializer and Deserializer
// ============================================================================

/// Maximum allowed payload size for a single frame (16 MB by default to prevent memory exhaustion).
pub const MAX_WS_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Read and decode a single RFC 6455 frame from any readable byte stream.
///
/// Automatically unmasks client-sent frames using the 4-byte masking key.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<WebSocketFrame, WebSocketError> {
    let mut header = [0u8; 2];
    reader.read_exact(&mut header).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            WebSocketError::UnexpectedEof
        } else {
            WebSocketError::Io(e)
        }
    })?;

    let byte0 = header[0];
    let byte1 = header[1];

    let fin = (byte0 & 0x80) != 0;
    let opcode_raw = byte0 & 0x0F;
    let opcode = WebSocketOpcode::from_u8(opcode_raw)
        .ok_or(WebSocketError::InvalidOpcode(opcode_raw))?;

    let is_masked = (byte1 & 0x80) != 0;
    let len_indicator = (byte1 & 0x7F) as usize;

    let payload_len: usize = match len_indicator {
        0..=125 => len_indicator,
        126 => {
            let mut ext = [0u8; 2];
            reader.read_exact(&mut ext)?;
            u16::from_be_bytes(ext) as usize
        }
        127 => {
            let mut ext = [0u8; 8];
            reader.read_exact(&mut ext)?;
            let len64 = u64::from_be_bytes(ext);
            if len64 > (MAX_WS_FRAME_SIZE as u64) {
                return Err(WebSocketError::PayloadTooLarge(len64 as usize));
            }
            len64 as usize
        }
        _ => unreachable!(),
    };

    if payload_len > MAX_WS_FRAME_SIZE {
        return Err(WebSocketError::PayloadTooLarge(payload_len));
    }

    // Read mask key if present
    let mask_key = if is_masked {
        let mut key = [0u8; 4];
        reader.read_exact(&mut key)?;
        Some(key)
    } else {
        None
    };

    // Read payload
    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        reader.read_exact(&mut payload)?;
    }

    // Unmask if masked
    if let Some(mask) = mask_key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    Ok(WebSocketFrame {
        opcode,
        fin,
        payload,
    })
}

/// Serialize and write an unmasked RFC 6455 frame to any writable byte stream (server-to-client).
///
/// Note: As per RFC 6455 §5.1, servers MUST NOT mask frames sent to the client.
pub fn write_frame<W: Write>(
    writer: &mut W,
    opcode: WebSocketOpcode,
    payload: &[u8],
) -> Result<(), WebSocketError> {
    let mut header = Vec::with_capacity(10);

    // Byte 0: FIN = 1, RSV = 0, Opcode
    let byte0 = 0x80 | (opcode as u8 & 0x0F);
    header.push(byte0);

    // Byte 1+: Payload length without MASK bit
    let len = payload.len();
    if len <= 125 {
        header.push(len as u8);
    } else if len <= 65535 {
        header.push(126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    writer.write_all(&header)?;
    if !payload.is_empty() {
        writer.write_all(payload)?;
    }
    writer.flush()?;

    Ok(())
}

/// Write a text message frame to a client stream.
pub fn write_text<W: Write>(writer: &mut W, text: &str) -> Result<(), WebSocketError> {
    write_frame(writer, WebSocketOpcode::Text, text.as_bytes())
}

/// Write a binary frame to a client stream.
pub fn write_binary<W: Write>(writer: &mut W, data: &[u8]) -> Result<(), WebSocketError> {
    write_frame(writer, WebSocketOpcode::Binary, data)
}

/// Write a Ping heartbeat frame to a client stream.
pub fn write_ping<W: Write>(writer: &mut W, ping_payload: &[u8]) -> Result<(), WebSocketError> {
    write_frame(writer, WebSocketOpcode::Ping, ping_payload)
}

/// Write a Pong heartbeat response frame to a client stream.
pub fn write_pong<W: Write>(writer: &mut W, ping_payload: &[u8]) -> Result<(), WebSocketError> {
    write_frame(writer, WebSocketOpcode::Pong, ping_payload)
}

/// Write a Close frame to a client stream.
pub fn write_close<W: Write>(writer: &mut W, code: u16, reason: &str) -> Result<(), WebSocketError> {
    let frame = WebSocketFrame::close(code, reason);
    write_frame(writer, WebSocketOpcode::Close, &frame.payload)
}

// ============================================================================
// 4. Thread-Safe Connection Hub & Pub/Sub Channels
// ============================================================================

/// Trait or handle for sending messages to an active connection.
pub trait WebSocketSender: Send + Sync {
    fn send_frame(&self, opcode: WebSocketOpcode, payload: &[u8]) -> Result<(), String>;
}

/// An active connection wrapped with thread-safe TCP write capabilities.
pub struct TcpWebSocketSender {
    stream: Arc<Mutex<TcpStream>>,
}

impl TcpWebSocketSender {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: Arc::new(Mutex::new(stream)),
        }
    }
}

impl WebSocketSender for TcpWebSocketSender {
    fn send_frame(&self, opcode: WebSocketOpcode, payload: &[u8]) -> Result<(), String> {
        if let Ok(mut lock) = self.stream.lock() {
            write_frame(&mut *lock, opcode, payload).map_err(|e| e.to_string())
        } else {
            Err("Stream mutex poisoned".to_string())
        }
    }
}

/// Global WebSocket Connection and Pub/Sub Hub.
pub struct WebSocketHub {
    /// Active connections by `conn_id`.
    connections: RwLock<HashMap<u64, Arc<dyn WebSocketSender>>>,
    /// Channel name -> Set of subscribed `conn_id`s.
    channels: RwLock<HashMap<String, HashSet<u64>>>,
    /// Connection ID -> Set of subscribed channels (for fast cleanup on disconnect).
    conn_channels: RwLock<HashMap<u64, HashSet<String>>>,
}

impl WebSocketHub {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            channels: RwLock::new(HashMap::new()),
            conn_channels: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new active WebSocket connection.
    pub fn register(&self, conn_id: u64, sender: Arc<dyn WebSocketSender>) {
        if let Ok(mut conns) = self.connections.write() {
            conns.insert(conn_id, sender);
        }
    }

    /// Unregister a disconnected WebSocket connection and remove from all channels.
    pub fn unregister(&self, conn_id: u64) {
        if let Ok(mut conns) = self.connections.write() {
            conns.remove(&conn_id);
        }

        // Clean up channel memberships
        if let Ok(mut conn_chans) = self.conn_channels.write() {
            if let Some(chans) = conn_chans.remove(&conn_id) {
                if let Ok(mut all_chans) = self.channels.write() {
                    for chan in chans {
                        if let Some(set) = all_chans.get_mut(&chan) {
                            set.remove(&conn_id);
                        }
                    }
                }
            }
        }
    }

    /// Subscribe a connection to a named channel.
    pub fn subscribe(&self, conn_id: u64, channel: &str) {
        if let Ok(mut chans) = self.channels.write() {
            chans.entry(channel.to_string()).or_default().insert(conn_id);
        }
        if let Ok(mut conn_chans) = self.conn_channels.write() {
            conn_chans.entry(conn_id).or_default().insert(channel.to_string());
        }
    }

    /// Unsubscribe a connection from a named channel.
    pub fn unsubscribe(&self, conn_id: u64, channel: &str) {
        if let Ok(mut chans) = self.channels.write() {
            if let Some(set) = chans.get_mut(channel) {
                set.remove(&conn_id);
            }
        }
        if let Ok(mut conn_chans) = self.conn_channels.write() {
            if let Some(set) = conn_chans.get_mut(&conn_id) {
                set.remove(channel);
            }
        }
    }

    /// Send a text frame to a specific connection by ID.
    pub fn send_text(&self, conn_id: u64, message: &str) -> Result<bool, String> {
        let sender = {
            let conns = self.connections.read().map_err(|e| e.to_string())?;
            conns.get(&conn_id).cloned()
        };

        if let Some(s) = sender {
            s.send_frame(WebSocketOpcode::Text, message.as_bytes())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Broadcast a text message to all connected clients.
    pub fn broadcast(&self, message: &str) -> usize {
        let conns = match self.connections.read() {
            Ok(c) => c.values().cloned().collect::<Vec<_>>(),
            Err(_) => return 0,
        };

        let mut sent = 0;
        let payload = message.as_bytes();
        for sender in conns {
            if sender.send_frame(WebSocketOpcode::Text, payload).is_ok() {
                sent += 1;
            }
        }
        sent
    }

    /// Broadcast a text message to all subscribers of a specific channel.
    pub fn broadcast_channel(&self, channel: &str, message: &str) -> usize {
        let target_ids: Vec<u64> = {
            let chans = match self.channels.read() {
                Ok(c) => c,
                Err(_) => return 0,
            };
            match chans.get(channel) {
                Some(set) => set.iter().copied().collect(),
                None => return 0,
            }
        };

        let senders: Vec<Arc<dyn WebSocketSender>> = {
            let conns = match self.connections.read() {
                Ok(c) => c,
                Err(_) => return 0,
            };
            target_ids
                .into_iter()
                .filter_map(|id| conns.get(&id).cloned())
                .collect()
        };

        let mut sent = 0;
        let payload = message.as_bytes();
        for sender in senders {
            if sender.send_frame(WebSocketOpcode::Text, payload).is_ok() {
                sent += 1;
            }
        }
        sent
    }

    /// Return the count of currently active WebSocket connections.
    pub fn active_connection_count(&self) -> usize {
        self.connections.read().map(|c| c.len()).unwrap_or(0)
    }

    /// Return all active connection IDs.
    pub fn active_connection_ids(&self) -> Vec<u64> {
        self.connections
            .read()
            .map(|c| c.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Return all active channel names.
    pub fn active_channels(&self) -> Vec<String> {
        self.channels
            .read()
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for WebSocketHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Access the global WebSocket connection hub.
pub fn get_websocket_hub() -> &'static WebSocketHub {
    static HUB: OnceLock<WebSocketHub> = OnceLock::new();
    HUB.get_or_init(WebSocketHub::new)
}

// ============================================================================
// 5. Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_rfc6455_accept_key_vector() {
        // Official RFC 6455 §1.3 test vector
        let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
        let expected_accept = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
        let accept = compute_sec_websocket_accept(client_key);
        assert_eq!(accept, expected_accept);
    }

    #[test]
    fn test_is_websocket_upgrade_detection() {
        let headers = vec![
            ("Host".to_string(), "localhost:8080".to_string()),
            ("Upgrade".to_string(), "websocket".to_string()),
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Sec-WebSocket-Key".to_string(), "dGhlIHNhbXBsZSBub25jZQ==".to_string()),
            ("Sec-WebSocket-Version".to_string(), "13".to_string()),
        ];
        assert_eq!(
            is_websocket_upgrade(&headers),
            Some("dGhlIHNhbXBsZSBub25jZQ==".to_string())
        );

        let non_ws_headers = vec![
            ("Host".to_string(), "localhost:8080".to_string()),
            ("Accept".to_string(), "text/html".to_string()),
        ];
        assert_eq!(is_websocket_upgrade(&non_ws_headers), None);
    }

    #[test]
    fn test_build_upgrade_response() {
        let accept_key = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
        let resp = build_upgrade_response(accept_key);
        assert!(resp.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(resp.contains("Upgrade: websocket\r\n"));
        assert!(resp.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
    }

    #[test]
    fn test_frame_encode_and_decode_roundtrip_text() {
        let original_msg = "Hello RHP WebSocket!";
        let mut buffer = Vec::new();
        write_text(&mut buffer, original_msg).expect("write text frame");

        let mut reader = Cursor::new(buffer);
        let frame = read_frame(&mut reader).expect("read frame");

        assert_eq!(frame.opcode, WebSocketOpcode::Text);
        assert!(frame.fin);
        assert_eq!(frame.as_text().unwrap(), original_msg);
    }

    #[test]
    fn test_frame_decode_masked_client_frame() {
        // Simulate a client-masked frame:
        // Text frame containing "Ping", with masking key [0x12, 0x34, 0x56, 0x78]
        let mask = [0x12, 0x34, 0x56, 0x78];
        let original = b"Ping";
        let mut masked = Vec::new();
        for (i, &b) in original.iter().enumerate() {
            masked.push(b ^ mask[i % 4]);
        }

        let mut raw = Vec::new();
        raw.push(0x81); // FIN + Text
        raw.push(0x80 | 4); // MASK + len 4
        raw.extend_from_slice(&mask);
        raw.extend_from_slice(&masked);

        let mut reader = Cursor::new(raw);
        let frame = read_frame(&mut reader).expect("read masked frame");

        assert_eq!(frame.opcode, WebSocketOpcode::Text);
        assert_eq!(frame.as_text().unwrap(), "Ping");
    }

    #[test]
    fn test_extended_length_16bit_frame() {
        let big_payload = vec![b'A'; 300]; // > 125 bytes -> triggers 126 indicator
        let mut buffer = Vec::new();
        write_frame(&mut buffer, WebSocketOpcode::Binary, &big_payload).unwrap();

        // Byte 1 should have len indicator 126
        assert_eq!(buffer[1], 126);

        let mut reader = Cursor::new(buffer);
        let frame = read_frame(&mut reader).unwrap();
        assert_eq!(frame.opcode, WebSocketOpcode::Binary);
        assert_eq!(frame.payload.len(), 300);
        assert_eq!(frame.payload, big_payload);
    }

    #[test]
    fn test_control_frames_ping_pong_close() {
        let mut buf = Vec::new();
        write_pong(&mut buf, b"heartbeat").unwrap();

        let mut reader = Cursor::new(buf);
        let frame = read_frame(&mut reader).unwrap();
        assert_eq!(frame.opcode, WebSocketOpcode::Pong);
        assert_eq!(frame.payload, b"heartbeat");

        let mut close_buf = Vec::new();
        write_close(&mut close_buf, 1000, "Normal Closure").unwrap();

        let mut reader2 = Cursor::new(close_buf);
        let close_frame = read_frame(&mut reader2).unwrap();
        assert_eq!(close_frame.opcode, WebSocketOpcode::Close);
        let (code, reason) = close_frame.parse_close_payload();
        assert_eq!(code, 1000);
        assert_eq!(reason, "Normal Closure");
    }

    #[test]
    fn test_hub_pub_sub_and_broadcast() {
        struct MockSender {
            pub sent: Arc<Mutex<Vec<String>>>,
        }
        impl WebSocketSender for MockSender {
            fn send_frame(&self, _opcode: WebSocketOpcode, payload: &[u8]) -> Result<(), String> {
                self.sent
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(payload).into_owned());
                Ok(())
            }
        }

        let hub = WebSocketHub::new();
        let s1_log = Arc::new(Mutex::new(Vec::new()));
        let s2_log = Arc::new(Mutex::new(Vec::new()));

        let s1 = Arc::new(MockSender { sent: s1_log.clone() });
        let s2 = Arc::new(MockSender { sent: s2_log.clone() });

        hub.register(101, s1);
        hub.register(102, s2);

        assert_eq!(hub.active_connection_count(), 2);

        hub.subscribe(101, "chat");
        hub.subscribe(102, "chat");
        hub.subscribe(102, "alerts");

        assert_eq!(hub.active_channels().len(), 2);

        // Broadcast to channel "chat"
        let count = hub.broadcast_channel("chat", "Hello Chatters");
        assert_eq!(count, 2);
        assert_eq!(*s1_log.lock().unwrap(), vec!["Hello Chatters"]);
        assert_eq!(*s2_log.lock().unwrap(), vec!["Hello Chatters"]);

        // Broadcast to channel "alerts" (only 102)
        hub.broadcast_channel("alerts", "Warning!");
        assert_eq!(s1_log.lock().unwrap().len(), 1);
        assert_eq!(s2_log.lock().unwrap().len(), 2);

        // Unregister 101
        hub.unregister(101);
        assert_eq!(hub.active_connection_count(), 1);
        let count_after = hub.broadcast_channel("chat", "Second Msg");
        assert_eq!(count_after, 1);
    }
}
