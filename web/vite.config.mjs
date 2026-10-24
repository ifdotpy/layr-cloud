import { defineConfig } from 'vite';
export default defineConfig({
  server: { host: '127.0.0.1', fs: { strict: true } },
  build: { target: 'es2022' },
  worker: { format: 'es' },
});
