import { defineConfig } from 'vite';

export default defineConfig({
  root: '.',
  publicDir: 'public',
  server: {
    host: '127.0.0.1',
    port: 4010,
    strictPort: true,
    open: '/',
    fs: { strict: false },
    allowedHosts: true,
  },
  preview: {
    host: '127.0.0.1',
    port: 4010,
    strictPort: true,
  },
  build: {
    target: 'esnext',
  },
});
