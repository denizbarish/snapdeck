# Contributing to Snapdeck

Thanks for looking. Snapdeck is a small macOS app, and everything below is written so that you can
get from a clean clone to a green build without guessing.

Please read [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) before taking part, and
[`SECURITY.md`](SECURITY.md) before reporting anything that looks like a vulnerability. A security
problem does not go in a public issue.

## Setting up

Snapdeck builds on macOS only. The capture core talks to ScreenCaptureKit, and the bundle declares
a minimum system version of 14.0 in `apps/desktop/src-tauri/tauri.conf.json`, so a Mac running
macOS 14 or later is what you need.

| Tool | Version | Where it is written down |
| --- | --- | --- |
| Rust | stable, 1.85 or later | `rust-version` in `Cargo.toml` |
| Node.js | 24 | `node-version` in `.github/workflows/ci.yml` |
| pnpm | 11.0.8 | `packageManager` in `package.json` |

There is no `rust-toolchain.toml`, so whatever stable toolchain you have is the one that is used.
CI installs stable through `dtolnay/rust-toolchain@stable`.

```bash
pnpm install
pnpm tauri dev
```

**`pnpm install` downloads a browser.** The root `prepare` script runs
`pnpm --filter @snapdeck/editor exec playwright install chromium`, which fetches a Chromium build of
roughly 150 MB on a first install and caches it afterwards. That download is what makes `pnpm test`
work from a clean clone: the editor's renderer, export and component tests draw on a real canvas and
measure the pixels that came back, so they run in Vitest browser mode against a real browser engine
rather than a Node canvas shim. The first install needs network access; later ones use the cache.

Snapdeck is a menu bar app. `pnpm tauri dev` opens no window and puts nothing in the Dock: look for
the icon in the menu bar. The first capture asks for Screen Recording permission, and you have to
quit and reopen the app after granting it, for the reason the [README](README.md#getting-started)
gives.

## The quality gates

Six commands. CI runs all six on every pull request, in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml), and a change is not finished until each one
passes locally.

| Gate | What it covers |
| --- | --- |
| `cargo fmt --all --check` | rustfmt, with its default settings. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Every warning is an error, tests and examples included. |
| `cargo test --workspace` | The Rust tests in both crates. |
| `pnpm lint` | `tsc --noEmit` in each package. There is no ESLint here; the type checker is the linter. |
| `pnpm test` | Vitest in `packages/editor` (a Node project and a browser project) and in `apps/desktop`. |
| `pnpm build` | The Vite bundle the app actually loads, plus `tsc --noEmit` in `apps/desktop`. |

CI adds one build that is not a gate you have to run by hand:
`cargo check -p snapdeck --features tauri/custom-protocol`. That feature is what embeds the frontend,
so it is the only configuration that reads `dist/`, the CSP and the asset scope, and it needs
`pnpm build` to have run first.

Two tests are marked `#[ignore]` because they need a real screen and a real Screen Recording grant.
Run them by hand when you touch the ScreenCaptureKit path:

```bash
cargo test -p snapdeck-capture -- --ignored
```

The Rust job runs on `macos-latest` and the web job on `ubuntu-latest`. The runner image is not the
app's runtime baseline: `screencapturekit` pulls in a Swift bridge that needs a newer Metal SDK than
the `macos-14` image ships, while the app itself still targets macOS 14.0.

## The three parts

Snapdeck is one workspace with three pieces, and which one you open depends on what you are
changing.

**`crates/capture`** (`snapdeck-capture`) is the capture core. It defines the `ScreenCapturer`
trait, the geometry and frame types the rest of the project speaks in, and a `mock` implementation
for tests. The macOS implementation behind `cfg(target_os = "macos")` is the only platform there is
so far. It knows nothing about Tauri, about windows or about files: it hands back a `Frame` and the
scale factor that frame was captured at. Every method blocks on a system round trip, so callers keep
it off the UI thread.

**`packages/editor`** (`@snapdeck/editor`) is the annotation editor as a library, and it is
deliberately free of Tauri. Below the `Editor` component there is no React and no DOM beyond the
canvas 2D API: the layer model, the undo stack, the hit testing, the renderer and the export are
plain TypeScript. `Editor` itself is a React component that knows nothing about its host, because
saving and copying leave through its props as a `Blob`. That boundary is what lets a browser
extension reuse the same editor later.

**`apps/desktop`** (`@snapdeck/desktop`, and the `snapdeck` crate under `src-tauri`) is the Tauri
shell and the only part that knows this is a Mac app. Rust owns the tray, the global shortcuts, the
overlay windows, the settings file, the file writing and the updater; each webview gets a narrow ACL
capability in `src-tauri/capabilities/`. The React side is three entry points, `overlay.html`,
`editor.html` and `settings.html`, each a thin host around code that lives elsewhere.

The dependency arrow points one way: `apps/desktop` depends on both of the others, and neither of
them depends on it.

## Commits

This repository follows Conventional Commits, with the scope naming the part that changed:

```
feat(app): check for updates from GitHub Releases
fix(editor): open the obscure tool in blackout
fix(overlay): stop WebKit tinting the frozen frame during a drag
ci: build and publish a release from a tag
docs: document the editor
```

The types in use are `feat`, `fix`, `ci`, `docs` and `chore`. The scopes in use are `app`, `editor`
and `overlay`; `ci`, `docs` and `chore` usually carry none.

The subject line is lowercase after the colon, describes what the change does rather than what you
did, and takes no full stop.

**The body is where the work is.** Commits here are wrapped at 72 columns and explain why the change
looks the way it does: what the old behaviour was, what it cost, what was measured, and what was
rejected along the way. Read `git log` for the house style. A one-line commit is fine for a typo and
wrong for anything with a reason behind it.

## Pull requests

Open an issue first for anything larger than a fix, so that the shape of it can be agreed before you
write it.

- One topic per pull request. A change that is easy to describe in a sentence is easy to review.
- All six gates green.
- New behaviour comes with a test. If it cannot be tested, say why in the body.
- Update the docs the change makes wrong. The README describes what the app does today, and
  `docs/RELEASING.md` describes how a release is cut.
- Do not promise features that do not exist. Screen recording and full-page scrolling capture are
  both unbuilt, and neither should appear in the docs as though it works.

[`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md) is the checklist you will be
handed when you open one.

## Releases

Releases are cut by pushing a `v*` tag and are described in [`docs/RELEASING.md`](docs/RELEASING.md).
Contributors do not need to do anything for a release beyond keeping the gates green.
