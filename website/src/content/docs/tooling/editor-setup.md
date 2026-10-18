---
title: "Editor Setup & LSP"
description: "Configure Visual Studio Code, Neovim, Tree-sitter, and Language Server Protocol for RHP."
section: "Tooling"
order: 4
---

## Visual Studio Code

Install the official **RHP Language Tools** extension from the VS Code Marketplace:

```bash
code --install-extension vuenice.rhp-vscode
```

### Features Included:
- Custom TextMate syntax highlighting for `.rhp` and `.php` files.
- Real-time diagnostic error squiggles from `rhp run lint`.
- Format on Save integration.
- Snippets for `Arr::list`, `Future::wait`, and `singleton class`.

---

## Neovim (Tree-sitter & LSP)

### 1. Tree-sitter Grammar
Add the RHP parser repository to your `nvim-treesitter` configuration:

```lua
local parser_config = require "nvim-treesitter.parsers".get_parser_configs()
parser_config.rhp = {
  install_info = {
    url = "https://github.com/vuenice/rhp-treesitter",
    files = {"src/parser.c"},
    branch = "main",
  },
  filetype = "rhp",
}
```

### 2. LSP Setup via `nvim-lspconfig`
Configure `rhp-lsp` using the standard `lspconfig` helper:

```lua
require'lspconfig'.rhp_lsp.setup{
  cmd = { "rhp", "lsp" },
  filetypes = { "rhp", "php" },
  root_dir = require('lspconfig.util').root_pattern("rhp.json", ".git"),
}
```

---

## Debug Adapter Protocol (DAP)

RHP's development mode includes a built-in DAP server for interactive step debugging:

```bash
rhp run dev src/main.rhp --dap-port 5005
```

Set breakpoints, inspect local variable environments, and evaluate runtime expressions in real time.
