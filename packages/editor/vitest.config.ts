import { configDefaults, defineConfig } from 'vitest/config'

/**
 * Two projects, because some of the test files need a browser and the rest do
 * not.
 *
 * The model, the commands and the hit tests are arithmetic; they run in Node
 * in milliseconds. `render` and `export` measure real pixels, and the only
 * honest place to measure them is the environment the code actually ships to:
 * a browser engine with a real `OffscreenCanvas`, a real 2D context and a real
 * `convertToBlob`. A Node canvas binding would be a different rasteriser than
 * either the Tauri webview or the extension, so a pass there would prove less
 * than it looks.
 *
 * `Editor` joins them for the same reason by a different route: it is driven by
 * real pointer and keyboard input through the provider, and the defects it
 * exists to catch live in what a range input does on the way from one value to
 * another and in what a browser does with Cmd+S. A simulated event or a
 * programmatic `fill` reproduces neither.
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
          include: ['src/render.test.ts', 'src/export.test.ts', 'src/Editor.test.tsx'],
          // Driving the component takes real presses and real key repeats, and
          // each one is a round trip to the browser. The renderer's own tests
          // are unaffected by the raise.
          testTimeout: 30_000,
          browser: {
            enabled: true,
            provider: 'playwright',
            headless: true,
            screenshotFailures: false,
            // Big enough to hold the editor's own 800x600 test container
            // without the provider having to scroll it into view, which would
            // move every coordinate a press is aimed at.
            viewport: { width: 1000, height: 800 },
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
})
