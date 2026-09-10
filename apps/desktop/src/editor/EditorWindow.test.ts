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
import { savePathFor } from './EditorWindow'

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
