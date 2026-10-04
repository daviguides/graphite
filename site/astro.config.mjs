import { defineConfig } from 'astro/config';
import mdx from '@astrojs/mdx';
import tailwindcss from '@tailwindcss/vite';

const basePath = (process.env.BASE_PATH ?? '/').replace(/\/+$/, '') || '/';

function rehypeBasePrefix() {
  const prefix = basePath === '/' ? '' : basePath;
  return (tree) => {
    if (!prefix) return;
    const walk = (node) => {
      if (node.type === 'element' && node.properties) {
        for (const key of ['href', 'src']) {
          const v = node.properties[key];
          if (typeof v === 'string' && v.startsWith('/') && !v.startsWith('//') && !v.startsWith(`${prefix}/`)) {
            node.properties[key] = prefix + v;
          }
        }
      }
      (node.children ?? []).forEach(walk);
    };
    walk(tree);
  };
}

export default defineConfig({
  site: 'https://tipharethstudio.github.io',
  base: basePath,
  integrations: [mdx()],
  vite: {
    plugins: [tailwindcss()],
  },
  markdown: {
    shikiConfig: {
      theme: 'github-dark-default',
    },
    rehypePlugins: [rehypeBasePrefix],
  },
});
