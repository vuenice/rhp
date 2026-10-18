---
title: "Modules and Imports"
description: "Organize code with namespaces, use statements, and automated symbol resolution in RHP."
section: "Language Guide"
order: 7
---

## Namespaces

RHP groups classes, interfaces, and functions under structured namespaces matching directory paths:

```php
<?rhp

namespace App\Services\Payment;

class StripeGateway {
    public function charge(int $amountCents): bool {
        return true;
    }
}
```

---

## The `use` Statement

Import classes and namespaces into your current scope using `use`:

```php
<?rhp

namespace App\Controllers;

use App\Services\Payment\StripeGateway;
use App\Models\User;

class CheckoutController {
    public function handle(int $userId, int $cents): bool {
        $gateway = new StripeGateway();
        return $gateway->charge($cents);
    }
}
```

---

## Aliasing with `as`

To resolve naming collisions between different modules, alias imported identifiers with `as`:

```php
<?rhp

use App\Services\Http\Client as HttpClient;
use External\Network\Client as ExternalClient;

$local = new HttpClient();
$ext = new ExternalClient();
```

---

## Auto-Loading and Symbol Lowering

Unlike traditional PHP which scans the filesystem on every request or relies on heavy Composer ClassLoader lookups, RHP's **Pratt Parser** resolves module symbols during initial parsing in the parent process.

- Built-in facades (`Arr`, `Str`, `Future`, `Sys`) are resolved in $O(1)$ time via compile-time DFA tokens.
- Application classes are loaded and compiled into the persistent engine AST index once.
