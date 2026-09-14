import { execFileSync } from 'node:child_process'
import { mkdirSync, readdirSync, readFileSync, rmSync } from 'node:fs'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Turns the build's `dist/` into the one file the Chrome Web Store accepts: a
 * ZIP with `manifest.json` at its root.
 *
 * The store's upload form takes a ZIP, not a folder, and it looks for the
 * manifest in the root directory rather than inside a wrapper. Zipping
 * `apps/extension/dist` from its parent would produce `dist/manifest.json` and
 * be rejected, so this runs `zip` from inside `dist` with a list of relative
 * paths, which is what puts the manifest where the store looks.
 *
 * The file list is built here rather than handed to `zip -r` because a
 * recursive zip packs whatever happens to be in the folder. A source map from
 * an experiment, a `.DS_Store` the Finder left behind: both would ship, and the
 * first ships the extension's sources to anyone who unzips it. So the list is
 * filtered by rule and both halves are printed, which makes what went in and
 * what stayed out something you read rather than something you assume.
 *
 * `zip` is macOS's own and has no install step. Adding a Node zip library to
 * get the same file would make this build's only production-adjacent
 * dependency a build tool.
 */

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const dist = join(root, 'dist')
/** Beside `dist/`, not inside it: the next build empties `dist/`. */
const outDir = join(root, 'dist-zip')

/**
 * Files that must not reach the store.
 *
 * Source maps are the important one. The rest is the debris a Mac leaves in a
 * folder, which costs bytes in the upload and reads as carelessness in review.
 */
const EXCLUDED = [
  { rule: /\.map$/, why: 'source map' },
  { rule: /(^|\/)\.DS_Store$/, why: 'Finder metadata' },
  { rule: /(^|\/)\./, why: 'dotfile' },
]

/** Why this file is excluded, or `undefined` if it is not. */
function excludedBecause(file) {
  return EXCLUDED.find((entry) => entry.rule.test(file))?.why
}

/** Every file in `dist`, as forward-slashed paths relative to `dist`. */
function producedFiles() {
  const files = []
  for (const entry of readdirSync(dist, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue
    files.push(relative(dist, join(entry.parentPath, entry.name)).split(sep).join('/'))
  }
  return files.sort()
}

let manifest
try {
  manifest = JSON.parse(readFileSync(join(dist, 'manifest.json'), 'utf8'))
} catch (cause) {
  console.error(`package: cannot read dist/manifest.json (${cause.message})`)
  console.error('package: run the build first; `pnpm --filter @snapdeck/extension build` writes dist/.')
  process.exit(1)
}

if (typeof manifest.version !== 'string' || manifest.version.length === 0) {
  console.error('package: dist/manifest.json has no version, and the store names uploads by it')
  process.exit(1)
}

const included = []
const excluded = []
for (const file of producedFiles()) {
  const why = excludedBecause(file)
  if (why) excluded.push(`${file} (${why})`)
  else included.push(file)
}

// The manifest was read from `dist/manifest.json`, so it is there; this is
// about the rules above never growing one that filters it out.
if (!included.includes('manifest.json')) {
  console.error('package: manifest.json is not in the file list, and the store looks for it in the root')
  process.exit(1)
}

const zipPath = join(outDir, `snapdeck-extension-${manifest.version}.zip`)

mkdirSync(outDir, { recursive: true })
// `zip` adds to an archive that is already there rather than replacing it, so a
// file dropped between two runs would survive in the second run's output.
rmSync(zipPath, { force: true })

// `-X` leaves out the extended attributes and resource forks a Mac would
// otherwise store beside each entry, which the store has no use for.
execFileSync('zip', ['-X', '-q', zipPath, ...included], { cwd: dist, stdio: 'inherit' })

console.log(`package: ${relative(root, zipPath)}`)
for (const file of included) console.log(`  + ${file}`)
for (const file of excluded) console.log(`  - ${file}`)
