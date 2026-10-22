import typography from '@tailwindcss/typography';

/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{astro,html,js,jsx,md,mdx,svelte,ts,tsx,vue}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        brand: {
          50: '#fff5f2',
          100: '#ffe8e2',
          200: '#ffd4c7',
          300: '#ffb5a1',
          400: '#fd8768',
          500: '#f06543', // Rust-Coral primary
          600: '#dc4a26',
          700: '#b83819',
          800: '#942f17',
          900: '#7a2b18',
          950: '#421309',
        },
        php: {
          500: '#6366f1',
          600: '#4f46e5',
        },
        dark: {
          bg: '#0a0d12',
          surface: '#111620',
          border: '#1f293d',
          card: '#161d2b',
        }
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['JetBrains Mono', 'Fira Code', 'monospace'],
      }
    },
  },
  plugins: [typography],
};
