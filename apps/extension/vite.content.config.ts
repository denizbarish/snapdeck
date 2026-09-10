import { defineConfig } from 'vite'

/**
 * The second of two builds, and it exists because a Manifest V3 content script
 * cannot be an ES module: Chrome evaluates the injected file as a classic
 * script, so an `import` in it is a syntax error at inject time. Library mode
 * with `formats: ['iife']` gives a single self-contained file instead.
 *
 * `emptyOutDir: false` because this pass runs after the main build and must add
 * to `dist/` rather than replace it, and `publicDir: false` because the main
 * build already copied `public/`.
 */
export default defineConfig({
  publicDir: false,
  build: {
    emptyOutDir: false,
    lib: {
      entry: 'src/content/main.ts',
      name: 'SnapdeckContent',
      formats: ['iife'],
      fileName: () => 'content.js',
    },
  },
})
