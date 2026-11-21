import vue from '@vitejs/plugin-vue'
import unocss from 'unocss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [unocss(), vue()],

  // Tauri owns the terminal output; do not let Vite wipe it.
  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})
