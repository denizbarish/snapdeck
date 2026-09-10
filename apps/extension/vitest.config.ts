import { defineConfig } from 'vitest/config'

/**
 * One project, Node environment. What this package tests is arithmetic over
 * plain data: the manifest is JSON and the checks read it as such. The parts
 * that genuinely touch a browser (`chrome.*`, the DOM, canvas) live behind thin
 * shells that are driven by injected collaborators, so a browser runner would
 * only make the suite slower without making a pass mean more.
 */
export default defineConfig({
  test: {
    name: 'node',
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
})
