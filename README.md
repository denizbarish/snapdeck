# Snapdeck

Snapdeck is an open-source macOS menu bar app for capturing, annotating, and sharing screenshots.

## Requirements

- macOS 14 or later
- Rust stable toolchain
- Node.js 24
- pnpm 11

## Getting started

```bash
pnpm install
pnpm tauri dev
```

Snapdeck is a menu bar app, so it has no Dock icon and opens no window on launch.

macOS asks for Screen Recording permission the first time you take a capture. Grant it in
System Settings > Privacy & Security > Screen & System Audio Recording, then **quit Snapdeck and
open it again**. Relaunching is not optional: macOS decides what a process may capture when the
process starts, so a running Snapdeck cannot see a permission granted after launch and every
capture keeps failing until it is restarted. This is the same reason macOS offers "Quit & Reopen"
on its own prompt.

## Usage

Snapdeck lives in the menu bar. Take a capture from the menu, or with a shortcut:

| Shortcut | Capture |
| --- | --- |
| `Cmd+Shift+7` | Region |
| `Cmd+Shift+8` | Window |
| `Cmd+Shift+9` | Full screen (see Known limitations) |

The screen freezes and every display is covered by the selection overlay.

- **Region.** Drag out a rectangle. Releasing the mouse does not capture it: the selection stays
  editable, so you can drag the eight handles to resize it, or nudge it with the arrow keys, one
  point at a time and ten with Shift held. Press `Enter` to capture it.
- **Window.** Move the pointer over a window to highlight it, then click to capture it.
- **`Esc`** cancels, on every display at once, and writes no file.
- **`C`** copies the hex colour under the magnifier to the clipboard, in region and window mode
  alike, whether or not anything is selected.

Every capture is saved as a PNG under `~/Pictures`, named `Snapdeck <date> at <time>.png`, and is
put on the clipboard at the same time, ready to paste. The region is captured again at the
display's own pixel density rather than cropped out of the frozen frame, so on a Retina display
the file is twice the size in pixels that you selected in points.

> **Unsigned builds:** Snapdeck releases are not notarized. Without an Apple Developer ID, macOS Gatekeeper will block the app on first launch. Right-click the app and choose Open, then confirm. Building from source avoids this.

## License

MIT, see [LICENSE](LICENSE).

## Known limitations

- **Full screen mode selects rather than captures.** `Cmd+Shift+9` and the menu's Capture Full
  Screen open the overlay in the same free-selection mode as `Cmd+Shift+7`. They do not yet capture
  the whole display in one keystroke; select the area you want and press `Enter`.
- **Filenames carry the UTC time, not yours.** A capture taken at 20:05 in UTC+3 is saved as
  `... at 17.05.06.png`. The name is built from the system clock without a time zone database.
- **Fullscreen Spaces.** Capturing while another app owns a fullscreen Space does not work yet. The
  overlay window is created with the frozen frame intact, but macOS keeps it on a normal Space, so
  nothing appears over the fullscreen app. Setting `canJoinAllSpaces | fullScreenAuxiliary`, raising the
  window level, ordering the window in on the same run loop turn and activating the app were all tried
  and none of them place the overlay above another app's fullscreen Space.

