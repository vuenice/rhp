import { defineCollection, z } from 'astro:content';
import { glob } from 'astro/loaders';

const docs = defineCollection({
  loader: glob({ pattern: '**/*.md', base: './src/content/docs' }),
  schema: z.object({
    title: z.string(),
    description: z.string().optional(),
    section: z.string(), // e.g. "Getting Started", "Language Guide", etc.
    order: z.number().default(99),
    keywords: z.union([z.string(), z.array(z.string())]).optional(),
  }),
});

export const collections = { docs };
