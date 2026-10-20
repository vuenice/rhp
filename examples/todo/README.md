# TaskFlow RHP: Reactive TODO Application

A full-stack, reactive TODO application written in **RHP** (Rust-Powered PHP-family language) paired with **HTMX** and **Alpine.js** in the frontend, proving that the RHP engine, compiler, type system, register bytecode VM, standard library facades, and built-in web server work seamlessly together.

---

## 🌟 What This Proves About RHP

This example demonstrates the core strengths and runtime features of the RHP language:

1. **Object-Oriented Programming (OOP) & Type System:**
   - Typed classes (`Todo`, `TodoStore`, `TodoRenderer`, `TodoApp`) with typed properties, constructors, and return signatures (`: void`, `: bool`, `: string`, `: int`, `: array`).
   - Strict compile-time type verification, boundary contracts, and method dispatch.

2. **Register-Based Bytecode Virtual Machine:**
   - Fast VM execution with copy-on-write array operations.
   - Built-in string interpolation and concatenation (`$a . $b`).

3. **Standard Library Facades & I/O:**
   - `File::read(string $path): string` & `File::write(string $path, string $contents): bool` for persistent storage to `todos.json`.
   - `File::exists(string $path): bool` for automated seed initializations.
   - `Arr::dict(...)` and `.toJson()` for serialization.
   - `json_encode()` and `json_decode()` legacy compatibility functions.
   - `Sys::print(...)` / `echo` output buffers.

4. **HTTP Server & Superglobals:**
   - Built-in `rhp serve` command routing HTTP requests into isolated VM execution frames.
   - Standard PHP superglobals: `$_SERVER['REQUEST_METHOD']`, `$_SERVER['REQUEST_URI']`, `$_GET`, `$_POST`, and HTMX request detection (`HTTP_HX_REQUEST`).

5. **Modern Reactive Frontend (HTMX + Alpine.js):**
   - **HTMX:** Server-driven UI updates via `hx-post`, `hx-target="#todo-list-container"`, and `hx-swap="innerHTML"`. Adding, toggling, or deleting a todo swaps only the dynamic task list fragment without page reload.
   - **Alpine.js:** Smooth client-side state handling, transitions, and inline UI interactions.
   - **Tailwind CSS:** Modern, responsive card design with stats counters.

---

## 📁 Project Structure

```
examples/todo/
├── app.rhp        # Complete RHP application (Model, Store, Renderer, HTTP Router)
├── todos.json     # Persistent JSON storage file
├── run.sh         # Helper bash script to start the HTTP server
├── index.html     # Client-side preview & architecture demonstration
└── README.md      # Documentation and usage guide
```

---

## 🚀 Getting Started

### 1. Launch the HTTP Web Server

To run the application with the built-in RHP web server:

```bash
# Using the helper script (default port: 8080)
./examples/todo/run.sh

# Or directly with cargo:
cargo run -- serve examples/todo/app.rhp --port 8080
```

Open your browser at **[http://localhost:8080](http://localhost:8080)**.

### 2. Inspecting the Application

- **Add a Task:** Type a task name, select a category, and press Add (or Enter). HTMX issues `POST /todos` and updates the list and statistics counters instantaneously.
- **Toggle Completion:** Click any task checkbox. HTMX issues `POST /todos/toggle?id=<ID>` and updates the item state and counters.
- **Delete a Task:** Click the trash icon. HTMX issues `POST /todos/delete?id=<ID>`.
- **Clear Completed:** Click "Clear completed" to purge all finished tasks.
- **JSON API:** Visit **[http://localhost:8080/api/todos](http://localhost:8080/api/todos)** to view real-time JSON task statistics.

---

## 💻 CLI Execution (`rhp run dev` and `build`)

You can execute the application in both **Development Mode** (`dev`) and **Production / JIT Mode** (`build`):

### 1. Development Mode (`rhp run dev`)
In Dev Mode, RHP executes direct AST evaluation with fast cold start:

```bash
# From repository root:
cargo run -- run dev examples/todo
# or shorthand:
cargo run -- dev examples/todo

# Direct file target:
cargo run -- run dev examples/todo/app.rhp

# If you are inside the examples/todo directory:
cd examples/todo
rhp run dev   # automatically runs index.rhp / app.rhp
```

### 2. Production / Build Mode (`rhp run build`)
In Build Mode, RHP compiles the script into register-based bytecode and executes it on the Register VM with JIT acceleration:

```bash
# From repository root:
cargo run -- run build examples/todo
# or shorthand:
cargo run -- build examples/todo

# Direct file target:
cargo run -- run build examples/todo/app.rhp

# If you are inside the examples/todo directory:
cd examples/todo
rhp run build
```

---

## 🔌 API Endpoints

| Method | Path | Description | Response Type |
|---|---|---|---|
| `GET` | `/` | Serves the full HTML page or HTMX fragment | `text/html` |
| `POST` | `/todos` | Creates a new task (`title`, `category`) | `text/html` (HTMX partial) |
| `POST` | `/todos/toggle?id={id}` | Toggles a task's completed state | `text/html` (HTMX partial) |
| `POST` | `/todos/delete?id={id}` | Deletes a task by ID | `text/html` (HTMX partial) |
| `POST` | `/todos/clear-completed` | Clears all completed tasks | `text/html` (HTMX partial) |
| `GET` | `/api/todos` | Retrieves task counts (`total`, `active`, `completed`) | `application/json` |

---

## 🧪 Testing

An automated integration test verifies compilation, routing, and persistence:

```bash
cargo test --test todo_example_integration_tests
```
