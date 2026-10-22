import { defineConfig } from 'astro/config';
import tailwind from '@astrojs/tailwind';
import rhpGrammar from './src/grammars/rhp.tmLanguage.json' with { type: 'json' };

// https://astro.build/config
export default defineConfig({
  integrations: [tailwind()],
  site: process.env.ASTRO_SITE || 'https://rhp.dev',
  base: process.env.ASTRO_BASE || '/',
  markdown: {
    shikiConfig: {
      theme: 'github-dark',
      langs: [
        {
          ...rhpGrammar,
          name: 'rhp',
          aliases: ['rhp-lang']
        }
      ],
      wrap: false
    }
  }
});
