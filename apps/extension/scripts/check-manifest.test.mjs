import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, copyFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { afterEach, describe, expect, it } from 'vitest'

/**
 * Plain JavaScript, like the script it drives. `check-manifest.mjs` is run by
 * Node with no build step, and the claim worth testing is what it does to its
 * exit code, so the test spawns it exactly the way the build script does.
 */

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const script = join(root, 'scripts', 'check-manifest.mjs')

/**
 * The file `chrome.scripting` injects. No line of the manifest names it, which
 * is the whole reason it needs a check of its own.
 */
const INJECTED = 'content.js'

let dist

afterEach(() => {
  if (dist) rmSync(dist, { recursive: true, force: true })
  dist = undefined
})

/** A `dist` holding the real manifest and each of `files` as an empty file. */
function fakeDist(files) {
  dist = mkdtempSync(join(tmpdir(), 'snapdeck-check-manifest-'))
  copyFileSync(join(root, 'public', 'manifest.json'), join(dist, 'manifest.json'))
  mkdirSync(join(dist, 'icons'), { recursive: true })
  for (const file of [...files, ...ICONS]) writeFileSync(join(dist, file), '')
  return dist
}

/** Named by the manifest, and copied rather than built, so always present. */
const ICONS = ['icons/16.png', 'icons/32.png', 'icons/48.png', 'icons/128.png']

function check(directory) {
  return spawnSync(process.execPath, [script, directory], { cwd: root, encoding: 'utf8' })
}

describe('check-manifest', () => {
  it('accepts a build that produced every file the extension loads', () => {
    const result = check(fakeDist(['background.js', 'options.html', INJECTED]))

    expect(result.stderr).toBe('')
    expect(result.status).toBe(0)
  })

  it('rejects a build that is missing the injected content script', () => {
    // The negative path this check exists for. `content.js` is injected on
    // demand rather than named in the manifest, so without this the build stays
    // green and the failure surfaces as a broken capture on a user's page.
    const result = check(fakeDist(['background.js', 'options.html']))

    expect(result.status).toBe(1)
    expect(result.stderr).toContain(INJECTED)
  })

  it('rejects a build that is missing a file the manifest names', () => {
    const result = check(fakeDist(['options.html', INJECTED]))

    expect(result.status).toBe(1)
    expect(result.stderr).toContain('background.js')
  })
})
