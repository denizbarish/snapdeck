/**
 * Export tests.
 *
 * Two things are worth proving here and neither is "the function returns a
 * canvas". First, that export and preview cannot drift, because export goes
 * through the same `renderDocument` and the test compares the two outputs byte
 * for byte. Second, that the file a user hands to a stranger has nothing of the
 * redacted region left in it: not in the decoded pixels, not in a metadata
 * chunk, not in a stray copy of the layer that redacted it.
 */

import { describe, expect, it } from 'vitest'
import type { EditorDocument, Layer, Rect } from './model'
import { exportCanvas, toBlob } from './export'
import { renderDocument } from './render'
import {
  blankCanvas,
  context2d,
  differingBytes,
  identicalFraction,
  neighbourDelta,
  noiseImage,
  readPixels,
  sourceCorrelation,
  textImage,
} from './__fixtures__/canvas'

function documentOf(width: number, height: number, layers: Layer[], crop: Rect | null = null): EditorDocument {
  return { width, height, crop, layers }
}

/** Decode an encoded blob back to pixels, the way a recipient's viewer would. */
async function decode(blob: Blob): Promise<ImageData> {
  const bitmap = await createImageBitmap(blob)
  const canvas = blankCanvas(bitmap.width, bitmap.height)
  context2d(canvas).drawImage(bitmap, 0, 0)
  bitmap.close()
  return readPixels(canvas)
}

/** The four-character type of every chunk in a PNG file, in order. */
function pngChunkTypes(bytes: Uint8Array): string[] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const types: string[] = []
  let offset = 8
  while (offset + 8 <= bytes.length) {
    const length = view.getUint32(offset)
    types.push(String.fromCharCode(...bytes.subarray(offset + 4, offset + 8)))
    offset += 12 + length
  }
  return types
}

function containsAscii(bytes: Uint8Array, text: string): boolean {
  const needle = [...text].map((character) => character.charCodeAt(0))
  outer: for (let start = 0; start + needle.length <= bytes.length; start += 1) {
    for (let index = 0; index < needle.length; index += 1) {
      if (bytes[start + index] !== needle[index]) continue outer
    }
    return true
  }
  return false
}

describe('exportCanvas', () => {
  it('is the size of the document when there is no crop, and of the crop when there is', () => {
    const image = noiseImage(64, 48)

    const full = exportCanvas(image, documentOf(64, 48, []))
    expect([full.width, full.height]).toEqual([64, 48])

    const cropped = exportCanvas(image, documentOf(64, 48, [], { x: 5, y: 7, width: 31, height: 23 }))
    expect([cropped.width, cropped.height]).toEqual([31, 23])

    // A fractional crop is rounded once, in `viewOf`, so the size and the
    // origin the renderer translates by cannot disagree.
    const fractional = exportCanvas(image, documentOf(64, 48, [], { x: 5.5, y: 7.5, width: 31.2, height: 23.4 }))
    expect([fractional.width, fractional.height]).toEqual([31, 23])
  })

  it('never redacts the preview more strongly than the file it hands over', () => {
    // Zoom-to-fit on a large capture is the default view, so the preview is
    // usually the smaller of the two. The user judges "is this covered?" on
    // what is on screen, so the file may not be the weaker of the two.
    const image = textImage(128, 24)
    const region = { x: 16, y: 0, width: 96, height: 24 }
    const doc = documentOf(128, 24, [
      { id: 'a', kind: 'obscure', rect: region, mode: 'pixelate', intensity: 2 },
    ])
    const plain = documentOf(128, 24, [])

    /**
     * How much of the unredacted picture's structure the redacted one still
     * carries at a given scale.
     *
     * Measured against an unredacted render at the SAME scale, so the detail a
     * zoomed-out preview loses to resampling divides out and what is left is
     * the redaction's own effect. Otherwise the two scales are not comparable
     * and neither is the claim.
     */
    const detailAt = (scale: number): number => {
      const size = { width: Math.round(128 * scale), height: Math.round(24 * scale) }
      const draw = (document: EditorDocument): ImageData => {
        const canvas = blankCanvas(size.width, size.height)
        const ctx = context2d(canvas)
        ctx.scale(scale, scale)
        renderDocument(ctx, image, document)
        return readPixels(canvas)
      }
      // Inset by a device pixel, so the rounding of the region's own edges
      // cannot put a source pixel inside the window being measured.
      const left = Math.ceil(region.x * scale) + 1
      const top = Math.ceil(region.y * scale) + 1
      const window = {
        x: left,
        y: top,
        width: Math.floor((region.x + region.width) * scale) - left - 1,
        height: Math.floor((region.y + region.height) * scale) - top - 1,
      }
      return sourceCorrelation(draw(doc), draw(plain), window)
    }

    const exported = sourceCorrelation(
      readPixels(exportCanvas(image, doc)),
      readPixels(exportCanvas(image, plain)),
      { x: region.x + 1, y: region.y + 1, width: region.width - 2, height: region.height - 2 },
    )

    // The export is genuinely redacted, not merely no worse than a preview
    // that is itself doing nothing.
    expect(exported).toBeLessThan(0.5)
    // Scales chosen so the mapping to device pixels is fractional: a six pixel
    // block is 1.5 device pixels at a quarter scale, where rounding to two
    // would make the preview a third stronger than the file.
    for (const scale of [0.25, 0.3, 0.5, 0.6]) {
      expect(exported).toBeLessThanOrEqual(detailAt(scale) + 0.02)
    }

    // Direction is all the loop above claims, and at a small enough scale it
    // claims nothing: the block is measured in the device pixels of the target
    // and floored at 1, so a six-pixel block is one device pixel at a quarter
    // scale and the preview is the identity function. "No stronger than an
    // unredacted render" is true of everything, so the loop passes there
    // whatever the renderer does.
    //
    // Both halves are therefore pinned by value. Where the block survives as
    // three device pixels the preview is genuinely redacted, which is what
    // makes the inequality above a test rather than a tautology.
    for (const scale of [0.5, 0.6]) {
      expect(detailAt(scale)).toBeLessThan(0.8)
    }
    // And where it does not, the preview keeps nearly all of the structure the
    // file has lost. That is the trap `renderDocument` warns importers about
    // and the reason `Editor.tsx` renders through `exportCanvas` below 1:1: the
    // file is redacted exactly as asked while the screen shows the secret. A
    // renderer that ever fixed this in place would make this expectation fail,
    // which is the right way round for it to be noticed.
    for (const scale of [0.25, 0.3]) {
      expect(detailAt(scale)).toBeGreaterThan(0.9)
    }
  })

  it('produces exactly what the preview draws, because it is the same code', () => {
    const image = noiseImage(64, 48)
    const crop = { x: 8, y: 6, width: 40, height: 30 }
    const doc = documentOf(
      64,
      48,
      [
        { id: 'a', kind: 'obscure', rect: { x: 12, y: 10, width: 16, height: 12 }, mode: 'pixelate', intensity: 4 },
        { id: 'b', kind: 'arrow', from: { x: 10, y: 8 }, to: { x: 40, y: 30 }, style: { color: '#ff3b30', width: 3 } },
        { id: 'c', kind: 'step', center: { x: 30, y: 20 }, index: 1, style: { fill: '#ff3b30', color: '#ffffff', size: 14 } },
      ],
      crop,
    )

    const preview = blankCanvas(crop.width, crop.height)
    renderDocument(context2d(preview), image, doc)

    expect(differingBytes(readPixels(exportCanvas(image, doc)), readPixels(preview))).toBe(0)
  })
})

describe('toBlob', () => {
  it('encodes losslessly to PNG, so the decoded file is the exported canvas', async () => {
    const image = noiseImage(48, 32)
    const doc = documentOf(48, 32, [
      { id: 'a', kind: 'obscure', rect: { x: 8, y: 8, width: 16, height: 16 }, mode: 'blur', intensity: 3 },
    ])

    const blob = await toBlob(image, doc, 'image/png')

    expect(blob.type).toBe('image/png')
    expect(blob.size).toBeGreaterThan(0)
    expect(differingBytes(await decode(blob), readPixels(exportCanvas(image, doc)))).toBe(0)
  })

  it('passes the quality through to the lossy encoders', async () => {
    const image = noiseImage(96, 96)
    const doc = documentOf(96, 96, [])

    const coarse = await toBlob(image, doc, 'image/jpeg', 0.1)
    const fine = await toBlob(image, doc, 'image/jpeg', 0.95)

    expect(coarse.type).toBe('image/jpeg')
    expect(fine.type).toBe('image/jpeg')
    expect(coarse.size).toBeLessThan(fine.size)
  })

  it('refuses a format this platform will not encode', async () => {
    const image = noiseImage(32, 32)
    const doc = documentOf(32, 32, [])

    // `convertToBlob` is specified to answer a type it cannot honour with a
    // PNG, silently, and the caller names the file after what it asked for.
    // WKWebView does exactly that with `image/webp`, which is how a `.webp`
    // holding PNG bytes reached the pictures folder during verification. This
    // browser encodes all three of the formats the signature offers, so the
    // case has to be provoked with one it does not; what is under test is the
    // refusal, not which formats a particular engine happens to support.
    await expect(toBlob(image, doc, 'image/tiff' as 'image/png')).rejects.toThrow(/image\/tiff/)
  })

  it('leaves nothing of an obscured region in the exported file', async () => {
    const image = noiseImage(64, 64)
    const region = { x: 16, y: 16, width: 32, height: 32 }
    const doc = documentOf(64, 64, [
      { id: 'password-field-redaction', kind: 'obscure', rect: region, mode: 'pixelate', intensity: 8 },
    ])

    const blob = await toBlob(image, doc, 'image/png')
    const decoded = await decode(blob)
    const source = readPixels(image)
    const bytes = new Uint8Array(await blob.arrayBuffer())

    // Not in the image data.
    expect(identicalFraction(decoded, source, region)).toBe(0)
    expect(neighbourDelta(decoded, region)).toBeLessThan(neighbourDelta(source, region) * 0.2)
    // Not in metadata: a canvas-encoded PNG carries pixels and nothing else.
    expect(pngChunkTypes(bytes)).not.toContain('tEXt')
    expect(pngChunkTypes(bytes)).not.toContain('iTXt')
    expect(pngChunkTypes(bytes)).not.toContain('zTXt')
    expect(pngChunkTypes(bytes)).not.toContain('eXIf')
    // Not in layer data: the document never reaches the file.
    expect(containsAscii(bytes, 'password-field-redaction')).toBe(false)
    expect(containsAscii(bytes, 'obscure')).toBe(false)
  })
})
