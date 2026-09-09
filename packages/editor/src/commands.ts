/**
 * Edits and the history that makes them reversible.
 *
 * Every edit is a `Command`: it produces a new document, and it can name the
 * command that puts the document back. Undo is therefore an edit like any
 * other, not a saved copy of the whole document, which keeps the memory cost
 * of a long session flat no matter how large the capture is.
 *
 * `invert` is always given the document as it stood *before* `apply` ran. That
 * is where the information an inverse needs still exists: the layer a removal
 * is about to drop, the index it sits at, the crop that is about to be
 * replaced. `History` is what guarantees the ordering; a caller pairing the
 * two by hand has to do the same.
 */

import type { EditorDocument, Layer, Rect } from './model'

export type Command = { apply(doc: EditorDocument): EditorDocument; invert(doc: EditorDocument): Command }

/**
 * The inverse of an edit that found nothing to change.
 *
 * A command naming a layer id the document does not have is not an error worth
 * throwing over: the tool that built it may simply be one step behind the
 * document. Both halves stay consistent, so undo and redo keep working around
 * it instead of leaving a hole in the stack.
 */
const NOOP: Command = {
  apply: (doc) => doc,
  invert: () => NOOP,
}

export function addLayer(layer: Layer): Command {
  return {
    apply: (doc) => ({ ...doc, layers: [...doc.layers, layer] }),
    invert: () => removeLayer(layer.id),
  }
}

export function removeLayer(id: string): Command {
  return {
    apply: (doc) => ({ ...doc, layers: doc.layers.filter((layer) => layer.id !== id) }),
    invert: (doc) => {
      const index = doc.layers.findIndex((layer) => layer.id === id)
      const layer = doc.layers[index]
      // Restoring at `index` rather than appending. Layers paint back to
      // front, so an undo that appends would silently pull the layer to the
      // front of the drawing, over things it used to sit behind.
      return layer ? insertLayer(layer, index) : NOOP
    },
  }
}

export function updateLayer(id: string, next: Layer): Command {
  return {
    apply: (doc) => ({ ...doc, layers: doc.layers.map((layer) => (layer.id === id ? next : layer)) }),
    invert: (doc) => {
      // Read from the pre-edit document: the post-edit one holds `next`, and
      // an inverse built from that would undo to the state it started in.
      const previous = doc.layers.find((layer) => layer.id === id)
      return previous ? updateLayer(id, previous) : NOOP
    },
  }
}

export function setCrop(crop: Rect | null): Command {
  return {
    apply: (doc) => ({ ...doc, crop }),
    invert: (doc) => setCrop(doc.crop),
  }
}

/** Puts a layer back where it was. Internal: the inverse of `removeLayer`. */
function insertLayer(layer: Layer, index: number): Command {
  return {
    apply: (doc) => ({
      ...doc,
      layers: [...doc.layers.slice(0, index), layer, ...doc.layers.slice(index)],
    }),
    invert: () => removeLayer(layer.id),
  }
}

/** A command that has been applied, paired with the command that reverses it. */
type Entry = { command: Command; inverse: Command }

/**
 * The undo/redo stack, and the only owner of the current document.
 *
 * Inverses are built once, at `run`, against the document the command was
 * applied to. Undo restores exactly that state, so redo can replay the
 * original command against it and the pair stays valid however often the user
 * walks back and forth.
 */
export class History {
  #document: EditorDocument
  #done: Entry[] = []
  #undone: Entry[] = []

  constructor(initial: EditorDocument) {
    this.#document = initial
  }

  get document(): EditorDocument {
    return this.#document
  }

  get canUndo(): boolean {
    return this.#done.length > 0
  }

  get canRedo(): boolean {
    return this.#undone.length > 0
  }

  run(command: Command): void {
    const inverse = command.invert(this.#document)
    this.#document = command.apply(this.#document)
    this.#done.push({ command, inverse })
    // The redone future is no longer reachable from here: the user branched
    // away from it by editing, so keeping it would redo into a document those
    // commands were never applied to.
    this.#undone.length = 0
  }

  undo(): void {
    const entry = this.#done.pop()
    if (!entry) return
    this.#document = entry.inverse.apply(this.#document)
    this.#undone.push(entry)
  }

  redo(): void {
    const entry = this.#undone.pop()
    if (!entry) return
    this.#document = entry.command.apply(this.#document)
    this.#done.push(entry)
  }
}
