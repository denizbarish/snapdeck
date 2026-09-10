import { defineConfig } from 'vitest/config'

/**
 * One project, Node environment. The contract is JSON in and JSON out: there is
 * no canvas, no DOM and no browser API anywhere in this package, so a browser
 * runner would only make the suite slower without making a pass mean more.
 */
export default defineConfig({
  test: {
    name: 'node',
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
})
