/**
 * The editor's chrome metrics: numbers about the editing surface that a host
 * has to know before the surface exists.
 *
 * There is exactly one of these today and it is here because it is needed in
 * three places that cannot see each other: the toolbar's own layout, the
 * desktop window's minimum width, and the Rust that builds that window. The
 * value itself lives in `chrome.json` rather than in this file, because
 * `apps/desktop/src-tauri/build.rs` reads it too and a build script can parse
 * JSON but not TypeScript.
 */

import metrics from '../chrome.json'

/**
 * The narrowest the toolbar can be laid out at without wrapping onto a third
 * row, in CSS pixels.
 *
 * Measured against the packaged editor: at this width the toolbar fills two
 * rows and its right-hand button group ends about thirteen points from the
 * edge. It is a property of the toolbar's contents, so it changes when the
 * toolbar does, and everything that depends on it reads this one number.
 */
export const TOOLBAR_MIN_WIDTH: number = metrics.toolbarMinWidth
