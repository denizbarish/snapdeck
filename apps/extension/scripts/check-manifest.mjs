import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

import contentConfig from '../vite.content.config.ts'
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
/** The build's own output, or the directory a test hands over. */
const dist = process.argv[2] ? resolve(process.argv[2]) : join(root, 'dist')

/** Every file in `dist`, as forward-slashed paths relative to `dist`. */
function producedFiles() {
  const files = []
  for (const entry of readdirSync(dist, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue
    files.push(relative(dist, join(entry.parentPath, entry.name)).split(sep).join('/'))
  }
  return files
}

/**
 * The scripts the extension injects with `chrome.scripting` rather than naming
 * in the manifest, which is why `missingManifestFiles` cannot see them: there
 * is no line of the manifest to read them off. The name is taken from the build
 * that produces it instead of written here a second time, so renaming the
 * output cannot leave this check watching a file nobody makes any more.
 */
function injectedFiles() {
  const lib = contentConfig.build?.lib
  const fileName = typeof lib?.fileName === 'function' ? lib.fileName('iife', 'main') : lib?.fileName
  if (typeof fileName !== 'string' || fileName.length === 0) {
    console.error('check-manifest: cannot read the content script name out of vite.content.config.ts')
    process.exit(1)
  }
  return [fileName]
}

/** Injected scripts the build did not produce. */
function missingInjectedFiles(injected, produced) {
  const available = new Set(produced)
  return injected.filter((file) => !available.has(file))
}

let manifest
try {
  manifest = JSON.parse(readFileSync(join(dist, 'manifest.json'), 'utf8'))
} catch (cause) {
  console.error(`check-manifest: cannot read dist/manifest.json (${cause.message})`)
  console.error('check-manifest: run the Vite builds first; `public/` is copied by the main one.')
  process.exit(1)
}

const produced = producedFiles()

const missing = missingManifestFiles(manifest, produced)
if (missing.length > 0) {
  console.error(`check-manifest: the manifest names ${missing.length} file(s) the build did not produce:`)
  for (const file of missing) console.error(`  - ${file}`)
  process.exit(1)
}

const missingInjected = missingInjectedFiles(injectedFiles(), produced)
if (missingInjected.length > 0) {
  console.error(
    `check-manifest: the extension injects ${missingInjected.length} file(s) the build did not produce:`,
  )
  for (const file of missingInjected) console.error(`  - ${file}`)
  console.error('check-manifest: the content build is the second Vite pass; it may not have run.')
  process.exit(1)
}

console.log('check-manifest: every file the manifest names or injects is in dist/.')
