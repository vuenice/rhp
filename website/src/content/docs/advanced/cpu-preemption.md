---
title: "CPU Preemption & Instruction Budgeting"
description: "Cooperative preemption, instruction tick budget (10,000 ticks), Cranelift JIT yield trampolines, Dev Mode infinite loop detection (1,000 warning, 100,000 hard limit), and ini_set exemptions."
section: "Advanced"
order: 7
---

## Overview

In modern high-concurrency runtimes, worker processes rely on a cooperative, single-threaded or thread-pool event loop to handle concurrent HTTP requests, database queries, and async I/O fibers. 

A traditional pitfall in interpreted and JIT-compiled runtimes is the **unyielding tight loop**:

```php
// An accidental or intentional infinite loop
while ($running) {
    // Tight CPU-bound compute with zero I/O
    $hash = hash_step($hash);
}
```

Without preemption, such a loop monopolizes the worker's CPU thread, starving other queued requests, background timers, and I/O tasks. 

RHP solves this through a **dual-tier CPU scheduling and preemption architecture**:
1. **Production Mode (Default):** Seamless **Instruction Budgeting & Cooperative Tick Slicing** (10,000 instructions per quantum). When the tick budget expires at a loop back-edge or function prologue, the VM or Cranelift JIT yields execution cooperatively to the event loop and immediately resumes. Valid long-running tasks (e.g. processing 1,000,000 rows) complete without error.
2. **Development Mode (`rhp dev`):** An **Infinite Loop Watchdog** that emits an early diagnostic warning at **1,000 iterations** and enforces a hard ceiling at **100,000 iterations (1 lakh)** to immediately catch runaway loops during local development.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   RHP CPU PREEMPTION & SCHEDULING                      │
├───────────────────────────────────┬────────────────────────────────────┤
│       PRODUCTION MODE             │       DEVELOPMENT MODE             │
│  (rhp run build / Default)        │       (rhp dev / rhp test)         │
├───────────────────────────────────┼────────────────────────────────────┤
│ • Instruction Budget: 10,000 ticks│ • Warn Threshold: 1,000 iterations │
│ • At Back-Edges & Prologues:      │   (Emits diagnostic warning once)  │
│   tick_counter.saturating_sub(1)  │ • Hard Ceiling: 100,000 iterations │
│ • When Counter == 0:              │   (Raises InfiniteLoopLimitError)  │
│   - Reset budget to 10,000        │ • In-Code Exemption:               │
│   - Fiber yield / thread yield    │   ini_set('max_loop_iterations',0) │
│   - Resumes seamlessly (No Crash) │ • Catches accidental while(true){} │
└───────────────────────────────────┴────────────────────────────────────┘
```

---

## 1. Instruction Budgeting & Cooperative Preemption (Production)

### Execution Mechanics

Every [`VM`](/docs/advanced/compiler-internals) instance maintains execution budget state:
* `tick_budget: u32`: Configured quantum (default: `10,000` instructions).
* `tick_counter: u32`: Decrementing instruction counter.
* `total_preemptions: u64`: Telemetry tracking total cooperative preemption events.

At every **function prologue** (`ip == 0`) and **loop back-edge** (`target <= ip`), the VM checks the instruction counter:

```rust
// Simplified interpreter dispatch logic from src/vm/engine.rs
if self.tick_counter == 0 {
    self.tick_counter = self.tick_budget;
    self.total_preemptions += 1;
    
    // 1. Record yield in fiber telemetry stats
    crate::vm::fiber::record_fiber_yield();
    
    // 2. Invoke scheduler yielder hook if registered
    crate::vm::fiber::CURRENT_YIELDER.with(|cell| {
        if let Some(yielder) = cell.borrow_mut().as_mut() {
            yielder.suspend(YieldReason::CooperativeYield);
        }
    });
    
    // 3. Relinquish OS thread quantum
    std::thread::yield_now();
} else {
    self.tick_counter = self.tick_counter.saturating_sub(1);
}
```

### Zero-Allocation Fiber Yielding

When executing inside an RHP local fiber ([`enter_fiber`](/docs/guide/standard-library)):
1. `record_fiber_yield()` increments `fiber.stats.yield_count` and updates the CPU duration timestamp.
2. The scheduler re-enqueues the fiber onto the ready queue.
3. Pending network packets, database responses, or other concurrent fibers run.
4. The fiber resumes at the exact instruction pointer without stack unwinding or memory allocation.

---

## 2. Cranelift JIT Loop Preemption

Hot loops in RHP are compiled down to native machine code via **Cranelift JIT** (`src/jit/compiler.rs`). An uninstrumented native loop would run in bare CPU registers and bypass the interpreter's dispatch checks entirely.

To preserve the preemption contract in compiled native code without degrading CPU pipelining:

### Native Register Counter & Trampoline
In `JitCompiler::compile_loop`:
1. A 64-bit integer variable `tick_counter_var` is allocated in Cranelift IR, initialized to `10,000`.
2. At every backward jump block (`Jump`, `JumpIfTrue`, `JumpIfFalse` where `target <= ip`), Cranelift emits:
   ```text
   sub   rax, 1             ; decrement native tick counter
   jz    .yield_trampoline  ; jump if budget reached 0
   jmp   .loop_header       ; loop at CPU register speed
   ```
3. When `tick_counter_var == 0`, execution branches to `.yield_trampoline`:
   * Stores all live Cranelift CPU registers back into the [`NativeSlot`](/docs/advanced/compiler-internals) register file.
   * Returns `target` (the loop header IP) to the VM.
4. The VM engine recognizes the budget exhaustion, resets `vm.tick_counter = DEFAULT_BUDGET`, invokes `trigger_cooperative_yield()`, and resumes execution.

```text
┌────────────────────────────────────────────────────────┐
│               CRANELIFT JIT NATIVE LOOP                │
│                                                        │
│   .loop_header:                                        │
│       ; Unboxed native math & register operations      │
│       ...                                              │
│       sub  rax, 1                                      │
│       jz   .yield_trampoline                           │
│       jmp  .loop_header                                │
│                                                        │
│   .yield_trampoline:                                   │
│       ; Flush registers to NativeSlot slots_ptr        │
│       store_regs(slots_ptr)                            │
│       return loop_header_ip   ──► Resumes in VM engine │
└────────────────────────────────────────────────────────┘
```

---

## 3. Development Mode: Warnings & Hard Limits

During development and automated testing (`rhp dev`, `rhp test`), developers frequently introduce accidental infinite loops (e.g. missing `$i++` incrementor or inverted exit conditions).

To prevent IDEs and terminal sessions from locking up, RHP activates the **Dev Mode Infinite Loop Guard**:

### 1,000 Iteration Warning
When any loop executes more than **1,000 iterations**, the VM triggers a non-fatal warning:

```text
Warning: Loop in <main> at ip=4 exceeded 1000 iterations in Dev Mode (possible infinite loop). in server.rhp on line 12
```

* Emitted **only once** per loop run to avoid log flooding.
* Does not interrupt execution; loops performing moderate iterations continue normally.

### 100,000 Iteration Hard Limit (1 Lakh)
If the loop continues spinning and reaches **100,000 iterations**, RHP immediately halts execution with a fatal runtime error:

```text
Fatal Error: Loop exceeded hard limit of 100000 iterations in Dev Mode (at ip=4). 
Ensure loop termination condition or run in production mode with cooperative preemption.
```

* Emits [`RuntimeErrorKind::InfiniteLoopLimitExceeded`](/docs/advanced/compiler-internals).
* Automatically resets when loops exit or when function frames return.

---

## 4. In-Code & Configuration Exemptions

If an algorithm legitimately requires hundreds of thousands of iterations in Dev Mode (such as large matrix operations, image manipulation, or cryptography):

### Dynamic `ini_set` and `ini_get` Directives

Developers can inspect or override limits directly in PHP/RHP code:

```php
// 1. Completely exempt the current script from the 100,000 dev-mode hard limit
ini_set('max_loop_iterations', '0');

// 2. Or raise the limit for heavy data processing
ini_set('max_loop_iterations', '500000');

// 3. Customize the warning threshold
ini_set('warn_loop_iterations', '5000');

// 4. Inspect or adjust the instruction budget quantum (ticks before yield)
$budget = ini_get('instruction_budget'); // "10000"
ini_set('instruction_budget', '25000');
```

### Manual Cooperative Yielding

Calling `Fiber::yield()` voluntarily gives up execution time to other fibers and immediately resets the instruction budget:

```php
while ($streaming) {
    processBatchChunk();
    
    // Voluntarily yield time-slice to pending network requests
    Fiber::yield();
}
```

---

## 5. How RHP Compares to Other Runtimes

| Runtime / Language | Preemption Mechanism | Unbounded Loop Behavior | Per-Loop Hard Limit? |
| :--- | :--- | :--- | :--- |
| **RHP (Production)** | **Instruction Budgeting (10k ticks)** on loop back-edges & prologues in Interpreter & Cranelift JIT | **Yields cooperatively**; resumes seamlessly without crashing | **No** (Unbounded in production; 100k limit in Dev Mode) |
| **Erlang / BEAM** | **Reductions (~2,000–4,000)** decremented per function call / loop | **Yields process slice**; runs infinite loops safely | **No** (Processes share CPU fairly forever) |
| **Go Runtime** | **Asynchronous Preemption** via POSIX signals (`SIGURG` in Go 1.14+) | Preempts goroutine; moves to run queue | **No** |
| **WebAssembly (Wasmtime)** | **Fuel Metering** or **Epoch Interruption** injected into loop headers | Traps or yields to host event loop | Configurable fuel limits |
| **Roblox Luau / Lua** | `lua_sethook` with `LUA_MASKCOUNT` instruction counter | Yields or triggers frame execution timeout | Dev watchdog in Studio |
| **PHP (Zend Engine)** | `max_execution_time` via OS timer signals (`SIGVTALRM`) | Aborts script after $N$ seconds wall/CPU time | **No** (`declare(ticks=N)` deprecated for JIT) |
| **V8 / Node.js** | **Stack Guard Interrupt Flag** checked on loop headers | Watchdog thread sets interrupt for timeouts | Configured at Worker/VM level |
| **Solidity / EVM** | **Gas Metering** per opcode | Aborts with `OutOfGas` | Hard limit bounded by block gas |

---

## 6. Rust API Reference

For developers embedding RHP or building custom schedulers:

```rust
use rhp::vm::VM;
use rhp::vm::fiber::{set_current_yielder, YieldReason};

// 1. Configure VM preemption & Dev Mode
let mut vm = VM::new(module, /* jit_enabled */ true)
    .with_dev_mode(true)
    .with_instruction_budget(10_000)
    .with_loop_limits(/* warn */ 1_000, /* hard */ 100_000);

// 2. Attach a custom cooperative yielder
set_current_yielder(Some(Box::new(|reason: YieldReason| {
    match reason {
        YieldReason::CooperativeYield => {
            // Re-enqueue task in Tokio or custom event loop
        }
        YieldReason::ManualYield => { /* Fiber::yield() */ }
        YieldReason::IoWait(io) => { /* Async I/O wait */ }
    }
})));

let result = vm.run()?;
println!("Total preemption yields: {}", vm.total_preemptions);
```
