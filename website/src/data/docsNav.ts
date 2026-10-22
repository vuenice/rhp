export interface DocNavItem {
  title: string;
  slug: string;
  href: string;
}

export interface DocNavSection {
  title: string;
  items: DocNavItem[];
}

export const docsNavigation: DocNavSection[] = [
  {
    title: "Getting Started",
    items: [
      { title: "Installation", slug: "getting-started/installation", href: "/docs/getting-started/installation" },
      { title: "Hello World", slug: "getting-started/hello-world", href: "/docs/getting-started/hello-world" },
      { title: "Core Architecture", slug: "getting-started/core-architecture", href: "/docs/getting-started/core-architecture" },
      { title: "Project Structure", slug: "getting-started/project-structure", href: "/docs/getting-started/project-structure" },
      { title: "CLI Reference", slug: "getting-started/cli-reference", href: "/docs/getting-started/cli-reference" },
    ]
  },
  {
    title: "Language Guide",
    items: [
      { title: "Variables and Types", slug: "guide/variables-and-types", href: "/docs/guide/variables-and-types" },
      { title: "Numeric Types", slug: "guide/numeric-types", href: "/docs/guide/numeric-types" },
      { title: "Operators", slug: "guide/operators", href: "/docs/guide/operators" },
      { title: "Control Flow", slug: "guide/control-flow", href: "/docs/guide/control-flow" },
      { title: "Functions and Closures", slug: "guide/functions", href: "/docs/guide/functions" },
      { title: "Classes, Structs & Singletons", slug: "guide/classes-and-objects", href: "/docs/guide/classes-and-objects" },
      { title: "Error Handling", slug: "guide/error-handling", href: "/docs/guide/error-handling" },
      { title: "Modules and Imports", slug: "guide/modules-and-imports", href: "/docs/guide/modules-and-imports" },
      { title: "Concurrency & Async", slug: "guide/concurrency-async", href: "/docs/guide/concurrency-async" },
      { title: "Standard Library Overview", slug: "guide/standard-library", href: "/docs/guide/standard-library" },
      { title: "Observability & OpenTelemetry", slug: "guide/observability-opentelemetry", href: "/docs/guide/observability-opentelemetry" },
    ]
  },
  {
    title: "Reference",
    items: [
      { title: "Grammar & Syntax Reference", slug: "reference/grammar", href: "/docs/reference/grammar" },
      { title: "Lexical & Syntax Architecture", slug: "reference/syntax-architecture", href: "/docs/reference/syntax-architecture" },
      { title: "Keywords Reference", slug: "reference/keywords", href: "/docs/reference/keywords" },
      { title: "Built-in Functions", slug: "reference/builtins", href: "/docs/reference/builtins" },
      { title: "Standard Library API", slug: "reference/stdlib-api", href: "/docs/reference/stdlib-api" },
    ]
  },
  {
    title: "Tooling",
    items: [
      { title: "Package Manager", slug: "tooling/package-manager", href: "/docs/tooling/package-manager" },
      { title: "Auto-Formatter", slug: "tooling/formatter", href: "/docs/tooling/formatter" },
      { title: "Linter & Static Typing", slug: "tooling/linter", href: "/docs/tooling/linter" },
      { title: "Editor Setup & LSP", slug: "tooling/editor-setup", href: "/docs/tooling/editor-setup" },
    ]
  },
  {
    title: "Advanced",
    items: [
      { title: "Memory Model & Zero-GC", slug: "advanced/memory-model", href: "/docs/advanced/memory-model" },
      { title: "FFI & Rust Native Interop", slug: "advanced/ffi-interop", href: "/docs/advanced/ffi-interop" },
      { title: "Compiler & VM Internals", slug: "advanced/compiler-internals", href: "/docs/advanced/compiler-internals" },
    ]
  },
  {
    title: "Community & Meta",
    items: [
      { title: "Contributing Guide", slug: "contributing", href: "/docs/contributing" },
      { title: "Changelog", slug: "changelog", href: "/docs/changelog" },
      { title: "Frequently Asked Questions", slug: "faq", href: "/docs/faq" },
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
