/**
 * A stand-in for the Tauri side of the app, so two windows written against it
 * can be rendered by a plain browser.
 *
 * `@tauri-apps/api` is a thin wrapper: every call in it ends at
 * `window.__TAURI_INTERNALS__`, which the webview injects. Defining that object
 * is therefore the whole of what a host has to provide, and it is a truer stand
 * in than mocking the modules would be, because the components keep calling the
 * real `invoke` and the real `getCurrentWindow`.
 *
 * What it answers with is the narrow part. Only the commands these two windows
 * issue on the way to their first paint are answered; anything else resolves to
 * `undefined`, which is what a command whose result nothing reads already looks
 * like. A command that would change something, a capture or a save, is never
 * reached, because the harness never presses those controls.
 *
 * None of this ships. It is imported by `tools/screenshots` and by nothing in
 * `apps/desktop`.
 */

/** The commands these pages issue, and what Rust would have answered. */
export type Answers = Record<string, unknown>

type Internals = {
  metadata: { currentWindow: { label: string }; currentWebview: { label: string; windowLabel: string } }
  invoke(command: string, args?: unknown): Promise<unknown>
  transformCallback(callback: (payload: unknown) => void, once?: boolean): number
  unregisterCallback(id: number): void
  convertFileSrc(path: string, protocol?: string): string
}

const LABEL = 'screenshots'

/**
 * Installs the stand-in.
 *
 * `assetUrl` is what `convertFileSrc` answers with, whatever path it is given:
 * the overlay asks for the frozen frame by an absolute path it was handed in
 * its URL, and here there is one picture and no file system to find it on.
 */
export function installTauriStub(answers: Answers, assetUrl: string): void {
  let nextCallback = 1
  const callbacks = new Map<number, (payload: unknown) => void>()

  const internals: Internals = {
    metadata: {
      currentWindow: { label: LABEL },
      currentWebview: { label: LABEL, windowLabel: LABEL },
    },
    invoke(command) {
      return Promise.resolve(Object.hasOwn(answers, command) ? answers[command] : undefined)
    },
    transformCallback(callback) {
      const id = nextCallback
      nextCallback += 1
      callbacks.set(id, callback)
      return id
    },
    unregisterCallback(id) {
      callbacks.delete(id)
    },
    convertFileSrc() {
      return assetUrl
    },
  }

  // `unknown` first: `Window` has no `__TAURI_INTERNALS__`, and the alternative
  // to widening here is declaring a global that would then exist for every file
  // in this package, including the two that must not reach for it.
  ;(window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__ = internals
}
