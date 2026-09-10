import { PROTOCOL_VERSION } from '@snapdeck/protocol'

/**
 * Service worker shell. The capture loop, the layer composite, the bridge
 * client and the download fallback arrive in tasks 8 and 9.
 *
 * What is already true here is the wiring those tasks depend on: the worker is
 * an ES module, so it can reach `@snapdeck/protocol` by a static import, and it
 * is the only part of the extension that does. A content script talks to this
 * worker over `chrome.runtime` and never to the bridge, which keeps the schema
 * out of the code injected into a page.
 */
export const BRIDGE_PROTOCOL_VERSION: number = PROTOCOL_VERSION
