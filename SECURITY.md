# Security

Snapdeck reads the contents of your screen, writes files, tells you that a redaction has destroyed
pixels, and installs updates it has verified a signature on. Every one of those is a promise that
can be broken by a bug, so please report one rather than filing it as an ordinary defect.

## Reporting a vulnerability

Email **claude.ai@boenstitu.com**. Put `snapdeck` in the subject.

Include what you have: the version of Snapdeck, your macOS version, whether the build came from a
release DMG or from a local build, what you did, and what you got. The version is in the DMG's
filename, in **Get Info** on `Snapdeck.app`, and in what **Check for Updates…** says when there is
nothing newer. A minimal reproduction is worth more than a description. If you have a proof of
concept that captures or recovers content, describe it rather than attaching real screen contents:
do not send anyone else's data.

**Please do not open a public issue, a pull request or a discussion for a vulnerability**, and
please give the maintainer a chance to publish a fix before writing about it in public.

This is a one-maintainer project with no funded security programme. There is no response deadline
being promised here and no bounty; what you will get is an answer when it is read, and credit in the
release notes if you want it.

## What is in scope

The whole repository is in scope: `crates/capture`, `packages/editor`, `apps/desktop` and the
release workflow. The classes below are the ones that matter most for an app shaped like this one.

**Captured pixels reaching something that should not have them.** The capture core hands back a
full-resolution copy of what was on screen. Anything that widens where those bytes can go is a
vulnerability: an asset protocol scope broader than the one capture it was opened for, a webview
that can read a file it was never granted, a Tauri command reachable from a window whose capability
does not name it, a weakened CSP, an IPC surface that takes a path from the page.

**Frozen frames left on disk.** Freezing the screen writes a lossless PNG of every display into the
application cache directory. They are meant to be deleted when the capture ends, cancelled or not.
A path where they survive that they should not, where they are written somewhere other than the
app's own cache, or where a file that is not one of them gets unlinked, is worth reporting. That a
crash or a force quit leaves them behind until the next capture overwrites them is known and
documented in the README; that is a limitation, not a report.

**Redaction that does not redact.** Black out is documented as destroying the pixels under it, and
the redaction is applied to the picture before it is encoded rather than drawn over it. Anything
that breaks that is serious: original pixels surviving in the saved file, a redacted region that is
recoverable from what was written, a preview and an export that disagree about how strong a
redaction is, a document or its layer coordinates leaking into file metadata, or an unredacted copy
reaching the clipboard at a moment the app says it has replaced it. That pixelate and blur are
attenuation rather than destruction is documented at length in the README, with measurements: it is
the reason the tool opens in Black out, not a vulnerability.

**Writing outside where the user said.** Captures go to the save folder, under a filename built
from a template the user controls. A template or a name that escapes that folder, a save that
overwrites a file it was not aiming at, a rename-into-place that can be redirected through a
symlink, or a temporary file left with contents and permissions it should not have, are all in
scope.

**Updates.** An installed copy will replace itself with a download. Anything that lets it accept a
build this project did not sign is the most serious report there is: a signature check that can be
skipped or fooled, a manifest or endpoint that can be substituted, a downgrade or a pre-release
being offered to a build that should refuse it, an archive unpacked somewhere it can do harm before
it has been verified. Report anything about the release workflow that could expose the signing key
or let a build be published from outside it.

**Permissions and privilege.** Snapdeck asks for Screen Recording and for nothing else. A path that
acquires another permission, that prompts in a misleading way, or that keeps capturing after the
grant is gone, is in scope.

**Dependencies.** A vulnerable crate or npm package that Snapdeck actually reaches is worth
reporting; an advisory against a code path this project does not call is worth mentioning but is not
urgent.

## A known limit of the update channel

The signature on an update covers the archive and nothing else. The version number, the release
notes and the download URL published alongside it are not signed, which is why Snapdeck checks them
itself: it refuses a release that is not newer than the highest version it has ever run, and it
refuses to fetch the archive from any host but GitHub.

One case survives that. Whoever controls what the update endpoint serves could publish a manifest
claiming a high version number while pointing at an older archive this project genuinely signed.
Snapdeck would verify the signature, because the archive really is ours, and install an older build
than the one it claimed. Closing it properly means comparing the unpacked bundle's version against
the manifest's claim, which lives inside the updater plugin's install path rather than in this
repository.

Reaching it requires control of the release endpoint, which means GitHub itself or an account with
write access to this repository. We would rather write that down than let the word "signed" carry
more than it earns. If you see a way to reach it without that level of access, report it.

## What is not a vulnerability

These are known, deliberate and already written down. Reporting them is welcome but they will be
closed with a link.

- **The app is not signed with an Apple Developer ID.** Releases are signed ad-hoc, so a first
  launch has to go through Control-click then **Open**, or through **Open Anyway** in System
  Settings on macOS 15 and later. See the README.
- **An update re-triggers the Screen Recording prompt.** macOS ties the grant to the signature, and
  an ad-hoc signature differs in every build, so the updated app is a different application as far
  as macOS is concerned.
- **Pixelate and blur do not destroy information.** Documented, measured, and the reason Black out
  is the default.
- **Anything Snapdeck writes is readable by you.** Captures land in your own save folder with
  ordinary permissions. Snapdeck does not encrypt them and does not claim to.
- **A capture photographs whatever is on screen.** Window mode captures that rectangle of screen,
  including anything on top of the window. This is documented behaviour.

## Which versions get fixes

The latest release. Snapdeck is at `0.1.0`, there are no maintained branches behind it, and a fix
ships in the next tag.
