/**
 * The one place a document becomes pixels.
 *
 * Preview and export both call `renderDocument`, so what the user approves on
 * screen and what leaves the machine cannot drift apart: there is no second
 * drawing path to fall behind. The function knows nothing about screen scale or
 * device pixel ratio. The caller sets a transform, the renderer draws in
 * source-image coordinates into it, and the same code therefore serves a
 * half-size preview and a full-size export without a branch between them.
 *
 * Host-free like the rest of the package: the canvas 2D API and nothing else,
 * so the browser extension reuses this file unchanged.
 *
 * The `obscure` layer is the reason this file matters. It does not paint a
 * translucent veil over a secret, it reads the pixels that are there and writes
 * different ones back. After that the original values are not in the canvas, so
 * they cannot be in the file the canvas is encoded to.
 */

import type { EditorDocument, Layer, Rect } from './model'

export type RenderTarget = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D

/**
 * One member of the `Layer` union, picked by kind.
 *
 * `rect` and `ellipse` share a member of the union, so they are asked for
 * together as `LayerOf<'rect' | 'ellipse'>`; asking for either alone matches no
 * member and quietly resolves to `never`.
 */
type LayerOf<K extends Layer['kind']> = Extract<Layer, { kind: K }>

/** The font a step badge's number is set in. `BadgeStyle` carries no family. */
const BADGE_FONT_STACK = 'system-ui, -apple-system, "Helvetica Neue", sans-serif'

/** Used when a layer's own font string turns out not to be one. */
const FALLBACK_FONT_STACK = 'sans-serif'

/** Used when a layer's font size is not a usable number either. */
const FALLBACK_FONT_SIZE = 16

/** How many box-blur passes approximate a Gaussian. Three is the usual answer. */
const BLUR_PASSES = 3

/**
 * The privacy floors, in source-image pixels.
 *
 * Both parts of each pair matter. The absolute floor is what protects a small
 * box; the divisor is what protects a large one, because a box drawn tightly
 * round a line of text holds glyphs about as tall as the box itself, so a
 * strength that erases 16px text is a rounding error on 48px text.
 *
 * Measured over text-like strokes (stem width 12% of the region's height, glyph
 * height 72% of it, irregular spacing) at region sizes from 12x12 to 400x120,
 * reporting peak-to-trough stroke contrast and the correlation between the
 * result and the source, where 255 and 1.0 are "untouched":
 *
 * | rule | worst contrast | worst correlation | worst identical bytes |
 * |---|---|---|---|
 * | mosaic, block 2 (the old floor) | 255 | 0.97 | 0.95 |
 * | mosaic, block >= max(6, min/8) | 255 | 0.80 | 0.61 |
 * | mosaic, block >= max(6, min/4) | 182 | 0.42 | 0.05 |
 * | blur, radius 1 (the old floor) | 255 | 0.97 | 0.71 |
 * | blur, radius >= max(4, min/16) | 208 | 0.73 | 0.00 |
 * | blur, radius >= max(4, min/6) | 122 | 0.46 | 0.00 |
 *
 * A quarter of the smaller side puts four mosaic blocks across the text; a
 * sixth of it gives the blur a window spanning about a third of it. Neither
 * correlation reaches zero and neither can: both modes preserve local ink
 * density, so "there was writing here" survives by construction. That is the
 * honest limit of any mode that averages, and it is why `blackout` is the
 * default of the obscure tool.
 */
const MIN_PIXELATE_BLOCK = 6
const PIXELATE_REGION_DIVISOR = 4
const MIN_BLUR_RADIUS = 4
const BLUR_REGION_DIVISOR = 6

/**
 * Draw a document into a context.
 *
 * The context's own transform is left as it was found. Inside, the origin is
 * moved to the crop, which is what makes a crop cost one `translate` rather
 * than an offset threaded through every layer: layers keep their source-image
 * coordinates, and anything outside the crop falls off the canvas. That last
 * part is a requirement on the caller, not a property of this function: the
 * target has to be the size of the view (`viewOf(doc)`), because the only thing
 * discarding the cropped-away pixels is the edge of the canvas. Draw a cropped
 * document into a larger target and the region outside the crop is drawn, not
 * dropped.
 *
 * The target is NOT cleared: the document's own image covers it, and a caller
 * that wants the canvas emptied first (a preview surface drawing a document
 * smaller than itself, say) owns that decision along with the canvas.
 *
 * `globalAlpha`, `globalCompositeOperation` and `filter` are reset before
 * anything is drawn, and restored after. A preview canvas is reused frame after
 * frame and an export canvas is fresh, so any of those left set by a caller
 * would be a way for the two to disagree, which is the one thing this module
 * rules out.
 */
export function renderDocument(ctx: RenderTarget, image: CanvasImageSource, doc: EditorDocument): void {
  const crop = viewOf(doc)
  ctx.save()
  try {
    ctx.globalAlpha = 1
    ctx.globalCompositeOperation = 'source-over'
    ctx.filter = 'none'
    ctx.translate(-crop.x, -crop.y)
    // Sized explicitly rather than drawn at its intrinsic size: the document's
    // dimensions are the coordinate space every layer was placed in, and an
    // image that does not match them must be made to, not allowed to shift the
    // annotations off the things they point at.
    ctx.drawImage(image, 0, 0, doc.width, doc.height)
    for (const layer of doc.layers) drawLayer(ctx, layer)
  } finally {
    ctx.restore()
  }
}

/**
 * The region of the source the document shows: its crop, or all of it.
 *
 * Rounded to whole pixels here and nowhere else. A crop is dragged with a
 * pointer, so its origin is as likely to be 5.5 as 5, and an export canvas has
 * to have an integer size. Rounding the size at the encoder and translating by
 * the raw origin at the renderer would put every source pixel half a pixel off
 * its own and resample the entire screenshot; rounding once, here, is what
 * makes "one source pixel is one output pixel" true rather than intended.
 *
 * Edges are rounded independently of the origin, so a crop keeps the pixels it
 * covers rather than its width. Sides are floored at one: a drag that produced
 * a sliver should still yield a file rather than an exception.
 */
export function viewOf(doc: EditorDocument): Rect {
  const view = doc.crop ?? { x: 0, y: 0, width: doc.width, height: doc.height }
  const x = Math.round(view.x)
  const y = Math.round(view.y)
  return {
    x,
    y,
    width: Math.max(1, Math.round(view.x + view.width) - x),
    height: Math.max(1, Math.round(view.y + view.height) - y),
  }
}

function drawLayer(ctx: RenderTarget, layer: Layer): void {
  switch (layer.kind) {
    case 'arrow':
      return drawArrow(ctx, layer)
    case 'line':
      return drawLine(ctx, layer)
    case 'rect':
      return drawRect(ctx, layer)
    case 'ellipse':
      return drawEllipse(ctx, layer)
    case 'text':
      return drawText(ctx, layer)
    case 'highlight':
      return drawHighlight(ctx, layer)
    case 'obscure':
      return drawObscure(ctx, layer)
    case 'step':
      return drawStep(ctx, layer)
  }
}

function drawArrow(ctx: RenderTarget, layer: LayerOf<'arrow'>): void {
  const { from, to, style } = layer
  const length = Math.hypot(to.x - from.x, to.y - from.y)
  // A zero-length arrow has no direction, so it has no head to point with and
  // nothing to draw. It happens on a click that was meant to be a drag.
  if (length === 0 || style.width <= 0) return

  const angle = Math.atan2(to.y - from.y, to.x - from.x)
  // The head is proportional to the stroke, so a thick arrow does not end in a
  // pinprick, but never longer than the arrow itself, so a short one is not all
  // head and no shaft.
  const head = Math.min(style.width * 4, length)
  const base = { x: to.x - Math.cos(angle) * head, y: to.y - Math.sin(angle) * head }
  const spread = head * 0.45
  const acrossX = -Math.sin(angle) * spread
  const acrossY = Math.cos(angle) * spread

  ctx.save()
  ctx.strokeStyle = style.color
  ctx.fillStyle = style.color
  ctx.lineWidth = style.width
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  // The shaft stops where the head starts. Running it to the tip would show
  // through a head drawn in a colour with any transparency to it.
  ctx.beginPath()
  ctx.moveTo(from.x, from.y)
  ctx.lineTo(base.x, base.y)
  ctx.stroke()
  ctx.beginPath()
  ctx.moveTo(to.x, to.y)
  ctx.lineTo(base.x + acrossX, base.y + acrossY)
  ctx.lineTo(base.x - acrossX, base.y - acrossY)
  ctx.closePath()
  ctx.fill()
  ctx.restore()
}

function drawLine(ctx: RenderTarget, layer: LayerOf<'line'>): void {
  const first = layer.points[0]
  if (!first || layer.style.width <= 0) return

  ctx.save()
  ctx.strokeStyle = layer.style.color
  ctx.fillStyle = layer.style.color
  ctx.lineWidth = layer.style.width
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  if (layer.points.length === 1) {
    // A stroke sampled once is a dot. Stroking a path of one point draws
    // nothing at all, which would make a tap with the pen vanish.
    ctx.beginPath()
    ctx.arc(first.x, first.y, layer.style.width / 2, 0, Math.PI * 2)
    ctx.fill()
  } else {
    ctx.beginPath()
    ctx.moveTo(first.x, first.y)
    for (const point of layer.points.slice(1)) ctx.lineTo(point.x, point.y)
    ctx.stroke()
  }
  ctx.restore()
}

function drawRect(ctx: RenderTarget, layer: LayerOf<'rect' | 'ellipse'>): void {
  const { rect, style } = layer
  ctx.save()
  if (style.fill !== null) {
    ctx.fillStyle = style.fill
    ctx.fillRect(rect.x, rect.y, rect.width, rect.height)
  }
  if (style.stroke.width > 0) {
    ctx.strokeStyle = style.stroke.color
    ctx.lineWidth = style.stroke.width
    ctx.strokeRect(rect.x, rect.y, rect.width, rect.height)
  }
  ctx.restore()
}

function drawEllipse(ctx: RenderTarget, layer: LayerOf<'rect' | 'ellipse'>): void {
  const { rect, style } = layer
  // Inscribed in the rect, matching how `hit.ts` decides what the pointer is
  // over. The radii are taken as positive because a rect can arrive flipped.
  const radiusX = Math.abs(rect.width) / 2
  const radiusY = Math.abs(rect.height) / 2
  if (radiusX === 0 || radiusY === 0) return

  ctx.save()
  ctx.beginPath()
  ctx.ellipse(rect.x + rect.width / 2, rect.y + rect.height / 2, radiusX, radiusY, 0, 0, Math.PI * 2)
  if (style.fill !== null) {
    ctx.fillStyle = style.fill
    ctx.fill()
  }
  if (style.stroke.width > 0) {
    ctx.strokeStyle = style.stroke.color
    ctx.lineWidth = style.stroke.width
    ctx.stroke()
  }
  ctx.restore()
}

/**
 * Set a font, falling back to one that is certainly valid.
 *
 * Assigning an invalid font string to a context is a silent no-op: the context
 * keeps whatever font it had, which is inherited state, so the same document
 * would set its text in one face on a reused preview canvas and another in a
 * fresh export one. Assigning the fallback first makes the failure deterministic
 * instead: the size the layer asked for, in a family that always parses.
 */
function setFont(ctx: RenderTarget, size: number, family: string, weight = ''): void {
  const prefix = weight === '' ? '' : `${weight} `
  const points = Number.isFinite(size) && size > 0 ? size : FALLBACK_FONT_SIZE
  ctx.font = `${prefix}${points}px ${FALLBACK_FONT_STACK}`
  ctx.font = `${prefix}${points}px ${family}`
}

function drawText(ctx: RenderTarget, layer: LayerOf<'text'>): void {
  ctx.save()
  ctx.fillStyle = layer.style.color
  setFont(ctx, layer.style.size, layer.style.family)
  // Top-left, so the text starts at the corner of the box the tool sized for
  // it. Lines are split but not wrapped: the box follows the text, not the
  // other way round, so wrapping here would fight the tool that sized it.
  ctx.textAlign = 'left'
  ctx.textBaseline = 'top'
  const lineHeight = layer.style.size * 1.25
  const lines = layer.content.split('\n')
  for (let index = 0; index < lines.length; index += 1) {
    ctx.fillText(lines[index] ?? '', layer.rect.x, layer.rect.y + index * lineHeight)
  }
  ctx.restore()
}

function drawHighlight(ctx: RenderTarget, layer: LayerOf<'highlight'>): void {
  ctx.save()
  // Multiply, not a translucent fill. A highlighter darkens what is under it
  // and leaves black text black; an alpha fill washes the text out towards the
  // highlight colour and costs the legibility the user was pointing at.
  ctx.globalCompositeOperation = 'multiply'
  ctx.fillStyle = layer.color
  ctx.fillRect(layer.rect.x, layer.rect.y, layer.rect.width, layer.rect.height)
  ctx.restore()
}

function drawStep(ctx: RenderTarget, layer: LayerOf<'step'>): void {
  // `size` is the badge diameter and `center` is its centre, which is the
  // reading `model.boundsOf` and `hit.ts` already committed to: both lay a
  // square of `size` around `center` and hit test within `size / 2` of it.
  const radius = layer.style.size / 2
  if (radius <= 0) return

  ctx.save()
  ctx.beginPath()
  ctx.arc(layer.center.x, layer.center.y, radius, 0, Math.PI * 2)
  ctx.fillStyle = layer.style.fill
  ctx.fill()
  ctx.fillStyle = layer.style.color
  setFont(ctx, Math.round(layer.style.size * 0.6), BADGE_FONT_STACK, '600')
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  ctx.fillText(String(layer.index), layer.center.x, layer.center.y)
  ctx.restore()
}

/**
 * Replace a region with pixels that no longer contain what was there.
 *
 * The region is read back from the canvas, not from the source image, so an
 * obscure covers whatever is beneath it in paint order: a redaction placed over
 * a highlight or an earlier annotation hides that too.
 *
 * All three modes go through `getImageData` and `putImageData` rather than
 * through a fill or a filtered `drawImage`. That buys three things. The write
 * is a raw pixel write, so no blend mode, no `globalAlpha` and no antialiased
 * edge can leave a trace of the original showing through. The region is an
 * integer rectangle of device pixels, so "every pixel of it" is a statement
 * that can be checked. And the transform is honoured explicitly, so the same
 * call redacts the same pixels at any zoom.
 *
 * A tainted canvas makes `getImageData` throw, and that is the behaviour to
 * want: refusing to export is the safe failure, silently shipping an
 * unredacted screenshot is not.
 */
function drawObscure(ctx: RenderTarget, layer: LayerOf<'obscure'>): void {
  const region = deviceRect(ctx, layer.rect)
  if (!region) return

  const pixels = ctx.getImageData(region.x, region.y, region.width, region.height)
  switch (layer.mode) {
    case 'blackout':
      flatten(pixels)
      break
    case 'pixelate':
      pixelate(pixels, deviceStrength(obscureStrength('pixelate', layer.rect, layer.intensity), ctx, region))
      break
    case 'blur':
      blur(pixels, deviceStrength(obscureStrength('blur', layer.rect, layer.intensity), ctx, region))
      break
  }
  ctx.putImageData(pixels, region.x, region.y)
}

/**
 * How strong an obscure has to be, in SOURCE-image pixels.
 *
 * Resolved in source pixels and only then mapped to the device, because the
 * question "is this covered?" is one the user answers on screen and the file
 * has to keep. Applying a floor after the scale, as this used to, makes the
 * strength depend on the zoom: at zoom-to-fit a 2px mosaic became an 8px one in
 * the preview and stayed 2px in the file, so the user approved a redaction
 * stronger than the one shipped.
 *
 * The result is the strongest of three: what the tool asked for, what the size
 * of the region demands, and an absolute floor. The tool's number is a request
 * that may be raised and never lowered, so this is a backstop rather than the
 * contract. See the constants above for the measurements the two minimums come
 * from.
 *
 * Not part of the package's public surface (`index.ts` does not re-export it);
 * exported so the floors can be pinned in a test that does not need a canvas.
 */
export function obscureStrength(mode: 'blur' | 'pixelate', rect: Rect, intensity: number): number {
  // A document is data, and data arrives broken: `NaN` used to end the mosaic's
  // loop before its first step, leaving every source pixel in place with
  // nothing thrown, and `Infinity` used to hang the blur mid-drag.
  const requested = Number.isFinite(intensity) ? Math.round(intensity) : 0
  const span = Math.min(Math.abs(rect.width), Math.abs(rect.height))
  const divisor = mode === 'pixelate' ? PIXELATE_REGION_DIVISOR : BLUR_REGION_DIVISOR
  const proportional = Number.isFinite(span) ? Math.ceil(span / divisor) : 0
  const floor = mode === 'pixelate' ? MIN_PIXELATE_BLOCK : MIN_BLUR_RADIUS
  return Math.max(requested, proportional, floor)
}

/**
 * A source-pixel strength in the device pixels the region is made of.
 *
 * Rounded down, never up: the preview may be gentler than the file, because a
 * user who ships something stronger than what they approved has lost nothing.
 * The reverse is the failure.
 *
 * Clamped to the region's span as well, which is the only bound either mode
 * gets. Past the span a blur's window already covers every sample on the line
 * and a mosaic's block already covers the region, so the clamp costs nothing
 * and it keeps an absurd intensity from walking a loop for minutes.
 */
function deviceStrength(source: number, ctx: RenderTarget, region: Rect): number {
  const scaled = Math.floor(source * deviceScale(ctx))
  return Math.min(Math.max(1, scaled), Math.max(region.width, region.height))
}

/**
 * The layer's rect in the canvas's own pixels, clipped to the canvas.
 *
 * `getImageData` and `putImageData` ignore the transform and the clip; they
 * address raw device pixels. So the transform has to be applied by hand here.
 * All four corners are mapped rather than just two, because that stays correct
 * if a caller ever rotates or flips the view.
 *
 * The bounds are rounded outwards. A redaction that covers one pixel more than
 * asked is a cosmetic flaw; one that covers a pixel less leaves a sliver of the
 * secret along its edge.
 */
function deviceRect(ctx: RenderTarget, rect: Rect): Rect | null {
  const matrix = ctx.getTransform()
  const xs: number[] = []
  const ys: number[] = []
  for (const corner of corners(rect)) {
    xs.push(matrix.a * corner.x + matrix.c * corner.y + matrix.e)
    ys.push(matrix.b * corner.x + matrix.d * corner.y + matrix.f)
  }
  const left = Math.max(0, Math.floor(Math.min(...xs)))
  const top = Math.max(0, Math.floor(Math.min(...ys)))
  const right = Math.min(ctx.canvas.width, Math.ceil(Math.max(...xs)))
  const bottom = Math.min(ctx.canvas.height, Math.ceil(Math.max(...ys)))
  if (right <= left || bottom <= top) return null
  return { x: left, y: top, width: right - left, height: bottom - top }
}

function corners(rect: Rect): { x: number; y: number }[] {
  return [
    { x: rect.x, y: rect.y },
    { x: rect.x + rect.width, y: rect.y },
    { x: rect.x + rect.width, y: rect.y + rect.height },
    { x: rect.x, y: rect.y + rect.height },
  ]
}

/**
 * How many device pixels one source pixel currently covers.
 *
 * The square root of the transform's determinant: the area scale reduced to a
 * length, which is the right answer for a uniform zoom and a sane one for a
 * transform that is not.
 *
 * A degenerate transform yields 1 rather than zero, infinity or `NaN`, so a
 * strength derived from this is always a usable number.
 */
function deviceScale(ctx: RenderTarget): number {
  const matrix = ctx.getTransform()
  const scale = Math.sqrt(Math.abs(matrix.a * matrix.d - matrix.b * matrix.c))
  return Number.isFinite(scale) && scale > 0 ? scale : 1
}

/** Every pixel to opaque black. Nothing is averaged, so nothing is left. */
function flatten(pixels: ImageData): void {
  for (let index = 0; index < pixels.data.length; index += 4) {
    pixels.data[index] = 0
    pixels.data[index + 1] = 0
    pixels.data[index + 2] = 0
    pixels.data[index + 3] = 255
  }
}

/**
 * Mosaic: every pixel of a block becomes the block's mean.
 *
 * Blocks are laid out from the region's own corner, so the mosaic lines up with
 * the redaction rather than with some grid the user cannot see. Alpha is
 * averaged along with the colours; leaving it alone would keep the shape of
 * whatever was there legible in the alpha channel of a PNG.
 */
function pixelate(pixels: ImageData, block: number): void {
  const { width, height, data } = pixels
  for (let top = 0; top < height; top += block) {
    for (let left = 0; left < width; left += block) {
      const right = Math.min(left + block, width)
      const bottom = Math.min(top + block, height)
      let red = 0
      let green = 0
      let blue = 0
      let alpha = 0
      let count = 0
      for (let y = top; y < bottom; y += 1) {
        for (let x = left; x < right; x += 1) {
          const index = (y * width + x) * 4
          red += data[index] ?? 0
          green += data[index + 1] ?? 0
          blue += data[index + 2] ?? 0
          alpha += data[index + 3] ?? 0
          count += 1
        }
      }
      if (count === 0) continue
      const meanRed = Math.round(red / count)
      const meanGreen = Math.round(green / count)
      const meanBlue = Math.round(blue / count)
      const meanAlpha = Math.round(alpha / count)
      for (let y = top; y < bottom; y += 1) {
        for (let x = left; x < right; x += 1) {
          const index = (y * width + x) * 4
          data[index] = meanRed
          data[index + 1] = meanGreen
          data[index + 2] = meanBlue
          data[index + 3] = meanAlpha
        }
      }
    }
  }
}

/**
 * Blur, as three box passes on each axis.
 *
 * Separable and running-sum, so the cost is a handful of operations per pixel
 * whatever the radius, which matters because this runs on every frame of a drag
 * over a retina-sized capture. Three passes rather than one: a single box blur
 * is a crude low-pass filter that a determined attacker can partly undo, while
 * three approximate a Gaussian closely enough that what is left after 8-bit
 * rounding is not the original.
 *
 * Sampling stops at the edges of the region and clamps, so a blur reads and
 * writes nothing outside the rectangle the user drew.
 *
 * The radius must already be bounded by the region's span, which is what
 * `deviceStrength` does: past that the clamped window covers every sample on
 * the line anyway, so a larger radius buys nothing, and an unbounded one walks
 * a loop that never advances.
 */
function blur(pixels: ImageData, radius: number): void {
  if (!(radius >= 1)) return
  // Float, not another `Uint8ClampedArray`: rounding to bytes between six
  // passes accumulates a visible banding that a redaction does not need.
  const primary = Float32Array.from(pixels.data)
  const scratch = new Float32Array(primary.length)
  for (let pass = 0; pass < BLUR_PASSES; pass += 1) {
    blurAxis(primary, scratch, pixels.width, pixels.height, radius, true)
    blurAxis(scratch, primary, pixels.width, pixels.height, radius, false)
  }
  pixels.data.set(primary)
}

/** One box-blur pass along one axis, from `source` into `target`. */
function blurAxis(
  source: Float32Array,
  target: Float32Array,
  width: number,
  height: number,
  radius: number,
  horizontal: boolean,
): void {
  const lines = horizontal ? height : width
  const span = horizontal ? width : height
  // The stride from one sample to the next along the axis being blurred, and
  // from one line to the next across it.
  const step = (horizontal ? 1 : width) * 4
  const lineStep = (horizontal ? width : 1) * 4
  const window = radius * 2 + 1

  for (let line = 0; line < lines; line += 1) {
    const base = line * lineStep
    for (let channel = 0; channel < 4; channel += 1) {
      let sum = 0
      for (let offset = -radius; offset <= radius; offset += 1) {
        sum += source[base + clamp(offset, span) * step + channel] ?? 0
      }
      for (let position = 0; position < span; position += 1) {
        target[base + position * step + channel] = sum / window
        // Slide the window: the sample entering on the right minus the one
        // leaving on the left, both clamped to the edge of the region.
        sum += source[base + clamp(position + radius + 1, span) * step + channel] ?? 0
        sum -= source[base + clamp(position - radius, span) * step + channel] ?? 0
      }
    }
  }
}

function clamp(value: number, span: number): number {
  return Math.min(Math.max(value, 0), span - 1)
}
