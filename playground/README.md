# RHP Playground

A clean, minimalist, high-performance desktop playground for **RHP / PHP**, designed exactly like [TweakPHP](https://github.com/tweakphp/tweakphp).

Built 100% in **pure Rust** with **`eframe` / `egui`**, **`syntect`**, **`tokio`**, and **`rfd`**.

---

## 📸 Layout Overview

```
+----+--------------------------------+--------------------------------+
|    |  LEFT PANEL: CODE EDITOR       |  RIGHT PANEL: OUTPUT           |
| [+]|  1 <?php                       |  1 Array                       |
|    |  2                             |  2 (                           |
| [L]|  3 collect([                   |  3     [name] => TweakPHP      |
|    |  4     "name" => "TweakPHP",   |  4     [link] => ...           |
| [V]|  5 ]);                         |  5 )                           |
|    |                                |                                |
|    |             [ ▶ Run (Ctrl+↵) ] |                                |
+----+--------------------------------+--------------------------------+
```

### 1. Left Sidebar Rail
- **`+` Button (Create Connection)**: Opens a modal popup with 3 tabs: **Local**, **Docker**, and **Remote**.
  - **Local Connection**: Requires **Project Path** and **RHP Path** (with native folder/file pickers).
  - **Docker & Remote**: Reserved for upcoming releases.
- **App Badges (`L`, `V`, ...)**: Displays each configured app with a rounded badge containing the first letter of the app's name.
  - Clicking a badge seamlessly switches the active workspace and playground state.

### 2. Two Simple Plain Panels
- **Left Panel (Editor)**:
  - Line numbers gutter (`1, 2, 3...`).
  - Monospace code editor with real-time PHP / RHP syntax highlighting.
  - Minimalist and uncluttered.
- **Right Panel (Output)**:
  - Line numbers gutter (`1, 2, 3...`).
  - Selectable monospace execution output.
- **Draggable Splitter**: Easily adjust the horizontal ratio between the editor and output panels.

### 3. Run & Cancel Controls
- **Run Button**: Clean, prominent execution trigger (supports `Ctrl+Enter` / `⌘+Enter`).
- **Loading State**: When executing, the button enters a loading state (`⏳ Running...`).
- **Cancel Button**: A `⏹ Cancel` button dynamically appears while execution is in progress, allowing you to immediately terminate long-running or looping scripts.

---

## 🚀 Installation & Running

The Playground is built as a dedicated companion binary named **`rhp-play`**:

### Run with Cargo:
```bash
cargo run -p rhp-play
```

### Build & Install:
```bash
cargo build --release -p rhp-play
# Produces: target/release/rhp-play
```

To install both `rhp` and `rhp-play` to `~/.cargo/bin`:
```bash
cargo install --path .
cargo install --path playground
```

Once installed, simply run:
```bash
rhp-play
```
`rhp-play` automatically discovers its companion `rhp` compiler binary from the same directory or system `PATH`!
