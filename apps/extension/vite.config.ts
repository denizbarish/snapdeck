import { defineConfig } from 'vite'

/**
 * The first of two builds: the service worker and the options page, both as ES
 * modules. `apps/desktop` already drives multi-entry plain Vite the same way,
 * and keeping that shape here is what lets the manifest stay hand-written -
 * there is no generated manifest that can quietly drift from the real file
 * names. `scripts/check-manifest.mjs` is the audit that keeps them honest.
 *
 * Names are unhashed on purpose: the manifest points at `background.js` by that
 * exact name, and a content-hash suffix would break it on every edit.
 *
 * `public/manifest.json` needs no rule of its own; Vite copies `public/`
 * verbatim into `dist/`.
 */
export default defineConfig({
  build: {
    emptyOutDir: true,
    rollupOptions: {
      input: {
        background: 'src/background/main.ts',
        options: 'options.html',
      },
      output: {
        entryFileNames: '[name].js',
        // Shared code goes in a folder of its own. Named at the top level it
        // takes the name of whichever module Rollup happened to hoist it out
        // of, which reads like an entry point and can collide with a real one.
        chunkFileNames: 'chunks/[name].js',
        assetFileNames: '[name][extname]',
      },
    },
  },
})
