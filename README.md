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
| `Cmd+Shift+9` | Full screen |

The screen freezes and every display is covered by the selection overlay.

- **Region.** Drag out a rectangle. Releasing the mouse does not capture it: the selection stays
  editable, so you can drag the eight handles to resize it, or nudge it with the arrow keys, one
  point at a time and ten with Shift held. Press `Enter` to capture it.
- **Window.** Move the pointer over a window to highlight it, then click to capture it. Whatever
  covers the highlighted window is photographed with it: the capture is a picture of that
  rectangle of screen, not of the window on its own, so move an overlapping window away first.
- **Full screen.** The whole display arrives already selected, so press `Enter` to capture it.
  It is an ordinary selection until you do, so you can still drag out a smaller one, resize it
  with the handles, or nudge it with the arrow keys.
- **`Esc`** cancels, on every display at once, and writes no file.
- **`C`** copies the hex colour under the magnifier to the clipboard, in all three modes alike,
  whether or not anything is selected.

Every capture is saved as a PNG under `~/Pictures`, named `Snapdeck <date> at <time>.png` in your
own time zone, and is put on the clipboard at the same time, ready to paste. The region is
captured again at the display's own pixel density rather than cropped out of the frozen frame, so
on a Retina display the file is twice the size in pixels that you selected in points.

That second capture happens the moment you press `Enter`, not when the screen froze, so the file
holds the screen as it is then rather than the frozen frame you selected against. For still
content the two are the same picture; over a video, an animation or anything else that moves, the
saved pixels are the ones from the instant you confirmed.

> **Unsigned builds:** Snapdeck releases are not notarized. Without an Apple Developer ID, macOS Gatekeeper will block the app on first launch. Right-click the app and choose Open, then confirm. Building from source avoids this.

## License

MIT, see [LICENSE](LICENSE).

## Known limitations

- **Fullscreen Spaces.** Capturing while another app owns a fullscreen Space does not work yet. The
  overlay window is created with the frozen frame intact, but macOS keeps it on a normal Space, so
  nothing appears over the fullscreen app. Setting `canJoinAllSpaces | fullScreenAuxiliary`, raising the
  window level, ordering the window in on the same run loop turn and activating the app were all tried
  and none of them place the overlay above another app's fullscreen Space.

