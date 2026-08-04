import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { fileURLToPath } from 'node:url';

// Vite build for the Electron renderer (Codex-style UI on Vue 3 + TS).
// Styles use Emotion (CSS-in-JS) — see src/renderer/**/*.styles.ts + styles/.
// Dev: `npm run dev` serves on :5199; Electron main loads the built
// dist/renderer/index.html for production.
export default defineConfig({
  root: 'src/renderer',
  base: './', // file:// protocol (Electron loadFile) needs relative asset paths
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src/renderer', import.meta.url)),
    },
  },
  build: {
    outDir: '../../dist/renderer',
    emptyOutDir: true,
    target: 'chrome120',
  },
  server: {
    port: 5199,
    strictPort: true,
  },
});
