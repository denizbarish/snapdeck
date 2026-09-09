import { configDefaults, defineConfig } from 'vitest/config'

/**
 * Two projects, because two of the test files need a rasteriser and the rest
 * do not.
 *
 * The model, the commands and the hit tests are arithmetic; they run in Node
 * in milliseconds. `render` and `export` measure real pixels, and the only
 * honest place to measure them is the environment the code actually ships to:
 * a browser engine with a real `OffscreenCanvas`, a real 2D context and a real
 * `convertToBlob`. A Node canvas binding would be a different rasteriser than
 * either the Tauri webview or the extension, so a pass there would prove less
 * than it looks.
 */
export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'node',
          include: ['src/**/*.test.ts'],
          // Extends the defaults rather than replacing them: written as a bare
          // list, this project would start collecting tests out of
          // `node_modules` and `dist`.
          exclude: [...configDefaults.exclude, 'src/render.test.ts', 'src/export.test.ts'],
        },
      },
      {
        test: {
          name: 'browser',
          include: ['src/render.test.ts', 'src/export.test.ts'],
          browser: {
            enabled: true,
            provider: 'playwright',
            headless: true,
            screenshotFailures: false,
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
})
