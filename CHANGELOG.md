# Changelog

Every release has a section here, and the heading is the tag: `## v0.1.0`.

This file is not a courtesy. `.github/workflows/release.yml` reads the section matching the tag
into the GitHub release notes and into `latest.json`, which is the text the in-app update dialog
shows. A tag with no section here, or with an empty one, fails the version job before anything is
built, because a release nobody can read is not a release worth publishing. See
[docs/RELEASING.md](docs/RELEASING.md).

Use `###` for the headings inside a section. A `##` heading ends the section, so a second one
inside it would silently cut the notes short.

## v0.2.0

Full-page capture: a browser extension that captures a whole page, and an experimental way to
capture anything else that scrolls. Apple silicon, macOS 14 or later.

### Added

- **A Chrome extension that captures a whole page** and hands it to Snapdeck, where it lands in the
  same place every other capture lands: the same folder, the same filename template, the same
  format, the clipboard, the editor and the list of recent captures. The extension measures the
  page, hides its pinned headers after the first layer so they appear once rather than on every
  band, scrolls a viewport at a time within Chrome's capture quota, and joins the layers at the
  screen's own pixel density. Installation and pairing are in [docs/EXTENSION.md](docs/EXTENSION.md).
- **A bridge that never leaves the machine.** Snapdeck listens on `127.0.0.1` only. A handshake has
  to name a Chrome extension as its origin and this listener as its host, present a pairing token
  you copy from Settings, and stay inside a few kilobytes until it does. Snapdeck then proves
  itself in return: the extension sends a nonce and refuses to hand over the page unless the answer
  carries HMAC-SHA256 of that nonce under the token. Something else sitting on the port cannot
  produce that, and the extension says so plainly instead of pretending Snapdeck is closed.
- **When Snapdeck is not running**, the extension downloads the PNG instead and marks its toolbar
  icon. The file name carries the page's host and path, and deliberately not its query string.
- **Capture Scrolling Window (Experimental)** in the menu bar, for anything that scrolls and is not
  a browser tab. It scrolls the frontmost window, joins its own frames by finding where they
  overlap, and asks for Accessibility permission at that moment rather than at launch. A run that
  ends early is still saved, with a sentence saying a piece may be missing.
- **Recent Captures** in the menu bar, holding the last five, each one revealing its own file in
  the Finder.

### Changed

- The settings file is written so only its owner can read it. It now holds the bridge's pairing
  token, and the default mode left that readable by every account on the machine.

### Known limitations

- Lazy-loaded images that take longer than a moment to arrive, and virtualized lists that never put
  the whole page in the document, can come out incomplete. This is measured, with numbers, in
  [docs/EXTENSION.md](docs/EXTENSION.md).
- The scrolling-window capture cannot tell a pinned header from content, which is why it is
  labelled experimental.
- Still no screen recording.

## v0.1.0

The first release. Apple silicon, macOS 14 or later.

### Added

- **Capture** from the menu bar or with a shortcut: region (`Cmd+Shift+7`), window
  (`Cmd+Shift+8`) and full screen (`Cmd+Shift+9`). The screen freezes and the selection stays
  editable until `Enter`: eight handles to resize it, arrow keys to nudge it, `Esc` to cancel on
  every display at once, and `C` to copy the hex colour under the magnifier. The region is
  captured again at the display's own pixel density rather than cropped out of the frozen frame.
- **Every capture is saved and copied at once**, as a PNG under `~/Pictures` named
  `Snapdeck <date> at <time>.png`, and on the clipboard ready to paste.
- **An editor on every capture**, opened beside the finished file rather than in front of it:
  arrow, rectangle, ellipse, freehand line, text, highlight, obscure, step number and crop.
  Colour and width apply to the tool in hand and to the selected annotation, and width also sets
  the text size, the badge size and the redaction strength.
- **Saving** as PNG writes the edited picture back over the capture through a rename, so a failed
  save leaves the picture you already had. Saving as JPEG writes a second file beside it and
  leaves the original alone. Either way the edited picture goes on the clipboard.
- **Recent Captures** in the menu bar, holding the last five, each one revealing its own file in
  the Finder. The list survives a restart and holds paths and nothing else: no thumbnails, no
  pixels, nothing the Finder would not show you anyway. A capture that has since been deleted is
  taken out of the list rather than failing silently.
- **A settings window** for the capture folder, the filename template, the format and the three
  shortcuts.
- **Check for Updates…**, and the same check at launch behind a checkbox that is off by default.
  Snapdeck verifies every download against a public key built into the app before it is unpacked,
  refuses any release that is not newer than the highest version it has ever run, and fetches the
  archive from no host but GitHub.

### Known limitations

- Releases are signed ad-hoc rather than with an Apple Developer ID, so the first launch of a
  downloaded copy takes an extra step and macOS asks for Screen Recording permission again after
  an update. Both are explained in the README.
- Sharing is planned and is not part of this release.
