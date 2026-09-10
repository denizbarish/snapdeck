import { configDefaults, defineConfig } from 'vitest/config'

/**
 * Two projects, the same split `packages/editor` makes and for the same reason:
 * some of what this package claims is arithmetic and some of it is a question
 * only a browser engine can answer.
 *
 * The manifest checks, the page measurement and the scroll plan are arithmetic
 * over plain data. They run in Node in milliseconds, and a browser would make
 * the suite slower without making a pass mean more.
 *
 * `sticky` is the other kind. It asks the engine for an element's used
 * `position` and it claims something about what the engine does to a page's
 * layout when an element stops being painted. A simulated DOM answers both out
 * of values the test itself wrote, so a pass there would prove nothing about
 * the page the content script is actually injected into.
 */
export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'node',
          environment: 'node',
          // `scripts` is in the list because `check-manifest.mjs` is plain
          // JavaScript run by Node, and its test drives it as a subprocess.
          include: ['src/**/*.test.ts', 'scripts/**/*.test.mjs'],
          // Extends the defaults rather than replacing them: written as a bare
          // list, this project would start collecting tests out of
          // `node_modules` and `dist`.
          exclude: [...configDefaults.exclude, 'src/content/sticky.test.ts'],
        },
      },
      {
        test: {
          name: 'browser',
          include: ['src/content/sticky.test.ts'],
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
