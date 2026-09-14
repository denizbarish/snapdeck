/**
 * The promotional tile's one moving part: the icon.
 *
 * Everything else on the tile is written in `promo.html`, because it is text
 * and it does not change. The icon is here because it is a file in
 * `apps/extension`, and it arrives as a data URL on the scene rather than as a
 * bundled asset so that the tile shows the very bytes the extension ships to
 * the store rather than a copy that could fall behind them.
 */

import { scene } from './scene'

const icon = document.getElementById('icon')
if (!(icon instanceof HTMLImageElement)) throw new Error('screenshots: #icon is missing')

icon.src = scene().icon
