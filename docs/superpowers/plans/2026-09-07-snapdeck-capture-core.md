# Snapdeck Capture Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Kullanıcı global kısayola bastığında ekran donar, bölge/pencere/tam ekran seçer, sonuç PNG olarak diske kaydedilir ve panoya kopyalanır.

**Architecture:** Rust tarafında platform bağımsız bir `ScreenCapturer` trait'i ve macOS için ScreenCaptureKit implementasyonu. Tauri kabuğu tepsi menüsü ve global kısayolu yönetir, her monitör için donmuş kare gösteren şeffaf bir overlay penceresi açar. Seçim geometrisi saf TypeScript modülüdür ve overlay arayüzünden bağımsız test edilir. Seçim onaylandığında donmuş kare kırpılmaz; bölge ScreenCaptureKit'ten yeniden, tam çözünürlükte yakalanır.

**Tech Stack:** Rust (edition 2021), Tauri 2.11.5, screencapturekit 9.0.1, core-graphics 0.25, image 0.25, React 19, TypeScript, Vite, Tailwind, Vitest, pnpm workspaces.

## Global Constraints

- Hedef platform: macOS 14.0+. `SCScreenshotManager` Apple'da macOS 14 API'sidir, crate'te `macos_14_0` feature'ı arkasındadır; taban bu yüzden 13.0 değil 14.0. Windows/Linux bu planın dışında.
- Doğrulanmış sürümler, birebir kullanılacak: `tauri = "2.11.5"`, `tauri-plugin-global-shortcut = "2.3.2"`, `tauri-plugin-clipboard-manager = "2.3.3"`, `tauri-plugin-store = "2.4.4"`, `screencapturekit = "9.0.1"`, `core-graphics = "0.25"`, `image = "0.25"`.
- Kod, yorum, commit mesajı, README ve uygulama arayüzü metinleri **İngilizce**. Açık kaynak proje, uluslararası katkıcı hedefliyor. Sadece `docs/superpowers/` altındaki tasarım ve plan dokümanları Türkçe.
- Hiçbir ağ çağrısı yok. Uygulama tamamen yereldir, telemetri yoktur.
- Lisans MIT.
- `cargo clippy -- -D warnings` ve `cargo fmt --check` her commit'te temiz olmalı.
- İzin reddedildiğinde asla boş/siyah kare üretilmez; `CaptureError::PermissionDenied` döner.
- Frame verisi her zaman kendi `scale_factor` değerini taşır; ölçek asla sonradan tahmin edilmez.

---

## File Structure

```
Cargo.toml                                  Cargo workspace tanımı
package.json                                pnpm workspace kökü, ortak scriptler
pnpm-workspace.yaml                         workspace paket listesi
LICENSE                                     MIT
README.md                                   kurulum, geliştirme, imzalama uyarısı
.github/workflows/ci.yml                    fmt + clippy + cargo test + pnpm test

crates/capture/Cargo.toml
crates/capture/src/lib.rs                   trait + yeniden dışa aktarımlar
crates/capture/src/types.rs                 Rect, Frame, DisplayInfo, WindowInfo, CaptureTarget, PixelFormat
crates/capture/src/error.rs                 CaptureError
crates/capture/src/mock.rs                  testler için MockCapturer
crates/capture/src/macos/mod.rs             MacCapturer, ScreenCapturer implementasyonu
crates/capture/src/macos/permission.rs      TCC preflight/request FFI

apps/desktop/src-tauri/Cargo.toml
apps/desktop/src-tauri/tauri.conf.json      pencere, tepsi, asset protokolü, bundle
apps/desktop/src-tauri/Info.plist           LSUIElement, minimum sistem sürümü
apps/desktop/src-tauri/build.rs
apps/desktop/src-tauri/src/main.rs          giriş noktası
apps/desktop/src-tauri/src/lib.rs           Builder kurulumu, plugin kaydı
apps/desktop/src-tauri/src/state.rs         AppState, paylaşılan capturer
apps/desktop/src-tauri/src/tray.rs          tepsi menüsü ve olayları
apps/desktop/src-tauri/src/shortcuts.rs     global kısayol kaydı
apps/desktop/src-tauri/src/overlay.rs       monitör başına overlay penceresi
apps/desktop/src-tauri/src/commands.rs      frontend'e açılan komutlar
apps/desktop/src-tauri/src/output.rs        dosya adı şablonu, PNG kaydetme, panoya kopyalama

apps/desktop/index.html                     ana pencere (ayarlar) girişi
apps/desktop/overlay.html                   overlay penceresi girişi
apps/desktop/src/main.tsx                   ana pencere React kökü
apps/desktop/src/overlay/main.tsx           overlay React kökü
apps/desktop/src/overlay/Overlay.tsx        seçim arayüzü
apps/desktop/src/overlay/selection.ts       saf geometri, DOM bağımsız
apps/desktop/src/overlay/selection.test.ts
apps/desktop/src/overlay/snap.ts            imleç altındaki pencereyi bulma, saf
apps/desktop/src/overlay/snap.test.ts
apps/desktop/src/overlay/magnifier.ts       piksel örnekleme, hex dönüşümü, saf
apps/desktop/src/overlay/magnifier.test.ts
apps/desktop/src/lib/ipc.ts                 tip güvenli invoke sarmalayıcıları
```

Sorumluluk sınırı: `crates/capture` Tauri'yi bilmez, `apps/desktop/src/overlay/*.ts` saf modülleri DOM ve Tauri API'sini bilmez. İkisi de bağımsız birim testine sahiptir.

---

### Task 1: Monorepo iskeleti, Tauri kabuğu ve CI

**Files:**
- Create: `Cargo.toml`, `package.json`, `pnpm-workspace.yaml`, `.gitignore`, `LICENSE`, `README.md`
- Create: `apps/desktop/package.json`, `apps/desktop/vite.config.ts`, `apps/desktop/tsconfig.json`, `apps/desktop/index.html`, `apps/desktop/overlay.html`, `apps/desktop/src/main.tsx`
- Create: `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/build.rs`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/Info.plist`, `apps/desktop/src-tauri/src/main.rs`, `apps/desktop/src-tauri/src/lib.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: yok, ilk görev.
- Produces: `pnpm dev`, `pnpm build`, `pnpm test`, `pnpm tauri dev` komutları; Cargo workspace kökü.

- [ ] **Step 1: Workspace kökünü oluştur**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["apps/desktop/src-tauri"]
# Task 2 adds "crates/capture" to this list.

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.85"
license = "MIT"
repository = "https://github.com/OWNER/snapdeck"

[workspace.dependencies]
screencapturekit = "9.0.1"
core-graphics = "0.25"
image = { version = "0.25", default-features = false, features = ["png"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
```

`pnpm-workspace.yaml`:

```yaml
packages:
  - "apps/*"
  - "packages/*"
```

`package.json`:

```json
{
  "name": "snapdeck",
  "private": true,
  "packageManager": "pnpm@11.0.8",
  "scripts": {
    "dev": "pnpm --filter @snapdeck/desktop dev",
    "build": "pnpm --filter @snapdeck/desktop build",
    "test": "pnpm -r test",
    "lint": "pnpm -r lint",
    "tauri": "pnpm --filter @snapdeck/desktop tauri"
  }
}
```

`.gitignore`:

```
node_modules/
dist/
target/
.DS_Store
*.log
```

- [ ] **Step 2: Frontend iskeletini oluştur**

`apps/desktop/package.json`:

```json
{
  "name": "@snapdeck/desktop",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "test": "vitest run --passWithNoTests",
    "lint": "tsc --noEmit",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.9.0",
    "@tauri-apps/plugin-clipboard-manager": "^2.3.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.4",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.6.0",
    "vite": "^6.0.0",
    "vitest": "^3.2.0"
  }
}
```

`apps/desktop/vite.config.ts`:

```ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        overlay: resolve(__dirname, 'overlay.html'),
      },
    },
  },
})
```

`apps/desktop/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": ["vite/client", "vitest/globals"]
  },
  "include": ["src"]
}
```

`apps/desktop/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Snapdeck</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`apps/desktop/overlay.html`. Vite'ın `overlay` girişi bu dosyayı ister; olmazsa
`pnpm build` ve dolayısıyla `pnpm tauri build` hiç çalışmaz. Script etiketi yok, çünkü
`src/overlay/main.tsx` henüz mevcut değil. Task 6 bu dosyanın tamamını değiştirir:

```html
<!doctype html>
<!-- Placeholder. Task 6 replaces this with the real overlay entry point. -->
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Snapdeck Overlay</title>
  </head>
  <body>
    <div id="overlay-root"></div>
  </body>
</html>
```

`apps/desktop/src/main.tsx`:

```tsx
import React from 'react'
import { createRoot } from 'react-dom/client'

function App() {
  return <main style={{ fontFamily: 'system-ui', padding: 24 }}>Snapdeck is running.</main>
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
```

- [ ] **Step 3: Tauri kabuğunu oluştur**

`apps/desktop/src-tauri/Cargo.toml`:

```toml
[package]
name = "snapdeck"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lib]
name = "snapdeck_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2.11.5", features = ["tray-icon", "image-png"] }
tauri-plugin-global-shortcut = "2.3.2"
tauri-plugin-clipboard-manager = "2.3.3"
tauri-plugin-store = "2.4.4"
snapdeck-capture = { path = "../../../crates/capture" }
serde.workspace = true
serde_json = "1"
thiserror.workspace = true
image.workspace = true
```

`apps/desktop/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`apps/desktop/src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    snapdeck_lib::run()
}
```

`apps/desktop/src-tauri/src/lib.rs`:

```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .run(tauri::generate_context!())
        .expect("error while running snapdeck");
}
```

`apps/desktop/src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Snapdeck",
  "version": "0.1.0",
  "identifier": "com.snapdeck.app",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "macOSPrivateApi": true,
    "windows": [],
    "security": {
      "csp": "default-src 'self'; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'",
      "assetProtocol": { "enable": true, "scope": ["$APPCACHE/**"] }
    }
  },
  "bundle": {
    "active": true,
    "targets": ["dmg", "app"],
    "icon": ["icons/icon.icns"],
    "macOS": { "minimumSystemVersion": "14.0" }
  }
}
```

`apps/desktop/src-tauri/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>LSUIElement</key>
    <true/>
  </dict>
</plist>
```

`app.windows` boş bırakıldı: uygulama menü çubuğu uygulamasıdır, pencereler çalışma zamanında açılır.

- [ ] **Step 4: Uygulamanın çalıştığını doğrula**

Run: `pnpm install && pnpm build && pnpm tauri dev`
Expected: Derleme başarılı, hata yok. Dock'ta ikon görünmez (LSUIElement), pencere açılmaz. `Ctrl+C` ile durdur.

Run: `cargo fmt --check && cargo clippy --workspace -- -D warnings`

Bu noktada workspace tek üye içerir; `crates/capture` Task 2'de eklenir.

- [ ] **Step 5: CI kur**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace

  web:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # No `version:` input: it must not disagree with packageManager in
      # package.json, and the action reads that field on its own.
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      - run: pnpm lint
      - run: pnpm test
```

- [ ] **Step 6: Lisans ve README yaz**

`LICENSE`: standart MIT metni, telif satırı `Copyright (c) 2026 Snapdeck contributors`.

`README.md` şunları içermeli: tek cümlelik ürün tanımı, gereksinimler (macOS 14+, Rust stable, Node 24, pnpm 11), `pnpm install` ve `pnpm tauri dev` adımları, ekran kaydı izninin ilk yakalamada isteneceği notu, ve şu uyarı bölümü:

> **Unsigned builds:** Snapdeck releases are not notarized. Without an Apple Developer ID, macOS Gatekeeper will block the app on first launch. Right-click the app and choose Open, then confirm. Building from source avoids this.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: scaffold monorepo, Tauri shell, and CI"
```

---

### Task 2: Capture crate çekirdek tipleri ve trait

**Files:**
- Create: `crates/capture/Cargo.toml`, `crates/capture/src/lib.rs`, `crates/capture/src/types.rs`, `crates/capture/src/error.rs`, `crates/capture/src/mock.rs`
- Test: `crates/capture/src/types.rs` (inline `#[cfg(test)]`), `crates/capture/tests/mock_capturer.rs`

**Interfaces:**
- Consumes: yok.
- Produces:
  - `Rect { x: f64, y: f64, width: f64, height: f64 }`, metotları `intersect(&self, &Rect) -> Option<Rect>`, `clamp_to(&self, &Rect) -> Rect`, `is_empty(&self) -> bool`
  - `PixelFormat { Rgba8, Bgra8 }`
  - `Frame { data: Vec<u8>, width: u32, height: u32, stride: usize, pixel_format: PixelFormat, scale_factor: f32, captured_at: SystemTime }`, metodu `to_rgba8(&self) -> Result<Vec<u8>, CaptureError>`
  - `DisplayInfo { id: u32, bounds: Rect, scale_factor: f32, is_primary: bool }`
  - `WindowInfo { id: u32, title: Option<String>, app_name: Option<String>, bounds: Rect, layer: i32, is_on_screen: bool }`
  - `CaptureTarget { Display(u32), Window(u32), Region(Rect) }`
  - `CaptureError { PermissionDenied, TargetNotFound(String), Platform(String) }`
  - `trait ScreenCapturer { fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError>; fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError>; fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError>; }`
  - `MockCapturer::new(displays: Vec<DisplayInfo>, windows: Vec<WindowInfo>, frame: Frame)`, `MockCapturer::with_error(self, error: CaptureError) -> Self`

- [ ] **Step 1: Crate iskeletini oluştur**

`crates/capture/Cargo.toml`:

```toml
[package]
name = "snapdeck-capture"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
description = "Cross-platform screen capture abstraction for Snapdeck"

[dependencies]
serde.workspace = true
thiserror.workspace = true

[target.'cfg(target_os = "macos")'.dependencies]
screencapturekit.workspace = true
core-graphics.workspace = true
```

`crates/capture/src/lib.rs`:

```rust
//! Platform-independent screen capture abstraction.

pub mod error;
pub mod mock;
pub mod types;

// Task 3 adds: #[cfg(target_os = "macos")] pub mod macos;

pub use error::CaptureError;
pub use types::{CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo};

/// Enumerates capture targets and produces frames from them.
pub trait ScreenCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError>;
    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError>;
    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError>;
}
```

`crates/capture/src/error.rs`:

```rust
use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize, Clone, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum CaptureError {
    /// macOS screen recording permission is missing or was revoked.
    #[error("screen recording permission denied")]
    PermissionDenied,
    #[error("capture target not found: {0}")]
    TargetNotFound(String),
    #[error("platform capture failed: {0}")]
    Platform(String),
}
```

`crates/capture/src/mock.rs`:

```rust
use crate::{
    error::CaptureError,
    types::{CaptureTarget, DisplayInfo, Frame, WindowInfo},
    ScreenCapturer,
};

/// In-memory capturer used by tests. Never touches platform APIs.
pub struct MockCapturer {
    pub displays: Vec<DisplayInfo>,
    pub windows: Vec<WindowInfo>,
    pub frame: Frame,
    /// When set, every capture fails with this error. Consumers need this to
    /// test the permission-denied path without a platform capturer.
    pub error: Option<CaptureError>,
}

impl MockCapturer {
    pub fn new(displays: Vec<DisplayInfo>, windows: Vec<WindowInfo>, frame: Frame) -> Self {
        Self { displays, windows, frame, error: None }
    }

    pub fn with_error(mut self, error: CaptureError) -> Self {
        self.error = Some(error);
        self
    }
}

impl ScreenCapturer for MockCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError> {
        Ok(self.displays.clone())
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError> {
        Ok(self.windows.clone())
    }

    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        // Regions are not validated against the displays, so a consumer test
        // that captures an off-screen region here proves nothing about the
        // real capturer, which rejects it.
        match target {
            CaptureTarget::Display(id) if !self.displays.iter().any(|d| d.id == id) => {
                Err(CaptureError::TargetNotFound(format!("display {id}")))
            }
            CaptureTarget::Window(id) if !self.windows.iter().any(|w| w.id == id) => {
                Err(CaptureError::TargetNotFound(format!("window {id}")))
            }
            _ => Ok(self.frame.clone()),
        }
    }
}
```

- [ ] **Step 2: Başarısız testleri yaz**

`crates/capture/src/types.rs` dosyasının sonuna:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    #[test]
    fn intersect_returns_overlap() {
        let a = rect(0.0, 0.0, 100.0, 100.0);
        let b = rect(50.0, 50.0, 100.0, 100.0);
        assert_eq!(a.intersect(&b), Some(rect(50.0, 50.0, 50.0, 50.0)));
    }

    #[test]
    fn intersect_returns_none_when_disjoint() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(20.0, 20.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), None);
    }

    #[test]
    fn intersect_returns_none_for_edge_touch() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(10.0, 0.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), None);
    }

    #[test]
    fn clamp_to_keeps_rect_inside_bounds() {
        let bounds = rect(0.0, 0.0, 100.0, 100.0);
        let outside = rect(90.0, 90.0, 50.0, 50.0);
        assert_eq!(outside.clamp_to(&bounds), rect(90.0, 90.0, 10.0, 10.0));
    }

    #[test]
    fn clamp_to_returns_empty_rect_when_disjoint() {
        let bounds = rect(0.0, 0.0, 100.0, 100.0);
        let elsewhere = rect(500.0, 500.0, 50.0, 50.0);
        let clamped = elsewhere.clamp_to(&bounds);
        assert!(clamped.is_empty());
        assert_eq!(clamped, rect(500.0, 500.0, 0.0, 0.0));
    }

    #[test]
    fn to_rgba8_drops_stride_padding() {
        // 2x2 image, 4 bytes of row padding per row.
        let frame = Frame {
            data: vec![
                1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, //
                9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0,
            ],
            width: 2,
            height: 2,
            stride: 12,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: 1.0,
            captured_at: SystemTime::now(),
        };
        assert_eq!(
            frame.to_rgba8().unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn to_rgba8_swaps_bgra_channels_across_padded_rows() {
        // 2x2 BGRA with 4 bytes of row padding: the combination
        // ScreenCaptureKit actually delivers.
        let frame = Frame {
            data: vec![
                10, 20, 30, 40, 50, 60, 70, 80, 0, 0, 0, 0, //
                90, 100, 110, 120, 130, 140, 150, 160, 0, 0, 0, 0,
            ],
            width: 2,
            height: 2,
            stride: 12,
            pixel_format: PixelFormat::Bgra8,
            scale_factor: 2.0,
            captured_at: SystemTime::now(),
        };
        // Each BGRA pixel becomes RGBA, and the padding is dropped.
        assert_eq!(
            frame.to_rgba8().unwrap(),
            vec![30, 20, 10, 40, 70, 60, 50, 80, 110, 100, 90, 120, 150, 140, 130, 160]
        );
    }

    #[test]
    fn to_rgba8_rejects_a_stride_narrower_than_one_row() {
        let frame = Frame {
            data: vec![0; 16],
            width: 2,
            height: 2,
            // A row of 2 pixels needs 8 bytes.
            stride: 4,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: 1.0,
            captured_at: SystemTime::now(),
        };
        assert!(matches!(frame.to_rgba8(), Err(CaptureError::Platform(_))));
    }

    #[test]
    fn to_rgba8_rejects_a_buffer_too_short_for_its_rows() {
        let frame = Frame {
            data: vec![0; 8],
            width: 2,
            height: 2,
            stride: 8,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: 1.0,
            captured_at: SystemTime::now(),
        };
        assert!(matches!(frame.to_rgba8(), Err(CaptureError::Platform(_))));
    }
}
```

`crates/capture/tests/mock_capturer.rs`:

```rust
use std::time::SystemTime;

use snapdeck_capture::{
    mock::MockCapturer, CaptureError, CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect,
    ScreenCapturer, WindowInfo,
};

fn frame() -> Frame {
    Frame {
        data: vec![0, 0, 0, 255],
        width: 1,
        height: 1,
        stride: 4,
        pixel_format: PixelFormat::Rgba8,
        scale_factor: 1.0,
        captured_at: SystemTime::now(),
    }
}

fn display() -> DisplayInfo {
    DisplayInfo {
        id: 1,
        bounds: Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 },
        scale_factor: 2.0,
        is_primary: true,
    }
}

fn window() -> WindowInfo {
    WindowInfo {
        id: 7,
        title: Some("Editor".to_string()),
        app_name: Some("Snapdeck".to_string()),
        bounds: Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 },
        layer: 0,
        is_on_screen: true,
    }
}

#[test]
fn capture_unknown_display_reports_target_not_found() {
    let capturer = MockCapturer::new(vec![display()], vec![], frame());

    let err = capturer.capture(CaptureTarget::Display(99)).unwrap_err();

    assert_eq!(err, CaptureError::TargetNotFound("display 99".to_string()));
}

#[test]
fn capture_unknown_window_reports_target_not_found() {
    let capturer = MockCapturer::new(vec![display()], vec![window()], frame());

    let err = capturer.capture(CaptureTarget::Window(42)).unwrap_err();

    assert_eq!(err, CaptureError::TargetNotFound("window 42".to_string()));
}

#[test]
fn injected_error_fails_every_capture() {
    // Consumers must be able to exercise the permission-denied path, which
    // never yields an empty or black frame.
    let capturer = MockCapturer::new(vec![display()], vec![window()], frame())
        .with_error(CaptureError::PermissionDenied);

    assert_eq!(
        capturer.capture(CaptureTarget::Display(1)).unwrap_err(),
        CaptureError::PermissionDenied
    );
    assert_eq!(
        capturer.capture(CaptureTarget::Window(7)).unwrap_err(),
        CaptureError::PermissionDenied
    );
}
```

- [ ] **Step 3: Testin başarısız olduğunu doğrula**

Run: `cargo test -p snapdeck-capture`
Expected: FAIL, `types.rs` içindeki tipler tanımlı değil (`cannot find type Rect in this scope`).

- [ ] **Step 4: Tipleri yaz**

`crates/capture/src/types.rs` (test modülünün üstüne):

```rust
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::CaptureError;

/// Rectangle in display points (not pixels), origin at top-left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Overlapping area, or `None` when the rectangles do not overlap.
    /// Rectangles that only touch at an edge do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let candidate = Rect { x, y, width: right - x, height: bottom - y };
        (!candidate.is_empty()).then_some(candidate)
    }

    /// Returns the part of the rectangle that lies inside `bounds`, or an
    /// empty rect at the original origin when the two do not overlap. That
    /// origin is outside `bounds`, so callers must check `is_empty` before
    /// trusting `x` and `y`.
    pub fn clamp_to(&self, bounds: &Rect) -> Rect {
        self.intersect(bounds).unwrap_or(Rect { x: self.x, y: self.y, width: 0.0, height: 0.0 })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PixelFormat {
    Rgba8,
    Bgra8,
}

/// A single captured image. Always carries its own scale factor so callers
/// never have to guess the pixel-to-point ratio.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row, including any padding.
    pub stride: usize,
    pub pixel_format: PixelFormat,
    /// Pixels per point, for example 2.0 on a Retina display.
    pub scale_factor: f32,
    /// When the frame was captured. Recording (v2) orders frames by this.
    pub captured_at: SystemTime,
}

impl Frame {
    /// Tightly packed RGBA8 copy with row padding removed.
    ///
    /// Fails when the frame's own fields contradict each other: a stride
    /// narrower than one row of pixels, or a buffer too short to hold
    /// `height` rows. Frames come from platform APIs, so these fields are
    /// validated rather than trusted. An unchecked stride either panics on
    /// the slice index or silently repeats a row.
    pub fn to_rgba8(&self) -> Result<Vec<u8>, CaptureError> {
        let row_bytes = self.width as usize * 4;
        let height = self.height as usize;
        if self.stride < row_bytes {
            return Err(CaptureError::Platform(format!(
                "frame stride {} is narrower than one row of {row_bytes} bytes",
                self.stride
            )));
        }
        let required = match height.checked_sub(1) {
            Some(rows_before_last) => self.stride * rows_before_last + row_bytes,
            None => 0,
        };
        if self.data.len() < required {
            return Err(CaptureError::Platform(format!(
                "frame buffer holds {} bytes, needs {required}",
                self.data.len()
            )));
        }
        let mut out = Vec::with_capacity(row_bytes * height);
        for row in 0..height {
            let start = row * self.stride;
            let row_slice = &self.data[start..start + row_bytes];
            match self.pixel_format {
                PixelFormat::Rgba8 => out.extend_from_slice(row_slice),
                PixelFormat::Bgra8 => {
                    for px in row_slice.chunks_exact(4) {
                        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: u32,
    /// Bounds in the global point coordinate space.
    pub bounds: Rect,
    pub scale_factor: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: u32,
    pub title: Option<String>,
    pub app_name: Option<String>,
    pub bounds: Rect,
    /// Window layer; 0 is the normal application layer.
    pub layer: i32,
    pub is_on_screen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CaptureTarget {
    Display(u32),
    Window(u32),
    Region(Rect),
}
```

- [ ] **Step 5: Testlerin geçtiğini doğrula**

Run: `cargo test -p snapdeck-capture`
Expected: PASS, 12 test geçer (9 birim, 3 entegrasyon).

Run: `cargo clippy -p snapdeck-capture --all-targets -- -D warnings`
Expected: uyarı yok.

- [ ] **Step 6: Commit**

```bash
git add crates/capture
git commit -m "feat(capture): add core types, capturer trait, and mock implementation"
```

---

### Task 3: macOS yakalama implementasyonu

**Files:**
- Create: `crates/capture/src/macos/mod.rs`
- Test: `crates/capture/tests/macos_capture.rs`

**Interfaces:**
- Consumes: Task 2'deki `ScreenCapturer`, `Frame`, `DisplayInfo`, `WindowInfo`, `CaptureTarget`, `CaptureError`, `Rect`, `PixelFormat`.
- Produces: `MacCapturer::new() -> MacCapturer` ve onun `ScreenCapturer` implementasyonu.

Doğrulanmış screencapturekit 9.0.1 API'si:
`SCShareableContent::get() -> Result<Self, SCError>`, `.displays() -> Vec<SCDisplay>`, `.windows() -> Vec<SCWindow>`;
`SCDisplay::display_id() -> u32`, `.frame() -> CGRect`, `.width() -> u32`, `.height() -> u32`;
`SCWindow::window_id() -> u32`, `.title() -> Option<String>`, `.frame() -> CGRect`, `.window_layer() -> i32`, `.is_on_screen() -> bool`, `.owning_application() -> Option<SCRunningApplication>`;
`SCContentFilter::create().with_display(&d).with_excluding_windows(&[]).build()`, `.with_window(&w).build()`;
`SCStreamConfiguration::new().with_width(u32).with_height(u32).with_pixel_format(PixelFormat::BGRA).with_shows_cursor(bool).with_source_rect(CGRect)`;

Sürüm kapısı uyarısı: `capture_image_in_rect` crate'in `macos_15_2` feature'ı arkasındadır ve macOS 15.2 gerektirir.
Ürün tabanı macOS 14.0 olduğu için kullanılmaz. Bölge yakalama, ekran filtresi + `with_source_rect` ile yapılır;
bu API sürüm kapısı altında değildir.
`SCScreenshotManager::capture_image(&filter, &config) -> Result<CGImage, SCError>`;
`SCStreamConfiguration::with_source_rect(CGRect)`, ekranın kendi başlangıç noktasına göre bölge seçer;
`CGImageExt::rgba_data(&self) -> Result<Vec<u8>, SCError>` (satır dolgusu olmadan sıkı paketlenmiş);
`CGImage::width() -> usize`, `.height() -> usize`.

- [ ] **Step 1: Başarısız testi yaz**

> **Aşılmış kod.** Aşağıdaki blok görevin başlangıç noktasıdır, sevk edilen hali değil. Bu görevin
> sonundaki "Task 3 incelemesinden gelen ek kurallar" bölümü onu geçersiz kılar: ilk eşleşen ekran
> yerine en büyük kesişim, `as u32` kesmesi yerine yuvarlama ve 1 piksel tabanı, ekran başlangıcında
> duran anlamsız entegrasyon testi yerine saf yardımcı birim testleri. Gerçek kod
> `crates/capture/src/macos/mod.rs` içindedir.

`crates/capture/tests/macos_capture.rs`:

```rust
#![cfg(target_os = "macos")]

use snapdeck_capture::{macos::MacCapturer, CaptureTarget, Rect, ScreenCapturer};

/// Requires screen recording permission; run manually with
/// `cargo test -p snapdeck-capture -- --ignored`.
#[test]
#[ignore]
fn captures_a_region_at_native_resolution() {
    let capturer = MacCapturer::new();
    let displays = capturer.displays().expect("displays");
    let primary = displays.iter().find(|d| d.is_primary).expect("primary display");

    let region = Rect { x: primary.bounds.x, y: primary.bounds.y, width: 100.0, height: 50.0 };
    let frame = capturer.capture(CaptureTarget::Region(region)).expect("capture");

    let scale = primary.scale_factor;
    assert_eq!(frame.width, (100.0 * scale) as u32);
    assert_eq!(frame.height, (50.0 * scale) as u32);
    assert_eq!(frame.scale_factor, scale);
    assert_eq!(frame.data.len(), frame.stride * frame.height as usize);
}

#[test]
#[ignore]
fn lists_at_least_one_display_with_a_sane_scale_factor() {
    let capturer = MacCapturer::new();
    let displays = capturer.displays().expect("displays");
    assert!(!displays.is_empty());
    for d in &displays {
        assert!(d.scale_factor >= 1.0 && d.scale_factor <= 4.0, "bad scale: {}", d.scale_factor);
        assert!(!d.bounds.is_empty());
    }
}
```

- [ ] **Step 2: Testin derlenmediğini doğrula**

Run: `cargo test -p snapdeck-capture --test macos_capture`
Expected: FAIL, `could not find macos in snapdeck_capture`.

- [ ] **Step 3: Implementasyonu yaz**

> **Aşılmış kod.** Aşağıdaki blok görevin başlangıç noktasıdır, sevk edilen hali değil. Bu görevin
> sonundaki "Task 3 incelemesinden gelen ek kurallar" bölümü onu geçersiz kılar: ilk eşleşen ekran
> yerine en büyük kesişim, `as u32` kesmesi yerine yuvarlama ve 1 piksel tabanı, ekran başlangıcında
> duran anlamsız entegrasyon testi yerine saf yardımcı birim testleri. Gerçek kod
> `crates/capture/src/macos/mod.rs` içindedir.

Önce `crates/capture/src/lib.rs` içindeki Task 2'den kalan yer tutucu yorumu gerçek bildirimle değiştir:

```rust
#[cfg(target_os = "macos")]
pub mod macos;
```

Sonra `crates/capture/src/macos/mod.rs`:

```rust
// Task 4 adds: pub mod permission;

use core_graphics::display::{CGDisplay, CGMainDisplayID};
use screencapturekit::prelude::*;

use crate::{
    error::CaptureError,
    types::{CaptureTarget, DisplayInfo, Frame, PixelFormat, Rect, WindowInfo},
    ScreenCapturer,
};

/// ScreenCaptureKit-backed capturer. Requires macOS 14.0 or newer.
pub struct MacCapturer;

impl MacCapturer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacCapturer {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps a ScreenCaptureKit error to a capture error.
///
/// Match on the error's variant, never on its message: macOS localizes
/// `NSError` descriptions, so a Turkish system reports declined TCC consent
/// in Turkish and any English substring test silently misclassifies a
/// permission denial as a generic platform failure. Consult the `SCError`
/// variants in the installed crate version, map declined-consent and
/// unauthorized cases to `CaptureError::PermissionDenied`, and everything
/// else to `CaptureError::Platform` carrying the original text.

fn to_rect(frame: CGRect) -> Rect {
    Rect {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    }
}

/// Pixels per point for a display, read from its current display mode.
fn scale_factor_for(display_id: u32) -> f32 {
    let display = CGDisplay::new(display_id);
    match display.display_mode() {
        Some(mode) if mode.width() > 0 => mode.pixel_width() as f32 / mode.width() as f32,
        _ => 1.0,
    }
}

fn frame_from_image(image: &CGImage, scale_factor: f32) -> Result<Frame, CaptureError> {
    let data = image.rgba_data().map_err(map_err)?;
    let width = image.width() as u32;
    let height = image.height() as u32;
    Ok(Frame {
        data,
        width,
        height,
        // rgba_data returns tightly packed rows.
        stride: width as usize * 4,
        pixel_format: PixelFormat::Rgba8,
        scale_factor,
        captured_at: std::time::SystemTime::now(),
    })
}

impl ScreenCapturer for MacCapturer {
    fn displays(&self) -> Result<Vec<DisplayInfo>, CaptureError> {
        let content = SCShareableContent::get().map_err(map_err)?;
        let main_id = unsafe { CGMainDisplayID() };
        Ok(content
            .displays()
            .into_iter()
            .map(|d| {
                let id = d.display_id();
                DisplayInfo {
                    id,
                    bounds: to_rect(d.frame()),
                    scale_factor: scale_factor_for(id),
                    is_primary: id == main_id,
                }
            })
            .collect())
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, CaptureError> {
        let content = SCShareableContent::get().map_err(map_err)?;
        Ok(content
            .windows()
            .into_iter()
            .filter(|w| w.is_on_screen())
            .map(|w| WindowInfo {
                id: w.window_id(),
                title: w.title(),
                app_name: w.owning_application().and_then(|a| a.application_name()),
                bounds: to_rect(w.frame()),
                layer: w.window_layer(),
                is_on_screen: true,
            })
            .collect())
    }

    fn capture(&self, target: CaptureTarget) -> Result<Frame, CaptureError> {
        match target {
            CaptureTarget::Region(rect) => {
                if rect.is_empty() {
                    return Err(CaptureError::Platform("empty region".to_string()));
                }
                let content = SCShareableContent::get().map_err(map_err)?;
                let displays = content.displays();
                // The region arrives in global points; capture it from the
                // display it overlaps.
                let display = displays
                    .iter()
                    .find(|d| to_rect(d.frame()).intersect(&rect).is_some())
                    .ok_or_else(|| {
                        CaptureError::TargetNotFound("no display intersects the region".to_string())
                    })?;
                let bounds = to_rect(display.frame());
                let scale = scale_factor_for(display.display_id());
                // sourceRect is relative to the display's own origin, not to
                // the global coordinate space.
                let local = CGRect {
                    origin: CGPoint { x: rect.x - bounds.x, y: rect.y - bounds.y },
                    size: CGSize { width: rect.width, height: rect.height },
                };
                let filter = SCContentFilter::create()
                    .with_display(display)
                    .with_excluding_windows(&[])
                    .build();
                let config = SCStreamConfiguration::new()
                    .with_source_rect(local)
                    .with_width((rect.width * scale as f64) as u32)
                    .with_height((rect.height * scale as f64) as u32)
                    .with_shows_cursor(false);
                let image = SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
            CaptureTarget::Display(id) => {
                let content = SCShareableContent::get().map_err(map_err)?;
                let display = content
                    .displays()
                    .into_iter()
                    .find(|d| d.display_id() == id)
                    .ok_or_else(|| CaptureError::TargetNotFound(format!("display {id}")))?;
                let scale = scale_factor_for(id);
                let filter = SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_windows(&[])
                    .build();
                let config = SCStreamConfiguration::new()
                    .with_width((display.width() as f32 * scale) as u32)
                    .with_height((display.height() as f32 * scale) as u32)
                    .with_shows_cursor(false);
                let image = SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
            CaptureTarget::Window(id) => {
                let content = SCShareableContent::get().map_err(map_err)?;
                let window = content
                    .windows()
                    .into_iter()
                    .find(|w| w.window_id() == id)
                    .ok_or_else(|| CaptureError::TargetNotFound(format!("window {id}")))?;
                let bounds = to_rect(window.frame());
                let scale = self
                    .displays()?
                    .into_iter()
                    .find(|d| d.bounds.intersect(&bounds).is_some())
                    .map(|d| d.scale_factor)
                    .unwrap_or(1.0);
                let filter = SCContentFilter::create().with_window(&window).build();
                let config = SCStreamConfiguration::new()
                    .with_width((bounds.width * scale as f64) as u32)
                    .with_height((bounds.height * scale as f64) as u32)
                    .with_shows_cursor(false);
                let image = SCScreenshotManager::capture_image(&filter, &config).map_err(map_err)?;
                frame_from_image(&image, scale)
            }
        }
    }
}
```

Not: `SCRunningApplication::application_name()` `String` döner, `Option<String>` değil, yani `map` kullanılır, `and_then` değil. `CGDisplayMode::pixel_width()` ve `width()` `u64` döner. Bir isim crate sürümünde farklıysa `cargo doc -p screencapturekit --no-deps` ile doğrula ve karşılığını kullan; davranış aynı kalmalı (uygulama adı ve piksel genişliği).

Not: `crates/capture/build.rs`, Swift çalışma zamanı kütüphaneleri için rpath ekler. Onsuz test binary'leri yüklenmeden çöker.

Task 3 incelemesinden gelen ek kurallar:

- **Hedef ekran seçimi en büyük kesişimle yapılır**, ilk eşleşenle değil. `content.displays()` sırası
  belgelenmemiştir; ilk eşleşme, iki ekrana değen bir bölgede hangi yarının kazanacağını rastlantıya bırakır.
  Aynı kural pencerenin ölçek katsayısını bulurken de geçerlidir: 1x ekran listede önce gelirse 2x ekrandaki
  pencere 1x yakalanır.
- **Bölge tek bir ekranın içinde olmak zorundadır.** Overlay her monitör için ayrı pencere açtığı ve seçimi o
  pencerenin sınırlarına kırptığı için bu, arayüzün zaten sağladığı bir sözleşmedir. Seçilen ekranın dışına
  taşan bölge sessizce kırpılmaz, açık hata döner: ScreenCaptureKit `sourceRect`'i kırpar ama çıktıyı yine
  istenen tam boyuta yerleştirir, sonuç kısmen siyah ve yanlış ölçekli bir "başarılı" karedir.
- **Koordinat aritmetiği saf bir yardımcıya çıkarılır** (`source_rect_for(region, display_bounds) -> CGRect`)
  ve birim testleri yazılır: negatif başlangıçlı ikincil ekran (`x: -1920`), `y: -1080` ve ekran başlangıcı
  olmayan bölge. Donanımın erişemediği dal budur; entegrasyon testi birincil ekranın başlangıcında çalıştığı
  için `rect.x - bounds.x` her zaman 0 çıkar ve aritmetiği hiç sınamaz.
- **`map_err` yalnızca variant'a bakar.** İngilizce alt dizi kontrolü kaldırılır: hem yanıltıcıdır hem de
  bu dosyanın var oluş sebebine aykırıdır. Dört dal için birim testi yazılır (izin reddi, kullanıcı reddi
  kodu, paylaşılabilir içerik yok, ve `Platform`'a düşen herhangi bir variant).
- **Piksel boyutları yuvarlanır, kırpılmaz**, ve eksen başına en az 1 piksel istenir. `as u32` kesmesi
  kesirli seçimde eksen başına bir piksel kaybettirir ve nokta altı bir bölgede 0x0 yakalama ister.
- `ScreenCapturer` metotları çağıran iş parçacığını platform gidiş dönüşü boyunca bloklar. Task 5 bunları
  Tauri ana iş parçacığında çağırmamalıdır; trait'in doküman yorumuna yazılır.

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test -p snapdeck-capture`
Expected: PASS. `macos_capture` testleri `ignored` olarak atlanır.

Run: `cargo test -p snapdeck-capture -- --ignored`
Expected: PASS. İlk çalıştırmada macOS ekran kaydı izni ister; izin ver ve tekrar çalıştır. İzin verilmezse test `PermissionDenied` ile başarısız olur, bu beklenen davranıştır.

- [ ] **Step 5: Commit**

```bash
git add crates/capture
git commit -m "feat(capture): add macOS ScreenCaptureKit implementation"
```

---

### Task 4: Ekran kaydı izni akışı

**Files:**
- Create: `crates/capture/src/macos/permission.rs`
- Test: `crates/capture/src/macos/permission.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: yok.
- Produces:
  - `PermissionState { Granted, Denied }`, `PermissionState::from_granted(bool) -> Self`, `is_granted(&self) -> bool`
  - `fn screen_capture_permission() -> PermissionState`
  - `fn request_screen_capture_permission() -> PermissionState`
  - `const SETTINGS_DEEP_LINK: &str`

- [ ] **Step 1: Başarısız testi yaz**

Önce `crates/capture/src/macos/mod.rs` içindeki yer tutucu yorumu gerçek bildirimle değiştir:

```rust
pub mod permission;
```

Sonra `crates/capture/src/macos/permission.rs` sonuna:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_granted_maps_true_to_granted() {
        assert_eq!(PermissionState::from_granted(true), PermissionState::Granted);
        assert!(PermissionState::from_granted(true).is_granted());
    }

    #[test]
    fn from_granted_maps_false_to_denied() {
        assert_eq!(PermissionState::from_granted(false), PermissionState::Denied);
        assert!(!PermissionState::from_granted(false).is_granted());
    }

    #[test]
    fn settings_deep_link_points_at_screen_recording_pane() {
        assert_eq!(
            SETTINGS_DEEP_LINK,
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        );
    }
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p snapdeck-capture permission`
Expected: FAIL, `cannot find type PermissionState in this scope`.

- [ ] **Step 3: Implementasyonu yaz**

`crates/capture/src/macos/permission.rs` (test modülünün üstüne):

```rust
use serde::Serialize;

/// Deep link to System Settings, Privacy and Security, Screen Recording.
pub const SETTINGS_DEEP_LINK: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
}

impl PermissionState {
    pub fn from_granted(granted: bool) -> Self {
        if granted {
            Self::Granted
        } else {
            Self::Denied
        }
    }

    pub fn is_granted(&self) -> bool {
        matches!(self, Self::Granted)
    }
}

/// Checks the current permission without showing a prompt.
pub fn screen_capture_permission() -> PermissionState {
    PermissionState::from_granted(unsafe { CGPreflightScreenCaptureAccess() })
}

/// Triggers the system prompt the first time it is called. On later calls
/// macOS does not prompt again, so the caller must send the user to
/// `SETTINGS_DEEP_LINK` when this returns `Denied`.
pub fn request_screen_capture_permission() -> PermissionState {
    PermissionState::from_granted(unsafe { CGRequestScreenCaptureAccess() })
}
```

- [ ] **Step 4: Testlerin geçtiğini doğrula**

Run: `cargo test -p snapdeck-capture permission`
Expected: PASS, 3 test.

- [ ] **Step 5: Commit**

```bash
git add crates/capture
git commit -m "feat(capture): add macOS screen recording permission checks"
```

**Aşağı akış uyarısı (Task 6 ve Task 10 bunu bilmek zorunda).** Çalışan bir süreç, ekran kaydı izni
verildikten sonra bunu yeniden başlatılmadan göremez; macOS'un ayarlar panelinde "Quit & Reopen"
önermesinin sebebi budur. Kullanıcı `SETTINGS_DEEP_LINK`'i izleyip anahtarı açar ve uygulamaya geri
dönerse, preflight `Granted` derken `SCShareableContent::get()` hâlâ başarısız olabilir; hata o zaman
`PermissionDenied` değil `Platform` olarak gelir. Yani içerik listelemeden gelen bir `Platform` hatası,
iznin kullanılabilir olduğunun kanıtı değildir. İzin yönlendirme arayüzü kullanıcıya anahtarı açtıktan
sonra uygulamayı yeniden başlatmasını da söylemelidir.

---

### Task 5: Uygulama durumu, tepsi menüsü ve global kısayol

**Files:**
- Create: `apps/desktop/src-tauri/src/state.rs`, `apps/desktop/src-tauri/src/tray.rs`, `apps/desktop/src-tauri/src/shortcuts.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/shortcuts.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 3'teki `MacCapturer`, Task 4'teki `screen_capture_permission`.
- Produces:
  - `AppState { capturer: MacCapturer }`, `AppState::new()`
  - `Shortcuts { capture_region: String, capture_window: String, capture_display: String }`, `Shortcuts::default()`, `Shortcuts::parse_all(&self) -> Result<Vec<Shortcut>, String>`, `Shortcuts::MODES: [&str; 3]`, `Shortcuts::mode_for_parsed(&self, &Shortcut) -> Option<&'static str>`
  - `fn build_tray(app: &AppHandle) -> tauri::Result<()>`
  - `fn register_shortcuts(app: &AppHandle, shortcuts: &Shortcuts) -> Result<(), String>`
  - `fn request_capture(app: &AppHandle, mode: &str)`, tepsi ve kısayolun ortak giriş noktası (Task 6'da overlay açacak şekilde doldurulur)

- [ ] **Step 1: Başarısız testi yaz**

`apps/desktop/src-tauri/src/shortcuts.rs` sonuna:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_parseable_shortcuts() {
        let shortcuts = Shortcuts::default();
        let parsed = shortcuts.parse_all().expect("defaults must parse");
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn invalid_shortcut_is_reported_with_its_value() {
        let shortcuts = Shortcuts {
            capture_region: "NotAKey+++".to_string(),
            ..Shortcuts::default()
        };
        let err = shortcuts.parse_all().unwrap_err();
        assert!(err.contains("NotAKey+++"), "error should name the bad value: {err}");
    }

    #[test]
    fn modes_line_up_with_parsed_shortcuts() {
        let shortcuts = Shortcuts::default();
        let parsed = shortcuts.parse_all().expect("defaults must parse");
        assert_eq!(parsed.len(), Shortcuts::MODES.len());
        // Every position, not just one: swapping MODES 0 and 2 would otherwise
        // pass, and would silently make the region shortcut capture the whole
        // screen once Task 6 acts on the mode.
        for (index, mode) in Shortcuts::MODES.iter().enumerate() {
            assert_eq!(shortcuts.mode_for_parsed(&parsed[index]), Some(*mode));
        }
    }

    #[test]
    fn mode_for_parsed_ignores_foreign_shortcuts() {
        let shortcuts = Shortcuts::default();
        let foreign = Shortcut::from_str("CmdOrCtrl+Alt+K").expect("valid");
        assert_eq!(shortcuts.mode_for_parsed(&foreign), None);
    }

    #[test]
    fn defaults_do_not_collide_with_each_other() {
        // Compare parsed values, not strings: "Cmd+Shift+7" and
        // "CmdOrCtrl+Shift+7" are different strings that collide at
        // registration, which is the whole reason mode_for_parsed exists.
        // A set, not dedup(): dedup only collapses adjacent duplicates, so a
        // collision between the first and third binding would slip through.
        let parsed = Shortcuts::default().parse_all().expect("defaults must parse");
        let unique: std::collections::HashSet<_> = parsed.iter().collect();
        assert_eq!(unique.len(), 3);
    }
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p snapdeck shortcuts`
Expected: FAIL, `cannot find type Shortcuts in this scope`.

- [ ] **Step 3: Implementasyonu yaz**

`apps/desktop/src-tauri/src/shortcuts.rs` (test modülünün üstüne):

```rust
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcuts {
    pub capture_region: String,
    pub capture_window: String,
    pub capture_display: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        // Avoids the macOS system screenshot bindings (Cmd+Shift+3/4/5).
        Self {
            capture_region: "CmdOrCtrl+Shift+7".to_string(),
            capture_window: "CmdOrCtrl+Shift+8".to_string(),
            capture_display: "CmdOrCtrl+Shift+9".to_string(),
        }
    }
}

impl Shortcuts {
    /// Capture modes, in the same order as `parse_all` returns shortcuts.
    pub const MODES: [&'static str; 3] = ["region", "window", "display"];

    pub fn parse_all(&self) -> Result<Vec<Shortcut>, String> {
        [&self.capture_region, &self.capture_window, &self.capture_display]
            .into_iter()
            .map(|raw| {
                Shortcut::from_str(raw).map_err(|e| format!("invalid shortcut '{raw}': {e}"))
            })
            .collect()
    }

    /// Capture mode for an already parsed shortcut, or `None` when it is not
    /// ours. Compares parsed values rather than strings, so "CmdOrCtrl+Shift+7"
    /// and "Cmd+Shift+7" match the same binding.
    pub fn mode_for_parsed(&self, shortcut: &Shortcut) -> Option<&'static str> {
        let parsed = self.parse_all().ok()?;
        let index = parsed.iter().position(|candidate| candidate == shortcut)?;
        Self::MODES.get(index).copied()
    }
}

pub fn register_shortcuts(app: &AppHandle, shortcuts: &Shortcuts) -> Result<(), String> {
    let parsed = shortcuts.parse_all()?;
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;
    for shortcut in parsed {
        manager.register(shortcut).map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

Not: `crates/capture/build.rs` Swift köprüsü için bir rpath link argümanı yayar, ama `rustc-link-arg`
yalnızca onu yayan paketin kendi çıktılarına uygulanır, bağımlılarına değil. `MacCapturer` uygulamaya
bağlandığı anda aynı satırın `apps/desktop/src-tauri/build.rs` içinde de bulunması gerekir; yoksa
uygulama `dyld: Library not loaded: @rpath/libswift_Concurrency.dylib` ile `main` öncesi çöker. Bu
tekrar kasıtlıdır, tekilleştirilmemelidir.

`apps/desktop/src-tauri/src/state.rs`:

```rust
use snapdeck_capture::macos::MacCapturer;

/// Shared application state. The capturer is stateless and cheap to share.
pub struct AppState {
    pub capturer: MacCapturer,
}

impl AppState {
    pub fn new() -> Self {
        Self { capturer: MacCapturer::new() }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
```

`apps/desktop/src-tauri/src/tray.rs`:

```rust
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

/// Single entry point for every capture request, from the tray or a shortcut.
/// Task 6 replaces the body with `crate::overlay::open_overlays`.
pub fn request_capture(app: &AppHandle, mode: &str) {
    let _ = app;
    println!("capture requested: {mode}");
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItemBuilder::with_id("capture_region", "Capture Region").build(app)?;
    let window = MenuItemBuilder::with_id("capture_window", "Capture Window").build(app)?;
    let display = MenuItemBuilder::with_id("capture_display", "Capture Full Screen").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Snapdeck").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&region, &window, &display])
        .separator()
        .items(&[&settings, &quit])
        .build()?;

    TrayIconBuilder::with_id("main")
        .menu(&menu)
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::UnknownPath)?,
        )
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture_region" => request_capture(app, "region"),
            "capture_window" => request_capture(app, "window"),
            "capture_display" => request_capture(app, "display"),
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
```

`apps/desktop/src-tauri/src/lib.rs` yerine. Bu görevde yalnızca bu görevin modülleri bildirilir; `overlay` Task 6'da, `commands` Task 10'da eklenecek:

```rust
mod shortcuts;
mod state;
mod tray;

use shortcuts::{register_shortcuts, Shortcuts};
use state::AppState;
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if let Some(mode) = Shortcuts::default().mode_for_parsed(shortcut) {
                        tray::request_capture(app, mode);
                    }
                })
                .build(),
        )
        .manage(AppState::new())
        .setup(|app| {
            let handle = app.handle().clone();
            tray::build_tray(&handle)?;
            if let Err(err) = register_shortcuts(&handle, &Shortcuts::default()) {
                eprintln!("failed to register shortcuts: {err}");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running snapdeck");
}
```

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test -p snapdeck shortcuts`
Expected: PASS, 5 test.

- [ ] **Step 5: Tepsi menüsünü elle doğrula**

Run: `pnpm tauri dev`
Expected: Menü çubuğunda Snapdeck ikonu görünür; menüde Capture Region, Capture Window, Capture Full Screen, Settings, Quit maddeleri var. Quit uygulamayı kapatır. `CmdOrCtrl+Shift+7` basıldığında terminalde `capture requested: region` yazar.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri
git commit -m "feat(app): add tray menu, shared state, and global shortcuts"
```

---

### Task 6: Overlay pencereleri ve donmuş kare

**Files:**
- Create: `apps/desktop/src-tauri/src/overlay.rs`, `apps/desktop/src-tauri/src/output.rs`
- Create: `apps/desktop/src/overlay/main.tsx`, `apps/desktop/src/overlay/Overlay.tsx`
- Modify: `apps/desktop/overlay.html` (Task 1'in yer tutucusunun tamamını değiştir), `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/src/tray.rs`
- Test: `apps/desktop/src-tauri/src/overlay.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 2'deki `DisplayInfo`, `Frame`; Task 3'teki `MacCapturer`; Task 4'teki `screen_capture_permission`; Task 5'teki `AppState`.
- Produces:
  - `fn open_overlays(app: &AppHandle, mode: &str) -> Result<(), String>`
  - `fn close_overlays(app: &AppHandle)`
  - `fn overlay_label(display_id: u32) -> String` (biçim: `overlay-<display_id>`)
  - `fn frozen_frame_path(cache_dir: &Path, display_id: u32) -> PathBuf` (biçim: `<cache>/frozen-<display_id>.png`)
  - Overlay penceresi URL'i: `overlay.html?display=<id>&mode=<mode>&scale=<scale>`
  - `save_png(frame: &Frame, path: &Path) -> Result<(), String>` (`output.rs`, Task 10 de bunu kullanır)

- [ ] **Step 1: Başarısız testi yaz**

`apps/desktop/src-tauri/src/overlay.rs` sonuna:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn overlay_label_is_unique_per_display() {
        assert_eq!(overlay_label(1), "overlay-1");
        assert_ne!(overlay_label(1), overlay_label(2));
    }

    #[test]
    fn frozen_frame_path_lives_in_cache_dir() {
        let path = frozen_frame_path(Path::new("/tmp/cache"), 7);
        assert_eq!(path, Path::new("/tmp/cache/frozen-7.png"));
    }

    #[test]
    fn overlay_url_carries_display_mode_and_scale() {
        let url = overlay_url(3, "region", 2.0);
        assert_eq!(url, "overlay.html?display=3&mode=region&scale=2");
    }
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p snapdeck overlay`
Expected: FAIL, `cannot find function overlay_label in this scope`.

- [ ] **Step 3: Implementasyonu yaz**

`apps/desktop/src-tauri/src/overlay.rs` (test modülünün üstüne):

```rust
use std::path::{Path, PathBuf};

use snapdeck_capture::{
    macos::permission::screen_capture_permission, CaptureTarget, ScreenCapturer,
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{output::save_png, state::AppState};

pub fn overlay_label(display_id: u32) -> String {
    format!("overlay-{display_id}")
}

pub fn frozen_frame_path(cache_dir: &Path, display_id: u32) -> PathBuf {
    cache_dir.join(format!("frozen-{display_id}.png"))
}

pub fn overlay_url(display_id: u32, mode: &str, scale: f32) -> String {
    format!("overlay.html?display={display_id}&mode={mode}&scale={scale}")
}

/// Captures every display, writes each frame to the cache directory, and opens
/// one transparent always-on-top window per display showing that frame.
pub fn open_overlays(app: &AppHandle, mode: &str) -> Result<(), String> {
    if !screen_capture_permission().is_granted() {
        // The settings window owns the permission guidance UI.
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        return Err("screen recording permission denied".to_string());
    }

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache dir: {e}"))?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    let displays = state.capturer.displays().map_err(|e| e.to_string())?;

    for display in displays {
        let frame = state
            .capturer
            .capture(CaptureTarget::Display(display.id))
            .map_err(|e| e.to_string())?;
        save_png(&frame, &frozen_frame_path(&cache_dir, display.id))?;

        let label = overlay_label(display.id);
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.close();
        }

        WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App(overlay_url(display.id, mode, display.scale_factor).into()),
        )
        .position(display.bounds.x, display.bounds.y)
        .inner_size(display.bounds.width, display.bounds.height)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(true)
        .build()
        .map_err(|e| format!("failed to create overlay window: {e}"))?;
    }

    Ok(())
}

pub fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("overlay-") {
            let _ = window.close();
        }
    }
}
```

`apps/desktop/src-tauri/src/output.rs` (Task 10 bu dosyaya dosya adı şablonunu ekleyecek):

```rust
use std::path::Path;

use snapdeck_capture::Frame;

/// Writes a frame as PNG. Row padding is removed and BGRA is converted by
/// `Frame::to_rgba8`, so the file is always tightly packed RGBA.
pub fn save_png(frame: &Frame, path: &Path) -> Result<(), String> {
    let rgba = frame.to_rgba8().map_err(|e| e.to_string())?;
    image::save_buffer(path, &rgba, frame.width, frame.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("failed to save png: {e}"))
}
```

`apps/desktop/src-tauri/src/lib.rs` içindeki modül bildirimlerine ekle:

```rust
mod output;
mod overlay;
```

`apps/desktop/src-tauri/src/tray.rs` içindeki `request_capture` gövdesini gerçek implementasyonla değiştir:

```rust
pub fn request_capture(app: &AppHandle, mode: &str) {
    if let Err(err) = crate::overlay::open_overlays(app, mode) {
        eprintln!("failed to open overlays: {err}");
    }
}
```

`apps/desktop/overlay.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Snapdeck Overlay</title>
    <style>
      html, body, #overlay-root { margin: 0; height: 100%; width: 100%; overflow: hidden; }
      body { background: transparent; cursor: crosshair; user-select: none; }
    </style>
  </head>
  <body>
    <div id="overlay-root"></div>
    <script type="module" src="/src/overlay/main.tsx"></script>
  </body>
</html>
```

`apps/desktop/src/overlay/main.tsx`:

```tsx
import React from 'react'
import { createRoot } from 'react-dom/client'
import { Overlay } from './Overlay'

const params = new URLSearchParams(window.location.search)

createRoot(document.getElementById('overlay-root')!).render(
  <React.StrictMode>
    <Overlay
      displayId={Number(params.get('display'))}
      mode={params.get('mode') ?? 'region'}
      scale={Number(params.get('scale') ?? 1)}
    />
  </React.StrictMode>,
)
```

`Overlay` bileşeninin tam hali Task 7'de yazılacak. Bu görevde `apps/desktop/src/overlay/Overlay.tsx` yalnızca donmuş kareyi gösterir:

```tsx
import { convertFileSrc } from '@tauri-apps/api/core'
import { appCacheDir, join } from '@tauri-apps/api/path'
import { useEffect, useState } from 'react'

export function Overlay({ displayId }: { displayId: number; mode: string; scale: number }) {
  const [src, setSrc] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      const path = await join(await appCacheDir(), `frozen-${displayId}.png`)
      setSrc(convertFileSrc(path))
    })()
  }, [displayId])

  if (!src) return null
  return <img src={src} alt="" style={{ width: '100%', height: '100%', display: 'block' }} />
}
```

`@tauri-apps/plugin-fs` gerekmez; `appCacheDir` ve `join` core API'sindedir.

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test -p snapdeck overlay`
Expected: PASS, 3 test.

- [ ] **Step 5: Elle doğrula**

Run: `pnpm tauri dev`, sonra `CmdOrCtrl+Shift+7` bas.
Expected: Her monitörde ekranın donmuş görüntüsünü gösteren tam ekran pencere açılır. Görüntü ekranla birebir hizalı olmalı; kayma varsa `position`/`inner_size` nokta biriminde verilmediği içindir. Pencereleri kapatmak için uygulamayı durdur (Escape desteği Task 7'de gelecek).

- [ ] **Step 6: Commit**

```bash
git add apps/desktop
git commit -m "feat(overlay): open frozen-frame overlay windows per display"
```

### Task 6 incelemesinden gelen ek kurallar

Bu kurallar yukarıdaki kod bloklarını geçersiz kılar; çakışma olursa bunlar geçerlidir.

**1. Yakalama ve kodlama ana iş parçacığından çıkar, pencere kurulumu ana iş parçacığında kalır.**
`request_capture` bir işçi iş parçacığı başlatır: `displays()`, her ekran için `capture()` ve `save_png()`
orada koşar ve `Vec<(DisplayInfo, PathBuf)>` toplar. Sonuç `app.run_on_main_thread(...)` ile ana iş
parçacığına verilir; pencereler orada kurulur, çünkü macOS pencere oluşturmayı ana iş parçacığında
zorunlu tutar. Ölçüm: tek ekranda 1.4-1.7 sn donma, %78'i PNG kodlaması. Bölünmüş halde ana iş
parçacığında kalan iş ~100-260 ms olur ve ekran sayısından bağımsızdır.

**2. Pencere görünmez kurulur, arka planı yüklendikten sonra gösterilir.** Aksi halde kullanıcı,
donmuş kare boyanana kadar tamamen şeffaf, tıklama yutan, her zaman üstte bir dikdörtgenle baş başa
kalır ve altındaki ekran hâlâ hareket eder; bu, özelliğin var oluş sebebi olan donmayı ortadan kaldırır.
`.visible(false)` ile kurulur, görüntü yüklendiğinde frontend pencereyi gösterir.

**3. Eski overlay'ler yakalama döngüsünden ÖNCE, tek seferde kapatılır.** Mevcut "varsa kapat, sonra
kur" koruması ölü koddur: `WebviewWindow::close()` olay döngüsü proxy'sine kuyruklanır, ana iş parçacığı
bloklu olduğu için döngü içinde işlenemez, `build()` `WindowLabelAlreadyExists` ile düşer ve `?` tüm
işlemi iptal eder. Sonuç: ilk yakalama çalışır, ikincisi sessizce başarısız olur, üçüncüsü çalışır.
Ayrıca eski overlay kapanmadan yakalama yapıldığı için yeni donmuş kare eskisini içine gömer.

**4. Pencere seviyesi menü çubuğunun üstüne çıkarılır ve tüm Space'lere katılır.** `always_on_top`
pencereyi CGWindowLevel 5'e koyar; Dock 20, menü çubuğu 24'tedir, yani ikisi de donmuş karenin üstüne
çizilir ve ekranın üst/alt şeritleri seçilemez hale gelir. `ns_window()` üzerinden seviye menü çubuğunun
üstüne alınır, `collectionBehavior`'a `canJoinAllSpaces | fullScreenAuxiliary` eklenir; ikincisi olmadan
overlay tam ekran Space'lerin üstünde hiç görünmez.

**5. Donmuş karenin yolu Rust'tan sorgu parametresiyle gelir.** `overlay.html?...&path=<mutlak yol>`
şeklinde; frontend yalnızca `convertFileSrc` çağırır. Bu, `appCacheDir()` + `join()` IPC gidiş
dönüşlerini, onlar için gereken `capabilities/overlay.json` dosyasını ve dosya adı şablonunun
TypeScript'te ikinci kez yazılmasını birden ortadan kaldırır. `<img>` etiketine `onError` eklenir,
yoksa yol bozulduğunda tanısı olmayan boş bir pencere kalır.

**6. `save_png` sıkıştırma seviyesini parametre alır.** Arka plan `CompressionType::Fast` +
`FilterType::NoFilter` ile yazılır (30 MB RGBA için ~1.1 sn yerine ~150-250 ms); Task 10'un kullanıcıya
kaydettiği dosya varsayılan sıkıştırmayı kullanır, çünkü orada dosya boyutu gerçek bir maliyettir.
Sıkıştırma kayıpsızdır, yalnızca deflate akışını değiştirir, bu yüzden Task 9'un renk seçicisi etkilenmez.

**7. İzin reddi kullanıcıya görünür olmalıdır.** `eprintln!`, `LSUIElement` uygulamasında ve
`"windows": []` yapılandırmasında kimseye ulaşmaz; `get_webview_window("main")` her zaman `None` döner.
Preflight izni yok derse `request_screen_capture_permission()` çağrılır (ilk seferde sistem sorar), hâlâ
reddediliyorsa `SETTINGS_DEEP_LINK` açılır. Böylece kullanıcı sessiz bir hiçlik yerine ya sistem
sorusunu ya da ilgili ayar panelini görür.

**8. `save_png` test edilir.** Bilinen bir 2x2 BGRA frame geçici bir dizine yazılır ve çözülen baytlar
doğrulanır. Ekran veya izin gerektirmez. Bu fonksiyon hem arka planı hem kullanıcının kaydettiği
dosyayı üretir, yani bir bayt sırası veya stride hatası ikisini birden sessizce bozar.

### Task 6 ikinci incelemesinden gelen ek kurallar

**9. Aynı anda tek yakalama.** `open_overlays` sınırsız işçi başlatıyor ve hiçbir koruma yok. Hızlı iki
basışta iki işçi aynı `frozen-<id>.png` dosyasını sıfır offsetten yazar (iç içe geçmiş baytlar, bozuk
PNG), ikinci işçi `WindowLabelAlreadyExists` alır, hata yolu tüm overlay'leri kapatır ve kullanıcı iki
basış sonunda **hiç** overlay göremez. Bu, yerine geçtiği hatadan daha kötüdür ve ekranda 1.5 sn hiçbir
şey görünmediği için kullanıcının doğal davranışıdır. `AppState` içinde bir bayrak tutulur; uçuşta bir
yakalama varken gelen istek düşürülür. Bayrak, panik ve erken dönüşlerde de temizlenmesi için bir guard
tipiyle (Drop) yönetilir.

**10. Kodlama süresi release build'de ölçülür, sonra format seçilir.** Rule 6 yanlış bir varsayıma
dayanıyordu: `image` crate'inin `CompressionType` varsayılanı zaten `Fast`, `FilterType` varsayılanı
`Adaptive`. Yani tek gerçek değişiklik Adaptive'den NoFilter'a geçmekti ve bu, kodlamayı 1.10-1.17
sn'den 1.37-1.80 sn'ye **yavaşlattı**. Üstelik tüm ölçümler debug build'de yapıldı. Önce release
build'de gerçek 3420x2224 kare üzerinde Adaptive ve NoFilter ölçülür. Hedef, kısayoldan overlay'in
ekrana gelmesine kadar geçen süredir; arka plan geçici bir dosyadır, PNG olmak zorunda değildir.
Kayıpsız kalmak şartıyla (Task 9 renk seçici) sıkıştırmasız bir format da meşrudur; hangisi seçilirse
ölçümüyle birlikte raporlanır.

**11. Görüntü yüklenemezse pencere sonsuza kadar gizli kalmaz.** `show()` yalnızca `onLoad` yolundan
çağrılıyor; `onError` durumunda pencere ne gösteriliyor ne kapatılıyor, pencere haritasında hayalet
olarak kalıyor ve `LSUIElement` sürümünde konsol da olmadığı için hiçbir iz kalmıyor. Bekleme
sınırlandırılır (yaklaşık 500 ms) ve süre dolduğunda pencere kapatılır; `show()` zincirine sonlandırıcı
bir `catch` eklenir.

**12. Boşaltma zaman aşımı yakalamayı iptal eder.** Eski overlay kapanmadıysa işçi devam etmemelidir;
aksi halde yeni donmuş kare eski overlay'i içine gömer ve `build()` zaten etiket çakışmasıyla düşer.

**13. Donmuş kareler overlay kapanınca silinir.** Kullanıcının ekranının tam çözünürlüklü kopyaları
`~/Library/Caches` altında süresiz birikmemelidir; bu bir gizlilik özelliğidir, bilinçli karar verilir.

**14. Tam ekran Space denemesi sınırlıdır.** `CanJoinAllSpaces | FullScreenAuxiliary` tek başına
yetmedi. İki hipotez denenir: (a) `collectionBehavior` ayarı ve pencerenin öne alınması aynı ana iş
parçacığı turunda yapılır (rule 2 `show()`'u yüzlerce ms sonraya taşıdı, Space ataması pencere ilk öne
alındığında belirlenir), (b) `LSUIElement` uygulamasında `NSApp.activate` ile birlikte öne alınır.
İkisi de tutmazsa bu, README'de yazılı bilinen bir sınırlama olarak kabul edilir ve daha fazla
zorlanmaz.

---

### Task 7: Seçim geometrisi ve overlay arayüzü

**Files:**
- Create: `apps/desktop/src/overlay/selection.ts`, `apps/desktop/src/overlay/selection.test.ts`
- Modify: `apps/desktop/src/overlay/Overlay.tsx`

**Interfaces:**
- Consumes: Task 6'daki overlay penceresi ve URL parametreleri.
- Produces:
  - `type Point = { x: number; y: number }`
  - `type Rect = { x: number; y: number; width: number; height: number }`
  - `type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w'`
  - `normalizeRect(from: Point, to: Point): Rect`
  - `clampRect(rect: Rect, bounds: Rect): Rect`
  - `nudgeRect(rect: Rect, dx: number, dy: number, bounds: Rect): Rect`
  - `resizeRect(rect: Rect, handle: Handle, pointer: Point, bounds: Rect): Rect`
  - `isUsable(rect: Rect): boolean` (kenarı 4 pikselden küçük seçimleri eler)

- [ ] **Step 1: Başarısız testi yaz**

`apps/desktop/src/overlay/selection.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { clampRect, isUsable, normalizeRect, nudgeRect, resizeRect } from './selection'

const bounds = { x: 0, y: 0, width: 1000, height: 800 }

describe('normalizeRect', () => {
  it('builds a positive rect when dragging down-right', () => {
    expect(normalizeRect({ x: 10, y: 20 }, { x: 110, y: 70 })).toEqual({
      x: 10, y: 20, width: 100, height: 50,
    })
  })

  it('builds a positive rect when dragging up-left', () => {
    expect(normalizeRect({ x: 110, y: 70 }, { x: 10, y: 20 })).toEqual({
      x: 10, y: 20, width: 100, height: 50,
    })
  })
})

describe('clampRect', () => {
  it('trims a rect that runs past the right edge', () => {
    expect(clampRect({ x: 950, y: 10, width: 100, height: 50 }, bounds)).toEqual({
      x: 950, y: 10, width: 50, height: 50,
    })
  })

  it('trims a rect that starts before the origin', () => {
    expect(clampRect({ x: -20, y: -10, width: 100, height: 50 }, bounds)).toEqual({
      x: 0, y: 0, width: 80, height: 40,
    })
  })
})

describe('nudgeRect', () => {
  it('moves the rect by the delta', () => {
    expect(nudgeRect({ x: 10, y: 10, width: 50, height: 50 }, 5, -5, bounds)).toEqual({
      x: 15, y: 5, width: 50, height: 50,
    })
  })

  it('stops at the bounds instead of moving partially outside', () => {
    expect(nudgeRect({ x: 960, y: 10, width: 40, height: 40 }, 10, 0, bounds)).toEqual({
      x: 960, y: 10, width: 40, height: 40,
    })
  })
})

describe('resizeRect', () => {
  it('moves the east edge to the pointer', () => {
    const rect = { x: 100, y: 100, width: 100, height: 100 }
    expect(resizeRect(rect, 'e', { x: 250, y: 150 }, bounds)).toEqual({
      x: 100, y: 100, width: 150, height: 100,
    })
  })

  it('keeps the rect positive when the pointer crosses the opposite edge', () => {
    const rect = { x: 100, y: 100, width: 100, height: 100 }
    expect(resizeRect(rect, 'e', { x: 60, y: 150 }, bounds)).toEqual({
      x: 60, y: 100, width: 40, height: 100,
    })
  })

  it('clamps a resize that leaves the display', () => {
    const rect = { x: 900, y: 100, width: 50, height: 50 }
    expect(resizeRect(rect, 'e', { x: 1200, y: 150 }, bounds)).toEqual({
      x: 900, y: 100, width: 100, height: 50,
    })
  })
})

describe('isUsable', () => {
  it('rejects an accidental click', () => {
    expect(isUsable({ x: 10, y: 10, width: 2, height: 2 })).toBe(false)
  })

  it('accepts a real selection', () => {
    expect(isUsable({ x: 10, y: 10, width: 40, height: 30 })).toBe(true)
  })
})
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `pnpm --filter @snapdeck/desktop test`
Expected: FAIL, `Failed to resolve import "./selection"`.

- [ ] **Step 3: Implementasyonu yaz**

`apps/desktop/src/overlay/selection.ts`:

```ts
export type Point = { x: number; y: number }
export type Rect = { x: number; y: number; width: number; height: number }
export type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w'

/** Smallest selection worth capturing, in CSS pixels. */
const MIN_EDGE = 4

/** Builds a positive-size rect from two drag points, in any direction. */
export function normalizeRect(from: Point, to: Point): Rect {
  return {
    x: Math.min(from.x, to.x),
    y: Math.min(from.y, to.y),
    width: Math.abs(to.x - from.x),
    height: Math.abs(to.y - from.y),
  }
}

/** Trims a rect so it stays inside `bounds`. */
export function clampRect(rect: Rect, bounds: Rect): Rect {
  const x = Math.max(rect.x, bounds.x)
  const y = Math.max(rect.y, bounds.y)
  const right = Math.min(rect.x + rect.width, bounds.x + bounds.width)
  const bottom = Math.min(rect.y + rect.height, bounds.y + bounds.height)
  return { x, y, width: Math.max(0, right - x), height: Math.max(0, bottom - y) }
}

/** Moves a rect without resizing it; a move that would leave `bounds` is refused. */
export function nudgeRect(rect: Rect, dx: number, dy: number, bounds: Rect): Rect {
  const moved = { ...rect, x: rect.x + dx, y: rect.y + dy }
  const fits =
    moved.x >= bounds.x &&
    moved.y >= bounds.y &&
    moved.x + moved.width <= bounds.x + bounds.width &&
    moved.y + moved.height <= bounds.y + bounds.height
  return fits ? moved : rect
}

/** Drags one edge or corner to the pointer, staying positive and inside bounds. */
export function resizeRect(rect: Rect, handle: Handle, pointer: Point, bounds: Rect): Rect {
  let left = rect.x
  let top = rect.y
  let right = rect.x + rect.width
  let bottom = rect.y + rect.height

  if (handle.includes('w')) left = pointer.x
  if (handle.includes('e')) right = pointer.x
  if (handle.includes('n')) top = pointer.y
  if (handle.includes('s')) bottom = pointer.y

  const normalized = normalizeRect({ x: left, y: top }, { x: right, y: bottom })
  return clampRect(normalized, bounds)
}

export function isUsable(rect: Rect): boolean {
  return rect.width >= MIN_EDGE && rect.height >= MIN_EDGE
}
```

- [ ] **Step 4: Testlerin geçtiğini doğrula**

Run: `pnpm --filter @snapdeck/desktop test`
Expected: PASS, 11 test.

- [ ] **Step 5: Overlay arayüzünü bağla**

`apps/desktop/src/overlay/Overlay.tsx` tam içeriği:

```tsx
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { appCacheDir, join } from '@tauri-apps/api/path'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCallback, useEffect, useRef, useState } from 'react'
import { clampRect, isUsable, normalizeRect, nudgeRect, type Point, type Rect } from './selection'

type Props = { displayId: number; mode: string; scale: number }

export function Overlay({ displayId, scale }: Props) {
  const [frozenSrc, setFrozenSrc] = useState<string | null>(null)
  const [selection, setSelection] = useState<Rect | null>(null)
  const dragStart = useRef<Point | null>(null)
  const bounds = { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }

  useEffect(() => {
    void (async () => {
      const path = await join(await appCacheDir(), `frozen-${displayId}.png`)
      setFrozenSrc(convertFileSrc(path))
    })()
  }, [displayId])

  const cancel = useCallback(() => {
    void invoke('close_overlays')
  }, [])

  const confirm = useCallback(
    (rect: Rect) => {
      if (!isUsable(rect)) return cancel()
      void invoke('capture_region', { displayId, rect })
    },
    [cancel, displayId, scale],
  )

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') return cancel()
      if (!selection) return
      if (event.key === 'Enter') return confirm(selection)
      const step = event.shiftKey ? 10 : 1
      const deltas: Record<string, [number, number]> = {
        ArrowLeft: [-step, 0],
        ArrowRight: [step, 0],
        ArrowUp: [0, -step],
        ArrowDown: [0, step],
      }
      const delta = deltas[event.key]
      if (delta) {
        event.preventDefault()
        setSelection(nudgeRect(selection, delta[0], delta[1], bounds))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  useEffect(() => {
    void getCurrentWindow().setFocus()
  }, [])

  const onPointerDown = (event: React.PointerEvent) => {
    dragStart.current = { x: event.clientX, y: event.clientY }
    setSelection({ x: event.clientX, y: event.clientY, width: 0, height: 0 })
  }

  const onPointerMove = (event: React.PointerEvent) => {
    if (!dragStart.current) return
    const rect = normalizeRect(dragStart.current, { x: event.clientX, y: event.clientY })
    setSelection(clampRect(rect, bounds))
  }

  const onPointerUp = () => {
    dragStart.current = null
    if (selection) confirm(selection)
  }

  return (
    <div
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      style={{ position: 'relative', width: '100%', height: '100%' }}
    >
      {frozenSrc && (
        <img src={frozenSrc} alt="" style={{ width: '100%', height: '100%', display: 'block' }} />
      )}
      <div style={{ position: 'absolute', inset: 0, background: 'rgba(0,0,0,0.35)' }} />
      {selection && (
        <>
          <div
            style={{
              position: 'absolute',
              left: selection.x,
              top: selection.y,
              width: selection.width,
              height: selection.height,
              boxShadow: '0 0 0 9999px rgba(0,0,0,0.35)',
              outline: '1px solid #fff',
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: selection.x,
              top: Math.max(0, selection.y - 24),
              padding: '2px 6px',
              background: '#000',
              color: '#fff',
              font: '12px ui-monospace, monospace',
              borderRadius: 4,
            }}
          >
            {Math.round(selection.width * scale)} × {Math.round(selection.height * scale)}
          </div>
        </>
      )}
    </div>
  )
}
```

Karartma iki katmanla değil tek `box-shadow` ile yapılır; ikinci `inset: 0` katmanı yalnızca seçim yokken görünür karartmayı sağlar. Seçim varken `box-shadow` onun üstüne biner.

- [ ] **Step 6: Kenar tutamaçlarını bağla**

Sürükleme bittikten sonra seçim, sekiz tutamaçtan biriyle yeniden boyutlandırılabilir. `Overlay.tsx` içine ekle:

```tsx
import { resizeRect, type Handle } from './selection'

const HANDLES: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']

function handlePosition(rect: Rect, handle: Handle): { left: number; top: number } {
  const left =
    handle.includes('w') ? rect.x : handle.includes('e') ? rect.x + rect.width : rect.x + rect.width / 2
  const top =
    handle.includes('n') ? rect.y : handle.includes('s') ? rect.y + rect.height : rect.y + rect.height / 2
  return { left, top }
}
```

Bileşen içinde aktif tutamağı tut ve pointer hareketinde onu uygula:

```tsx
const activeHandle = useRef<Handle | null>(null)

// onPointerMove içinde, dragStart kontrolünden önce:
if (activeHandle.current && selection) {
  setSelection(
    resizeRect(selection, activeHandle.current, { x: event.clientX, y: event.clientY }, bounds),
  )
  return
}
```

Seçim varken tutamakları çiz. `stopPropagation` şart, yoksa tutamağa basmak yeni bir sürükleme başlatır:

```tsx
{selection &&
  HANDLES.map((handle) => {
    const pos = handlePosition(selection, handle)
    return (
      <div
        key={handle}
        onPointerDown={(event) => {
          event.stopPropagation()
          activeHandle.current = handle
        }}
        style={{
          position: 'absolute',
          left: pos.left - 4,
          top: pos.top - 4,
          width: 8,
          height: 8,
          background: '#fff',
          border: '1px solid #000',
          cursor: `${handle}-resize`,
        }}
      />
    )
  })}
```

`onPointerUp` içinde `activeHandle.current` doluysa seçim onaylanmaz, sadece tutamak bırakılır:

```tsx
const onPointerUp = () => {
  if (activeHandle.current) {
    activeHandle.current = null
    return
  }
  dragStart.current = null
  if (selection) confirm(selection)
}
```

Yeniden boyutlandırıldıktan sonra seçim `Enter` ile onaylanır.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/overlay
git commit -m "feat(overlay): add selection geometry, drag-to-select, and resize handles"
```

---

### Task 8: Pencereye yapışma

**Files:**
- Create: `apps/desktop/src/overlay/snap.ts`, `apps/desktop/src/overlay/snap.test.ts`
- Modify: `apps/desktop/src/overlay/Overlay.tsx`

**Interfaces:**
- Consumes: Task 2'deki `WindowInfo` (frontend'e `list_windows` komutuyla gelir, Task 10), Task 7'deki `Rect`.
- Produces:
  - `type WindowBounds = { id: number; title: string | null; appName: string | null; bounds: Rect; layer: number }`
  - `windowUnderPoint(windows: WindowBounds[], point: Point): WindowBounds | null`
  - `toLocalRect(bounds: Rect, displayOrigin: Point): Rect`

- [ ] **Step 1: Başarısız testi yaz**

`apps/desktop/src/overlay/snap.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { toLocalRect, windowUnderPoint, type WindowBounds } from './snap'

const win = (id: number, x: number, y: number, w: number, h: number, layer = 0): WindowBounds => ({
  id,
  title: `w${id}`,
  appName: 'App',
  bounds: { x, y, width: w, height: h },
  layer,
})

describe('windowUnderPoint', () => {
  it('returns null when no window contains the point', () => {
    expect(windowUnderPoint([win(1, 0, 0, 10, 10)], { x: 500, y: 500 })).toBeNull()
  })

  it('returns the frontmost window when several overlap', () => {
    const windows = [win(1, 0, 0, 100, 100), win(2, 10, 10, 50, 50)]
    // The list is front-to-back, so the first match wins.
    expect(windowUnderPoint(windows, { x: 20, y: 20 })?.id).toBe(1)
  })

  it('ignores windows above the normal layer, such as the overlay itself', () => {
    const windows = [win(9, 0, 0, 100, 100, 25), win(1, 0, 0, 100, 100, 0)]
    expect(windowUnderPoint(windows, { x: 20, y: 20 })?.id).toBe(1)
  })
})

describe('toLocalRect', () => {
  it('converts global window bounds to display-local coordinates', () => {
    expect(toLocalRect({ x: 1500, y: 100, width: 200, height: 100 }, { x: 1440, y: 0 })).toEqual({
      x: 60, y: 100, width: 200, height: 100,
    })
  })
})
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `pnpm --filter @snapdeck/desktop test snap`
Expected: FAIL, `Failed to resolve import "./snap"`.

- [ ] **Step 3: Implementasyonu yaz**

`apps/desktop/src/overlay/snap.ts`:

```ts
import type { Point, Rect } from './selection'

export type WindowBounds = {
  id: number
  title: string | null
  appName: string | null
  bounds: Rect
  layer: number
}

/** Layers above this belong to system UI and our own overlay windows. */
const NORMAL_LAYER = 0

function contains(rect: Rect, point: Point): boolean {
  return (
    point.x >= rect.x &&
    point.y >= rect.y &&
    point.x < rect.x + rect.width &&
    point.y < rect.y + rect.height
  )
}

/**
 * Frontmost normal-layer window containing the point. The list is expected in
 * front-to-back order, which is what ScreenCaptureKit returns.
 */
export function windowUnderPoint(windows: WindowBounds[], point: Point): WindowBounds | null {
  return windows.find((w) => w.layer <= NORMAL_LAYER && contains(w.bounds, point)) ?? null
}

/** Rebases global (multi-display) coordinates onto one display's overlay. */
export function toLocalRect(bounds: Rect, displayOrigin: Point): Rect {
  return {
    x: bounds.x - displayOrigin.x,
    y: bounds.y - displayOrigin.y,
    width: bounds.width,
    height: bounds.height,
  }
}
```

- [ ] **Step 4: Testlerin geçtiğini doğrula**

Run: `pnpm --filter @snapdeck/desktop test`
Expected: PASS, 15 test (11 selection + 4 snap).

- [ ] **Step 5: Overlay'e bağla**

`Overlay.tsx` bileşen imzasını `({ displayId, mode, scale }: Props)` olacak şekilde güncelle (Task 7'de `mode` kullanılmıyordu). Sonra: `mode === 'window'` olduğunda sürükleme yerine, pointer hareketinde `windowUnderPoint` ile bulunan pencerenin `toLocalRect` ile yerelleştirilmiş dikdörtgeni vurgulanır, tıklamada `confirm` çağrılır. Pencere listesi mount sırasında bir kez alınır:

```tsx
const [windows, setWindows] = useState<WindowBounds[]>([])
const [displayOrigin, setDisplayOrigin] = useState<Point>({ x: 0, y: 0 })

useEffect(() => {
  if (mode !== 'window') return
  void (async () => {
    const result = await invoke<{ windows: WindowBounds[]; origin: Point }>('list_windows', {
      displayId,
    })
    setWindows(result.windows)
    setDisplayOrigin(result.origin)
  })()
}, [displayId, mode])
```

Pointer hareketinde:

```tsx
if (mode === 'window') {
  const hit = windowUnderPoint(windows, {
    x: event.clientX + displayOrigin.x,
    y: event.clientY + displayOrigin.y,
  })
  setSelection(hit ? clampRect(toLocalRect(hit.bounds, displayOrigin), bounds) : null)
  return
}
```

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/overlay
git commit -m "feat(overlay): snap selection to the window under the cursor"
```

---

### Task 9: Büyüteç ve renk seçici

**Files:**
- Create: `apps/desktop/src/overlay/magnifier.ts`, `apps/desktop/src/overlay/magnifier.test.ts`
- Modify: `apps/desktop/src/overlay/Overlay.tsx`

**Interfaces:**
- Consumes: Task 7'deki `Point`.
- Produces:
  - `type Rgba = { r: number; g: number; b: number; a: number }`
  - `samplePixel(data: Uint8ClampedArray, width: number, point: Point): Rgba | null`
  - `toHex(color: Rgba): string`
  - `magnifierSourceRect(point: Point, size: number, bounds: Rect): Rect`

- [ ] **Step 1: Başarısız testi yaz**

`apps/desktop/src/overlay/magnifier.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { magnifierSourceRect, samplePixel, toHex } from './magnifier'

// 2x1 RGBA image: red pixel, then green pixel.
const data = new Uint8ClampedArray([255, 0, 0, 255, 0, 128, 0, 255])

describe('samplePixel', () => {
  it('reads the pixel at the given point', () => {
    expect(samplePixel(data, 2, { x: 1, y: 0 })).toEqual({ r: 0, g: 128, b: 0, a: 255 })
  })

  it('returns null outside the buffer', () => {
    expect(samplePixel(data, 2, { x: 5, y: 0 })).toBeNull()
  })

  it('returns null for negative coordinates', () => {
    expect(samplePixel(data, 2, { x: -1, y: 0 })).toBeNull()
  })
})

describe('toHex', () => {
  it('formats an uppercase six-digit hex string', () => {
    expect(toHex({ r: 0, g: 128, b: 0, a: 255 })).toBe('#008000')
  })

  it('pads single-digit channels', () => {
    expect(toHex({ r: 1, g: 2, b: 3, a: 255 })).toBe('#010203')
  })
})

describe('magnifierSourceRect', () => {
  const bounds = { x: 0, y: 0, width: 100, height: 100 }

  it('centers the rect on the point', () => {
    expect(magnifierSourceRect({ x: 50, y: 50 }, 10, bounds)).toEqual({
      x: 45, y: 45, width: 10, height: 10,
    })
  })

  it('clamps at the top-left corner', () => {
    expect(magnifierSourceRect({ x: 1, y: 1 }, 10, bounds)).toEqual({
      x: 0, y: 0, width: 10, height: 10,
    })
  })

  it('clamps at the bottom-right corner', () => {
    expect(magnifierSourceRect({ x: 99, y: 99 }, 10, bounds)).toEqual({
      x: 90, y: 90, width: 10, height: 10,
    })
  })
})
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `pnpm --filter @snapdeck/desktop test magnifier`
Expected: FAIL, `Failed to resolve import "./magnifier"`.

- [ ] **Step 3: Implementasyonu yaz**

`apps/desktop/src/overlay/magnifier.ts`:

```ts
import type { Point, Rect } from './selection'

export type Rgba = { r: number; g: number; b: number; a: number }

/** Reads one RGBA pixel from a canvas buffer, or null when out of range. */
export function samplePixel(
  data: Uint8ClampedArray,
  width: number,
  point: Point,
): Rgba | null {
  const x = Math.floor(point.x)
  const y = Math.floor(point.y)
  if (x < 0 || y < 0 || x >= width) return null
  const offset = (y * width + x) * 4
  if (offset < 0 || offset + 3 >= data.length) return null
  return { r: data[offset]!, g: data[offset + 1]!, b: data[offset + 2]!, a: data[offset + 3]! }
}

export function toHex(color: Rgba): string {
  const channel = (value: number) => value.toString(16).padStart(2, '0').toUpperCase()
  return `#${channel(color.r)}${channel(color.g)}${channel(color.b)}`
}

/** Square source region for the magnifier, kept fully inside `bounds`. */
export function magnifierSourceRect(point: Point, size: number, bounds: Rect): Rect {
  const half = size / 2
  const x = Math.min(Math.max(point.x - half, bounds.x), bounds.x + bounds.width - size)
  const y = Math.min(Math.max(point.y - half, bounds.y), bounds.y + bounds.height - size)
  return { x, y, width: size, height: size }
}
```

Not: `toHex` çıktısı büyük harflidir; test bunu doğrular. `#008000` gibi.

- [ ] **Step 4: Testlerin geçtiğini doğrula**

Run: `pnpm --filter @snapdeck/desktop test`
Expected: PASS, 23 test.

- [ ] **Step 5: Overlay'e bağla**

Donmuş kare bir kez offscreen canvas'a çizilir ve piksel buffer'ı okunur:

```tsx
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { magnifierSourceRect, samplePixel, toHex, type Rgba } from './magnifier'

const pixels = useRef<{ data: Uint8ClampedArray; width: number } | null>(null)
const [hoverColor, setHoverColor] = useState<Rgba | null>(null)

useEffect(() => {
  if (!frozenSrc) return
  const image = new Image()
  image.src = frozenSrc
  image.onload = () => {
    const canvas = document.createElement('canvas')
    canvas.width = image.naturalWidth
    canvas.height = image.naturalHeight
    const context = canvas.getContext('2d', { willReadFrequently: true })
    if (!context) return
    context.drawImage(image, 0, 0)
    const buffer = context.getImageData(0, 0, canvas.width, canvas.height)
    pixels.current = { data: buffer.data, width: canvas.width }
  }
}, [frozenSrc])
```

Pointer hareketinde imlecin altındaki renk okunur. Donmuş kare piksel, imleç ise nokta biriminde
olduğu için koordinat `scale` ile çarpılır:

```tsx
if (pixels.current) {
  setHoverColor(
    samplePixel(pixels.current.data, pixels.current.width, {
      x: event.clientX * scale,
      y: event.clientY * scale,
    }),
  )
}
```

Büyüteç, imlecin yanında 8x ölçekli ve `imageRendering: 'pixelated'` olarak çizilir; kaynak bölge
`magnifierSourceRect({ x: event.clientX, y: event.clientY }, 16, bounds)` ile hesaplanır. Hex kodu
büyütecin altında gösterilir ve `C` tuşuyla panoya yazılır:

```tsx
if (event.key.toLowerCase() === 'c' && hoverColor) {
  void writeText(toHex(hoverColor))
}
```

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/overlay
git commit -m "feat(overlay): add magnifier with pixel color readout"
```

---

### Task 10: Yakalama komutları ve çıktı yolu

**Files:**
- Create: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/output.rs`, `apps/desktop/src-tauri/src/lib.rs`, `README.md`
- Test: `apps/desktop/src-tauri/src/output.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 2'deki `Frame`, `Rect`, `CaptureTarget`; Task 3'teki `MacCapturer`; Task 4'teki izin fonksiyonları; Task 5'teki `AppState`; Task 6'daki `close_overlays` ve `save_png`.
- Produces:
  - `render_filename(template: &str, at: OffsetDateTimeParts, width: u32, height: u32) -> String`
  - `struct OffsetDateTimeParts { year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8 }`, `OffsetDateTimeParts::now()`
  - Tauri komutları: `permission_state`, `request_permission`, `list_windows`, `capture_region`, `close_overlays`

- [ ] **Step 1: Başarısız testi yaz**

`apps/desktop/src-tauri/src/output.rs` sonuna:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> OffsetDateTimeParts {
        OffsetDateTimeParts { year: 2026, month: 9, day: 7, hour: 4, minute: 5, second: 6 }
    }

    #[test]
    fn renders_the_default_template() {
        let name = render_filename("Snapdeck {date} at {time}", parts(), 800, 600);
        assert_eq!(name, "Snapdeck 2026-09-07 at 04.05.06");
    }

    #[test]
    fn renders_size_tokens() {
        let name = render_filename("shot-{width}x{height}", parts(), 800, 600);
        assert_eq!(name, "shot-800x600");
    }

    #[test]
    fn leaves_unknown_tokens_untouched() {
        let name = render_filename("{nope}-{width}", parts(), 10, 20);
        assert_eq!(name, "{nope}-10");
    }

    #[test]
    fn strips_path_separators_from_the_result() {
        let name = render_filename("a/b{width}", parts(), 5, 5);
        assert_eq!(name, "a-b5");
    }
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p snapdeck output`
Expected: FAIL, `cannot find function render_filename in this scope`.

- [ ] **Step 3: Implementasyonu yaz**

`apps/desktop/src-tauri/src/output.rs` içine, mevcut `save_png` fonksiyonunun altına ve test modülünün üstüne:

```rust
/// Wall-clock parts used by the filename template. Kept as plain fields so the
/// renderer is pure and testable without a clock.
#[derive(Debug, Clone, Copy)]
pub struct OffsetDateTimeParts {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl OffsetDateTimeParts {
    /// Current local time, read from the system clock.
    pub fn now() -> Self {
        // std has no calendar math, so derive the parts from the Unix epoch.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let days = secs.div_euclid(86_400);
        let time_of_day = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (time_of_day / 3600) as u8,
            minute: ((time_of_day % 3600) / 60) as u8,
            second: (time_of_day % 60) as u8,
        }
    }
}

/// Howard Hinnant's days-from-civil inverse; converts a Unix day number to a date.
fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Expands `{date}`, `{time}`, `{width}` and `{height}` in a filename template.
/// Unknown tokens are left as-is. Path separators are replaced with hyphens so
/// the result is always a single filename.
pub fn render_filename(
    template: &str,
    at: OffsetDateTimeParts,
    width: u32,
    height: u32,
) -> String {
    let date = format!("{:04}-{:02}-{:02}", at.year, at.month, at.day);
    // Colons are not usable in macOS filenames, so time uses dots.
    let time = format!("{:02}.{:02}.{:02}", at.hour, at.minute, at.second);
    template
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{width}", &width.to_string())
        .replace("{height}", &height.to_string())
        .replace(['/', '\\'], "-")
}
```

`apps/desktop/src-tauri/src/commands.rs`:

```rust
use serde::Serialize;
use snapdeck_capture::{
    macos::permission::{
        request_screen_capture_permission, screen_capture_permission, PermissionState,
        SETTINGS_DEEP_LINK,
    },
    CaptureTarget, Rect, ScreenCapturer,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    output::{render_filename, save_png, OffsetDateTimeParts},
    overlay,
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReport {
    pub state: PermissionState,
    pub settings_url: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowListItem {
    pub id: u32,
    pub title: Option<String>,
    pub app_name: Option<String>,
    pub bounds: Rect,
    pub layer: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowListResult {
    pub windows: Vec<WindowListItem>,
    pub origin: Origin,
}

#[derive(Serialize)]
pub struct Origin {
    pub x: f64,
    pub y: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[tauri::command]
pub fn permission_state() -> PermissionReport {
    PermissionReport { state: screen_capture_permission(), settings_url: SETTINGS_DEEP_LINK }
}

#[tauri::command]
pub fn request_permission() -> PermissionReport {
    PermissionReport {
        state: request_screen_capture_permission(),
        settings_url: SETTINGS_DEEP_LINK,
    }
}

#[tauri::command]
pub fn list_windows(
    app: AppHandle,
    display_id: u32,
) -> Result<WindowListResult, String> {
    let state = app.state::<AppState>();
    let origin = state
        .capturer
        .displays()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.id == display_id)
        .map(|d| Origin { x: d.bounds.x, y: d.bounds.y })
        .ok_or_else(|| format!("display {display_id} not found"))?;

    let windows = state
        .capturer
        .windows()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|w| WindowListItem {
            id: w.id,
            title: w.title,
            app_name: w.app_name,
            bounds: w.bounds,
            layer: w.layer,
        })
        .collect();

    Ok(WindowListResult { windows, origin })
}

/// Captures the selected region again at native resolution, saves it, and
/// copies it to the clipboard. The frozen overlay image is never used as the
/// final pixels; it only served as the selection backdrop.
#[tauri::command]
pub fn capture_region(
    app: AppHandle,
    display_id: u32,
    rect: Rect,
) -> Result<CaptureResult, String> {
    overlay::close_overlays(&app);

    let state = app.state::<AppState>();
    let display = state
        .capturer
        .displays()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.id == display_id)
        .ok_or_else(|| format!("display {display_id} not found"))?;

    // The overlay reports display-local points; capture expects global points.
    let global = Rect {
        x: display.bounds.x + rect.x,
        y: display.bounds.y + rect.y,
        width: rect.width,
        height: rect.height,
    };

    let frame = state
        .capturer
        .capture(CaptureTarget::Region(global))
        .map_err(|e| e.to_string())?;

    let dir = app
        .path()
        .picture_dir()
        .map_err(|e| format!("no pictures dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = render_filename(
        "Snapdeck {date} at {time}",
        OffsetDateTimeParts::now(),
        frame.width,
        frame.height,
    );
    let path = dir.join(format!("{name}.png"));
    save_png(&frame, &path)?;

    let rgba = frame.to_rgba8().map_err(|e| e.to_string())?;
    app.clipboard()
        .write_image(&tauri::image::Image::new(&rgba, frame.width, frame.height))
        .map_err(|e| format!("failed to copy to clipboard: {e}"))?;

    Ok(CaptureResult {
        path: path.to_string_lossy().to_string(),
        width: frame.width,
        height: frame.height,
    })
}

#[tauri::command]
pub fn close_overlays(app: AppHandle) {
    overlay::close_overlays(&app);
}
```

`lib.rs` içine `mod commands;` bildirimini ekle ve `.manage(...)` çağrısından sonra komutları kaydet:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::permission_state,
            commands::request_permission,
            commands::list_windows,
            commands::capture_region,
            commands::close_overlays,
        ])
```

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test --workspace`
Expected: PASS. Tüm Rust testleri geçer.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: uyarı yok.

- [ ] **Step 5: Uçtan uca elle doğrula**

Run: `pnpm tauri dev`

Sırayla doğrula:
1. `CmdOrCtrl+Shift+7` bas, ekran donar.
2. Bir bölge seç, fareyi bırak. Overlay kapanır.
3. `~/Pictures` altında `Snapdeck <tarih> at <saat>.png` dosyası oluşur.
4. Dosyanın piksel boyutu, seçtiğin nokta boyutunun `scale_factor` katıdır (Retina'da 2x).
5. Bir metin düzenleyiciye yapıştır, görüntü panodan gelir.
6. Sistem Ayarları'ndan ekran kaydı iznini kaldır, kısayola tekrar bas: overlay açılmaz, ana pencere görünür ve izin yönlendirmesi gösterilir. Boş veya siyah bir dosya **oluşmamalıdır**.

- [ ] **Step 6: README'yi tamamla**

`README.md` içine kullanım bölümü ekle: varsayılan kısayollar (`Cmd+Shift+7` bölge, `Cmd+Shift+8` pencere, `Cmd+Shift+9` tam ekran), çıktının `~/Pictures` altına kaydedildiği ve panoya kopyalandığı, `Esc` ile iptal, ok tuşlarıyla ince ayar, `C` ile imleç altındaki rengin kopyalanması.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop README.md
git commit -m "feat(app): capture selected region to file and clipboard"
```

---

## Bu planın dışı

Ayrı planlar halinde yazılacak, her biri kendi başına çalışan yazılım üretir:

- **Plan 2, Editör:** `packages/editor`, katmanlı annotation modeli, undo/redo, blur/redaction düzleştirme, dışa aktarma. Bu planın `capture_region` komutu, kaydetmek yerine editörü açacak şekilde değiştirilecek.
- **Plan 3, Full-page:** `apps/extension`, `packages/protocol`, loopback WebSocket köprüsü, `crates/stitch` ve masaüstü otomatik kaydırma fallback'i.
- **Plan 4, Sürüm:** ayarlar arayüzü, kısayol yeniden atama, `tauri-plugin-updater`, imzalama ve notarization, ilk release.
