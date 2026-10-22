# RHP Playground

A clean, minimalist, high-performance desktop playground for **RHP / PHP**, designed like [TweakPHP](https://github.com/tweakphp/tweakphp).

Built with **Tauri v2**, **Vanilla TypeScript**, and **CodeMirror 6** for effortless styling, maintainability, and native desktop performance without heavy frontend frameworks.

---

## 📸 Layout Overview

```
+----+--------------------------------+--------------------------------+
|    |  LEFT PANEL: CODE EDITOR       |  RIGHT PANEL: OUTPUT           |
| [+]|  1 $data = [                   |  1 Array                       |
|    |  2     "name" => "RHP",        |  2 (                           |
| [L]|  3 ];                          |  3     [name] => RHP           |
|    |  4                             |  4 )                           |
| [V]|  5 print_r($data);             |                                |
|    |                                |                                |
|    |             [ ▶ Run (Ctrl+↵) ] |                                |
+----+--------------------------------+--------------------------------+
```

### 1. Left Sidebar Rail
- **`▶` Button (Standalone Scratchpad)**: Switches instantly to the standalone RHP scratchpad.
- **`+` Button (Create Connection)**: Opens a modal popup with 3 tabs: **Local**, **Docker**, and **Remote**.
  - **Local Connection**: Requires **Project Path** and **RHP Path** (with native folder/file pickers).
  - **Docker & Remote**: Reserved for upcoming releases.
- **App Badges (`L`, `V`, ...)**: Displays each configured app with a rounded badge containing the initial letter of the app's name.
  - Clicking a badge seamlessly switches the active workspace and playground state.

### 2. Two Resizable Panels
- **Left Panel (Editor)**:
  - Powered by **CodeMirror 6** with real-time PHP / RHP syntax highlighting and bracket matching.
  - Line numbers gutter (`1, 2, 3...`).
  - Active line highlighting, indentation guides, and clean dark mode styling.
- **Right Panel (Output)**:
  - Selectable monospace execution output.
  - Formatted error banners, execution duration indicator (`✓ 12ms`), copy button, and clear button.
- **Draggable Splitter**: Easily adjust the horizontal ratio between the editor and output panels.

### 3. Run & Cancel Controls
- **Run Button**: Clean, prominent execution trigger (supports `Ctrl+Enter` / `⌘+Enter`).
- **Loading State**: When executing, the button enters a loading state (`⏳ Running...`).
- **Cancel Button**: A `⏹ Cancel` button dynamically appears while execution is in progress, allowing you to immediately terminate long-running or looping scripts.

---

## 🚀 Development & Running

### Prerequisites
On Linux (Debian/Ubuntu):
```bash
sudo apt update && sudo apt install -y libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
```

### Run in Development (with Hot Reload):
```bash
cd playground
npm install
npm run tauri dev
```

### Build Frontend & Release Binary:
```bash
cd playground
npm run build
cargo build --release
```
The compiled binary will be located at:
```bash
./playground/target/release/rhp-play
```

### Run Unit Tests:
```bash
cd playground
cargo test
```
