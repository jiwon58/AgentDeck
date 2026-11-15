import { defineConfig } from 'vite';
export default defineConfig({
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
  build: { rollupOptions: { output: { manualChunks: { terminal: ['@xterm/xterm', '@xterm/addon-fit'] } } } },
  clearScreen: false,
});
