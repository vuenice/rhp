//! Integration tests for RFC 6455 WebSocket Upgrade Pipeline, Framing, Hub, and Facade

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use rhp::runtime::reactor::{run_reactor_simple, ReactorHandle, WorkerRouter};
use rhp::runtime::types::IpcMessage;
use rhp::runtime::websocket::{read_frame, WebSocketOpcode};
use rhp::{execute_isolated_request, ParentProcess, PhpValue, WorkerRequest};

#[test]
fn test_rhp_websocket_facade_methods_in_script() {
    let script = r#"
        $key = 'dGhlIHNhbXBsZSBub25jZQ==';
        $accept = WebSocket::acceptKey($key);

        $clientCount = WebSocket::clients();
        $broadcastRes = WebSocket::broadcastTo('test_chan', 'Hello from RHP script!');
        
        // Subscription checks
        WebSocket::subscribe(101, 'general');
        $channels = WebSocket::channels();
        $hasGeneral = $channels->contains('general');
        WebSocket::unsubscribe(101, 'general');

        return [
            'accept' => $accept,
            'count' => $clientCount,
            'broadcast' => $broadcastRes,
            'hasGeneral' => $hasGeneral,
        ];
    "#;

    let state = ParentProcess::boot(script).expect("compile script");
    let req = WorkerRequest::new(script);
    let resp = execute_isolated_request(&state, &req, 0);

    assert_eq!(resp.status, rhp::WorkerResponseStatus::Success);
    let val = resp.to_php_value();
    if let PhpValue::Dict(d) = val {
        let map = d.borrow();
        assert_eq!(
            map.get("accept").map(|v| v.to_php_string()),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".to_string())
        );
        assert_eq!(map.get("hasGeneral"), Some(&PhpValue::Bool(true)));
    } else {
        panic!("Expected Dict return value, got {:?}", val);
    }
}

#[test]
fn test_rhp_websocket_is_upgrade_flag_detection() {
    let script = r#"
        $up = WebSocket::isUpgrade();
        return ['is_upgrade' => $up];
    "#;

    let state = ParentProcess::boot(script).expect("compile script");
    let mut req = WorkerRequest::new(script);
    req = req
        .with_server("REQUEST_METHOD", "WEBSOCKET")
        .with_server("HTTP_UPGRADE", "websocket");

    let resp = execute_isolated_request(&state, &req, 0);
    assert_eq!(resp.status, rhp::WorkerResponseStatus::Success);

    let val = resp.to_php_value();
    if let PhpValue::Dict(d) = val {
        let map = d.borrow();
        assert_eq!(map.get("is_upgrade"), Some(&PhpValue::Bool(true)));
    } else {
        panic!("Expected Dict, got {:?}", val);
    }
}

#[test]
fn test_reactor_websocket_upgrade_handshake_and_framing_e2e() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    let (tx, mut rx) = rhp::runtime::ipc::ipc_channel();
    let router = WorkerRouter::new(vec![tx]);
    let handle = ReactorHandle::new();
    let handle_clone = handle.clone();

    // Spawn reactor in background thread
    let server_thread = thread::spawn(move || {
        run_reactor_simple(listener, router, handle_clone);
    });

    // Connect client
    let mut client = TcpStream::connect(addr).expect("connect to reactor");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
    let upgrade_request = format!(
        "GET /chat HTTP/1.1\r\n\
         Host: {}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        addr, client_key
    );

    client.write_all(upgrade_request.as_bytes()).unwrap();
    client.flush().unwrap();

    // Read HTTP 101 Switching Protocols response
    let mut resp_buf = [0u8; 1024];
    let n = client.read(&mut resp_buf).expect("read upgrade response");
    let resp_str = String::from_utf8_lossy(&resp_buf[..n]);

    assert!(resp_str.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
    assert!(resp_str.contains("Upgrade: websocket\r\n"));
    assert!(resp_str.contains("Connection: Upgrade\r\n"));
    assert!(resp_str.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));

    // Verify Worker received WsUpgrade IPC message
    let ipc_msg = rx.blocking_recv().expect("recv ipc message");
    match ipc_msg {
        IpcMessage::WsUpgrade { uri, conn_id, .. } => {
            assert_eq!(uri, "/chat");
            assert!(conn_id > 0);
        }
        other => panic!("Expected IpcMessage::WsUpgrade, got {:?}", other),
    }

    // Client sends masked text frame: "Hello Reactor"
    let mask = [0xAA, 0xBB, 0xCC, 0xDD];
    let msg_bytes = b"Hello Reactor";
    let mut masked_payload = Vec::new();
    for (i, &b) in msg_bytes.iter().enumerate() {
        masked_payload.push(b ^ mask[i % 4]);
    }

    let mut client_frame = Vec::new();
    client_frame.push(0x81); // FIN + Text opcode
    client_frame.push(0x80 | (msg_bytes.len() as u8)); // Mask bit + len
    client_frame.extend_from_slice(&mask);
    client_frame.extend_from_slice(&masked_payload);

    client.write_all(&client_frame).unwrap();
    client.flush().unwrap();

    // Verify Worker received WsFrame with unmasked content
    let frame_msg = rx.blocking_recv().expect("recv frame ipc");
    match frame_msg {
        IpcMessage::WsFrame { opcode, payload, .. } => {
            assert_eq!(opcode, WebSocketOpcode::Text as u8);
            assert_eq!(payload, msg_bytes);
        }
        other => panic!("Expected IpcMessage::WsFrame, got {:?}", other),
    }

    // Client sends Ping frame: [0x89, 0x84, mask, masked("ping")]
    let ping_bytes = b"ping";
    let mut masked_ping = Vec::new();
    for (i, &b) in ping_bytes.iter().enumerate() {
        masked_ping.push(b ^ mask[i % 4]);
    }
    let mut ping_frame = Vec::new();
    ping_frame.push(0x89); // FIN + Ping
    ping_frame.push(0x80 | (ping_bytes.len() as u8));
    ping_frame.extend_from_slice(&mask);
    ping_frame.extend_from_slice(&masked_ping);

    client.write_all(&ping_frame).unwrap();
    client.flush().unwrap();

    // Server should immediately reply with Pong containing identical payload
    let pong_resp = read_frame(&mut client).expect("read pong from server");
    assert_eq!(pong_resp.opcode, WebSocketOpcode::Pong);
    assert_eq!(pong_resp.payload, ping_bytes);

    // Client sends Close frame: code 1000 ("Normal")
    let close_bytes = 1000u16.to_be_bytes();
    let mut masked_close = Vec::new();
    for (i, &b) in close_bytes.iter().enumerate() {
        masked_close.push(b ^ mask[i % 4]);
    }
    let mut close_frame = Vec::new();
    close_frame.push(0x88); // FIN + Close
    close_frame.push(0x80 | 2);
    close_frame.extend_from_slice(&mask);
    close_frame.extend_from_slice(&masked_close);

    client.write_all(&close_frame).unwrap();
    client.flush().unwrap();

    // Server should echo Close frame and terminate session
    let server_close = read_frame(&mut client).expect("read close from server");
    assert_eq!(server_close.opcode, WebSocketOpcode::Close);
    let (code, _) = server_close.parse_close_payload();
    assert_eq!(code, 1000);

    // Shutdown reactor
    handle.signal_shutdown();
    drop(client);
    let _ = TcpStream::connect(addr);
    let _ = server_thread.join();
}
