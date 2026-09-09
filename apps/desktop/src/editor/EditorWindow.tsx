/**
 * The desktop host for `@snapdeck/editor`.
 *
 * Everything Tauri-shaped lives here, and nothing Tauri-shaped lives in the
 * editor package: the component is handed a decoded image and three callbacks,
 * and this file is the only place that knows the picture came from a file, that
 * saving means a command, or that closing means a window. That is what keeps
 * the planned browser extension on the same editor.
 *
 * Two decisions worth stating.
 *
 * The picture is read by the webview over the asset protocol rather than sent
 * over IPC. A full-resolution capture is tens of megabytes of raw pixels, and
 * the IPC channel serialises to JSON; the file is already on disk, so the
 * cheapest thing to send is its path.
 *
 * The finished picture goes back the other way as encoded bytes, which does
 * cross the IPC boundary. That is an encoded image rather than a bitmap,
 * roughly a thirtieth of the size, and it is what the file needs anyway.
 */

import { Editor, type ExportType } from '@snapdeck/editor'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useState } from 'react'
import type { JSX } from 'react'

export type EditorWindowProps = {
  /** Absolute path of the capture, as Rust wrote it. */
  path: string
  /** The capture's size in pixels, so the document exists before the decode does. */
  width: number
  height: number
}

/**
 * The file extension each format the editor can encode is saved under.
 *
 * The editor hands the type it encoded to `onExport`, so this is the whole of
 * the format decision on this side: the extension picks the path, and the path
 * is what decides whether the capture is updated or a second file appears
 * beside it. Rust refuses anything not on its own list.
 *
 * No `image/webp`. WKWebView does not encode it, and the editor no longer
 * offers it; an entry here would only describe a file that cannot be made.
 */
const EXTENSIONS: Record<ExportType, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
}

/**
 * Where a picture of this type is saved.
 *
 * The same path for the format the capture is already in, which is what makes
 * Save update the file the user has rather than leave a copy per edit. A
 * different format only changes the extension, so the new file lands in the
 * same directory, under the same name, beside the original.
 *
 * An unknown type keeps the original path rather than inventing an extension:
 * it cannot happen from this editor, and writing `.undefined` would be worse
 * than writing the format the user asked to replace.
 */
export function savePathFor(path: string, type: ExportType): string {
  const extension = EXTENSIONS[type]
  if (!extension) return path
  // Only a trailing extension, and only in the last path segment: a directory
  // called `Screenshots.2026` above a file with no extension of its own must
  // not be the thing that gets rewritten.
  return path.replace(/(\.[^./]*)?$/, `.${extension}`)
}

/**
 * How long the page waits for the capture to decode before it says so, in ms.
 *
 * An image that neither loads nor fails fires neither handler, and the page
 * would then say "Opening the capture…" for as long as the window is open. Long
 * enough that a slow disk is never mistaken for a broken load; short enough
 * that the user is told rather than left watching.
 */
const LOAD_DEADLINE_MS = 10_000

export function EditorWindow({ path, width, height }: EditorWindowProps): JSX.Element {
  const [image, setImage] = useState<HTMLImageElement | null>(null)
  const [failure, setFailure] = useState<string | null>(null)

  // Snapdeck is a menu bar agent and is never the active application, so
  // without this the editor opens behind whatever the user was looking at and
  // the capture they just took appears to have done nothing. It is asked for
  // here rather than by the window builder because AppKit ignores a focus
  // request made before the window is composited: the builder's own `focused`
  // flag was measured leaving the window at 495,373 with `onscreen = false`.
  // Once, on mount: a window that steals focus back every render would be
  // impossible to alt-tab away from.
  useEffect(() => {
    getCurrentWindow()
      .setFocus()
      .catch((error: unknown) => {
        // Not fatal, and not silent either. The window is on screen and in the
        // window list; it is simply behind, which the user can fix and this
        // line explains.
        console.error('editor: the window could not take focus', error)
      })
  }, [])

  // `crossOrigin` is the load-bearing line, and the overlay measured it rather
  // than assuming it. The capture is served from `asset://localhost`, a
  // different origin from this page, so a plain load taints the canvas and the
  // `getImageData` behind every redaction throws `SecurityError`. Tauri's asset
  // protocol already answers with `Access-Control-Allow-Origin` for this
  // window's own origin; this is what makes the webview perform a CORS-mode
  // fetch so that header is honoured. It relaxes nothing: without it the header
  // is simply ignored.
  useEffect(() => {
    const decoded = new Image()
    decoded.crossOrigin = 'anonymous'
    // An image load cannot be cancelled, so the cleanup revokes the handlers
    // and this flag makes that stick. `StrictMode` mounts the effect twice in
    // development, and without it the first load would still be in flight when
    // the second starts.
    let abandoned = false
    decoded.onload = () => {
      if (!abandoned) setImage(decoded)
    }
    decoded.onerror = () => {
      if (abandoned) return
      // The path is in the message because the one way this fails in practice
      // is a file outside the asset protocol's scope, and the path is what says
      // which file that was.
      setFailure(`Snapdeck could not open ${path}. The capture is still on disk and on the clipboard.`)
    }
    // The load is not abandoned here, only reported: nothing can cancel an
    // image load, so a decode that finally arrives should still replace the
    // message rather than be thrown away on top of it.
    const deadline = window.setTimeout(() => {
      if (!abandoned) {
        setFailure(
          `Snapdeck is still waiting for ${path}. The capture is on disk and on the clipboard.`,
        )
      }
    }, LOAD_DEADLINE_MS)
    decoded.src = convertFileSrc(path)
    return () => {
      abandoned = true
      window.clearTimeout(deadline)
      decoded.onload = null
      decoded.onerror = null
    }
  }, [path])

  if (!image) {
    return (
      <div style={statusStyle} role="status">
        {failure ?? 'Opening the capture…'}
      </div>
    )
  }

  return (
    <Editor
      image={image}
      width={width}
      height={height}
      // Rejections are deliberately left to propagate. The editor's own
      // delivery wrapper catches them and shows the user that the picture was
      // not saved, which is the surface closest to the button they pressed and
      // the only one that stops a failed save from looking like a save.
      // The name of the file that was written goes back to the editor, which
      // puts it in its status bar. It is the only thing that tells a Save that
      // replaced the capture apart from a Save that left a JPEG beside it, and
      // this side is the one that knows: Rust answers with the path it used,
      // and only the last segment is worth showing, because the directory is
      // the same one every time and would push the name off the line.
      onExport={async (blob, type) => {
        const written = await invoke<string>('save_edited', {
          path: savePathFor(path, type),
          bytes: new Uint8Array(await blob.arrayBuffer()),
        })
        return written.slice(written.lastIndexOf('/') + 1)
      }}
      onCopy={async (blob) => {
        await invoke('copy_edited', { bytes: new Uint8Array(await blob.arrayBuffer()) })
      }}
      onClose={() => {
        invoke('close_editor').catch((error: unknown) => {
          // The window is the only way out of the editor, so a close that does
          // not close has to be visible somewhere. There is no notification
          // surface here and the title bar's own button still works.
          console.error('editor: could not close the window', error)
        })
      }}
    />
  )
}

const statusStyle = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  height: '100%',
  padding: '0 24px',
  textAlign: 'center',
  background: '#1c1c1e',
  color: '#f5f5f7',
  font: '13px system-ui, -apple-system, "Helvetica Neue", sans-serif',
} as const
