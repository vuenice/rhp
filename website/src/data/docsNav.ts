import { siteUrl } from '../lib/paths';

export interface DocNavItem {
  title: string;
  slug: string;
  href: string;
}

export interface DocNavSection {
  title: string;
  items: DocNavItem[];
}

const item = (title: string, slug: string): DocNavItem => ({
  title,
  slug,
  href: siteUrl(`/docs/${slug}`)
});

export const docsNavigation: DocNavSection[] = [
  {
    title: "Getting Started",
    items: [
      item("Installation", "getting-started/installation"),
      item("Hello World", "getting-started/hello-world"),
      item("Core Architecture", "getting-started/core-architecture"),
      item("Project Structure", "getting-started/project-structure"),
      item("CLI Reference", "getting-started/cli-reference"),
    ]
  },
  {
    title: "Language Guide",
    items: [
      item("Variables and Types", "guide/variables-and-types"),
      item("Numeric Types", "guide/numeric-types"),
      item("Operators", "guide/operators"),
      item("Control Flow", "guide/control-flow"),
      item("Functions and Closures", "guide/functions"),
      item("Classes, Structs & Singletons", "guide/classes-and-objects"),
      item("Error Handling", "guide/error-handling"),
      item("Modules and Imports", "guide/modules-and-imports"),
      item("Concurrency & Async", "guide/concurrency-async"),
      item("Standard Library Overview", "guide/standard-library"),
      item("Observability & OpenTelemetry", "guide/observability-opentelemetry"),
    ]
  },
  {
    title: "Reference",
    items: [
      item("Grammar & Syntax Reference", "reference/grammar"),
      item("Lexical & Syntax Architecture", "reference/syntax-architecture"),
      item("Keywords Reference", "reference/keywords"),
      item("Built-in Functions", "reference/builtins"),
      item("Standard Library API", "reference/stdlib-api"),
    ]
  },
  {
    title: "Tooling",
    items: [
      item("Package Manager", "tooling/package-manager"),
      item("Auto-Formatter", "tooling/formatter"),
      item("Linter & Static Typing", "tooling/linter"),
      item("Editor Setup & LSP", "tooling/editor-setup"),
    ]
  },
  {
    title: "Advanced",
    items: [
      item("Memory Model & Zero-GC", "advanced/memory-model"),
      item("FFI & Rust Native Interop", "advanced/ffi-interop"),
      item("Compiler & VM Internals", "advanced/compiler-internals"),
    ]
  },
  {
    title: "Community & Meta",
    items: [
      item("Contributing Guide", "contributing"),
      item("Changelog", "changelog"),
      item("Frequently Asked Questions", "faq"),
    ]
  }
];

// Helper to find prev and next items
export function getPrevNextDocs(currentSlug: string): { prev: DocNavItem | null; next: DocNavItem | null } {
  const flatItems: DocNavItem[] = [];
  for (const section of docsNavigation) {
    for (const item of section.items) {
      flatItems.push(item);
    }
  }

  // Normalize slug
  const cleanSlug = currentSlug.replace(/^\/docs\/?/, '').replace(/\/$/, '');
  const index = flatItems.findIndex(i => i.slug === cleanSlug);

  if (index === -1) {
    return { prev: null, next: null };
  }

  return {
    prev: index > 0 ? flatItems[index - 1] : null,
    next: index < flatItems.length - 1 ? flatItems[index + 1] : null,
  };
}
