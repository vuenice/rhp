//! # Synchronous-by-Default Execution Model & Local Execution Fibers
//!
//! Implements Technical Point #16 of the RHP Architecture Specification:
//! - **Synchronous-by-Default Execution Model:** Local execution fibers run standard helpers
//!   and I/O calls (`File::read()`, `DB::table()`, `HTTP::get()`) sequentially and blocking by default
//!   for predictable, linear developer ergonomics with zero async/await boilerplate.
//! - **Cooperative Non-Blocking Core:** Helpers and standard library calls execute line-by-line
//!   synchronously on local execution fibers without blocking underlying OS worker threads.
//! - **Fiber Lifecycle & State Machine:** Tracks fiber states (`Created`, `Running`, `Yielded`,
//!   `WaitingIo`, `Completed`, `Failed`) with high-resolution I/O and CPU duration accounting.
//! - **Fiber-Local Context & Scoping:** Per-fiber isolated key-value storage (`FiberContext`)
//!   for request metadata, tenant context, and transaction handles.
//! - **Cooperative Fiber Scheduler (`FiberScheduler`):** Schedules and interleaves multiple
//!   cooperative fibers across worker threads with round-robin task queues, cooperative suspension,
//!   and instant resumption.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::lexer::span::Span;
use crate::vm::error::{RuntimeError, RuntimeErrorKind};
use crate::vm::value::PhpValue;

/// Monotonically increasing atomic counter for unique fiber IDs.
static NEXT_FIBER_ID: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for an execution fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FiberId(pub u64);

impl FiberId {
    /// Allocate a new globally unique [`FiberId`].
    pub fn next() -> Self {
        Self(NEXT_FIBER_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Retrieve the raw numeric identifier.
    #[inline]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for FiberId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fiber#{}", self.0)
    }
}

/// Categorized I/O operation kind for fiber wait tracking and telemetry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoKind {
    /// File read operation (`File::read()`).
    FileRead { path: String },
    /// File write operation (`File::write()`).
    FileWrite { path: String, bytes: usize },
    /// File existence check (`File::exists()`).
    FileExists { path: String },
    /// File deletion (`File::delete()`).
    FileDelete { path: String },
    /// Database table selection / query builder (`DB::table()`).
    DbTable { table: String },
    /// Database raw query execution (`DB::raw()`, `DB::select()`, etc.).
    DbQuery { sql: String },
    /// Database primary key lookup (`DB::find()`, `$query->find()`).
    DbFind { table: String, id: String },
    /// Outgoing HTTP GET request (`HTTP::get()`).
    HttpGet { url: String },
    /// Outgoing HTTP POST request (`HTTP::post()`).
    HttpPost { url: String },
    /// Cooperative timer sleep (`Future::delayed()`).
    Timer { milliseconds: u64 },
    /// Custom plugin / Tier 2 I/O operation.
    Custom { category: String, operation: String },
}

impl IoKind {
    /// General category string of this I/O operation (`"File"`, `"DB"`, `"HTTP"`, `"Timer"`, `"Custom"`).
    pub fn category(&self) -> &'static str {
        match self {
            Self::FileRead { .. }
            | Self::FileWrite { .. }
            | Self::FileExists { .. }
            | Self::FileDelete { .. } => "File",
            Self::DbTable { .. } | Self::DbQuery { .. } | Self::DbFind { .. } => "DB",
            Self::HttpGet { .. } | Self::HttpPost { .. } => "HTTP",
            Self::Timer { .. } => "Timer",
            Self::Custom { .. } => "Custom",
        }
    }

    /// Human-readable description of the target I/O operation.
    pub fn description(&self) -> String {
        match self {
            Self::FileRead { path } => format!("File::read({})", path),
            Self::FileWrite { path, bytes } => format!("File::write({}, {} bytes)", path, bytes),
            Self::FileExists { path } => format!("File::exists({})", path),
            Self::FileDelete { path } => format!("File::delete({})", path),
            Self::DbTable { table } => format!("DB::table({})", table),
            Self::DbQuery { sql } => format!("DB::query({})", sql),
            Self::DbFind { table, id } => format!("DB::table({})->find({})", table, id),
            Self::HttpGet { url } => format!("HTTP::get({})", url),
            Self::HttpPost { url } => format!("HTTP::post({})", url),
            Self::Timer { milliseconds } => format!("Timer::sleep({}ms)", milliseconds),
            Self::Custom { category, operation } => format!("{}::{}", category, operation),
        }
    }
}

/// Information captured when a fiber is suspended waiting for an I/O operation.
#[derive(Debug, Clone, PartialEq)]
pub struct IoWaitInfo {
    /// The target I/O operation.
    pub kind: IoKind,
    /// Timestamp when the I/O wait commenced.
    pub started_at: Instant,
    /// Total duration spent waiting for I/O so far.
    pub duration: Duration,
}

/// Lifecycle states of a local execution fiber.
#[derive(Debug, Clone, PartialEq)]
pub enum FiberState {
    /// Newly instantiated fiber, not yet running.
    Created,
    /// Actively executing on an OS worker thread.
    Running,
    /// Suspended explicitly or via scheduler slice.
    Suspended { reason: String },
    /// Cooperatively yielded via `Fiber::yield()`.
    Yielded,
    /// Cooperatively waiting for a non-blocking I/O operation.
    WaitingIo(IoWaitInfo),
    /// Successfully completed execution with a final result.
    Completed(PhpValue),
    /// Terminated abnormally due to an unhandled exception or runtime error.
    Failed(String),
}

impl FiberState {
    /// Returns true if the fiber is currently executing on a worker thread.
    #[inline]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Returns true if the fiber is waiting for an I/O operation.
    #[inline]
    pub fn is_waiting_io(&self) -> bool {
        matches!(self, Self::WaitingIo(_))
    }

    /// Returns true if the fiber has finished successfully.
    #[inline]
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed(_))
    }

    /// Returns true if the fiber has terminated with an error.
    #[inline]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// Returns true if the fiber has reached a terminal state (completed or failed).
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Failed(_))
    }

    /// Canonical status name for introspection and logging.
    pub fn status_name(&self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Running => "Running",
            Self::Suspended { .. } => "Suspended",
            Self::Yielded => "Yielded",
            Self::WaitingIo(_) => "WaitingIo",
            Self::Completed(_) => "Completed",
            Self::Failed(_) => "Failed",
        }
    }
}

/// Comprehensive telemetry and execution statistics for an individual fiber.
#[derive(Debug, Clone, PartialEq)]
pub struct FiberStats {
    /// Associated fiber ID.
    pub fiber_id: FiberId,
    /// Associated fiber label or name.
    pub name: String,
    /// Total wall-clock time elapsed from fiber creation to completion.
    pub total_duration: Duration,
    /// CPU execution duration (excluding cooperative I/O wait time).
    pub cpu_duration: Duration,
    /// Accumulated duration spent in cooperative I/O wait states.
    pub io_wait_duration: Duration,
    /// Number of times the fiber yielded execution cooperatively.
    pub yield_count: usize,
    /// Total count of I/O operations invoked.
    pub io_count: usize,
    /// Breakdown of I/O operations by category (`"File"`, `"DB"`, `"HTTP"`, `"Timer"`, etc.).
    pub io_breakdown: IndexMap<String, usize>,
    /// History of I/O operations executed by this fiber.
    pub io_history: Vec<(String, Duration)>,
    /// Whether the fiber has finished execution.
    pub is_completed: bool,
}

impl FiberStats {
    /// Create initial statistics for a newly spawned fiber.
    pub fn new(fiber_id: FiberId, name: impl Into<String>) -> Self {
        Self {
            fiber_id,
            name: name.into(),
            total_duration: Duration::ZERO,
            cpu_duration: Duration::ZERO,
            io_wait_duration: Duration::ZERO,
            yield_count: 0,
            io_count: 0,
            io_breakdown: IndexMap::new(),
            io_history: Vec::new(),
            is_completed: false,
        }
    }

    /// Convert statistics to a script-land subtyped `dict` (`Arr::dict`).
    pub fn to_php_value(&self) -> PhpValue {
        let mut map = IndexMap::new();
        map.insert("id".to_string(), PhpValue::Int(self.fiber_id.0 as i32));
        map.insert("name".to_string(), PhpValue::string(self.name.clone()));
        map.insert("yield_count".to_string(), PhpValue::Int(self.yield_count as i32));
        map.insert("io_count".to_string(), PhpValue::Int(self.io_count as i32));
        map.insert(
            "total_duration_us".to_string(),
            PhpValue::Int(self.total_duration.as_micros() as i32),
        );
        map.insert(
            "cpu_duration_us".to_string(),
            PhpValue::Int(self.cpu_duration.as_micros() as i32),
        );
        map.insert(
            "io_wait_duration_us".to_string(),
            PhpValue::Int(self.io_wait_duration.as_micros() as i32),
        );
        map.insert("is_completed".to_string(), PhpValue::Bool(self.is_completed));

        let mut breakdown_map = IndexMap::new();
        for (k, v) in &self.io_breakdown {
            breakdown_map.insert(k.clone(), PhpValue::Int(*v as i32));
        }
        map.insert("io_breakdown".to_string(), PhpValue::dict_from_map(breakdown_map));

        PhpValue::dict_from_map(map)
    }
}

/// Fiber-local context storage, providing isolated request-scoped variables.
#[derive(Debug, Clone, PartialEq)]
pub struct FiberContext {
    /// Unique fiber ID.
    pub id: FiberId,
    /// Human-readable fiber name.
    pub name: String,
    /// Optional parent fiber ID if spawned as a child task.
    pub parent_id: Option<FiberId>,
    /// Fiber-local storage variables.
    pub locals: IndexMap<String, PhpValue>,
    /// Timestamp when context was initialized.
    pub created_at: Instant,
}

impl FiberContext {
    /// Create a new empty fiber context.
    pub fn new(id: FiberId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            parent_id: None,
            locals: IndexMap::new(),
            created_at: Instant::now(),
        }
    }

    /// Retrieve a fiber-local value.
    pub fn get(&self, key: &str) -> Option<&PhpValue> {
        self.locals.get(key)
    }

    /// Store a fiber-local value.
    pub fn set(&mut self, key: impl Into<String>, value: PhpValue) {
        self.locals.insert(key.into(), value);
    }

    /// Remove a fiber-local value.
    pub fn remove(&mut self, key: &str) -> Option<PhpValue> {
        self.locals.shift_remove(key)
    }

    /// Clear all fiber-local variables.
    pub fn clear(&mut self) {
        self.locals.clear();
    }
}

/// An execution fiber representing a lightweight cooperative task.
#[derive(Debug, Clone)]
pub struct Fiber {
    /// Unique fiber ID.
    pub id: FiberId,
    /// Human-readable fiber name.
    pub name: String,
    /// Current lifecycle state.
    pub state: FiberState,
    /// Fiber-local context storage.
    pub context: FiberContext,
    /// Execution statistics and telemetry.
    pub stats: FiberStats,
    /// Creation instant.
    pub created_at: Instant,
    /// Last state transition instant.
    pub last_state_change: Instant,
    /// Active I/O operation if currently waiting.
    pub current_io: Option<(IoKind, Instant)>,
    /// Final result if completed.
    pub result: Option<PhpValue>,
}

impl Fiber {
    /// Create a new fiber in the `Created` state.
    pub fn new(name: impl Into<String>) -> Self {
        let id = FiberId::next();
        let name_str = name.into();
        let now = Instant::now();
        Self {
            id,
            name: name_str.clone(),
            state: FiberState::Created,
            context: FiberContext::new(id, name_str.clone()),
            stats: FiberStats::new(id, name_str),
            created_at: now,
            last_state_change: now,
            current_io: None,
            result: None,
        }
    }

    /// Set an explicit [`FiberId`].
    pub fn with_id(mut self, id: FiberId) -> Self {
        self.id = id;
        self.context.id = id;
        self.stats.fiber_id = id;
        self
    }

    /// Set a parent fiber ID.
    pub fn with_parent(mut self, parent_id: FiberId) -> Self {
        self.context.parent_id = Some(parent_id);
        self
    }

    /// Get the fiber ID.
    #[inline]
    pub fn id(&self) -> FiberId {
        self.id
    }

    /// Get the fiber name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get current state.
    #[inline]
    pub fn state(&self) -> &FiberState {
        &self.state
    }

    /// Get execution statistics.
    #[inline]
    pub fn stats(&self) -> &FiberStats {
        &self.stats
    }

    /// Access fiber context.
    #[inline]
    pub fn context(&self) -> &FiberContext {
        &self.context
    }

    /// Mutably access fiber context.
    #[inline]
    pub fn context_mut(&mut self) -> &mut FiberContext {
        &mut self.context
    }

    /// Transition fiber state to `Running`.
    pub fn set_running(&mut self) {
        self.transition_to(FiberState::Running);
    }

    /// Transition state and record timing.
    pub fn transition_to(&mut self, new_state: FiberState) {
        let now = Instant::now();
        let elapsed_since_last = now.duration_since(self.last_state_change);

        if self.state.is_running() {
            self.stats.cpu_duration += elapsed_since_last;
        }

        self.last_state_change = now;
        self.state = new_state;
    }

    /// Record start of a cooperative I/O operation.
    pub fn record_io_start(&mut self, kind: IoKind) {
        let now = Instant::now();
        if self.state.is_running() {
            self.stats.cpu_duration += now.duration_since(self.last_state_change);
        }

        self.current_io = Some((kind.clone(), now));
        self.last_state_change = now;
        self.state = FiberState::WaitingIo(IoWaitInfo {
            kind,
            started_at: now,
            duration: Duration::ZERO,
        });
    }

    /// Record completion of an I/O operation and restore `Running` state.
    pub fn record_io_finish(&mut self) {
        let now = Instant::now();
        if let Some((kind, start_time)) = self.current_io.take() {
            let io_duration = now.duration_since(start_time);
            self.stats.io_wait_duration += io_duration;
            self.stats.io_count += 1;

            let cat = kind.category().to_string();
            *self.stats.io_breakdown.entry(cat).or_insert(0) += 1;
            self.stats.io_history.push((kind.description(), io_duration));
        }

        self.last_state_change = now;
        self.state = FiberState::Running;
    }

    /// Record a cooperative yield event.
    pub fn record_yield(&mut self) {
        let now = Instant::now();
        if self.state.is_running() {
            self.stats.cpu_duration += now.duration_since(self.last_state_change);
        }
        self.stats.yield_count += 1;
        self.last_state_change = now;
        self.state = FiberState::Yielded;
    }

    /// Mark the fiber as successfully completed.
    pub fn complete(&mut self, value: PhpValue) {
        let now = Instant::now();
        if self.state.is_running() {
            self.stats.cpu_duration += now.duration_since(self.last_state_change);
        }
        self.stats.total_duration = now.duration_since(self.created_at);
        self.stats.is_completed = true;
        self.result = Some(value.clone());
        self.state = FiberState::Completed(value);
        self.last_state_change = now;
    }

    /// Mark the fiber as failed with an error message.
    pub fn fail(&mut self, error_msg: impl Into<String>) {
        let now = Instant::now();
        if self.state.is_running() {
            self.stats.cpu_duration += now.duration_since(self.last_state_change);
        }
        self.stats.total_duration = now.duration_since(self.created_at);
        let msg = error_msg.into();
        self.state = FiberState::Failed(msg);
        self.last_state_change = now;
    }
}

// =========================================================================
// Thread-Local Fiber Management & Scoping
// =========================================================================

thread_local! {
    /// Active fiber currently executing on this OS worker thread.
    static CURRENT_FIBER: RefCell<Option<Fiber>> = const { RefCell::new(None) };

    /// Fiber stack allowing safe nested fiber execution (e.g., child fibers, defer hooks).
    static FIBER_STACK: RefCell<Vec<Fiber>> = const { RefCell::new(Vec::new()) };
}

/// Check if the current worker thread is executing inside a fiber context.
pub fn is_current_fiber() -> bool {
    CURRENT_FIBER.with(|cell| cell.borrow().is_some())
}

/// Retrieve the active [`FiberId`] if running in a fiber.
pub fn current_fiber_id() -> Option<FiberId> {
    CURRENT_FIBER.with(|cell| cell.borrow().as_ref().map(|f| f.id))
}

/// Retrieve the active fiber's name if running in a fiber.
pub fn current_fiber_name() -> Option<String> {
    CURRENT_FIBER.with(|cell| cell.borrow().as_ref().map(|f| f.name.clone()))
}

/// Retrieve the active fiber's state if running in a fiber.
pub fn current_fiber_state() -> Option<FiberState> {
    CURRENT_FIBER.with(|cell| cell.borrow().as_ref().map(|f| f.state.clone()))
}

/// Retrieve a snapshot of the active fiber's statistics.
pub fn current_fiber_stats() -> Option<FiberStats> {
    CURRENT_FIBER.with(|cell| cell.borrow().as_ref().map(|f| f.stats.clone()))
}

/// Execute a closure with mutable access to the current thread's active fiber.
pub fn with_current_fiber<R>(f: impl FnOnce(&mut Fiber) -> R) -> Option<R> {
    CURRENT_FIBER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        borrow.as_mut().map(f)
    })
}

/// Record the start of an I/O operation on the currently active fiber.
pub fn record_fiber_io_start(kind: IoKind) {
    with_current_fiber(|fiber| {
        fiber.record_io_start(kind);
    });
}

/// Record the completion of an I/O operation on the currently active fiber.
pub fn record_fiber_io_finish() {
    with_current_fiber(|fiber| {
        fiber.record_io_finish();
    });
}

/// Record a cooperative yield event on the currently active fiber.
pub fn record_fiber_yield() {
    with_current_fiber(|fiber| {
        fiber.record_yield();
    });
}

/// Retrieve a fiber-local variable from the active fiber.
pub fn get_fiber_local(key: &str) -> Option<PhpValue> {
    with_current_fiber(|fiber| fiber.context.get(key).cloned()).flatten()
}

/// Store a fiber-local variable in the active fiber.
pub fn set_fiber_local(key: impl Into<String>, value: PhpValue) {
    with_current_fiber(|fiber| {
        fiber.context.set(key, value);
    });
}

/// RAII scope guard that activates a fiber in thread-local storage and restores
/// any previously active fiber when dropped.
#[derive(Debug)]
pub struct FiberScopeGuard {
    active: bool,
}

impl FiberScopeGuard {
    /// Enter the scope of the given fiber.
    pub fn enter(mut fiber: Fiber) -> Self {
        fiber.set_running();
        CURRENT_FIBER.with(|cell| {
            let previous = cell.borrow_mut().take();
            if let Some(prev) = previous {
                FIBER_STACK.with(|stack| stack.borrow_mut().push(prev));
            }
            *cell.borrow_mut() = Some(fiber);
        });
        Self { active: true }
    }

    /// Mark the fiber as completed with the given value and extract it.
    pub fn finish(mut self, value: PhpValue) -> Fiber {
        self.active = false;
        let mut fiber = CURRENT_FIBER.with(|cell| cell.borrow_mut().take())
            .expect("Fiber must be active in scope");
        fiber.complete(value);

        // Restore parent fiber if any
        CURRENT_FIBER.with(|cell| {
            let parent = FIBER_STACK.with(|stack| stack.borrow_mut().pop());
            *cell.borrow_mut() = parent;
        });

        fiber
    }

    /// Mark the fiber as failed with the given error message and extract it.
    pub fn fail(mut self, error: impl Into<String>) -> Fiber {
        self.active = false;
        let mut fiber = CURRENT_FIBER.with(|cell| cell.borrow_mut().take())
            .expect("Fiber must be active in scope");
        fiber.fail(error);

        // Restore parent fiber if any
        CURRENT_FIBER.with(|cell| {
            let parent = FIBER_STACK.with(|stack| stack.borrow_mut().pop());
            *cell.borrow_mut() = parent;
        });

        fiber
    }

    /// Extract the active fiber without modifying its terminal status.
    pub fn extract(mut self) -> Fiber {
        self.active = false;
        let fiber = CURRENT_FIBER.with(|cell| cell.borrow_mut().take())
            .expect("Fiber must be active in scope");

        // Restore parent fiber if any
        CURRENT_FIBER.with(|cell| {
            let parent = FIBER_STACK.with(|stack| stack.borrow_mut().pop());
            *cell.borrow_mut() = parent;
        });

        fiber
    }
}

impl Drop for FiberScopeGuard {
    fn drop(&mut self) {
        if self.active {
            // Fiber exited without explicit finish/fail: pop and restore
            let _ = CURRENT_FIBER.with(|cell| cell.borrow_mut().take());
            CURRENT_FIBER.with(|cell| {
                let parent = FIBER_STACK.with(|stack| stack.borrow_mut().pop());
                *cell.borrow_mut() = parent;
            });
        }
    }
}

/// Activate a fiber for the duration of a scope, automatically popping when the guard drops.
pub fn enter_fiber(fiber: Fiber) -> FiberScopeGuard {
    FiberScopeGuard::enter(fiber)
}

// =========================================================================
// Cooperative Fiber Scheduler
// =========================================================================

/// Result of executing one cooperative step on a fiber task.
pub enum FiberStepResult {
    /// The fiber task finished with a final value.
    Done(PhpValue),
    /// The fiber task yielded control cooperatively.
    Yield,
    /// The fiber task is suspended waiting for an I/O operation.
    WaitIo(IoKind),
    /// The fiber task failed with an unhandled runtime error.
    Error(RuntimeError),
}

/// Scheduler telemetry and aggregated metrics across all scheduled fibers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchedulerStats {
    /// Total fibers spawned on this scheduler.
    pub total_spawned: usize,
    /// Total fibers completed successfully.
    pub total_completed: usize,
    /// Total fibers failed.
    pub total_failed: usize,
    /// Total cooperative context switches executed.
    pub total_context_switches: usize,
    /// Total cooperative I/O wait events handled.
    pub total_io_events: usize,
}

/// Trait object representing an executable task inside a fiber.
type FiberTask = Box<dyn FnMut(&mut Fiber) -> FiberStepResult + Send>;

/// High-performance cooperative Fiber Scheduler running multiple local fibers
/// sequentially or interleaved on an OS worker thread.
pub struct FiberScheduler {
    /// Map of all fibers registered in the scheduler.
    fibers: IndexMap<FiberId, Fiber>,
    /// FIFO queue of ready fiber IDs.
    ready_queue: VecDeque<FiberId>,
    /// Dynamic step closures corresponding to active fibers.
    tasks: HashMap<FiberId, FiberTask>,
    /// Completed fiber results.
    results: IndexMap<FiberId, Result<PhpValue, RuntimeError>>,
    /// Aggregated scheduler metrics.
    stats: SchedulerStats,
}

impl Default for FiberScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl FiberScheduler {
    /// Create a new empty fiber scheduler.
    pub fn new() -> Self {
        Self {
            fibers: IndexMap::new(),
            ready_queue: VecDeque::new(),
            tasks: HashMap::new(),
            results: IndexMap::new(),
            stats: SchedulerStats::default(),
        }
    }

    /// Spawn a new cooperative fiber with an iterative step closure.
    pub fn spawn<F>(&mut self, name: impl Into<String>, task: F) -> FiberId
    where
        F: FnMut(&mut Fiber) -> FiberStepResult + Send + 'static,
    {
        let fiber = Fiber::new(name);
        let id = fiber.id;
        self.fibers.insert(id, fiber);
        self.tasks.insert(id, Box::new(task));
        self.ready_queue.push_back(id);
        self.stats.total_spawned += 1;
        id
    }

    /// Spawn a simple run-to-completion closure within a managed fiber.
    pub fn spawn_simple<F>(&mut self, name: impl Into<String>, task: F) -> FiberId
    where
        F: FnOnce() -> Result<PhpValue, RuntimeError> + Send + 'static,
    {
        let mut opt_task = Some(task);
        self.spawn(name, move |_fiber| {
            if let Some(f) = opt_task.take() {
                match f() {
                    Ok(val) => FiberStepResult::Done(val),
                    Err(err) => FiberStepResult::Error(err),
                }
            } else {
                FiberStepResult::Done(PhpValue::Null)
            }
        })
    }

    /// Execute a single step on the next ready fiber in the run queue.
    ///
    /// Returns `true` if a fiber made progress, or `false` if the ready queue was empty.
    pub fn step(&mut self) -> bool {
        let fiber_id = match self.ready_queue.pop_front() {
            Some(id) => id,
            None => return false,
        };

        let mut fiber = match self.fibers.shift_remove(&fiber_id) {
            Some(f) => f,
            None => return false,
        };

        let mut task = match self.tasks.remove(&fiber_id) {
            Some(t) => t,
            None => {
                self.fibers.insert(fiber_id, fiber);
                return false;
            }
        };

        self.stats.total_context_switches += 1;
        fiber.set_running();

        // Run one cooperative quantum/step within the fiber scope guard
        let guard = enter_fiber(fiber);
        let step_res = {
            let active_ref = CURRENT_FIBER.with(|cell| {
                // Safely borrow mutable fiber reference
                let mut borrow = cell.borrow_mut();
                if let Some(f) = borrow.as_mut() {
                    task(f)
                } else {
                    FiberStepResult::Error(RuntimeError::new(
                        RuntimeErrorKind::Custom {
                            message: "Fiber lost from thread-local storage".to_string(),
                        },
                        Span::default(),
                    ))
                }
            });
            active_ref
        };

        let mut fiber = guard.extract();

        match step_res {
            FiberStepResult::Done(val) => {
                fiber.complete(val.clone());
                self.results.insert(fiber_id, Ok(val));
                self.fibers.insert(fiber_id, fiber);
                self.stats.total_completed += 1;
            }
            FiberStepResult::Error(err) => {
                fiber.fail(err.to_string());
                self.results.insert(fiber_id, Err(err));
                self.fibers.insert(fiber_id, fiber);
                self.stats.total_failed += 1;
            }
            FiberStepResult::Yield => {
                fiber.record_yield();
                self.fibers.insert(fiber_id, fiber);
                self.tasks.insert(fiber_id, task);
                self.ready_queue.push_back(fiber_id);
            }
            FiberStepResult::WaitIo(kind) => {
                fiber.record_io_start(kind);
                self.stats.total_io_events += 1;
                // In cooperative simulation, complete I/O immediately and re-queue
                fiber.record_io_finish();
                self.fibers.insert(fiber_id, fiber);
                self.tasks.insert(fiber_id, task);
                self.ready_queue.push_back(fiber_id);
            }
        }

        true
    }

    /// Run all queued fibers to completion.
    pub fn run_all(&mut self) -> IndexMap<FiberId, Result<PhpValue, RuntimeError>> {
        while self.step() {}
        self.results.clone()
    }

    /// Run the scheduler until a specific fiber completes.
    pub fn run_until_complete(
        &mut self,
        target_id: FiberId,
    ) -> Option<Result<PhpValue, RuntimeError>> {
        while !self.results.contains_key(&target_id) {
            if !self.step() {
                break;
            }
        }
        self.results.get(&target_id).cloned()
    }

    /// Access a fiber by ID.
    pub fn get_fiber(&self, id: FiberId) -> Option<&Fiber> {
        self.fibers.get(&id)
    }

    /// Access fiber statistics by ID.
    pub fn get_stats(&self, id: FiberId) -> Option<&FiberStats> {
        self.fibers.get(&id).map(|f| &f.stats)
    }

    /// Retrieve the final result of a completed fiber.
    pub fn get_result(&self, id: FiberId) -> Option<&Result<PhpValue, RuntimeError>> {
        self.results.get(&id)
    }

    /// Aggregated scheduler metrics.
    #[inline]
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    /// Number of active (in-flight) fibers remaining.
    #[inline]
    pub fn active_count(&self) -> usize {
        self.ready_queue.len()
    }

    /// List of all registered fiber IDs.
    pub fn all_fiber_ids(&self) -> Vec<FiberId> {
        self.fibers.keys().cloned().collect()
    }
}
