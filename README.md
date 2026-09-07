# Snapdeck

Snapdeck is an open-source macOS menu bar app for capturing, annotating, and sharing screenshots.

## Requirements

- macOS 13 or later
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
