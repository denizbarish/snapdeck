# Releasing

A release is cut by pushing a `v*` tag. [`.github/workflows/release.yml`](../.github/workflows/release.yml)
builds the app on `macos-latest`, signs it if it can, and opens a **draft** GitHub release with the
DMG, an `.app.tar.gz`, their SHA-256 checksums and the signed `latest.json` that installed copies
update from. Nothing becomes public until a human presses Publish.

## Cutting a release

1. Make sure CI is green on the commit you are about to tag. The release workflow builds, it does
   not re-run `cargo fmt`, `clippy`, the Rust tests, `pnpm lint` or `pnpm test`.
2. Set the new version in **all three** files, in the same commit:
   - `Cargo.toml`, under `[workspace.package]`
   - `apps/desktop/src-tauri/tauri.conf.json`, the top-level `version`
   - `apps/desktop/package.json`, the top-level `version`
3. Write the release notes into [`CHANGELOG.md`](../CHANGELOG.md), in the same commit, under a
   heading that is the tag: `## v0.2.0`. The build fails without it, on purpose, and this is not a
   thing that can be done afterwards. See [The release notes](#the-release-notes).
4. Tag and push:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

5. Watch the run: `gh run watch` or the Actions tab.
6. Open the draft release. Check the artifact names, the checksums and the signing line in the
   notes. Check that `latest.json` is there and that its `version` and `url` match the release; the
   run logs it with the signature elided. Then Publish.
7. Publishing is what makes the release reachable at `releases/latest`, which is both the README's
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

The first job compares four things and fails before the build starts if any of them disagree:

| Source | What it decides |
| --- | --- |
| the tag, minus its `v` | what the release claims to be |
| `apps/desktop/src-tauri/tauri.conf.json` `version` | the version the app reports, and the version in the bundle filenames |
| `Cargo.toml` `[workspace.package] version` | the version the binary is built as |
| `apps/desktop/package.json` `version` | nothing, today |

It also fails if `apps/desktop/src-tauri/Cargo.toml` stops inheriting the workspace version, because
then the check would be reading a file that no longer decides anything.

This is a hard failure on purpose. A DMG named `Snapdeck_0.1.0` hanging off a `v0.2.0` tag is the
kind of mismatch nobody notices until something refuses to line up with its own name.

The fourth row is the odd one and is worth being explicit about, because there were two ways to go.
`apps/desktop/package.json` is a private workspace package: nothing publishes it, nothing installs
it by version, and no code reads that field, so the version in it could equally have been deleted.
It is **checked rather than deleted**, for one reason: it is the file a person opens when they want
to know what version this app is, and a number sitting there that nothing compares is a number that
drifts and then misleads whoever trusted it. Checking it costs one `jq` call on a runner that was
going to start anyway. If it is ever deleted instead, delete the check with it in the same commit,
or the gate starts failing on a file that no longer exists.

## The release notes

The notes a release carries come from one place: the section of [`CHANGELOG.md`](../CHANGELOG.md)
whose heading is the tag. The version job reads it, hands it to the build job, and the build job
puts it in the generated notes as a `## Changes` section, above the signing paragraph. Those same
notes then go two ways: to the GitHub release page, and, minus the **Install** and **Checksums**
sections, into `latest.json` as the text the in-app update dialog shows.

**A tag with no section fails the build, and so does a section with nothing in it.** That is not
strictness for its own sake. It is that this is the one part of a release which genuinely cannot be
repaired afterwards:

- `latest.json` is generated during the run and is never written by hand. Editing the draft's notes
  on GitHub changes the release page and reaches no installed copy, because the manifest was already
  written with the old text and uploaded beside it.
- Re-running the build to pick up the edit overwrites the edit, because the notes are generated from
  `CHANGELOG.md` and the checksums, not read back from the release.

So the failure has to happen before anything is built, which is why the check lives in the version
job on the cheap runner rather than next to the step that writes the notes. The cost of getting this
wrong is not a broken release: it is a working release whose update dialog says nothing about what
changed, discovered by users, permanently.

Two rules for writing a section, both enforced by the shape of the pipeline rather than by taste:

- The heading is exactly the tag, `## v0.2.0`. It is matched literally.
- Inside a section, use `###` and below. The section ends at the next `##`, and the manifest filters
  the notes by `##` heading as well, so a level-2 heading inside a section truncates the notes in
  one place and confuses the filter in the other.

Blank lines around a section are trimmed; blank lines inside it are kept.

## Pinned actions

Every third-party action in `release.yml` is pinned to a 40-character commit SHA, with the version
it corresponds to in a trailing comment:

```yaml
- uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
```

The reason is the build job specifically. That job is granted `contents: write`, and one step later
it holds `TAURI_SIGNING_PRIVATE_KEY`. A tag like `@v4` is a mutable reference: the owner of that
repository can move it, and `dtolnay/rust-toolchain@stable` was not even a tag but a branch, which
is moved by design on every toolchain release. Whoever controls where those names point controls
code that runs on this runner.

The step-scoped `env:` on the signing step means a compromised action cannot read the signing key.
It does not need to. It runs **before** `pnpm tauri build`, in the same workspace, and can change
what gets built. This project then signs that result with its own key and publishes it, and every
installed copy verifies the signature happily, because the signature is real. The updater's
guarantee is "this archive is the one the workflow produced", and tampering with the build turns
that guarantee into the attacker's alibi.

The checkout in the build job also passes `persist-credentials: false`. By default
`actions/checkout` leaves the job's token in `.git/config` for the whole job, where every later step
can read it, and in this job that token can write releases. Nothing here talks to GitHub over git:
the release is created and uploaded with `gh` and an explicit `GH_TOKEN`.

### Why this comes before the Environment gate

The [updater signing key](#the-updater-signing-key) section describes a different way to reach the
key: anybody with write access to this repository can add a workflow that reads the secret. The fix
named there is a GitHub Environment with required reviewers.

That gate does not close this hole, which is why pinning came first. An Environment makes a human
approve the *release job* before the secrets are handed to it. The reviewer approving it sees a tag
and a workflow name; they do not see that a third-party action three steps earlier now resolves to a
different commit than it did last month. Approval of a tampered build is still a tampered build,
signed with this project's key and blessed by a human. Pinning removes the tampering; the
Environment removes an unrelated path to the secret. Both are worth having, in that order.

### Updating a pinned action

Pinning is a decision to upgrade deliberately rather than never to upgrade. To move one:

```bash
# What the tag you want points at today.
gh api repos/actions/checkout/commits/v4 --jq '.sha'

# Which release that commit is, for the trailing comment.
gh api 'repos/actions/checkout/tags?per_page=100' \
  --jq '.[] | select(.commit.sha == "<sha from above>") | .name'
```

Put the SHA after the `@` and the precise version in the comment, then read the action's changelog
between the old version and the new one before committing. The comment is documentation, not a
constraint: nothing verifies that it matches the SHA, so it is only true if it is kept true.

`dtolnay/rust-toolchain` has no release tags. Its pin is the head of the `stable` branch at the time
it was set, whose `action.yml` defaults the `toolchain` input to `stable`; the comment is `# stable`
rather than a version. Move it with `gh api repos/dtolnay/rust-toolchain/commits/stable --jq '.sha'`
when a newer Rust is wanted on the release runner.

`ci.yml` is deliberately not pinned. It has no secrets and no write permission, it runs on every
pull request including ones from strangers, and the cost of a moved tag there is a failed check
rather than a signed artifact. If it ever gains a secret, pin it in the same change.

## Proving the workflow without releasing

> [!IMPORTANT]
> **Do this once, before the first real tag, as the first thing after this branch is merged.**
>
> The manifest half of this workflow has never run. Every Release run on record predates the commit
> that added the `Sign the update archive and write the manifest` step, so the step that signs the
> archive, maps the platform key and writes `latest.json` has only ever been reasoned about, never
> executed. Both updater secrets are set, which means that without this run its first execution
> would be on the real `v0.1.0` tag.
>
> ```bash
> gh workflow run release.yml --ref main -f tag=v0.1.0
> ```
>
> Then download the run's artifact and confirm four things:
>
> 1. `latest.json` is in it at all.
> 2. Its `version` is `0.1.0`.
> 3. Its one platform key is `darwin-aarch64`, not `darwin-x64` or anything else.
> 4. The run log carries the line `signing key <id> matches the public key this build ships`.
>
> What is already known and what is not: the ordering is fail-safe, since the manifest is written
> before anything is published and the release step never runs on a dispatch. The Node self-check
> was executed against a real `tauri signer sign` output during review, and accepts a good
> signature, rejects a one-byte-tampered archive and catches a key-id mismatch; the assumption that
> the signer writes its signature to `<archive>.sig` was confirmed the same way. What has never run
> on a runner is `pnpm tauri signer sign` itself, the architecture mapping against a real bundle
> name, and the notes filter against real generated notes. That is what this run is for.
>
> `workflow_dispatch` cannot do this from a feature branch: GitHub only offers the entry point once
> the workflow file is on the default branch. So it is a post-merge step, not a pre-merge one.

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

It is worth being plain about what a repository secret is and is not. `TAURI_SIGNING_PRIVATE_KEY` is
an ordinary, unprotected repository secret, so anybody with write access to this repository can sign
arbitrary bytes with it: a workflow file is a thing they can add, a workflow is a thing that can read
the secret, and the key does not know the difference between an archive this workflow built and one
somebody handed it. Nothing in this repository stops that today. A GitHub Environment with required
reviewers, holding the two updater secrets and named by the release job, would: the job would then
wait for a human to approve before the secrets were available to it, and a workflow added by somebody
else could not reach them at all.

There is a second path to a release this project did not build, and it does not go through the
secret at all: change what gets built, and let this workflow sign it. Everything that runs in the
build job before `pnpm tauri build` is in a position to do that, third-party actions included, and
the key signs whatever it is handed. That is why every action in `release.yml` is pinned to a commit
rather than to a tag or a branch, and why that came before the Environment gate rather than after
it. See [Pinned actions](#pinned-actions).

The self-check in the signing step is a different question and does not answer this one. It proves
that the secret in use and the key in the bundle are two halves of one key, by verifying the
signature it just produced against `plugins.updater.pubkey`. Without it, a wrong, truncated or
rotated secret produces a release that looks perfect on the draft, and the only signal is that every
installed copy refuses the download it fetches. The job now fails instead.

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

`latest.json` is the only published asset with no line in `SHA256SUMS.txt`, and that follows from the
order rather than from an oversight: the checksums are written before the manifest, because the
release notes quote them and the manifest embeds those same notes. Nothing is lost by it.
`SHA256SUMS.txt` exists so a person can check a download they made by hand, and nobody downloads the
manifest by hand. What protects an installed copy from a tampered manifest is not a checksum
published beside it, which anybody able to rewrite the one could rewrite the other, but the signature
the manifest carries over the archive and the public key compiled into the bundle. The version, the
notes, the URL and the date in it are not signed by anything, which is why the app measures a
manifest against the highest version it has ever run and refuses a download URL on any host but
GitHub.

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
download with the signature rather than with a checksum a person compares by eye. What survives the
filter is the line naming the version, the `## Changes` section taken from
[`CHANGELOG.md`](../CHANGELOG.md) and the signing paragraph, which is why that section is required:
without it the update dialog would show a version number and a paragraph about Gatekeeper. See
[The release notes](#the-release-notes).

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
