import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig(({ mode }) => ({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: {
      '#app-entry': fileURLToPath(
        new URL(
          mode === 'weekly' ? './src/app/WeeklyApp.tsx' : './src/app/StandardApp.tsx',
          import.meta.url,
        ),
      ),
    },
  },
  server: {
    strictPort: true,
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
  },
}));
