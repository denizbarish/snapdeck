import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

import { missingManifestFiles } from '../src/manifest.ts'

/**
 * Runs after both Vite builds. The manifest is hand-written, so a renamed entry
 * or a dropped build pass would otherwise show up as a broken install rather
 * than a failed build. This turns that into a non-zero exit.
 *
 * Node strips the types out of the imported `.ts` on its own, which keeps the
 * one implementation of `missingManifestFiles` shared between this script and
 * its test instead of transcribed into a second copy that can drift.
 */

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const dist = join(root, 'dist')

/** Every file in `dist`, as forward-slashed paths relative to `dist`. */
function producedFiles() {
  const files = []
  for (const entry of readdirSync(dist, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue
    files.push(relative(dist, join(entry.parentPath, entry.name)).split(sep).join('/'))
  }
  return files
}

let manifest
try {
  manifest = JSON.parse(readFileSync(join(dist, 'manifest.json'), 'utf8'))
} catch (cause) {
  console.error(`check-manifest: cannot read dist/manifest.json (${cause.message})`)
  console.error('check-manifest: run the Vite builds first; `public/` is copied by the main one.')
  process.exit(1)
}

const missing = missingManifestFiles(manifest, producedFiles())
if (missing.length > 0) {
  console.error(`check-manifest: the manifest names ${missing.length} file(s) the build did not produce:`)
  for (const file of missing) console.error(`  - ${file}`)
  process.exit(1)
}

console.log('check-manifest: every file the manifest names is in dist/.')
