import { getCollection } from 'astro:content';

export async function getStaticPaths() {
  const docs = await getCollection('docs');
  return docs.map(doc => ({
    params: { slug: doc.id },
    props: { doc },
  }));
}

export async function GET({ props }: { props: { doc: any } }) {
  const payload = {
    title: props.doc.data.title,
    description: props.doc.data.description,
    section: props.doc.data.section,
    slug: props.doc.id,
    keywords: props.doc.data.keywords,
    content: props.doc.body,
  };

  return new Response(JSON.stringify(payload, null, 2), {
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': 'public, max-age=3600',
    },
  });
}
