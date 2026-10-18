import { defineConfig } from 'astro/config';
import tailwind from '@astrojs/tailwind';
import rhpGrammar from './src/grammars/rhp.tmLanguage.json' with { type: 'json' };

// https://astro.build/config
export default defineConfig({
  integrations: [tailwind()],
  site: 'https://rhp.dev',
  markdown: {
    shikiConfig: {
      theme: 'github-dark-dimmed',
      langs: [
        {
          ...rhpGrammar,
          name: 'rhp',
          aliases: ['rhp-lang']
        }
      ],
      wrap: true
    }
  }
});
