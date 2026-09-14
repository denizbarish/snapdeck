/**
 * The overlay page of the harness: the shipping `<Overlay/>`, over the demo
 * render standing in for a frozen screen.
 *
 * Unlike the editor this component does know about Tauri, so `installTauriStub`
 * goes in first and the page is a browser answering the handful of calls the
 * overlay makes before its first paint: `convertFileSrc` for the frozen frame,
 * `show` and `setFocus` on its own window. Region mode is the one driven here,
 * and region mode issues no command at all until `Enter` confirms a selection,
 * which the harness never presses.
 */

import { Overlay } from '@snapdeck/desktop/src/overlay/Overlay'
import { createRoot } from 'react-dom/client'
import { scene } from './scene'
import { installTauriStub } from './tauri'

const root = document.getElementById('overlay-root')
if (!root) throw new Error('screenshots: #overlay-root is missing')

installTauriStub({}, scene().image)

createRoot(root).render(
  // No `StrictMode`: the overlay's magnifier decodes the frame in an effect,
  // and a double mount would run that decode twice for a picture nobody is
  // waiting on twice.
  <Overlay
    displayId={1}
    mode="region"
    // The still capture, which is what these pictures are of. It is also what
    // any other value would give, but naming it keeps the readout's wording
    // something this file chose rather than something it fell into.
    action="capture"
    // One device pixel per point, because the frozen frame here is the demo
    // render at its own size. The size readout is in device pixels, so any
    // other value would print a number this picture does not contain.
    scale={1}
    // Never opened: `convertFileSrc` is stubbed and answers with the scene
    // whatever it is asked for. It is the path Rust would have passed.
    framePath="/tmp/snapdeck/frozen-1.png"
  />,
)
