# Releasing

A release is cut by pushing a `v*` tag. [`.github/workflows/release.yml`](../.github/workflows/release.yml)
builds the app on `macos-latest`, signs it if it can, and opens a **draft** GitHub release with the
DMG, an `.app.tar.gz` and their SHA-256 checksums. Nothing becomes public until a human presses
Publish.

## Cutting a release

1. Make sure CI is green on the commit you are about to tag. The release workflow builds, it does
   not re-run `cargo fmt`, `clippy`, the Rust tests, `pnpm lint` or `pnpm test`.
2. Set the new version in **both** files, in the same commit:
   - `Cargo.toml`, under `[workspace.package]`
   - `apps/desktop/src-tauri/tauri.conf.json`, the top-level `version`
3. Tag and push:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. Watch the run: `gh run watch` or the Actions tab.
5. Open the draft release. Check the artifact names, the checksums and the signing line in the
   notes, then Publish.

### If it goes wrong

Nothing is public until Publish, so a bad build is undone by deleting the draft release and the
tag:

```bash
gh release delete v0.2.0 --yes
git push --delete origin v0.2.0
git tag -d v0.2.0
```

Re-pushing the same tag re-runs the workflow. It updates the existing release's notes and assets
instead of creating a second one, and it does not un-publish a release that has already been
published.

## The version check

The first job compares three things and fails before the build starts if any of them disagree:

| Source | What it decides |
| --- | --- |
| the tag, minus its `v` | what the release claims to be |
| `apps/desktop/src-tauri/tauri.conf.json` `version` | the version the app reports, and the version in the bundle filenames |
| `Cargo.toml` `[workspace.package] version` | the version the binary is built as |

It also fails if `apps/desktop/src-tauri/Cargo.toml` stops inheriting the workspace version, because
then the check would be reading a file that no longer decides anything.

This is a hard failure on purpose. A DMG named `Snapdeck_0.1.0` hanging off a `v0.2.0` tag is the
kind of mismatch nobody notices until something refuses to line up with its own name.

## Proving the workflow without releasing

The workflow has a `workflow_dispatch` entry point. It runs the whole thing, version check
included, but publishes nothing: the bundles come back as workflow artifacts on the run.

```bash
gh workflow run release.yml --ref main -f tag=v0.2.0
```

The `tag` input is what the version check compares the manifests against, so passing a version that
does not exist in them is also how you confirm the check still fails.

`workflow_dispatch` only works from a branch where GitHub can see the workflow, which means the
workflow file has to be on the default branch. On a branch where it is not yet, verify with a
throwaway tag instead and delete it and its draft release afterwards.

## Secrets

Signing and notarization are optional. When the six secrets below are all set, the workflow hands
them to `tauri build`, which signs the bundle with the certificate and sends it to Apple for
notarization. When any of them is missing, that step is skipped, the build produces an ad-hoc
signed bundle, and the release notes say so. **The absence of these secrets is not an error.**

| Secret | What it does |
| --- | --- |
| `APPLE_CERTIFICATE` | The Developer ID Application certificate and its private key, exported as a `.p12` and base64-encoded. Tauri imports it into a temporary keychain. |
| `APPLE_CERTIFICATE_PASSWORD` | The password the `.p12` was exported with. |
| `APPLE_SIGNING_IDENTITY` | The identity to sign with, e.g. `Developer ID Application: Some Name (TEAMID)`. It has to match the certificate above or the build fails. |
| `APPLE_ID` | The Apple Account used for notarization. |
| `APPLE_PASSWORD` | An app-specific password for that account, not the account password. |
| `APPLE_TEAM_ID` | The ten-character team identifier notarization is filed under. |

They are all or nothing. Signing without notarization still leaves a build Gatekeeper stops, and
Tauri fails outright when it is given an Apple ID and password with no team ID, so the workflow
treats the six as one unit and skips them together.

The workflow does not decide what the notes say from which secrets were set. After the build it
reads the signature off the bundle with `codesign` and reports what Gatekeeper will actually see, so
a signing step that silently did nothing cannot be reported as a signed release.

## What an unsigned release is like for a user

The bundle carries only the ad-hoc signature the linker leaves on an Apple silicon binary. Nothing
seals the bundle itself, so `codesign --verify` on the `.app` fails outright and `spctl` cannot
assess it at all:

```
$ codesign --verify --deep --strict Snapdeck.app
Snapdeck.app: code has no resources but signature indicates they must be present
```

That is what `tauri build` produces without signing credentials, locally and in CI alike, and it
means the first launch of a downloaded copy is refused. There are three ways through it, in the
order to try them:

1. Control-click (or right-click) the app in Applications and choose **Open**, then confirm.
2. On macOS 15 and later that menu is often refused as well, and the app is allowed from System
   Settings > Privacy & Security, where a message about Snapdeck appears with an **Open Anyway**
   button.
3. If macOS refuses both, which it words as the app being damaged rather than unverified, the
   quarantine flag has to go: `xattr -dr com.apple.quarantine /Applications/Snapdeck.app`.

It is once per install; macOS remembers.

The app itself is the same either way. What signing changes is the first ten seconds of the first
launch, and how much a stranger has to trust the download.

Two consequences worth knowing:

- Ad-hoc signatures are not stable across builds. macOS ties the Screen Recording grant to the
  signature, so a user replacing one unsigned build with another is asked for the permission again.
  A Developer ID signature is stable and carries the grant across updates.
- There is nothing to revoke. If a release has to be taken back, it is taken back by removing it,
  not by revoking a certificate.

## What a release contains

| Asset | What it is |
| --- | --- |
| `Snapdeck_<version>_aarch64.dmg` | The disk image, which is what the README tells users to download. |
| `Snapdeck_<version>_aarch64.app.tar.gz` | The `.app` on its own, for anyone scripting an install. |
| `SHA256SUMS.txt` | The checksums of the two, in `shasum -a 256 -c` format. |

The architecture in those names is whatever `macos-latest` builds, currently Apple silicon. The
workflow reads it off the DMG that Tauri produced rather than assuming it, so a runner change shows
up in the filenames instead of being papered over.

`macos-latest` is deliberate, and it is the same reason CI gives: `screencapturekit` pulls in
`apple-metal`, whose Swift bridge needs a Metal SDK that `macos-14` does not ship. The image the
runner builds with is not the app's runtime baseline, which is still macOS 14.0 and set in
`tauri.conf.json`.
