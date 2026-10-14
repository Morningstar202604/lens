import { defineConfig } from 'vite';
import preact from '@preact/preset-vite';

// Vite config for Lens frontend.
// Build output goes to ../web which is served by the Rust axum backend.
export default defineConfig({
  plugins: [preact()],
  build: {
    outDir: '../web',
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:7317',
      '/static': 'http://127.0.0.1:7317',
    },
  },
});
