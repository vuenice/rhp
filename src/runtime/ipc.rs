//! IPC primitives for intra-process communication between Master, Manager, Workers,
//! and Task Workers.
//!
//! Uses a simple `Arc<tokio::sync::mpsc::UnboundedSender>` channel pair for now,
//! with a path to migrate to Unix Domain Sockets or shared-memory rings.

use std::sync::Arc;
use tokio::sync::mpsc;

use super::types::IpcMessage;

/// Sender half of an IPC channel (cheaply cloneable via `Arc`).
#[derive(Clone)]
pub struct IpcSender(pub Arc<mpsc::UnboundedSender<IpcMessage>>);

impl IpcSender {
    /// Send an IPC message, returning an error if the receiver has dropped.
    pub fn send(&self, msg: IpcMessage) -> Result<(), String> {
        self.0.send(msg).map_err(|e| e.to_string())
    }
}

/// Receiver half of an IPC channel (non-cloneable; owned by one actor).
pub struct IpcReceiver(pub mpsc::UnboundedReceiver<IpcMessage>);

impl IpcReceiver {
    /// Receive the next IPC message, waiting asynchronously.
    pub async fn recv(&mut self) -> Option<IpcMessage> {
        self.0.recv().await
    }
}

/// Create a linked `(IpcSender, IpcReceiver)` pair.
pub fn ipc_channel() -> (IpcSender, IpcReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (IpcSender(Arc::new(tx)), IpcReceiver(rx))
}
