import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

// The fixture build swaps the Soulseek client for a mock so browser tests stay offline.
export default defineConfig(({ mode }) => ({
  base: './',
  optimizeDeps: { exclude: ['@project516/nicotine-plus-wasm', '@project516/yt-dlp-wasm', '@ffmpeg/ffmpeg'] },
  worker: { format: 'es' },
  assetsInclude: ['**/*.whl'],
  resolve: mode === 'fixture'
    ? { alias: { '@project516/nicotine-plus-wasm': fileURLToPath(new URL('test-browser/nicotine-mock.js', import.meta.url)) } }
    : {},
  build: {
    outDir: mode === 'fixture' ? 'dist-fixture' : 'dist',
    rollupOptions: { output: { assetFileNames: (asset) => asset.names.some((name) => name.endsWith('.whl')) ? 'assets/[name][extname]' : 'assets/[name]-[hash][extname]' } },
  },
}));
