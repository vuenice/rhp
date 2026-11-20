---
title: "Template Engine (Rade)"
description: "High-performance Blade-inspired template engine for RHP with zero-GC compilation, dot-notation resolution, .rade.rhp file format, and automatic caching."
section: "Language Guide"
order: 9
keywords: ["rade", "template engine", "blade", "views", "view", "render", ".rade.rhp", "directives", "templating", "view:cache", "view:clear"]
---

## Overview

**Rade** is the official, high-performance templating engine built directly into RHP. Inspired by the clean, expressive syntax of Laravel Blade, Rade is engineered from the ground up in native Rust to deliver sub-millisecond compilation, zero garbage-collection pauses, and seamless integration with RHP's type system and runtime.

Templates in Rade are written using the **`.rade.rhp`** file extension. At build time or on demand, Rade lexes, analyzes, and lowers templates into pure, optimized RHP code utilizing string accumulator buffers and standard security escaping.

```html
<!-- resources/views/welcome.rade.rhp -->
<div class="user-card">
    <h1>Welcome, {{ $user['name'] }}!</h1>

    @if ($user['is_admin'])
        <span class="badge">Administrator</span>
    @elseif ($user['is_moderator'])
        <span class="badge">Moderator</span>
    @else
        <span>Member</span>
    @endif

    <ul>
    @for ($i = 0; $i < $items.len(); $i++)
        <li>Item #{{ $i + 1 }}: {{ $items[$i] }}</li>
    @endfor
    </ul>
</div>
```

---

## File Naming & Directory Structure

### File Extensions

Rade templates strictly use the following conventions:

* **Primary Extension:** **`.rade.rhp`** (e.g. `welcome.rade.rhp`, `home.rade.rhp`, `profile.rade.rhp`).
* **Short Extension:** **`.rade`** (e.g. `card.rade`).
* **Backward Compatibility:** Legacy `.blade.rhp` files continue to resolve automatically if present during migration.

> [!IMPORTANT]
> Always name your view files with the **`.rade.rhp`** extension. The `.rade.rhp` suffix signals both to code editors and to the RHP compiler that the source file is an HTML/RHP hybrid template managed by Rade.

### Default View Directories

By default, the Rade runtime searches for templates in standard locations in your project root:

1. `resources/views/` (Standard application structure)
2. `views/` (Minimal / flat structure)

For example, a view referenced as `'auth.login'` will automatically search:
* `resources/views/auth/login.rade.rhp`
* `resources/views/auth/login.rade`
* `views/auth/login.rade.rhp`
* `views/auth/login.rade`

---

## Rendering Views in RHP

RHP provides multiple ergonomic ways to render Rade templates in your application.

### The `view()` Helper

The global `view()` function resolves a template file by name, passes data variables, and returns the rendered HTML string:

```php
<?php
// Resolves resources/views/home.rade.rhp
$html = view('home', [
    'title' => 'Dashboard',
    'user' => ['name' => 'Alice', 'role' => 'admin'],
    'unread_count' => 5,
]);

print($html);
```

#### Dot Notation for Nested Views

Nested directory structures are expressed using intuitive dot syntax:

```php
// Resolves resources/views/admin/reports/analytics.rade.rhp
$report = view('admin.reports.analytics', [
    'year' => 2026,
    'metrics' => $metrics,
]);
```

### The `render()` Helper (Inline Template Strings)

To render raw template strings on the fly without a dedicated file on disk, use the `render()` helper:

```php
<?php
$template = '<h1>Hello, {{ $name }}!</h1><p>Active fibers: {{ $fibers }}</p>';

$output = render($template, [
    'name' => 'Antigravity',
    'fibers' => 16,
]);

print($output);
// Output: <h1>Hello, Antigravity!</h1><p>Active fibers: 16</p>
```

### The `View` & `Rade` Facades

RHP also exposes static methods on the `View` and `Rade` facades for structured programmatic access:

```php
<?php
// Render a view
$html = View::render('pages.about', ['company' => 'Acme Inc']);

// Verify if a view exists before rendering
if (View::exists('themes.custom')) {
    $content = View::render('themes.custom', $data);
} else {
    $content = View::render('themes.default', $data);
}

// Compile template string directly to RHP code
$rhpCode = Rade::compile('Hello, {{ $name }}!');

// Pre-compile all views in configured directory ahead-of-time
$compiledCount = View::cache();

// Flush all cached view files
$clearedCount = View::clear();
```

---

## Template Syntax & Expressions

### Escaped Output: `{{ $expression }}`

By default, all expressions inside `{{ ... }}` are automatically escaped using `Html::escape()` to prevent Cross-Site Scripting (XSS) attacks:

```html
<p>User bio: {{ $bio }}</p>
```

If `$bio` contains `<script>alert('xss')</script>`, Rade compiles the output safely to:

```html
&lt;script&gt;alert(&#039;xss&#039;)&lt;/script&gt;
```

Any valid RHP expression can be evaluated within `{{ }}`:

```html
<span>Status: {{ $user['active'] ? 'Online' : 'Offline' }}</span>
<span>Count: {{ $items.len() + 1 }}</span>
<span>Default: {{ $nickname ?? 'Guest' }}</span>
```

### Raw Unescaped Output: `{!! $expression !!}`

When you need to output raw, unescaped HTML (such as rich text from a trusted CMS or SVG icons), use `{!! ... !!}`:

```html
<div class="content-body">
    {!! $trustedArticleHtml !!}
</div>
```

> [!CAUTION]
> Be extremely careful when using `{!! ... !!}` with user-supplied content. Always sanitize untrusted input before rendering raw HTML to avoid XSS vulnerabilities.

### Escaped Verbatim Syntax: `@{{ expression }}`

If you are using JavaScript frameworks like Vue.js, Alpine.js, or Angular that also use mustache `{{ }}` tags, prefix the tag with `@` to instruct Rade to leave the expression intact:

```html
<!-- Rade leaves this untouched for client-side frameworks -->
<div x-data="{ count: 0 }">
    @{{ count }}
</div>
```

The compiled output rendered to the browser will literally be:

```html
<div x-data="{ count: 0 }">
    {{ count }}
</div>
```

### Comments: `{{-- comment --}}`

Rade comments are completely stripped out during compilation and never appear in the final HTML response:

```html
{{-- This comment is removed at compile time and will not be in the browser DOM --}}
<p>Public Content</p>
```

---

## Control Flow Directives

### Conditionals: `@if`, `@elseif`, `@else`, `@endif`

Rade provides structured conditional directives mirroring RHP's monolithic `elseif` rules:

```html
@if ($role === 'superadmin')
    <div class="banner bg-red">Full Access</div>
@elseif ($role === 'editor')
    <div class="banner bg-blue">Editor Access</div>
@else
    <div class="banner bg-gray">Read-Only</div>
@endif
```

> [!NOTE]
> Like RHP itself, Rade strictly enforces the single-word `@elseif` directive. Directive blocks must always be closed with `@endif`.

### Loops: `@for`, `@endfor`

Iterate with standard C-style loops:

```html
<ul class="pagination">
@for ($page = 1; $page <= $totalPages; $page++)
    <li class="{{ $page === $currentPage ? 'active' : '' }}">
        <a href="?page={{ $page }}">{{ $page }}</a>
    </li>
@endfor
</ul>
```

### Raw RHP Blocks: `@rhp` / `@endrhp`

When you need to perform local transformations, calculations, or variable setups directly within the view, embed native RHP code using `@rhp ... @endrhp` (or `@php ... @endphp`):

```html
@rhp
    $totalPrice = 0;
    foreach ($cart['items'] as $item) {
        $totalPrice += $item['price'] * $item['quantity'];
    }
    $tax = $totalPrice * 0.08;
@endrhp

<div class="invoice-summary">
    <p>Subtotal: ${{ $totalPrice }}</p>
    <p>Tax (8%): ${{ $tax }}</p>
    <p>Total: ${{ $totalPrice + $tax }}</p>
</div>
```

### Literal Directive Passthrough

If a directive is unregistered and does not contain parentheses (such as `@media` queries in `<style>` blocks or Alpine.js event handlers like `@click="toggle"`), Rade automatically treats it as literal text and passes it through untouched.

---

## Caching Architecture & Performance

Rade compiles `.rade.rhp` templates into native RHP scripts stored in a hashed cache directory (`.rhp/cache/views` or `storage/framework/views`).

```
views/welcome.rade.rhp  ──►  SHA-1 Digest  ──►  .rhp/cache/views/7a8f...3d.rhp
```

### Development Mode (`rhp dev`)

In development, Rade provides zero-friction hot reloading:
* On every request, Rade checks the file modification timestamp (`mtime`) of the `.rade.rhp` template.
* If the source template has been saved or modified, Rade automatically recompiles the view to disk in microseconds.
* No server restart or external file watcher process is needed.

### Production Mode (`rhp run build` / `RHP_ENV=production`)

In production environments:
* Rade skips all `mtime` disk checks, reading compiled bytecode or AST caches with zero filesystem overhead.
* All views can be pre-compiled during your CI/CD deployment step using the `rhp view:cache` command.

---

## CLI Tools & Management

RHP includes dedicated commands for managing Rade templates from the command line:

### `rhp view:cache` (Pre-compile Views)

Recursively compiles all `.rade.rhp` templates into hashed production cache files ahead of time:

```bash
# Pre-compile views in default directories
rhp view:cache

# Or specify custom views and cache storage directories
rhp view:cache resources/views storage/framework/views
```

### `rhp view:clear` (Clear View Cache)

Removes all cached `.rhp` view files:

```bash
# Clear default view cache
rhp view:clear

# Or clear a specific storage directory
rhp view:clear storage/framework/views
```

### `rhp view compile` (Inspect Generated RHP Code)

Compiles a single `.rade.rhp` template and outputs the generated RHP code to standard output or an output file:

```bash
# Print compiled RHP code to stdout
rhp view compile resources/views/welcome.rade.rhp

# Save compiled RHP code to a specific file
rhp view compile resources/views/welcome.rade.rhp -o dist/welcome.rhp
```

### Standalone Companion Binary: `rade`

RHP also ships with a dedicated standalone companion binary `rade` for standalone debugging, scripting, or build pipeline integrations:

```bash
# Compile template to stdout
rade compile views/home.rade.rhp

# Compile template to output file
rade compile views/home.rade.rhp -o cache/home.rhp

# Pre-compile an entire views directory
rade cache views/ storage/cache/

# Clear cache directory
rade clear storage/cache/
```

---

## Complete Real-World Example

Here is a full example illustrating an application layout with view rendering:

```html
<!-- resources/views/dashboard.rade.rhp -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>{{ $page_title }}</title>
</head>
<body>
    <header>
        <h1>{{ $app_name }}</h1>
        <p>Logged in as: <strong>{{ $user['name'] }}</strong></p>
    </header>

    <main>
        @if ($user['notifications'].len() > 0)
            <div class="notifications">
                <h3>You have {{ $user['notifications'].len() }} new notification(s):</h3>
                <ul>
                @for ($i = 0; $i < $user['notifications'].len(); $i++)
                    <li>{{ $user['notifications'][$i] }}</li>
                @endfor
                </ul>
            </div>
        @else
            <p>No new notifications.</p>
        @endif

        <div class="account-summary">
            @rhp
                $tier = $user['is_vip'] ? 'Gold VIP' : 'Standard Member';
            @endrhp
            <p>Membership: {{ $tier }}</p>
        </div>
    </main>

    <footer>
        <p>&copy; 2026 {{ $app_name }}. Powered by RHP & Rade.</p>
    </footer>
</body>
</html>
```

And in your `index.rhp` route handler:

```php
<?php
// index.rhp

$data = [
    'app_name' => 'Nexus Cloud',
    'page_title' => 'Nexus Cloud - Dashboard',
    'user' => [
        'name' => 'Sarah Connor',
        'is_vip' => true,
        'notifications' => [
            'Server #4 rebooted successfully',
            'Monthly usage statement available',
        ],
    ],
];

// Render template and send HTTP response
$html = view('dashboard', $data);
print($html);
```
