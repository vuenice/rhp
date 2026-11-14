//! # RHPM Master Reactor
//!
//! The Master process runs N Reactor OS threads (default = logical CPU count).
//! Each Reactor thread owns a Tokio runtime or accept loop on the TCP listener.
//! Its responsibilities are:
//!
//! 1. Accept client TCP streams and parse raw HTTP/1.1 from the byte stream.
//! 2. For small bodies (<64 KB), buffer the full body.
//! 3. Build an `IpcMessage::DispatchRequest` and route it to a healthy Worker
//!    via the least-loaded worker IPC channel.
//! 4. Receive the `IpcMessage::ResponseReady` reply and flush it to the client.
//! 5. Handle pipelining, keep-alive, and graceful shutdown.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::runtime::ipc::IpcSender;
use crate::runtime::types::{IpcMessage, RequestId};

// ============================================================================
// Reactor handle & metrics
// ============================================================================

/// Handle to the Master Reactor, allowing the Manager or CLI to signal shutdown
/// and inspect load metrics.
#[derive(Clone)]
pub struct ReactorHandle {
    /// Set to `true` to signal all Reactor threads to stop accepting connections.
    pub shutdown: Arc<AtomicBool>,
    /// Total HTTP requests accepted since startup.
    pub total_requests: Arc<AtomicUsize>,
    /// Total requests shed with 503 due to overload.
    pub total_shed: Arc<AtomicUsize>,
}

impl ReactorHandle {
    pub fn new() -> Self {
        Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            total_requests: Arc::new(AtomicUsize::new(0)),
            total_shed: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Signal all Reactor threads to stop accepting new connections.
    pub fn signal_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

impl Default for ReactorHandle {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Worker router (least-loaded selection)
// ============================================================================

/// Routes incoming requests to the least-loaded Worker IPC sender.
#[derive(Clone)]
pub struct WorkerRouter {
    slots: Arc<Vec<WorkerSlot>>,
}

#[derive(Clone)]
pub struct WorkerSlot {
    pub sender: IpcSender,
    pub pending: Arc<AtomicUsize>,
}

impl WorkerRouter {
    /// Create a router from a list of worker IPC senders.
    pub fn new(senders: Vec<IpcSender>) -> Self {
        let slots = senders
            .into_iter()
            .map(|sender| WorkerSlot {
                sender,
                pending: Arc::new(AtomicUsize::new(0)),
            })
            .collect();
        Self {
            slots: Arc::new(slots),
        }
    }

    /// Route a request to the least-loaded worker. Returns an error if all
    /// workers are saturated or none are available.
    pub fn route(&self, msg: IpcMessage) -> Result<(), String> {
        if self.slots.is_empty() {
            return Err("No worker processes registered".to_string());
        }
        let best = self
            .slots
            .iter()
            .min_by_key(|s| s.pending.load(Ordering::Relaxed))
            .expect("slots is non-empty");

        best.pending.fetch_add(1, Ordering::Relaxed);
        let result = best.sender.send(msg);
        if result.is_err() {
            best.pending.fetch_sub(1, Ordering::Relaxed);
        }
        result
    }

    /// Notify that a request has completed for a specific slot index.
    pub fn complete_slot_index(&self, index: usize) {
        if let Some(slot) = self.slots.get(index) {
            slot.pending.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

// ============================================================================
// Simple HTTP/1.1 request parser
// ============================================================================

/// Minimal parsed HTTP/1.1 request.
#[derive(Debug, Clone)]
pub struct ParsedRequest {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ParsedRequest {
    /// Check whether this parsed request is requesting an RFC 6455 WebSocket upgrade.
    pub fn is_websocket_upgrade(&self) -> Option<String> {
        crate::runtime::websocket::is_websocket_upgrade(&self.headers)
    }
}

/// Parse a minimal HTTP/1.1 request from a raw byte buffer.
pub fn parse_http_request(buf: &[u8]) -> Option<ParsedRequest> {
    let sep = find_double_crlf(buf)?;
    let header_part = std::str::from_utf8(&buf[..sep]).ok()?;
    let mut lines = header_part.lines();

    let request_line = lines.next()?;
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next()?.to_string();
    let uri = parts.next()?.to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name_lower = name.trim().to_ascii_lowercase();
            let value_trimmed = value.trim().to_string();
            if name_lower == "content-length" {
                content_length = value_trimmed.parse().unwrap_or(0);
            }
            headers.push((name_lower, value_trimmed));
        }
    }

    let body_start = sep + 4;
    if buf.len() < body_start + content_length {
        return None;
    }
    let body = buf[body_start..body_start + content_length].to_vec();

    Some(ParsedRequest {
        method,
        uri,
        version,
        headers,
        body,
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

// ============================================================================
// Reactor execution loop
// ============================================================================

/// Run a synchronous reactor accept loop on a bound [`TcpListener`].
///
/// Accepts client streams, parses HTTP/1.1 frames, handles WebSocket upgrades,
/// dispatches requests to workers via [`WorkerRouter`], and returns 503 if overloaded.
pub fn run_reactor_simple(
    listener: TcpListener,
    router: WorkerRouter,
    handle: ReactorHandle,
) {
    listener
        .set_nonblocking(false)
        .expect("Cannot set blocking mode");

    for stream in listener.incoming() {
        if handle.shutdown.load(Ordering::Relaxed) {
            break;
        }
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mut buf = vec![0u8; 65536];
        let n = match stream.read(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => continue,
        };
        buf.truncate(n);

        let req = match parse_http_request(&buf) {
            Some(r) => r,
            None => continue,
        };

        let request_id = RequestId::next();
        handle.total_requests.fetch_add(1, Ordering::Relaxed);

        // Check if this request is an RFC 6455 WebSocket Upgrade
        if let Some(sec_ws_key) = req.is_websocket_upgrade() {
            let accept_key = crate::runtime::websocket::compute_sec_websocket_accept(&sec_ws_key);
            let resp = crate::runtime::websocket::build_upgrade_response(&accept_key);
            if stream.write_all(resp.as_bytes()).is_ok() && stream.flush().is_ok() {
                let conn_id = crate::runtime::websocket::next_connection_id();
                let hub = crate::runtime::websocket::get_websocket_hub();
                if let Ok(cloned) = stream.try_clone() {
                    let sender = std::sync::Arc::new(crate::runtime::websocket::QueuedWebSocketSender::new(conn_id, cloned));
                    hub.register(conn_id, sender);
                }

                // Notify worker tier of the new WebSocket connection
                let _ = router.route(IpcMessage::WsUpgrade {
                    conn_id,
                    uri: req.uri.clone(),
                    headers: req.headers.clone(),
                });

                // Spawn connection session handler thread to keep reactor accept loop responsive
                let router_clone = router.clone();
                std::thread::spawn(move || {
                    handle_reactor_websocket_session(stream, conn_id, router_clone);
                });
            }
            continue;
        }

        let dispatch = IpcMessage::DispatchRequest {
            request_id,
            method: req.method,
            uri: req.uri,
            headers: req.headers,
            body: Some(req.body),
        };

        if router.route(dispatch).is_err() {
            let resp = b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n";
            let _ = stream.write_all(resp);
            handle.total_shed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Handle persistent WebSocket framing loop for an upgraded connection in the Master Reactor.
pub fn handle_reactor_websocket_session(
    mut stream: std::net::TcpStream,
    conn_id: u64,
    router: WorkerRouter,
) {
    let hub = crate::runtime::websocket::get_websocket_hub();
    let mut assembler = crate::runtime::websocket::MessageAssembler::new();

    loop {
        match crate::runtime::websocket::read_client_frame(&mut stream) {
            Ok(frame) => match assembler.process_frame(frame) {
                Ok(Some(msg)) => match msg.opcode {
                    crate::runtime::websocket::WebSocketOpcode::Ping => {
                        // RFC 6455 §5.5.2: MUST respond with Pong containing identical payload
                        let _ = hub.send_frame(
                            conn_id,
                            crate::runtime::websocket::WebSocketOpcode::Pong,
                            &msg.payload,
                        );
                    }
                    crate::runtime::websocket::WebSocketOpcode::Pong => {
                        // Ignore unsolicited heartbeats
                    }
                    crate::runtime::websocket::WebSocketOpcode::Close => {
                        // RFC 6455 §5.5.1: Reply with Close frame and terminate
                        let frame = crate::runtime::websocket::WebSocketFrame {
                            opcode: crate::runtime::websocket::WebSocketOpcode::Close,
                            fin: true,
                            payload: msg.payload,
                        };
                        let (code_opt, reason) = frame.close_code_and_reason();
                        let code = code_opt.unwrap_or(1000);
                        let close_payload = crate::runtime::websocket::WebSocketFrame::close(code, &reason).payload;
                        let _ = hub.send_frame(
                            conn_id,
                            crate::runtime::websocket::WebSocketOpcode::Close,
                            &close_payload,
                        );
                        let _ = router.route(IpcMessage::WsClose {
                            conn_id,
                            code,
                            reason,
                        });
                        break;
                    }
                    crate::runtime::websocket::WebSocketOpcode::Text
                    | crate::runtime::websocket::WebSocketOpcode::Binary => {
                        // Forward incoming reassembled data frame to worker coroutine fiber
                        let _ = router.route(IpcMessage::WsFrame {
                            conn_id,
                            opcode: msg.opcode as u8,
                            payload: msg.payload,
                        });
                    }
                    _ => {}
                },
                Ok(None) => {
                    // Frame buffered in assembler, awaiting continuation fragments
                }
                Err(e) => {
                    // RFC 6455 protocol violation or UTF-8 error
                    let code = e.close_code();
                    let reason = e.to_string();
                    let close_payload = crate::runtime::websocket::WebSocketFrame::close(code, &reason).payload;
                    let _ = hub.send_frame(
                        conn_id,
                        crate::runtime::websocket::WebSocketOpcode::Close,
                        &close_payload,
                    );
                    let _ = router.route(IpcMessage::WsClose {
                        conn_id,
                        code,
                        reason,
                    });
                    break;
                }
            },
            Err(e) => {
                let code = e.close_code();
                if e.is_protocol_error() {
                    let close_payload = crate::runtime::websocket::WebSocketFrame::close(code, "Protocol violation").payload;
                    let _ = hub.send_frame(
                        conn_id,
                        crate::runtime::websocket::WebSocketOpcode::Close,
                        &close_payload,
                    );
                }
                let _ = router.route(IpcMessage::WsClose {
                    conn_id,
                    code,
                    reason: e.to_string(),
                });
                break;
            }
        }
    }

    hub.unregister(conn_id);
}
