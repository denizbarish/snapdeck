import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'
import { defineConfig } from 'vite'

/**
 * The dev server the screenshot harness loads its pages from.
 *
 * A server rather than a build, because nothing here is shipped: `capture.mjs`
 * starts it, drives a headless Chromium over it and closes it again, so a
 * `dist/` would only be a directory to forget to clean up.
 *
 * `fs.allow` reaches the repository root on purpose. Two of them
 * import components out of `apps/desktop`, which pnpm links in as a workspace
 * dependency, and Vite resolves that symlink to its real path before it checks
 * whether it is allowed to serve it.
 */
export default defineConfig({
  plugins: [react()],
  root: __dirname,
  server: {
    fs: { allow: [resolve(__dirname, '../..')] },
  },
  build: {
    rollupOptions: {
      input: {
        editor: resolve(__dirname, 'editor.html'),
        overlay: resolve(__dirname, 'overlay.html'),
        settings: resolve(__dirname, 'settings.html'),
        promo: resolve(__dirname, 'promo.html'),
      },
    },
  },
})
