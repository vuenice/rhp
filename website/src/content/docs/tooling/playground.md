---
title: "Playground App"
description: "Interactive desktop app to run RHP scripts and debug local, Docker, or remote applications."
section: "Tooling"
order: 5
---

The **RHP Playground** (`rhp-play`) is a native, ultra-responsive desktop GUI application built for instant code execution, rapid prototyping, and real-time application debugging.

Inspired by tools like **TweakPHP**, RHP Playground lets developers test code snippets, verify syntax, inspect runtime telemetry, and interact directly with local, containerized, or remote applications without boilerplate or restarts.

<div class="my-8 rounded-2xl overflow-hidden border border-slate-200 dark:border-slate-800 shadow-2xl bg-[#0B0F19]">
  <div class="flex items-center justify-between px-4 py-2.5 bg-[#0E1422] border-b border-slate-800/80 text-xs text-slate-400">
    <div class="flex items-center space-x-2">
      <div class="w-3 h-3 rounded-full bg-rose-500/80"></div>
      <div class="w-3 h-3 rounded-full bg-amber-500/80"></div>
      <div class="w-3 h-3 rounded-full bg-emerald-500/80"></div>
    </div>
    <span class="font-mono text-slate-300 font-medium">RHP Playground — Standalone & Connected Runner</span>
    <span class="text-[10px] text-slate-500 font-mono">v0.1.0</span>
  </div>
  <img
    src="/playground/rhp-play.png"
    alt="RHP Playground Desktop App interface running an RHP script with execution telemetry"
    class="w-full h-auto block m-0"
    loading="lazy"
  />
</div>

---

## Key Capabilities

### 1. Standalone Script Runner
Jump right in and execute RHP code with zero configuration. Test modern language features including:
- **Rust-Backed Collections**: Experiment with `Arr::list`, `Arr::dict`, and `Arr::set` pipelines.
- **Tokio Concurrency**: Test `Future::wait` and asynchronous deferred tasks (`Future::defer`).
- **Deterministic Memory**: Validate bump arena allocations and zero-GC performance in real time.

### 2. Multi-Environment Application Debugging
Connect RHP Playground directly to your existing application stacks to evaluate scripts within the application's boot context:

- **Local Projects**: Point `rhp-play` to your local Laravel, Vapor, or RHP application folder. Scripts execute with access to your models, configuration, and services.
- **Docker Containers**: Debug containerized backend services running in local Docker environments without needing manual shell-exec gymnastics.
- **Remote SSH Environments**: Securely connect over SSH to staging or remote servers to test queries, inspect models, and reproduce edge cases on live runtimes.

### 3. Real-Time Telemetry & Performance Stats
Every execution provides immediate feedback in the dedicated output panel:
- **Startup & Execution Timing**: Measure compilation and execution latency in milliseconds.
- **Memory Footprint**: Track peak memory consumed during script execution.
- **GC Metrics**: Verify 0.0ms GC pause times thanks to RHP's deterministic RAII memory model.

### 4. Saved Connections & Workspaces
Easily switch between different projects and environments using the sidebar:
- Color-coded connection badges for quick identification.
- Independent scratchpads and code history per connection.
- Persistent configuration saved across sessions in `~/.rhp_playground_apps.json`.

---

## Launching RHP Playground

RHP Playground is built with Rust and `egui` / `eframe` for native multi-platform performance and low memory overhead.

### Running from Source

You can run the playground directly from the RHP workspace:

```bash
# Debug mode (fast compilation)
cargo run -p rhp-play

# Release mode (maximum UI and JIT responsiveness)
cargo run --release -p rhp-play
```

### CLI Options

You can also specify a custom project path or binary target upon launch:

```bash
# Launch with an active project directory
cargo run -p rhp-play -- --project /path/to/my-laravel-app
```

---

## Connecting to an Application

1. Click the **+** (New Connection) button in the sidebar.
2. Select your target connection type:
   - **Local**: Enter your project root path and the path to your `rhp` binary.
   - **Docker**: Select your container name and working directory.
   - **Remote**: Provide SSH host credentials and remote project path.
3. Click **Create Connection**.
4. Write your script in the editor and press <kbd>Ctrl</kbd>+<kbd>Enter</kbd> (or <kbd>⌘</kbd>+<kbd>Enter</kbd> on macOS) to execute.
