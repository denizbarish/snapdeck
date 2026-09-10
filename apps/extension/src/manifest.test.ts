import { describe, expect, it } from 'vitest'

import packageJson from '../package.json'
import manifestJson from '../public/manifest.json'
import { missingManifestFiles, type Manifest } from './manifest'

/**
 * The manifest is hand-written, so nothing but a test stands between an
 * absent-minded edit and a shipped permission. These read the real file rather
 * than a fixture for exactly that reason.
 */
const manifest: Manifest = manifestJson

describe('manifest.json', () => {
  it('declares no host permissions', () => {
    // E1. The extension can read a page only after the user clicks its button,
    // and only that tab. A host permission would silently turn that into
    // standing access, so its mere presence is the failure.
    expect('host_permissions' in manifestJson).toBe(false)
    expect(manifest.host_permissions).toBeUndefined()
  })

  it('asks for exactly the four permissions it uses', () => {
    // E2. `tabs` in particular hands over the URL of every open tab, and
    // nothing here needs it: `activeTab` covers the one tab the user pointed at.
    expect(manifest.permissions).toEqual(['activeTab', 'scripting', 'storage', 'downloads'])
    expect(manifest.permissions).not.toContain('tabs')
  })

  it('is a Manifest V3 extension with an ES module service worker', () => {
    // E3. Without `type: 'module'` the worker cannot use static imports, which
    // is how it reaches `@snapdeck/protocol`.
    expect(manifest.manifest_version).toBe(3)
    expect(manifest.background.type).toBe('module')
  })

  it('carries the same version as the package', () => {
    // E4. Two places to bump is one place to forget.
    expect(manifest.version).toBe(packageJson.version)
  })
})

describe('missingManifestFiles', () => {
  it('reports a file the manifest names that the build did not produce', () => {
    // E5, first half.
    expect(missingManifestFiles(manifest, ['content.js', 'options.html'])).toEqual([
      'background.js',
    ])
    expect(missingManifestFiles(manifest, ['background.js', 'content.js'])).toEqual([
      'options.html',
    ])
  })

  it('reports nothing when the build produced every file the manifest names', () => {
    // E5, second half.
    expect(
      missingManifestFiles(manifest, ['background.js', 'content.js', 'options.html']),
    ).toEqual([])
  })
})
