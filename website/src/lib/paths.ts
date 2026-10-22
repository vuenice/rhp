/**
 * Helper to ensure internal site links respect Astro's `base` configuration
 * (e.g. '/rhp' on GitHub Pages vs '/' locally or on custom domain).
 */
export function getBase(): string {
  const base = import.meta.env.BASE_URL || '/';
  return base.endsWith('/') ? base.slice(0, -1) : base;
}

export function siteUrl(path: string = ''): string {
  const base = getBase();
  const cleanPath = path.startsWith('/') ? path : `/${path}`;
  if (cleanPath === '/') {
    return base ? `${base}/` : '/';
  }
  return `${base}${cleanPath}`;
}
