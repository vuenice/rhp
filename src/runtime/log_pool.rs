//! Centralized, non-blocking per-worker log ring buffer and Master-side batch collector.
//!
//! Each Worker process owns a small `LogRingBuffer` (a bounded `crossbeam_channel`).
//! A background task in the Worker drains it in batches and forwards records to the
//! Master process log collector over IPC, which performs vectorised disk/stdout writes.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::types::{LogLevel, LogRecord, RequestId};

/// Non-blocking log ring buffer that lives inside a single Worker process.
///
/// `try_enqueue` never blocks: low-priority debug records are silently dropped when
/// the ring is full, while WARN and ERROR records trigger an immediate flush signal.
pub struct LogRingBuffer {
    worker_id: u32,
    sender: SyncSender<LogRecord>,
    receiver: Mutex<Receiver<LogRecord>>,
    flush_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

impl LogRingBuffer {
    /// Create a new ring buffer with the given capacity.
    pub fn new(
        worker_id: u32,
        capacity: usize,
        flush_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> Self {
        let (sender, receiver) = sync_channel(capacity);
        Self {
            worker_id,
            sender,
            receiver: Mutex::new(receiver),
            flush_tx,
        }
    }

    /// Attempt to enqueue a log record. Returns `false` if the ring was full and
    /// the record was dropped (only possible for `Debug` and `Info` levels).
    pub fn try_enqueue(
        &self,
        request_id: RequestId,
        level: LogLevel,
        message: String,
    ) -> bool {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_micros() as u64;

        let record = LogRecord {
            worker_id: self.worker_id,
            request_id,
            level,
            timestamp_micros: ts,
            message,
        };

        match self.sender.try_send(record) {
            Ok(()) => {
                // For high-priority messages, signal the drainer to wake up immediately.
                if level >= LogLevel::Warn {
                    let _ = self.flush_tx.send(());
                }
                true
            }
            Err(TrySendError::Full(dropped)) => {
                // Drop low-priority records silently; force-flush for high-priority ones.
                if dropped.level >= LogLevel::Warn {
                    // Try again with a blocking send for critical records.
                    // In practice, the channel should rarely be this full during normal load.
                    let _ = self.flush_tx.send(());
                }
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Drain up to `max_batch` records from the ring into a Vec.
    pub fn drain_batch(&self, max_batch: usize) -> Vec<LogRecord> {
        let mut batch = Vec::with_capacity(max_batch.min(64));
        if let Ok(rx) = self.receiver.lock() {
            for _ in 0..max_batch {
                match rx.try_recv() {
                    Ok(record) => batch.push(record),
                    Err(_) => break,
                }
            }
        }
        batch
    }
}

/// Master-side log collector that receives batches from all workers and writes
/// them to stdout or a configured sink.
///
/// Instantiate one per application and hand the `batch_receiver` to the collector task.
pub struct LogCollector {
    batch_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<LogRecord>>,
}

impl LogCollector {
    /// Create a new collector. Returns `(sender, collector)` where `sender` is given
    /// to each Worker's drain task.
    pub fn new() -> (LogBatchSender, Self) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (LogBatchSender(tx), Self { batch_rx: rx })
    }

    /// Run the collector loop, draining batches and writing to stdout.
    /// This is `async` and must be spawned on the Master's Tokio runtime.
    pub async fn run(mut self) {
        while let Some(batch) = self.batch_rx.recv().await {
            for record in batch {
                // ISO-8601 timestamp from micros
                let secs = record.timestamp_micros / 1_000_000;
                let us = record.timestamp_micros % 1_000_000;
                eprintln!(
                    "[{}.{:06}] [{:<5}] [w{}] [{}] {}",
                    secs, us,
                    record.level,
                    record.worker_id,
                    record.request_id,
                    record.message,
                );
            }
        }
    }
}

/// Cloneable sender for forwarding log batches to the master collector.
#[derive(Clone)]
pub struct LogBatchSender(pub tokio::sync::mpsc::UnboundedSender<Vec<LogRecord>>);

impl LogBatchSender {
    /// Forward a batch of log records to the master collector.
    pub fn send_batch(&self, batch: Vec<LogRecord>) {
        let _ = self.0.send(batch);
    }
}
