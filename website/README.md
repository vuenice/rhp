# RHP Programming Language — Official Website & Documentation

The official marketing landing page and documentation website for **RHP** (Rust-Powered High-Performance PHP-Family Language).

Built with **Astro**, **Tailwind CSS**, and **Shiki** with a custom TextMate grammar for native RHP syntax highlighting.

---

## ⚡ Tech Stack

- **Framework:** [Astro 7](https://astro.build) (Static Site Generation / Zero JavaScript hydration overhead for docs)
- **Styling:** [Tailwind CSS](https://tailwindcss.com) with custom dark mode default (`class="dark"`), obsidian surfaces, and Rust-coral brand accents
- **Syntax Highlighting:** Shiki with custom TextMate grammar (`src/grammars/rhp.tmLanguage.json`) supporting `.rhp` keywords, type hints, facades, and variables
- **Content:** Astro Content Collections (`src/content/docs/`) with typed frontmatter validation and automated Table of Contents extraction
- **Search:** Instant client-side fuzzy search modal (`Cmd/Ctrl+K`) indexing all guide chapters, CLI references, and API docs
- **SEO & Meta:** Open Graph social cards, Twitter cards, canonical tags, `sitemap-index.xml`, and `robots.txt`

---

## 📁 Project Structure

```text
website/
├── public/
│   ├── favicon.svg             # RHP favicon
│   ├── rhp-logo.svg            # Official vector SVG logo
│   ├── og-image.svg            # Open Graph social preview card
│   └── robots.txt              # Search engine crawler directives
├── src/
│   ├── components/
│   │   ├── Navbar.astro        # Responsive navigation with search & theme toggle
│   │   ├── Footer.astro        # Footer with ecosystem & legal links
│   │   ├── ThemeToggle.astro   # Dark / Light mode toggle (persisted to localStorage)
│   │   ├── SearchModal.astro   # Cmd/Ctrl+K interactive instant search dialog
│   │   ├── docs/
│   │   │   └── Callout.astro   # Note, Tip, Warning callout components
│   │   └── landing/            # 8 Landing page sections in required order
│   │       ├── Hero.astro
│   │       ├── WhyRhp.astro
│   │       ├── InteractiveShowcase.astro
│   │       ├── PerformanceBenchmarks.astro
│   │       ├── PlaygroundTeaser.astro
│   │       ├── EcosystemRoadmap.astro
│   │       ├── QuickStart.astro
│   │       └── CommunitySection.astro
│   ├── content/
│   │   └── docs/               # Markdown docs organized by section
│   │       ├── getting-started/
│   │       ├── guide/
│   │       ├── reference/
│   │       ├── tooling/
│   │       ├── advanced/
│   │       ├── contributing.md
│   │       ├── changelog.md
│   │       └── faq.md
│   ├── data/
│   │   ├── docsNav.ts          # Structured docs navigation & prev/next helper
│   │   └── searchIndex.ts      # Search index for instant Cmd+K search
│   ├── grammars/
│   │   └── rhp.tmLanguage.json # Custom TextMate grammar for RHP
│   ├── layouts/
│   │   ├── Layout.astro        # Base HTML layout with SEO & anti-FOUC script
│   │   └── DocsLayout.astro    # Docs shell with sidebar, TOC, and copy buttons
│   ├── lib/
│   │   └── playgroundClient.ts # RhpExecutionClient interface & WASM stub
│   ├── pages/
│   │   ├── index.astro         # Marketing landing page
│   │   ├── docs/
│   │   │   ├── index.astro     # Redirects to /docs/getting-started/installation
│   │   │   └── [...slug].astro # Dynamic static doc generator
│   └── styles/
│       └── global.css          # Tailwind base & custom scrollbar styles
├── astro.config.mjs            # Astro configuration with Shiki RHP grammar
├── tailwind.config.mjs         # Brand theme configuration
└── package.json
```

---

## 🛠️ Local Setup & Development

### Prerequisites
- Node.js `v18.14.1` or newer (Tested on Node `v22.23.3`)
- npm or pnpm

### 1. Install Dependencies
```bash
cd website
npm install
```

### 2. Start Local Development Server
```bash
npm run dev
```

Visit [http://localhost:4321](http://localhost:4321) in your browser.

---

## 🏗️ Production Build

Generate optimized static HTML assets in `dist/`:

```bash
npm run build
```

Preview the production build locally:

```bash
npm run preview
```

---

## 🚀 Deployment

The `dist/` directory contains pure static HTML, CSS, JavaScript, and SVG assets, ready for instant zero-config deployment to any static hosting provider.

### Vercel
```bash
npx vercel deploy --prod
```
- **Framework Preset:** Astro
- **Build Command:** `npm run build`
- **Output Directory:** `dist`

### Netlify
```bash
npx netlify deploy --prod --dir=dist
```

### GitHub Pages
Continuous deployment is automated via `.github/workflows/deploy-pages.yml`:
- **Build:** Sets up Node.js 22, installs dependencies via `npm ci`, and builds static assets to `dist/`.
- **Deploy:** Employs official `actions/deploy-pages@v4` with `actions/upload-pages-artifact@v3`.
- **Triggers:** Automatically on push to `main` branch (for changes in `website/`) and on manual `workflow_dispatch`.

---

## 🔌 Wiring Up the WebAssembly / Remote Playground

The interactive code playground on the landing page is decoupled behind the `RhpExecutionClient` interface located at `src/lib/playgroundClient.ts`:

```typescript
export interface RhpExecutionResult {
  stdout: string;
  stderr?: string;
  exitCode: number;
  durationMs: number;
  memoryUsageBytes: number;
  gcPauseMs: number;
  cowDetachments: number;
}

export interface RhpExecutionClient {
  run(code: string): Promise<RhpExecutionResult>;
}
```

To connect a real WebAssembly build of the RHP compiler or a remote container execution endpoint:
1. Implement `RhpExecutionClient` inside `src/lib/playgroundClient.ts`.
2. Replace `defaultExecutionClient` with your WASM or API instance.

---

## 📄 License

The website source code and documentation are released under the [MIT License](https://github.com/vuenice/rhp/blob/main/LICENSE).
