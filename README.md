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
System Settings > Privacy & Security > Screen & System Audio Recording, then try again.

> **Unsigned builds:** Snapdeck releases are not notarized. Without an Apple Developer ID, macOS Gatekeeper will block the app on first launch. Right-click the app and choose Open, then confirm. Building from source avoids this.

## License

MIT, see [LICENSE](LICENSE).

## Known limitations

- **Fullscreen Spaces.** Capturing while another app owns a fullscreen Space does not work yet. The
  overlay window is created with the frozen frame intact, but macOS keeps it on a normal Space, so
  nothing appears over the fullscreen app. Setting `canJoinAllSpaces | fullScreenAuxiliary`, raising the
  window level, ordering the window in on the same run loop turn and activating the app were all tried
  and none of them place the overlay above another app's fullscreen Space.

