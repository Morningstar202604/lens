/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        primary: {
          DEFAULT: '#6366f5',
          soft: 'rgba(99,102,245,.08)',
        },
        bg: {
          DEFAULT: '#14151a',
          card: '#1a1c22',
          elev: '#1e2028',
        },
        border: {
          DEFAULT: 'rgba(255,255,255,.06)',
          dim: 'rgba(255,255,255,.04)',
          hover: 'rgba(255,255,255,.12)',
          active: 'rgba(99,102,245,.4)',
        },
        text: {
          DEFAULT: '#e4e4e9',
          2: '#c8cae0',
          3: '#9a9cb5',
          4: '#6b6d85',
        },
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Fira Code', 'SF Mono', 'monospace'],
      },
    },
  },
  plugins: [],
};
