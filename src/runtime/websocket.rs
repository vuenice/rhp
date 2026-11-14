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

use crate::runtime::ipc::IpcSender;
use crate::runtime::types::IpcMessage;

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

    /// Parse the 2-byte status code and optional reason from a Close frame payload according to RFC 6455 §5.5.1 and §7.4.
    ///
    /// Returns:
    /// - `Ok(Some((code, reason)))` if a 2-byte code and valid UTF-8 reason are present.
    /// - `Ok(None)` if the payload is empty (0 bytes) indicating an unannotated closure (no status code).
    /// - `Err(WebSocketError)` if the payload is 1 byte, status code is invalid/reserved, or reason is invalid UTF-8.
    pub fn parse_close_payload(&self) -> Result<Option<(u16, String)>, WebSocketError> {
        if self.payload.is_empty() {
            return Ok(None);
        }
        if self.payload.len() == 1 {
            return Err(WebSocketError::Protocol {
                code: 1002,
                reason: "Close frame payload of 1 byte is invalid; status code must be 2 bytes".to_string(),
            });
        }
        let code = u16::from_be_bytes([self.payload[0], self.payload[1]]);
        if !is_valid_close_code(code) {
            return Err(WebSocketError::Protocol {
                code: 1002,
                reason: format!("Invalid or reserved close status code: {}", code),
            });
        }
        let reason = std::str::from_utf8(&self.payload[2..])
            .map_err(|_| WebSocketError::Protocol {
                code: 1007,
                reason: "Close frame reason is not valid UTF-8".to_string(),
            })?
            .to_string();

        Ok(Some((code, reason)))
    }

    /// Extract close code and reason, defaulting empty closures to `(None, "")` or error codes if invalid.
    pub fn close_code_and_reason(&self) -> (Option<u16>, String) {
        match self.parse_close_payload() {
            Ok(Some((code, reason))) => (Some(code), reason),
            Ok(None) => (None, String::new()),
            Err(e) => (Some(e.close_code()), e.to_string()),
        }
    }

    /// Get payload as a UTF-8 string slice if valid.
    pub fn as_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.payload)
    }
}

/// Check if a WebSocket Close status code is allowed over the wire (RFC 6455 §7.4.1/§7.4.2).
pub fn is_valid_close_code(code: u16) -> bool {
    matches!(
        code,
        1000 | 1001 | 1002 | 1003 | 1007 | 1008 | 1009 | 1010 | 1011 | 3000..=4999
    )
}

/// Errors that can occur during WebSocket frame reading or writing.
#[derive(Debug)]
pub enum WebSocketError {
    Io(std::io::Error),
    Protocol { code: u16, reason: String },
    UnexpectedEof,
    InvalidOpcode(u8),
    PayloadTooLarge(usize),
}

impl WebSocketError {
    /// Return the corresponding RFC 6455 Close status code for this error.
    pub fn close_code(&self) -> u16 {
        match self {
            Self::Protocol { code, .. } => *code,
            Self::InvalidOpcode(_) => 1002,
            Self::PayloadTooLarge(_) => 1009,
            Self::UnexpectedEof | Self::Io(_) => 1006,
        }
    }

    /// Check if this error represents a protocol violation that warrants sending a Close frame.
    pub fn is_protocol_error(&self) -> bool {
        matches!(
            self,
            Self::Protocol { .. } | Self::InvalidOpcode(_) | Self::PayloadTooLarge(_)
        )
    }
}

impl std::fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "WebSocket I/O error: {}", e),
            Self::Protocol { code, reason } => {
                write!(f, "WebSocket protocol violation (code {}): {}", code, reason)
            }
            Self::UnexpectedEof => write!(f, "WebSocket stream ended unexpectedly"),
            Self::InvalidOpcode(op) => write!(f, "Invalid WebSocket opcode: 0x{:02x}", op),
            Self::PayloadTooLarge(sz) => {
                write!(f, "WebSocket payload size exceeds limit: {} bytes", sz)
            }
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
// 3. Frame Serializer, Deserializer & Message Assembler
// ============================================================================

/// Maximum allowed payload size for a single frame (16 MB by default to prevent memory exhaustion).
pub const MAX_WS_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Connection role determining masking requirements (RFC 6455 §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRole {
    /// Server reading from client: Client frames MUST be masked.
    Server,
    /// Client reading from server: Server frames MUST NOT be masked.
    Client,
    /// Lenient (for testing/roundtrip): Accepts either masked or unmasked frames.
    Lenient,
}

/// Read and decode a single RFC 6455 frame with specific role masking enforcement.
pub fn read_frame_role<R: Read>(reader: &mut R, role: FrameRole) -> Result<WebSocketFrame, WebSocketError> {
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

    // RFC 6455 §5.2: RSV1, RSV2, RSV3 MUST be 0 unless an extension is negotiated.
    let rsv = byte0 & 0x70;
    if rsv != 0 {
        return Err(WebSocketError::Protocol {
            code: 1002,
            reason: "RSV bits must be 0 when no extension is negotiated".to_string(),
        });
    }

    let fin = (byte0 & 0x80) != 0;
    let opcode_raw = byte0 & 0x0F;
    let opcode = WebSocketOpcode::from_u8(opcode_raw)
        .ok_or(WebSocketError::InvalidOpcode(opcode_raw))?;

    let is_masked = (byte1 & 0x80) != 0;
    let len_indicator = (byte1 & 0x7F) as usize;

    // RFC 6455 §5.5: Control frames MUST NOT be fragmented and payload MUST be <= 125 bytes.
    if opcode.is_control() {
        if !fin {
            return Err(WebSocketError::Protocol {
                code: 1002,
                reason: "Control frames MUST NOT be fragmented (FIN bit must be 1)".to_string(),
            });
        }
        if len_indicator > 125 {
            return Err(WebSocketError::Protocol {
                code: 1002,
                reason: "Control frame payload length MUST be <= 125 bytes".to_string(),
            });
        }
    }

    // RFC 6455 §5.1: Masking verification based on endpoint role
    match role {
        FrameRole::Server => {
            if !is_masked {
                return Err(WebSocketError::Protocol {
                    code: 1002,
                    reason: "Client-to-server frames MUST be masked (RFC 6455 §5.1)".to_string(),
                });
            }
        }
        FrameRole::Client => {
            if is_masked {
                return Err(WebSocketError::Protocol {
                    code: 1002,
                    reason: "Server-to-client frames MUST NOT be masked (RFC 6455 §5.1)".to_string(),
                });
            }
        }
        FrameRole::Lenient => {}
    }

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
            // RFC 6455 §5.2: Most significant bit of 64-bit length MUST be 0
            if (len64 & 0x8000_0000_0000_0000) != 0 {
                return Err(WebSocketError::Protocol {
                    code: 1002,
                    reason: "64-bit payload length MSB must be 0".to_string(),
                });
            }
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

/// Read a client-sent RFC 6455 frame, strictly enforcing masking (Server reader).
pub fn read_client_frame<R: Read>(reader: &mut R) -> Result<WebSocketFrame, WebSocketError> {
    read_frame_role(reader, FrameRole::Server)
}

/// Read a server-sent RFC 6455 frame, strictly enforcing unmasked payloads (Client reader).
pub fn read_server_frame<R: Read>(reader: &mut R) -> Result<WebSocketFrame, WebSocketError> {
    read_frame_role(reader, FrameRole::Client)
}

/// Read and decode a single RFC 6455 frame (lenient masking for tests/utilities).
pub fn read_frame<R: Read>(reader: &mut R) -> Result<WebSocketFrame, WebSocketError> {
    read_frame_role(reader, FrameRole::Lenient)
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

/// Serialize and write a masked RFC 6455 frame to any writable byte stream (client-to-server).
pub fn write_client_frame<W: Write>(
    writer: &mut W,
    opcode: WebSocketOpcode,
    payload: &[u8],
    mask: [u8; 4],
) -> Result<(), WebSocketError> {
    let mut header = Vec::with_capacity(14);

    // Byte 0: FIN = 1, RSV = 0, Opcode
    let byte0 = 0x80 | (opcode as u8 & 0x0F);
    header.push(byte0);

    // Byte 1+: Payload length with MASK bit (0x80)
    let len = payload.len();
    if len <= 125 {
        header.push(0x80 | (len as u8));
    } else if len <= 65535 {
        header.push(0x80 | 126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(0x80 | 127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    header.extend_from_slice(&mask);
    writer.write_all(&header)?;

    let mut masked = payload.to_vec();
    for (i, byte) in masked.iter_mut().enumerate() {
        *byte ^= mask[i % 4];
    }
    if !masked.is_empty() {
        writer.write_all(&masked)?;
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

/// A reassembled WebSocket message ready for application processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketMessage {
    /// Message Opcode (Text, Binary, or Control).
    pub opcode: WebSocketOpcode,
    /// Complete unfragmented or reassembled payload bytes.
    pub payload: Vec<u8>,
}

impl WebSocketMessage {
    /// Return the payload as a UTF-8 string slice if valid.
    pub fn as_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.payload)
    }
}

/// RFC 6455 Frame Assembler for reassembling fragmented messages and validating UTF-8 (§5.4).
#[derive(Debug, Clone)]
pub struct MessageAssembler {
    current_opcode: Option<WebSocketOpcode>,
    buffer: Vec<u8>,
    max_message_size: usize,
}

impl MessageAssembler {
    pub fn new() -> Self {
        Self {
            current_opcode: None,
            buffer: Vec::new(),
            max_message_size: MAX_WS_FRAME_SIZE,
        }
    }

    pub fn with_max_size(max_message_size: usize) -> Self {
        Self {
            current_opcode: None,
            buffer: Vec::new(),
            max_message_size,
        }
    }

    /// Process a frame according to RFC 6455 §5.4 fragmentation and UTF-8 validation rules.
    pub fn process_frame(&mut self, frame: WebSocketFrame) -> Result<Option<WebSocketMessage>, WebSocketError> {
        // Control frames can be interleaved between fragments (§5.4).
        if frame.opcode.is_control() {
            return Ok(Some(WebSocketMessage {
                opcode: frame.opcode,
                payload: frame.payload,
            }));
        }

        match frame.opcode {
            WebSocketOpcode::Continuation => {
                let opcode = self.current_opcode.ok_or_else(|| WebSocketError::Protocol {
                    code: 1002,
                    reason: "Unexpected continuation frame without prior fragmented message".to_string(),
                })?;

                if self.buffer.len().saturating_add(frame.payload.len()) > self.max_message_size {
                    return Err(WebSocketError::PayloadTooLarge(
                        self.buffer.len() + frame.payload.len(),
                    ));
                }

                self.buffer.extend_from_slice(&frame.payload);

                if frame.fin {
                    self.current_opcode = None;
                    let payload = std::mem::take(&mut self.buffer);
                    if opcode == WebSocketOpcode::Text {
                        std::str::from_utf8(&payload).map_err(|_| WebSocketError::Protocol {
                            code: 1007,
                            reason: "Fragmented text message is not valid UTF-8".to_string(),
                        })?;
                    }
                    Ok(Some(WebSocketMessage { opcode, payload }))
                } else {
                    Ok(None)
                }
            }
            WebSocketOpcode::Text | WebSocketOpcode::Binary => {
                if self.current_opcode.is_some() {
                    return Err(WebSocketError::Protocol {
                        code: 1002,
                        reason: "New data frame arrived before previous fragmented message completed".to_string(),
                    });
                }

                if frame.fin {
                    if frame.opcode == WebSocketOpcode::Text {
                        std::str::from_utf8(&frame.payload).map_err(|_| WebSocketError::Protocol {
                            code: 1007,
                            reason: "Text frame payload is not valid UTF-8".to_string(),
                        })?;
                    }
                    Ok(Some(WebSocketMessage {
                        opcode: frame.opcode,
                        payload: frame.payload,
                    }))
                } else {
                    if frame.payload.len() > self.max_message_size {
                        return Err(WebSocketError::PayloadTooLarge(frame.payload.len()));
                    }
                    self.current_opcode = Some(frame.opcode);
                    self.buffer = frame.payload;
                    Ok(None)
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Default for MessageAssembler {
    fn default() -> Self {
        Self::new()
    }
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

/// A non-blocking, bounded-queue WebSocket connection sender that prevents slow clients
/// from stalling the broadcast loop.
pub struct QueuedWebSocketSender {
    pub conn_id: u64,
    tx: std::sync::mpsc::SyncSender<WebSocketFrame>,
}

impl QueuedWebSocketSender {
    pub const DEFAULT_QUEUE_CAPACITY: usize = 256;

    /// Create a new queued sender with a dedicated background writer thread.
    pub fn new(conn_id: u64, stream: TcpStream) -> Self {
        Self::with_capacity(conn_id, stream, Self::DEFAULT_QUEUE_CAPACITY)
    }

    /// Create a queued sender with custom capacity.
    pub fn with_capacity(conn_id: u64, mut stream: TcpStream, capacity: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<WebSocketFrame>(capacity);
        std::thread::Builder::new()
            .name(format!("rhp-ws-writer-{}", conn_id))
            .spawn(move || {
                while let Ok(frame) = rx.recv() {
                    if write_frame(&mut stream, frame.opcode, &frame.payload).is_err() {
                        break;
                    }
                }
            })
            .expect("Failed to spawn ws writer thread");

        Self { conn_id, tx }
    }
}

impl WebSocketSender for QueuedWebSocketSender {
    fn send_frame(&self, opcode: WebSocketOpcode, payload: &[u8]) -> Result<(), String> {
        let frame = WebSocketFrame {
            opcode,
            fin: true,
            payload: payload.to_vec(),
        };
        self.tx.try_send(frame).map_err(|e| match e {
            std::sync::mpsc::TrySendError::Full(_) => "Outbound queue full (slow client)".to_string(),
            std::sync::mpsc::TrySendError::Disconnected(_) => "Connection closed".to_string(),
        })
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

    /// Send a frame with arbitrary opcode to a specific connection by ID.
    pub fn send_frame(&self, conn_id: u64, opcode: WebSocketOpcode, payload: &[u8]) -> Result<bool, String> {
        let sender = {
            let conns = self.connections.read().map_err(|e| e.to_string())?;
            conns.get(&conn_id).cloned()
        };

        if let Some(s) = sender {
            s.send_frame(opcode, payload)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Send a text frame to a specific connection by ID.
    pub fn send_text(&self, conn_id: u64, message: &str) -> Result<bool, String> {
        self.send_frame(conn_id, WebSocketOpcode::Text, message.as_bytes())
    }

    /// Broadcast an arbitrary opcode frame to all connected clients, pruning dead/stalled connections.
    pub fn broadcast_opcode(&self, opcode: WebSocketOpcode, payload: &[u8]) -> usize {
        let conns = match self.connections.read() {
            Ok(c) => c.clone(),
            Err(_) => return 0,
        };

        let mut sent = 0;
        let mut dead = Vec::new();

        for (&id, sender) in &conns {
            match sender.send_frame(opcode, payload) {
                Ok(()) => sent += 1,
                Err(_) => dead.push(id),
            }
        }

        for id in dead {
            self.unregister(id);
        }

        sent
    }

    /// Broadcast a text message to all connected clients.
    pub fn broadcast(&self, message: &str) -> usize {
        self.broadcast_opcode(WebSocketOpcode::Text, message.as_bytes())
    }

    /// Broadcast an arbitrary opcode frame to all subscribers of a specific channel, pruning dead connections.
    pub fn broadcast_channel_opcode(&self, channel: &str, opcode: WebSocketOpcode, payload: &[u8]) -> usize {
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

        let senders: Vec<(u64, Arc<dyn WebSocketSender>)> = {
            let conns = match self.connections.read() {
                Ok(c) => c,
                Err(_) => return 0,
            };
            target_ids
                .into_iter()
                .filter_map(|id| conns.get(&id).map(|s| (id, s.clone())))
                .collect()
        };

        let mut sent = 0;
        let mut dead = Vec::new();

        for (id, sender) in senders {
            match sender.send_frame(opcode, payload) {
                Ok(()) => sent += 1,
                Err(_) => dead.push(id),
            }
        }

        for id in dead {
            self.unregister(id);
        }

        sent
    }

    /// Broadcast a text message to all subscribers of a specific channel.
    pub fn broadcast_channel(&self, channel: &str, message: &str) -> usize {
        self.broadcast_channel_opcode(channel, WebSocketOpcode::Text, message.as_bytes())
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
// 5. Dual-Mode WebSocket Facade Backend Abstraction
// ============================================================================

/// Pluggable backend for Ws:: / WebSocket:: facade methods.
pub trait WebSocketBackend: Send + Sync {
    fn send(&self, conn_id: u64, opcode: u8, payload: Vec<u8>) -> bool;
    fn broadcast(&self, channel: Option<String>, opcode: u8, payload: Vec<u8>) -> usize;
    fn subscribe(&self, conn_id: u64, channel: &str) -> bool;
    fn unsubscribe(&self, conn_id: u64, channel: &str) -> bool;
    fn close(&self, conn_id: u64, code: u16, reason: &str) -> bool;
    fn active_client_count(&self) -> usize;
    fn active_client_ids(&self) -> Vec<u64>;
    fn active_channels(&self) -> Vec<String>;
}

/// In-process local hub backend (for single process `rhp serve` & unit tests).
pub struct LocalHubBackend {
    hub: &'static WebSocketHub,
}

impl LocalHubBackend {
    pub fn new() -> Self {
        Self {
            hub: get_websocket_hub(),
        }
    }
}

impl Default for LocalHubBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketBackend for LocalHubBackend {
    fn send(&self, conn_id: u64, opcode: u8, payload: Vec<u8>) -> bool {
        if let Some(op) = WebSocketOpcode::from_u8(opcode) {
            self.hub.send_frame(conn_id, op, &payload).is_ok()
        } else {
            false
        }
    }

    fn broadcast(&self, channel: Option<String>, opcode: u8, payload: Vec<u8>) -> usize {
        if let Some(op) = WebSocketOpcode::from_u8(opcode) {
            if let Some(chan) = channel {
                self.hub.broadcast_channel_opcode(&chan, op, &payload)
            } else {
                self.hub.broadcast_opcode(op, &payload)
            }
        } else {
            0
        }
    }

    fn subscribe(&self, conn_id: u64, channel: &str) -> bool {
        self.hub.subscribe(conn_id, channel);
        true
    }

    fn unsubscribe(&self, conn_id: u64, channel: &str) -> bool {
        self.hub.unsubscribe(conn_id, channel);
        true
    }

    fn close(&self, conn_id: u64, code: u16, reason: &str) -> bool {
        let _ = self.hub.send_frame(
            conn_id,
            WebSocketOpcode::Close,
            &WebSocketFrame::close(code, reason).payload,
        );
        self.hub.unregister(conn_id);
        true
    }

    fn active_client_count(&self) -> usize {
        self.hub.active_connection_count()
    }

    fn active_client_ids(&self) -> Vec<u64> {
        self.hub.active_connection_ids()
    }

    fn active_channels(&self) -> Vec<String> {
        self.hub.active_channels()
    }
}

/// Cluster IPC backend routing WebSocket operations from Worker to Master Reactor.
pub struct IpcWebSocketBackend {
    sender: IpcSender,
}

impl IpcWebSocketBackend {
    pub fn new(sender: IpcSender) -> Self {
        Self { sender }
    }
}

impl WebSocketBackend for IpcWebSocketBackend {
    fn send(&self, conn_id: u64, opcode: u8, payload: Vec<u8>) -> bool {
        self.sender
            .send(IpcMessage::WsSend {
                conn_id,
                opcode,
                payload,
            })
            .is_ok()
    }

    fn broadcast(&self, channel: Option<String>, opcode: u8, payload: Vec<u8>) -> usize {
        let res = self.sender.send(IpcMessage::WsBroadcast {
            channel,
            opcode,
            payload,
        });
        if res.is_ok() {
            1
        } else {
            0
        }
    }

    fn subscribe(&self, conn_id: u64, channel: &str) -> bool {
        self.sender
            .send(IpcMessage::WsSubscribe {
                conn_id,
                channel: channel.to_string(),
            })
            .is_ok()
    }

    fn unsubscribe(&self, conn_id: u64, channel: &str) -> bool {
        self.sender
            .send(IpcMessage::WsUnsubscribe {
                conn_id,
                channel: channel.to_string(),
            })
            .is_ok()
    }

    fn close(&self, conn_id: u64, code: u16, reason: &str) -> bool {
        self.sender
            .send(IpcMessage::WsClose {
                conn_id,
                code,
                reason: reason.to_string(),
            })
            .is_ok()
    }

    fn active_client_count(&self) -> usize {
        get_websocket_hub().active_connection_count()
    }

    fn active_client_ids(&self) -> Vec<u64> {
        get_websocket_hub().active_connection_ids()
    }

    fn active_channels(&self) -> Vec<String> {
        get_websocket_hub().active_channels()
    }
}

thread_local! {
    static CURRENT_WS_BACKEND: std::cell::RefCell<Option<Arc<dyn WebSocketBackend>>> = const { std::cell::RefCell::new(None) };
}

/// Set the current thread's WebSocket backend (e.g. for worker processes).
pub fn set_current_ws_backend(backend: Option<Arc<dyn WebSocketBackend>>) {
    CURRENT_WS_BACKEND.with(|cell| {
        *cell.borrow_mut() = backend;
    });
}

/// Get the active WebSocket backend, falling back to [`LocalHubBackend`] if unset.
pub fn get_current_ws_backend() -> Arc<dyn WebSocketBackend> {
    CURRENT_WS_BACKEND.with(|cell| {
        if let Some(ref b) = *cell.borrow() {
            b.clone()
        } else {
            Arc::new(LocalHubBackend::new())
        }
    })
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
        let (code, reason) = close_frame.parse_close_payload().unwrap().unwrap();
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

    #[test]
    fn test_rfc6455_rsv_bits_must_be_zero() {
        // Frame with RSV1 bit set (0x40 in byte 0 -> byte0 = 0x81 | 0x40 = 0xC1)
        let raw = vec![0xC1, 0x00];
        let mut reader = Cursor::new(raw);
        let err = read_frame(&mut reader).unwrap_err();
        assert_eq!(err.close_code(), 1002);
    }

    #[test]
    fn test_rfc6455_client_frame_must_be_masked() {
        // Unmasked text frame sent to server: byte0=0x81, byte1=0x05 (mask bit clear)
        let raw = vec![0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let mut reader = Cursor::new(raw);
        let err = read_client_frame(&mut reader).unwrap_err();
        assert_eq!(err.close_code(), 1002);
        assert!(err.to_string().contains("Client-to-server frames MUST be masked"));
    }

    #[test]
    fn test_rfc6455_control_frames_fin_and_payload_limits() {
        // Fragmented Ping (FIN = 0, byte0 = 0x09)
        let frag_ping = vec![0x09, 0x00];
        let mut reader = Cursor::new(frag_ping);
        let err = read_frame(&mut reader).unwrap_err();
        assert_eq!(err.close_code(), 1002);

        // Oversized Ping (payload length indicator 126 > 125)
        let big_ping = vec![0x89, 126, 0x00, 126];
        let mut reader2 = Cursor::new(big_ping);
        let err2 = read_frame(&mut reader2).unwrap_err();
        assert_eq!(err2.close_code(), 1002);
    }

    #[test]
    fn test_rfc6455_64bit_length_msb_must_be_zero() {
        // 64-bit length with MSB set (0x8000_0000_0000_0001)
        let mut raw = vec![0x82, 127];
        raw.extend_from_slice(&0x8000_0000_0000_0001u64.to_be_bytes());
        let mut reader = Cursor::new(raw);
        let err = read_frame(&mut reader).unwrap_err();
        assert_eq!(err.close_code(), 1002);
    }

    #[test]
    fn test_rfc6455_empty_close_frame_has_no_status_code() {
        let frame = WebSocketFrame {
            opcode: WebSocketOpcode::Close,
            fin: true,
            payload: Vec::new(),
        };
        let res = frame.parse_close_payload().unwrap();
        assert_eq!(res, None);

        let (code, reason) = frame.close_code_and_reason();
        assert_eq!(code, None);
        assert_eq!(reason, "");
    }

    #[test]
    fn test_rfc6455_invalid_close_payloads() {
        // 1-byte payload is invalid according to RFC 6455
        let frame1 = WebSocketFrame {
            opcode: WebSocketOpcode::Close,
            fin: true,
            payload: vec![0x03],
        };
        assert_eq!(frame1.parse_close_payload().unwrap_err().close_code(), 1002);

        // Reserved code 1005 cannot be sent on the wire
        let frame2 = WebSocketFrame {
            opcode: WebSocketOpcode::Close,
            fin: true,
            payload: 1005u16.to_be_bytes().to_vec(),
        };
        assert_eq!(frame2.parse_close_payload().unwrap_err().close_code(), 1002);

        // Invalid UTF-8 reason string fails with 1007
        let mut payload3 = 1000u16.to_be_bytes().to_vec();
        payload3.extend_from_slice(&[0xFF, 0xFE]); // invalid UTF-8 bytes
        let frame3 = WebSocketFrame {
            opcode: WebSocketOpcode::Close,
            fin: true,
            payload: payload3,
        };
        assert_eq!(frame3.parse_close_payload().unwrap_err().close_code(), 1007);
    }

    #[test]
    fn test_rfc6455_message_assembler_fragmentation_and_utf8() {
        let mut assembler = MessageAssembler::new();

        // Fragment 1: Text opcode, FIN = 0, payload "Hello "
        let f1 = WebSocketFrame {
            opcode: WebSocketOpcode::Text,
            fin: false,
            payload: b"Hello ".to_vec(),
        };
        assert_eq!(assembler.process_frame(f1).unwrap(), None);

        // Interleaved control frame: Ping
        let ping = WebSocketFrame {
            opcode: WebSocketOpcode::Ping,
            fin: true,
            payload: b"heartbeat".to_vec(),
        };
        let ping_msg = assembler.process_frame(ping).unwrap().unwrap();
        assert_eq!(ping_msg.opcode, WebSocketOpcode::Ping);
        assert_eq!(ping_msg.payload, b"heartbeat");

        // Fragment 2: Continuation, FIN = 1, payload "World!"
        let f2 = WebSocketFrame {
            opcode: WebSocketOpcode::Continuation,
            fin: true,
            payload: b"World!".to_vec(),
        };
        let final_msg = assembler.process_frame(f2).unwrap().unwrap();
        assert_eq!(final_msg.opcode, WebSocketOpcode::Text);
        assert_eq!(final_msg.as_text().unwrap(), "Hello World!");

        // Fragmented text message with invalid UTF-8 fails with 1007
        let mut assembler2 = MessageAssembler::new();
        let bad_f1 = WebSocketFrame {
            opcode: WebSocketOpcode::Text,
            fin: false,
            payload: vec![0xC3], // Incomplete 2-byte sequence
        };
        assert_eq!(assembler2.process_frame(bad_f1).unwrap(), None);

        let bad_f2 = WebSocketFrame {
            opcode: WebSocketOpcode::Continuation,
            fin: true,
            payload: vec![0x28], // Invalid continuation byte
        };
        let err = assembler2.process_frame(bad_f2).unwrap_err();
        assert_eq!(err.close_code(), 1007);
    }

    #[test]
    fn test_broadcast_prunes_dead_connections() {
        struct DeadSender;
        impl WebSocketSender for DeadSender {
            fn send_frame(&self, _opcode: WebSocketOpcode, _payload: &[u8]) -> Result<(), String> {
                Err("Broken pipe".to_string())
            }
        }

        let hub = WebSocketHub::new();
        hub.register(201, Arc::new(DeadSender));
        assert_eq!(hub.active_connection_count(), 1);

        let sent = hub.broadcast("Ping");
        assert_eq!(sent, 0);
        // Dead connection must be pruned from hub
        assert_eq!(hub.active_connection_count(), 0);
    }

    #[test]
    fn test_facade_dual_mode_backend_switch() {
        let (tx, mut rx) = crate::runtime::ipc::ipc_channel();
        let ipc_backend = Arc::new(IpcWebSocketBackend::new(tx));
        set_current_ws_backend(Some(ipc_backend));

        let backend = get_current_ws_backend();
        assert!(backend.send(55, WebSocketOpcode::Text as u8, b"Hi IPC".to_vec()));

        let received = rx.blocking_recv().unwrap();
        match received {
            IpcMessage::WsSend { conn_id, payload, .. } => {
                assert_eq!(conn_id, 55);
                assert_eq!(payload, b"Hi IPC");
            }
            other => panic!("Unexpected IPC message: {:?}", other),
        }

        // Reset to default local backend
        set_current_ws_backend(None);
        let default_backend = get_current_ws_backend();
        assert_eq!(default_backend.active_client_count(), 0);
    }
}
