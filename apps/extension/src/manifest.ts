/**
 * The manifest is written by hand and the build is two plain Vite passes, so
 * nothing guarantees on its own that a name in the manifest matches a file in
 * `dist`. This module is the guarantee: it is pure, which lets a test read the
 * real manifest, and it is what `scripts/check-manifest.mjs` calls after the
 * build so a rename fails the build instead of the install.
 */

export type Manifest = {
  manifest_version: number
  version: string
  permissions: string[]
  host_permissions?: string[]
  background: { service_worker: string; type: string }
  options_ui: { page: string; open_in_tab: boolean }
  icons?: Record<string, string>
  action?: { default_title?: string; default_icon?: Record<string, string> }
}

/** Files the manifest names that the build did not produce. */
export function missingManifestFiles(manifest: Manifest, produced: string[]): string[] {
  const named = [
    manifest.background.service_worker,
    manifest.options_ui.page,
    // Icons too: a renamed icon leaves the build green and the installed
    // extension wearing Chrome's grey square.
    ...Object.values(manifest.icons ?? {}),
    ...Object.values(manifest.action?.default_icon ?? {}),
  ]
  const available = new Set(produced)
  const missing: string[] = []
  for (const file of named) {
    if (!available.has(file) && !missing.includes(file)) missing.push(file)
  }
  return missing
}
