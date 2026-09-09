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
 * It is a property of the toolbar's contents, so it changes when the toolbar
 * does, and everything that depends on it reads this one number. It was 720
 * until the save format buttons were added beside Save, which is a hundred
 * points of second row that were not there before.
 *
 * Re-measured against the packaged editor by resizing the window a step at a
 * time: the third row appears between 730 and 735 points, where the width
 * slider stops fitting beside the palette. 750 rather than 735, because the
 * wrap threshold is a text measurement and a system font that renders `Width`
 * a few points wider would otherwise cost a row of the picture silently.
 */
export const TOOLBAR_MIN_WIDTH: number = metrics.toolbarMinWidth
