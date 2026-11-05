//! # RHPM Manager
//!
//! The Manager is responsible for:
//! - Spawning and tracking all Worker and Task Worker OS threads/processes.
//! - Detecting exited or recycled workers and spawning fresh replacements.
//! - Coordinating graceful drain when a Worker signals recycling or shutdown.
//! - Exposing diagnostic status queries.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use crate::runtime::config::RuntimeConfig;
use crate::runtime::ipc::{ipc_channel, IpcSender};
use crate::runtime::log_pool::{LogBatchSender, LogRingBuffer};
use crate::runtime::types::IpcMessage;
use crate::runtime::worker::run_worker;

/// A handle to an active managed Worker thread.
pub struct ManagedWorker {
    pub worker_id: u32,
    pub ipc_tx: IpcSender,
    pub shutdown: Arc<AtomicBool>,
    pub handle: Option<thread::JoinHandle<()>>,
}

/// Manager that owns and supervises the Worker pools.
pub struct Manager {
    config: Arc<RuntimeConfig>,
    workers: Vec<ManagedWorker>,
    next_worker_id: AtomicU32,
    pub log_sender: LogBatchSender,
}

/// Handle returned by `Manager::start()` that the Master/CLI uses to route requests.
#[derive(Clone)]
pub struct ManagerHandle {
    /// IPC senders to each active worker.
    pub worker_senders: Vec<IpcSender>,
    /// Set to request full shutdown.
    pub shutdown: Arc<AtomicBool>,
}

impl Manager {
    /// Create a new Manager with the given configuration and log batch sender.
    pub fn new(config: Arc<RuntimeConfig>, log_sender: LogBatchSender) -> Self {
        Self {
            config,
            workers: Vec::new(),
            next_worker_id: AtomicU32::new(1),
            log_sender,
        }
    }

    /// Spawn all configured Worker threads and return a [`ManagerHandle`].
    pub fn start(&mut self) -> ManagerHandle {
        let count = self.config.worker_count;
        let mut senders = Vec::with_capacity(count);

        for _ in 0..count {
            let sender = self.spawn_worker();
            senders.push(sender);
        }

        ManagerHandle {
            worker_senders: senders,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Spawn a single Worker thread and return its IPC sender.
    pub fn spawn_worker(&mut self) -> IpcSender {
        let worker_id = self.next_worker_id.fetch_add(1, Ordering::Relaxed);
        let (master_tx, worker_rx) = ipc_channel();
        let (worker_tx, _master_rx) = ipc_channel();

        let shutdown = Arc::new(AtomicBool::new(false));
        let cfg = self.config.clone();

        let (flush_tx, _flush_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let log_buf = Arc::new(LogRingBuffer::new(
            worker_id,
            cfg.log_ring_size,
            flush_tx,
        ));

        let ipc_tx_clone = worker_tx;
        let shutdown_clone = shutdown.clone();

        let handle = thread::Builder::new()
            .name(format!("rhpm-worker-{}", worker_id))
            .spawn(move || {
                run_worker(
                    worker_id,
                    cfg,
                    ipc_tx_clone,
                    worker_rx,
                    log_buf,
                    shutdown_clone,
                );
            })
            .expect("Failed to spawn worker thread");

        self.workers.push(ManagedWorker {
            worker_id,
            ipc_tx: master_tx.clone(),
            shutdown,
            handle: Some(handle),
        });

        master_tx
    }

    /// Signal all workers to begin graceful shutdown and drain.
    pub fn shutdown_all(&mut self) {
        for w in &mut self.workers {
            let _ = w.ipc_tx.send(IpcMessage::GracefulShutdown);
            w.shutdown.store(true, Ordering::SeqCst);
        }
    }
}
