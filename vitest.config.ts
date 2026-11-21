import vue from '@vitejs/plugin-vue'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [vue()],

  resolve: {
    alias: {
      '@/test': fileURLToPath(new URL('./src/__test__', import.meta.url)),
    },
  },

  test: {
    environment: 'happy-dom',
    // Root-level tests cover the config files that sit next to them
    // (uno.config.ts); everything else is colocated under src/, including the
    // companion VS Code extension's own src/
    include: ['src/**/*.test.ts', 'extension/src/**/*.test.ts', '*.test.ts'],
    // Automatically reset mocks after each test
    mockReset: true,
  },
})
