/**
 * `savePathFor` is exported to be tested, and this is that test.
 *
 * It is three lines of string work with a consequence out of proportion to its
 * size: the path it returns is what decides whether a save updates the capture
 * the user already has or leaves a second file beside it, and Rust refuses
 * anything whose stem is not the capture's, so getting it wrong turns Save into
 * an error message rather than into a wrong file.
 */

import { describe, expect, it } from 'vitest'
import { formatForPath, savePathFor } from './EditorWindow'

const CAPTURE = '/Users/someone/Pictures/Snapdeck 2026-09-09 at 12.00.00.png'

describe('savePathFor', () => {
  // The common case, and the reason the page is handed the capture's own path:
  // saving as the format the capture is already in overwrites it instead of
  // growing a copy per edit.
  it('keeps the capture path when the format is unchanged', () => {
    expect(savePathFor(CAPTURE, 'image/png')).toBe(CAPTURE)
  })

  // A different format is a different extension, so a different path, so the
  // new file lands beside the original rather than replacing it.
  it('swaps the extension for a different format', () => {
    expect(savePathFor(CAPTURE, 'image/jpeg')).toBe(
      '/Users/someone/Pictures/Snapdeck 2026-09-09 at 12.00.00.jpg',
    )
  })

  // Only the last segment's own extension. A directory with a dot in its name
  // above a file that has none is the case the regex is written for: rewriting
  // the directory would send the save outside the pictures folder, where Rust
  // refuses it, and the user would see a save that simply fails.
  it('rewrites only the file name, never a dotted directory', () => {
    expect(savePathFor('/Users/someone/Screenshots.2026/capture', 'image/png')).toBe(
      '/Users/someone/Screenshots.2026/capture.png',
    )
    expect(savePathFor('/Users/someone/Screenshots.2026/capture.png', 'image/jpeg')).toBe(
      '/Users/someone/Screenshots.2026/capture.jpg',
    )
  })

  // Not reachable from this editor, which only ever asks for the two formats
  // above. Pinned because the alternative the code deliberately avoids, an
  // invented `.undefined`, would be a path Rust refuses and a file the user
  // never asked for; keeping the original at least names the file they meant.
  it('keeps the original path for a format it has no extension for', () => {
    expect(savePathFor(CAPTURE, 'image/webp' as 'image/png')).toBe(CAPTURE)
  })
})

/**
 * The other half of the same decision, and the one that decides whether the
 * original survives a save.
 *
 * `savePathFor` says where a format is written; this says which format the
 * editor opens on. Get it wrong for a JPEG capture and the two disagree: the
 * editor offers PNG, `savePathFor` turns the save into `X.png`, and `X.jpg`,
 * the unedited capture, stays on disk beside it. That is the file a user who
 * redacted a secret was trying to be rid of.
 */
describe('formatForPath', () => {
  // The setting is `defaultFormat`, so a capture is written under either
  // extension and the editor has to open on whichever one it was handed.
  it('reads the format the capture was written in', () => {
    expect(formatForPath(CAPTURE)).toBe('image/png')
    expect(formatForPath('/Users/someone/Pictures/Snapdeck 2026-09-09 at 12.00.00.jpg')).toBe(
      'image/jpeg',
    )
  })

  // macOS file names are not case sensitive and a hand-renamed capture is
  // still the capture the window was opened on.
  it('does not mind the case of the extension', () => {
    expect(formatForPath('/Users/someone/Pictures/capture.JPG')).toBe('image/jpeg')
    expect(formatForPath('/Users/someone/Pictures/capture.PNG')).toBe('image/png')
  })

  // Only the last segment's own extension, for the reason `savePathFor` reads
  // only that segment: a dotted directory above a file that has none must not
  // be what decides the format.
  it('reads only the file name, never a dotted directory', () => {
    expect(formatForPath('/Users/someone/Screenshots.png/capture')).toBeUndefined()
  })

  // Undefined rather than a format of this module's own. No capture Rust wrote
  // can land here, and the editor's own first format is the one place that
  // fallback is written down.
  it('names no format for an extension nothing here writes', () => {
    expect(formatForPath('/Users/someone/Pictures/capture.webp')).toBeUndefined()
    expect(formatForPath('/Users/someone/Pictures/capture')).toBeUndefined()
  })
})
