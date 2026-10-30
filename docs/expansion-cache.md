# Milestone 3 Design Specification: Deterministic Expansion Cache

## 1. Overview & Architectural Goal

Compile-time macro expansion and template desugaring can introduce noticeable latency in large applications containing thousands of routes, model attributes, and template views.

Milestone 3 specifies an on-disk, byte-deterministic expansion cache stored under `.rhp/cache/expand/`. If neither the handler implementation, the target AST node, nor the expansion arguments have changed, the compiler skips macro execution and directly injects the cached expanded AST subtree.

---

## 2. Cache Key Construction

The cache key must uniquely and deterministically identify the exact input state of an expansion site:

$$\text{CacheKey} = \text{SHA256}(\text{HandlerHash} \parallel \text{TargetAstHash} \parallel \text{ArgumentsHash} \parallel \text{CompilerVersion})$$

### 2.1 Component Hashes
1. **`HandlerHash`**:
   - For built-in Rust expanders: `SHA256("builtin:" + expander_name + ":" + RHP_VERSION)`.
   - For RHP comptime handlers: `SHA256(macro_source_bytecode + macro_dependencies_hash)`.
2. **`TargetAstHash`**:
   - Deterministic canonical serialization of the target node (e.g., class signature, method signature, or template body AST) omitting non-semantic whitespace and comments.
3. **`ArgumentsHash`**:
   - Deterministic canonical serialization of the evaluated literal arguments passed to the attribute or directive (e.g. `['name', 'email']`).
4. **`CompilerVersion`**:
   - Stable semantic version of the RHP compiler engine.

---

## 3. Cache Storage Layout

Expansion artifacts are stored in a two-tier content-addressable directory structure inside `.rhp/cache/expand/`:

```
.rhp/
└── cache/
    └── expand/
        ├── a4/
        │   └── a4f8e219c4b789...bin   # Binary bincode serialization of Expansion
        ├── 7c/
        │   └── 7cf91d0342a123...bin
        └── manifest.json             # Cache index metadata (hits, timestamps, sizes)
```

Each cache artifact contains:
- The serialized `Expansion` output (`AddClassMembers`, `Statements`, etc.).
- The relative synthetic `SpanChain` offsets (re-stamped against the caller's actual file/line on cache hit).
- Generated symbol names mapped through hygiene relocations.

---

## 4. Invalidation Rules

The expansion cache is automatically invalidated under any of the following conditions:

1. **Source Hash Mismatch:** Any modification to the macro implementation file (in `vendor/` or `app/Macros/`) changes `HandlerHash`.
2. **Target Node Edit:** Modifying the decorated class, property, method, or template directive body alters `TargetAstHash`.
3. **Argument Mutation:** Changing `#[fillable('name')]` to `#[fillable('name', 'email')]` alters `ArgumentsHash`.
4. **Compiler Upgrade:** Updating the `rhp` binary or Cranelift version invalidates all cached entries.
5. **Explicit Clean:** Running `rhp clean --cache` removes `.rhp/cache/`.

---

## 5. Non-Cacheable Handlers & Opt-Out Criteria

A macro handler is marked **non-cacheable** (`is_cacheable() == false`) if it relies on external or temporal state:

1. **Environmental Lookups:** Reading environment variables (`Env::get(...)`) or system time (`time()`, `now()`).
2. **External File Reads:** Reading files outside the current compilation unit (e.g. reading a schema migration file or external JSON config).
3. **Explicit Opt-Out:** Declaring `#[NonCacheable]` on the macro handler:
   ```php
   #[AttributeHandler('git_hash'), NonCacheable]
   public static function injectGitHash(ExpansionSite $site): Expansion { ... }
   ```
When a handler is non-cacheable, the compiler always re-executes the handler in the VM and bypasses reading/writing to `.rhp/cache/`.
