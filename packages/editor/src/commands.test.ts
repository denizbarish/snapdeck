import { describe, expect, it } from 'vitest'
import { addLayer, History, removeLayer, setCrop, updateLayer } from './commands'
import { createDocument, type Layer } from './model'

const stroke = { color: '#ff0000', width: 4 }

function rectLayer(id: string, x: number): Layer {
  return {
    id,
    kind: 'rect',
    rect: { x, y: 0, width: 10, height: 10 },
    style: { stroke, fill: null },
  }
}

const ids = (doc: { layers: Layer[] }): string[] => doc.layers.map((layer) => layer.id)

describe('addLayer', () => {
  it('adds the layer, and its inverse takes it back out', () => {
    const doc = createDocument(100, 100)
    const layer = rectLayer('a', 0)
    const command = addLayer(layer)

    const added = command.apply(doc)
    expect(added.layers).toEqual([layer])

    const undone = command.invert(doc).apply(added)
    expect(undone.layers).toEqual([])
  })
})

describe('updateLayer', () => {
  // The inverse has to capture the layer as it was before the edit. Reading
  // the layer out of the post-edit document instead would make undo a no-op.
  it('restores the old layer, not the new one', () => {
    const before = rectLayer('a', 0)
    const after = rectLayer('a', 500)
    const doc = { ...createDocument(100, 100), layers: [before] }
    const command = updateLayer('a', after)

    const edited = command.apply(doc)
    expect(edited.layers).toEqual([after])

    const undone = command.invert(doc).apply(edited)
    expect(undone.layers).toEqual([before])
  })

  it('leaves a document without that layer alone', () => {
    const doc = { ...createDocument(100, 100), layers: [rectLayer('a', 0)] }
    const command = updateLayer('missing', rectLayer('missing', 9))
    const applied = command.apply(doc)
    expect(applied.layers).toEqual(doc.layers)
    expect(command.invert(doc).apply(applied).layers).toEqual(doc.layers)
  })
})

describe('removeLayer', () => {
  // Layer order is paint order, so an undo that appends instead of inserting
  // silently sends the restored layer to the front of the drawing.
  it('puts the layer back at its original index, not at the end', () => {
    const doc = { ...createDocument(100, 100), layers: [rectLayer('a', 0), rectLayer('b', 1), rectLayer('c', 2)] }
    const command = removeLayer('b')

    const removed = command.apply(doc)
    expect(ids(removed)).toEqual(['a', 'c'])

    const restored = command.invert(doc).apply(removed)
    expect(ids(restored)).toEqual(['a', 'b', 'c'])
  })

  it('leaves a document without that layer alone', () => {
    const doc = { ...createDocument(100, 100), layers: [rectLayer('a', 0)] }
    const command = removeLayer('missing')
    const applied = command.apply(doc)
    expect(applied.layers).toEqual(doc.layers)
    expect(command.invert(doc).apply(applied).layers).toEqual(doc.layers)
  })
})

describe('setCrop', () => {
  it('sets the crop, and its inverse restores the previous one', () => {
    const doc = createDocument(100, 100)
    const crop = { x: 10, y: 10, width: 50, height: 50 }
    const command = setCrop(crop)

    const cropped = command.apply(doc)
    expect(cropped.crop).toEqual(crop)

    expect(command.invert(doc).apply(cropped).crop).toBeNull()
  })
})

describe('History', () => {
  it('undoes three commands in reverse order and redoes them in order', () => {
    const history = new History(createDocument(100, 100))
    history.run(addLayer(rectLayer('a', 0)))
    history.run(addLayer(rectLayer('b', 1)))
    history.run(addLayer(rectLayer('c', 2)))
    expect(ids(history.document)).toEqual(['a', 'b', 'c'])

    history.undo()
    expect(ids(history.document)).toEqual(['a', 'b'])
    history.undo()
    expect(ids(history.document)).toEqual(['a'])
    history.undo()
    expect(ids(history.document)).toEqual([])
    expect(history.canUndo).toBe(false)

    history.redo()
    expect(ids(history.document)).toEqual(['a'])
    history.redo()
    expect(ids(history.document)).toEqual(['a', 'b'])
    history.redo()
    expect(ids(history.document)).toEqual(['a', 'b', 'c'])
    expect(history.canRedo).toBe(false)
  })

  it('drops the redo stack when a new command is run', () => {
    const history = new History(createDocument(100, 100))
    history.run(addLayer(rectLayer('a', 0)))
    history.run(addLayer(rectLayer('b', 1)))
    history.undo()
    history.undo()
    expect(history.canRedo).toBe(true)

    history.run(addLayer(rectLayer('c', 2)))
    expect(history.canRedo).toBe(false)

    history.redo()
    expect(ids(history.document)).toEqual(['c'])
  })

  // Rendering holds on to the document it drew, so a command that edited it in
  // place would change what has already been painted and defeat undo.
  it('leaves the previous document untouched', () => {
    const history = new History(createDocument(100, 100))
    const before = history.document

    history.run(addLayer(rectLayer('a', 0)))

    expect(before.layers).toEqual([])
    expect(history.document).not.toBe(before)
  })
})
