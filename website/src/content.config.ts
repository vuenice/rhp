import { defineCollection, z } from 'astro:content';
import { glob } from 'astro/loaders';

const docs = defineCollection({
  loader: glob({ pattern: '**/*.md', base: './src/content/docs' }),
  schema: z.object({
    title: z.string(),
    description: z.string().optional(),
    section: z.string(), // e.g. "Getting Started", "Language Guide", etc.
    order: z.number().default(99),
  }),
});

export const collections = { docs };
