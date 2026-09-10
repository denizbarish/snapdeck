# Releasing

A release is cut by pushing a `v*` tag. [`.github/workflows/release.yml`](../.github/workflows/release.yml)
builds the app on `macos-latest`, signs it if it can, and opens a **draft** GitHub release with the
DMG, an `.app.tar.gz`, their SHA-256 checksums and the signed `latest.json` that installed copies
update from. Nothing becomes public until a human presses Publish.

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
   notes. Check that `latest.json` is there and that its `version` and `url` match the release; the
   run logs it with the signature elided. Then Publish.
6. Publishing is what makes the release reachable at `releases/latest`, which is both the README's
   install link and the updater's endpoint. Until then, installed copies see nothing.

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

The workflow reads two unrelated sets of secrets: the Apple Developer ID credentials, which decide
whether Gatekeeper knows who built the app, and the updater signing key, which decides whether an
installed copy will accept a download as coming from this project. They are independent. A release
can have either, both or neither.

### Apple Developer ID

> [!WARNING]
> **Before you add these six secrets, change how they reach the build.**
>
> The `Provide the Apple Developer ID credentials` step writes all six into `$GITHUB_ENV`. That
> file is not scoped to a step: everything after it in the job inherits the variables, including
> `actions/upload-artifact` and any other third-party action the job runs or later gains. The
> certificate and its password would be in the environment of code this project does not control.
> Today the secrets do not exist, so the step never runs and the variables are never written, which
> is the only reason this is written down rather than fixed.
>
> The fix is narrow and mechanical: duplicate `Build the app` into two steps guarded by
> `if: env.SIGNING == 'true'` and `if: env.SIGNING != 'true'`, put the six `secrets.APPLE_*`
> in the signed step's own `env:`, and delete the `$GITHUB_ENV` step. A step's `env:` block does not
> outlive the step. **Do this on the day the secrets are added, in the same change**, not after the
> first release that used them: a secret that has already been through an untrusted process has to
> be rotated, not narrowed.
>
> The ad-hoc step below writes only `APPLE_SIGNING_IDENTITY=-` to `$GITHUB_ENV`, which is not a
> secret and can stay where it is.

Signing and notarization are optional. When the six secrets below are all set, the workflow hands
them to `tauri build`, which signs the bundle with the certificate and sends it to Apple for
notarization. When any of them is missing, that step is skipped, the workflow signs the bundle
ad-hoc instead, and the release notes say so. **The absence of these secrets is not an error.**

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

### The updater signing key

| Secret | What it does |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The minisign private key, as written by `pnpm tauri signer generate`. The workflow signs the `.app.tar.gz` with it and puts the signature in `latest.json`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password that key was generated with. |

Both or neither: the key in use carries a password, and a signing step that runs without it fails in
the middle of a release rather than before it. When either is missing the workflow writes no
`latest.json`, logs a warning that says so, and publishes the release anyway. That release is one
nobody can update to: every installed copy asks `releases/latest/download/latest.json`, gets a 404,
and reports that it could not check.

**The private key is not in this repository and must never be.** It exists as that secret and as
whatever backup the maintainer keeps. The matching public key is `plugins.updater.pubkey` in
`apps/desktop/src-tauri/tauri.conf.json`, ships inside every build, and is what makes the check work
on a machine that has never seen this repository.

Losing the private key is not recoverable in place. A new key means a new `pubkey` in
`tauri.conf.json`, and every copy already installed carries the old one: those installations can
still check for updates, but they will refuse every download signed with the new key, and their
users have to download a release by hand once. Rotating the key is therefore a thing to do
deliberately and to say in the release notes, not a thing to do because the old one was mislaid.

To generate a key, if there is ever a reason to:

```bash
pnpm tauri signer generate -w /path/outside/this/repo/snapdeck.key
```

It writes the private key to that path and the public key beside it as `.pub`. Put the private key
and its password into the two secrets above, put the contents of the `.pub` file into
`plugins.updater.pubkey`, and keep a backup of both somewhere that is not a build machine.

## The update manifest

`Check for Updates…` in the app reads one file: `latest.json`, published as an asset of the latest
non-prerelease GitHub release. The `Sign the update archive and write the manifest` step writes it,
and **nothing writes it by hand**. That is the point of the step rather than a style preference: the
signature it carries is taken over the exact `.app.tar.gz` being uploaded in the same run, the URL
names that same asset, and the version is the one the version job already proved the tag,
`tauri.conf.json` and `Cargo.toml` agree on. There is no moment at which the manifest and the
release can come to describe different builds.

```json
{
  "version": "0.2.0",
  "notes": "…",
  "pub_date": "2026-01-01T00:00:00Z",
  "platforms": {
    "darwin-aarch64": { "signature": "…", "url": "https://github.com/…/Snapdeck_0.2.0_aarch64.app.tar.gz" }
  }
}
```

Three things about it are worth knowing.

The platform key is mapped from the bundle's own filename, not passed through. Tauri names an Intel
bundle `_x64` while the updater looks for `darwin-x86_64`, so the two vocabularies agree on Apple
silicon and disagree on Intel. An architecture the mapping does not recognise fails the step rather
than publishing a manifest no installed copy can match itself against.

The `notes` are the release notes minus the **Install** and **Checksums** sections. Those two are
for somebody downloading a DMG by hand: the updater does the installing itself, and it verifies the
download with the signature rather than with a checksum a person compares by eye.

The signature is inline. The `.sig` file the signer leaves next to the archive is deleted rather
than published, because two copies of one signature is one more thing that can disagree.

Because the release is a **draft** until a human publishes it, `latest.json` is not reachable at
`releases/latest/download/latest.json` until then. No installed copy sees a release before it is
published, which is the intended order.

## Pre-releases

A tag with a pre-release part, `v0.2.0-rc.1`, is published with `--prerelease`. Two things point at
GitHub's idea of "latest" and neither should ever reach a release candidate: the README's install
link, and the updater endpoint every installed copy asks. The flag is set explicitly either way, so
re-running a build cannot leave a previous tag's flag on the release.

The app refuses one more time on its own account. `updater::is_upgrade` will not offer a
pre-release to a build that is not itself a pre-release, however the two versions sort. That is
deliberate belt and braces: the `--prerelease` flag depends on a tag being spelled the way this
paragraph assumes, and on nobody publishing a manifest by hand.

## Ad-hoc signing

When the Developer ID secrets are absent the workflow exports `APPLE_SIGNING_IDENTITY=-` before the
build. `-` is codesign's ad-hoc identity: it needs no Apple account and costs nothing, and Tauri
signs the `.app` with it and only then builds the DMG around it, so the DMG carries the sealed
bundle. If the secrets are present, the real identity is already in the environment and the ad-hoc
step does not run.

This is not a substitute for a Developer ID. An ad-hoc signature names no developer and Gatekeeper
still rejects it. What it does is seal the bundle's resources, and that seal is the difference
between an app macOS can assess and one it cannot:

| | Without | With |
| --- | --- | --- |
| `Contents/_CodeSignature` | absent | present |
| `codesign --verify --deep --strict` | fails: `code has no resources but signature indicates they must be present` | passes |
| `spctl --assess` | the same failure, so no verdict at all | `rejected`, the verdict an unnotarised app is supposed to get |
| First launch of a download | often refused as **damaged**, which sends people to the Trash | refused as an **unidentified developer**, which is a dialog with a way through |

The `Collect the bundles` step runs `codesign --verify --deep --strict` on the finished `.app` and
fails the build if it does not pass, on both signing paths. It also logs `spctl --assess` without
asserting on it, since a rejection is the correct outcome for an ad-hoc build and a pass is the
correct outcome for a notarized one.

## What an unsigned release is like for a user

The first launch of a downloaded copy is refused, because macOS cannot attribute the app to a
developer. There are two ways through it, by macOS version:

1. **macOS 14.** Control-click (or right-click) the app in Applications and choose **Open**, then
   confirm.
2. **macOS 15 and later.** That dialog no longer offers a way through. Dismiss it, then open System
   Settings > Privacy & Security, where a message about Snapdeck now carries an **Open Anyway**
   button.

It is once per install; macOS remembers.

The app itself is the same either way. What a Developer ID changes is the first ten seconds of the
first launch, and how much a stranger has to trust the download.

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
| `Snapdeck_<version>_aarch64.app.tar.gz` | The `.app` on its own. It is what an in-app update downloads, and what anyone scripting an install would use. |
| `SHA256SUMS.txt` | The checksums of the two, in `shasum -a 256 -c` format. |
| `latest.json` | The update manifest, when the updater key is set. See [The update manifest](#the-update-manifest). |

The architecture in those names is whatever `macos-latest` builds, currently Apple silicon. The
workflow reads it off the DMG that Tauri produced rather than assuming it, so a runner change shows
up in the filenames instead of being papered over.

`macos-latest` is deliberate, and it is the same reason CI gives: `screencapturekit` pulls in
`apple-metal`, whose Swift bridge needs a Metal SDK that `macos-14` does not ship. The image the
runner builds with is not the app's runtime baseline, which is still macOS 14.0 and set in
`tauri.conf.json`.
