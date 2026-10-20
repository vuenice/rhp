---
title: "Keywords Reference"
description: "Exhaustive index of all reserved words, contextual keywords, and modifiers in RHP."
section: "Reference"
order: 2
---

## Reserved Keywords

The following identifiers are reserved language keywords and cannot be used as function or class names:

```text
abstract    as          async       break       case        catch
class       const       continue    default     defer       do
else        elseif      enum        extends     final       finally
fn          for         foreach     function    if          implements
interface   import      macro       match       namespace   new
private     protected   public      readonly    return      singleton
static      switch      throw       trait       try         use
while       yield
```

---

## Type Keywords

The following primitive and collection subtype keywords are reserved for type annotations:

```text
int         float       string      bool        void        mixed
array       list        dict        set         hashmap     callable
iterable    object      never       null
```

---

## Special Identifiers & Superglobals

| Identifier | Context | Description |
| :--- | :--- | :--- |
| `$this` | Class instances | Reference to current instance |
| `self` | Class scope | Reference to enclosing class definition |
| `parent` | Subclasses | Reference to parent class implementation |
| `$_GET` | Web requests | Request query parameters dictionary |
| `$_POST` | Web requests | Request body payload dictionary |
| `$_SERVER` | Web requests | Server environment parameters dictionary |
