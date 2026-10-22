---
title: "Playground App"
description: "Interactive desktop app to run RHP scripts and debug local, Docker, or remote applications."
section: "Tooling"
order: 5
---

The **RHP Playground** (`rhp-play`) is a clean, minimalist, high-performance desktop GUI application built for instant code execution, rapid prototyping, and real-time application debugging.

Designed specifically after the workflow popularized by **[TweakPHP](https://github.com/tweakphp/tweakphp)**, RHP Playground allows you to run scratchpad scripts with zero configuration, and connect directly to your local projects, Docker containers, or remote SSH servers to tweak and test code inside your running application context.

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

## How to Run the Playground App

RHP Playground is built with **Tauri v2**, **Vanilla TypeScript**, and **CodeMirror 6**, backed by a high-performance asynchronous Rust engine (`tokio`, `rfd`) for native performance, instant startup, and effortless styling without bloated frontend frameworks.

### 1. Development Mode (with Hot Reload)

From the `playground` directory:

```bash
cd playground
npm install
npm run tauri dev
```

### 2. Build & Run the Pre-compiled Binary

To build the frontend and compile an optimized release binary:

```bash
cd playground
npm run build
cargo build --release

# Launch the compiled binary:
./target/release/rhp-play
```

### 3. Install Globally to System `PATH`

You can install both the `rhp` engine compiler and the `rhp-play` desktop companion into `~/.cargo/bin`:

```bash
# Install the RHP compiler CLI
cargo install --path .

# Install the RHP Playground desktop GUI
cargo install --path playground
```

Once installed, launch the playground from anywhere:

```bash
rhp-play
```

> **Binary Auto-Discovery:** `rhp-play` automatically searches for the companion `rhp` compiler binary in the current directory, the parent directory, or in your system `$PATH`. You can also configure a custom binary path per project connection.

---

## Interface Layout & Features

### 1. Left Sidebar Rail
- **`+` Button (Create Connection)**: Opens a modal with connection options:
  - **Local**: Point to a local directory (e.g. `/home/user/code/my-app`) and optionally specify a custom `rhp` binary.
  - **Docker**: Containerized execution target for local Docker dev environments.
  - **Remote**: Remote server execution over SSH.
- **Connection Badges**: Displays each configured project with a distinctive initial badge (e.g. `L` for Local, `V` for Vapor, `M` for My App). Click any badge to instantly switch active workspace and restore your script state.
- **Right-Click Settings**: Click the settings icon or right-click any connection to update project paths or delete connections.

### 2. Split-Pane Workspace
- **Left Panel (Code Editor)**:
  - Clean monospace editor with line numbers.
  - Real-time syntax highlighting for PHP and RHP constructs.
  - Full keyboard editing with undo/redo support.
- **Right Panel (Output Console)**:
  - Live execution output with line numbering.
  - Selectable text for easy copying of query results, debug dumps (`print_r`, `var_dump`), or error traces.
- **Draggable Splitter**: Click and drag the divider between editor and output to adjust your ideal screen split.

### 3. Execution & Process Controls
- **Run Button**: Prominent execution trigger (or press <kbd>Ctrl</kbd>+<kbd>Enter</kbd> / <kbd>⌘</kbd>+<kbd>Enter</kbd>).
- **Execution Spinner**: Enters an active `⏳ Running...` state while your script executes asynchronously on the background Tokio runtime.
- **Instant Cancel**: A `⏹ Cancel` button appears dynamically during execution, allowing you to instantly terminate runaway queries, infinite loops, or long-running tasks without freezing the GUI.

---

## Workflows: Standalone vs. Connected

### Standalone Scratchpad Mode
By default, the playground operates as a lightweight scratchpad. Use it to:
- Test RHP language syntax and operators without project scaffolding.
- Experiment with native Rust collections (`Arr::list`, `Arr::dict`, `Arr::set`).
- Verify concurrent Tokio fibers with `Future::wait` and `Future::defer`.
- Inspect zero-GC performance and bump arena allocation behaviors.

### Connected Application Mode (TweakPHP Style)
When connected to an existing application directory:
1. Click the **`+`** icon in the sidebar rail.
2. Enter your **App Name** and select your **Project Path**.
3. Click **Create Connection**.
4. The playground runs scripts in the context of your application, letting you:
   - Call application classes, service providers, and models.
   - Run live database queries and test Eloquent or SQL logic.
   - Verify bug fixes or tweak controller logic before writing formal tests.

All connections and code snippets are automatically persisted in `~/.rhp_playground_apps.json` so your workspace is preserved between restarts.
