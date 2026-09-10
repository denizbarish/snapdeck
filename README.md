# Snapdeck

Snapdeck is an open-source macOS menu bar app for capturing screenshots: pick a region, a window
or a whole display, and the capture is saved as a PNG and put on the clipboard.

An editor opens on the capture afterwards, for arrows, captions, highlights, redaction and
cropping. Sharing is planned and is not part of this release.

## Install

Download `Snapdeck_<version>_aarch64.dmg` from the
[latest release](https://github.com/denizbarish/snapdeck/releases/latest). It is an Apple silicon
build and needs macOS 14 or later. Every release also publishes `SHA256SUMS.txt`, so the download
can be checked with `shasum -a 256 -c SHA256SUMS.txt`.

1. Open the DMG and drag **Snapdeck** into Applications.
2. **The first launch takes an extra step.** There is no Apple Developer ID for this project, so
   releases are signed ad-hoc rather than by a named developer, and macOS will not open a downloaded
   app it cannot attribute to one. On macOS 14, Control-click (or right-click) Snapdeck in
   Applications, choose **Open**, and confirm. On macOS 15 and later that dialog offers no way
   through: dismiss it, then open System Settings > Privacy & Security, where a message about
   Snapdeck now has an **Open Anyway** button.

   Either way it is once per install: macOS remembers, and every later launch is an ordinary
   double-click. Building from source avoids the step altogether.
3. Snapdeck is a menu bar app. Nothing appears in the Dock and no window opens on launch: look for
   its icon in the menu bar.
4. The first capture asks for Screen Recording permission. Grant it in System Settings > Privacy &
   Security > Screen & System Audio Recording, then **quit Snapdeck and open it again**. Relaunching
   is not optional, for the reason under [Getting started](#getting-started).

## Updates

**Snapdeck makes no network connection you did not ask for.** Checking for an update is the only
one it makes at all, it reaches only GitHub, the release manifest and the archive that manifest
names, and it is off until you turn it on.

- **Check for Updates…** in the menu bar asks the releases page whether there is a newer version.
  If there is, Snapdeck shows what it is and what the release says about it, and downloads nothing
  until you press **Update and Restart**. That one button does the whole thing: download, replace,
  restart.
- **Settings > Updates** has a checkbox for the same check at launch. It is **off by default**, and
  the menu item works whether or not you turn it on.

Every update is verified against a public key built into Snapdeck before it is unpacked, so an
update server that has been taken over cannot install code this project did not build. A download
whose signature does not verify is thrown away rather than installed, and Snapdeck puts the reason
in the menu bar and in its log instead of failing quietly.

It is worth being precise about the limit. Only the archive is signed, not the version number or
the notes printed beside it, so Snapdeck also refuses any release that is not newer than the
highest version it has ever run, and refuses to fetch the archive from any host but GitHub.

One consequence of the app being signed ad-hoc rather than with an Apple Developer ID: **macOS asks
for Screen Recording permission again after an update.** The permission is tied to the app's
signature, an ad-hoc signature is different in every build, so to macOS the updated Snapdeck is a
different application. Grant it again in System Settings > Privacy & Security > Screen & System
Audio Recording, then quit Snapdeck and open it again. The update dialog says so before you agree
to it.

## Requirements

- macOS 14 or later
- Rust stable toolchain
- Node.js 24
- pnpm 11

`pnpm install` runs a `prepare` script that downloads a Chromium build for Playwright, roughly 150 MB
on a first install and cached after that. The editor's renderer, export and component tests measure
real pixels from a real browser engine, so that download is what makes `pnpm test` work from a clean
clone. It needs network access the first time.

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
own time zone, and is put on the clipboard at the same time, ready to paste. The folder, the
filename template, the format and the three shortcuts are all settings; those are the defaults, and
they are what the rest of this README describes. The region is
captured again at the display's own pixel density rather than cropped out of the frozen frame, so
on a Retina display the file is twice the size in pixels that you selected in points.

That second capture happens the moment you press `Enter`, not when the screen froze, so the file
holds the screen as it is then rather than the frozen frame you selected against. For still
content the two are the same picture; over a video, an animation or anything else that moves, the
saved pixels are the ones from the instant you confirmed.

## Full page

A capture can only reach what is on screen, and a web page is usually taller than that. Snapdeck's
Chrome extension is how the rest of it gets captured: press its toolbar button and it scrolls the
page for you, photographs each viewport, joins the pictures into one PNG and hands that PNG to
Snapdeck. What arrives is not a second kind of capture. It goes through the same save path a region
capture does, so your Folder, File name and Format settings apply to it, the editor opens on it if
you have asked for that, and it lands on the clipboard like everything else.

The extension is not on the Chrome Web Store; you build it and load the folder.
[docs/EXTENSION.md](docs/EXTENSION.md) is the whole of it: installing, pairing, what each permission
is for, and what comes out wrong on a page that loads its images lazily.

The two programs talk over a WebSocket bound to `127.0.0.1` and nowhere else, so there is no socket
for another machine to reach. A browser fills in the `Origin` header and a page cannot forge it, so
a script on an ordinary web page that opens that port is refused before a session exists. Past that
gate the extension proves itself with a pairing token you copy out of Snapdeck's settings, and
Snapdeck proves itself back: the extension sends a fresh challenge and will not send a page until
the answer it gets is the one only something holding that token could compute. If the app is not
running, or something else holds the port and cannot answer, the capture goes to your downloads
folder rather than being lost or handed to a stranger, and the toolbar badge says which of the two
happened.

There is a second route that needs no browser, for the scrollable windows that are not web pages:
Snapdeck photographs the frontmost window, posts a synthetic scroll, photographs it again, and joins
the frames by finding where consecutive ones overlap. It is **experimental**, it says so in the menu
bar, and it deserves the word. Where the extension is told by Chrome exactly how far the page moved,
this one has to work it out from the pixels, and a window whose content repeats or does not move the
way it was asked to comes out wrong. It tells you when it could not find an overlap; it cannot tell
you when it found the wrong one.

## The editor

Every capture that produced a file opens in an editor window: the title bar carries the file's
name, and the picture is shown at its own size or fitted down if it does not fit the screen.

The editor is an offer, not a step. By the time it appears the capture is already finished: the
file is in `~/Pictures` and the image is on the clipboard. Closing the window without touching
anything leaves both exactly as they were.

Take a second capture and a second editor opens beside the first. The first keeps whatever you
had drawn in it, so a capture taken while you are still annotating cannot destroy your work.

### Tools

| Tool | What it does |
| --- | --- |
| Select | Pick an annotation, move it, or resize it by its eight handles. |
| Arrow | Drag from tail to head. |
| Rectangle, Ellipse | Drag out an outline. |
| Freehand line | Draw with the pointer held down. |
| Text | Click to open a box, type the caption, click away to commit it. |
| Highlight | Drag a translucent band over what you want the eye to land on. |
| Obscure | Drag over what has to be hidden. Opens in Black out; three modes, see below. |
| Step number | Click to drop a numbered badge; the number counts up on its own. |
| Crop | Drag out what to keep. Saving writes the cropped picture. |

The colour swatches, the custom-colour well and the width slider apply to the tool in hand and to
the selected annotation. Width also sets the text size, the badge size and the redaction strength.

### Saving

`PNG` and `JPEG` beside `Save` choose the format. The editor opens on the format the capture was
taken in, which is PNG unless you have changed **Default format** in Settings.

Saving in the format the capture is already in writes the edited picture back over it in
`~/Pictures`, under the same name, by writing a temporary file beside it and renaming it into
place: a failed save leaves the picture you already had rather than half of a new one. Saving in
the other format cannot overwrite that file, because the extension is part of the name, so it
writes a second file beside the capture, same name, `.jpg` in place of `.png` or the other way
about, and leaves the original untouched. Either way the status bar names the file that was
written.

JPEG is worth choosing when the picture has to travel and it is mostly photographic: a 2200 x 1500
capture of a desktop measured 544 KB as a PNG and 393 KB as a JPEG. A small capture of flat
interface goes the other way, because that is what PNG is good at: 103 KB as a PNG and 118 KB as a
JPEG. It is encoded at quality 0.92, which is high for this format on purpose, because a
screenshot is mostly text and flat colour and those are what JPEG handles worst. Text will still
be very slightly softer than the PNG. If that matters, keep PNG.

**Saving also replaces what is on the clipboard**, whenever you have drawn or cropped anything. The
capture went onto the clipboard before the editor opened, so after an edit the copy sitting there is
the picture you have just moved on from; saving puts the edited one there instead. What goes on the
clipboard is always the lossless picture, whichever format the file was written in, for the same
reason `Copy` is. Saving a document you have not touched leaves the clipboard alone, and so does
`Close`.

A save that replaces the capture does so by renaming a new file over it, which replaces the file
itself rather than its contents. Finder tags, other extended attributes and the original creation
date belong to the file that is replaced and do not survive the save; the name, the location and
the pixels do.

`Copy` puts the edited picture on the clipboard on its own, without writing anything. It is
unaffected by the format buttons: a clipboard image is handed to the next application as pixels
rather than as a file, so it is always the lossless one. `Close` closes the window and keeps the
file on disk.

### Keyboard

| Shortcut | What it does |
| --- | --- |
| `Cmd+Z` / `Cmd+Shift+Z` | Undo, redo. Every edit is one step, including a slider drag. |
| `Cmd+C` | Copy the edited picture to the clipboard. |
| `Cmd+S` | Save it, in the format the toolbar is set to. |
| `Delete` / `Backspace` | Remove the selected annotation. |
| `Esc` | Drop the selection; press it again with nothing selected to close the window. |

While a text box is open the keys belong to the box: `Esc` closes the box, `Cmd+Enter` commits
the caption, and `Cmd+S` commits it and then saves.

### Redaction: what each mode actually does

The obscure tool has three modes, and only one of them destroys anything.

- **Black out** writes a constant over the region. Nothing of the original survives it.
- **Pixelate** replaces each block with the average of that block.
- **Blur** replaces each pixel with an average of its neighbours.

Pixelate and blur are **attenuation, not destruction**. Both preserve local ink density, so "there
was writing here" survives by construction, and a region that is mostly one colour comes back as
that colour. Their strength is never allowed to be trivial: the block size and the blur radius are
raised to a floor derived from the region, so a box drawn round large text is redacted harder than
one drawn round small text. In source pixels, the block is at least `max(strength, ⌈shorter side /
4⌉, 6)` and the radius at least `max(strength, ⌈shorter side / 6⌉, 4)`.

Measured on this build, from the packaged app: one line of 28-point bold monospaced text captured
on a Retina display, a 1074 x 100 pixel band drawn over it, default width, the numbers read back
out of the saved PNG. Stroke contrast is peak-to-trough luminance across the band, where 255 is
untouched text and 0 is a flat field; correlation is against the same pixels in the original,
where 1 is untouched.

| Mode | Stroke contrast | Correlation with the original | Distinct colours in the band | Original text pixels left |
| --- | --- | --- | --- | --- |
| Black out | **0** | **0** (the band has no variation at all) | 1498 → **1** | none |
| Pixelate | 189 | 0.44 | 1498 → 52 | none |
| Blur | 107 | 0.42 | 1498 → 408 | none |

None of the three leaves a legible glyph, and none of the three leaves the original pixels in
place. But blackout is the only one whose output carries no signal at all: the correlation for
pixelate and blur bottoms out near 0.4 on text however hard they are pushed, because that is what
averaging does. **Use Black out for anything whose recovery would matter** (a password, a token, a
card number, an address). Blur and pixelate are for the face in the background and the name on the
tab, where the point is that nobody reads it over your shoulder.

**The tool opens in Black out**, for the reason in the table: somebody reaching for a redaction
tool is usually covering something whose recovery would matter, and the default has to be the mode
that leaves nothing to recover. The other two are one click away on the toolbar.

The redaction is applied to the picture before it is encoded, not drawn on top of it, so the
hidden pixels are not in the saved file at all and no undo of the file can bring them back.

Two things about the rest of the operation, because a redaction that is only true of the file is
not much of a redaction.

**The clipboard.** Snapdeck copies every capture the moment it takes it, before the editor opens, so
until you save, the unredacted original is the picture on the clipboard. Saving replaces it with the
edited one, which is why `Cmd+S` and then paste is safe. `Copy` does the same without writing a
file. Closing the editor without doing either leaves the original capture on the clipboard, because
that is where the capture put it.

**Saving in the other format.** The extension is part of the file name, so a save in a format the
capture is not already in writes a second file and leaves the capture where it is: the unredacted
original stays in `~/Pictures` beside the redacted new file, under the same name. If the point of
the redaction was that the original should not exist, save in the format the editor opened on,
which is the capture's own and which replaces it, or delete the original yourself.

## Settings

`Settings…` in the menu bar opens them. Nothing needs a relaunch: a change is in force as soon as
you save it.

**Shortcuts.** The three capture shortcuts are rebindable. Click one and press the combination you
want; there is no text field, so what you press is what you get. The defaults avoid the macOS
screenshot bindings on `Cmd+Shift+3/4/5`.

If a combination is already held by another application, macOS refuses to give it to Snapdeck. The
rebind is then refused as a whole rather than half applied, your previous binding is left as it was
on disk, and the window tells you the shortcut is not currently bound. The rest of your changes
still save: a save that only moves the folder does not fail because a shortcut is contested.

**Save folder.** `~/Pictures` unless you choose another. The folder you pick is proved writable by
writing to it, not by inspecting a permission bit, so a folder that will not work is refused while
you are still looking at the dialog. If it stops being writable later, because a volume was
unplugged or a permission changed, the capture falls back to `~/Pictures` and says so rather than
losing the picture.

**Filename template.** `Snapdeck {date} at {time}` by default. The tokens are `{date}`, `{time}`,
`{width}` and `{height}`, and the last two are the file's real pixel dimensions. Anything that
looks like a path separator becomes a hyphen, so a template cannot write outside the folder you
chose, and a name that is already taken gets a numeric suffix rather than overwriting the file that
is there.

**Default format.** PNG or JPEG, PNG by default. A screenshot is the worst case for a lossy
encoder: it is mostly hard edges, and most of those edges are letters. JPEG is there for when the
file has to be small enough to send.

It decides more than the size of the file. This is the format every capture is written in, so it is
also the format the editor opens on and the one a Save replaces the capture in. With JPEG chosen it
is saving as **PNG** that writes a second file and leaves the original beside it, which is the case
to keep in mind when the point of an edit was to be rid of the original.

**Open the editor after a capture.** On by default. Turn it off and a capture is written and
copied without the editor appearing, which is the faster path when you already know you are not
going to annotate it.

**Launch at login.** Off by default.

**Check for updates at launch.** Off by default, and deliberately so: it is the only network
connection Snapdeck makes, and one that happens because nobody turned it off is not one you asked
for. `Check for Updates…` in the menu bar works whatever this is set to.

Settings live in `~/Library/Application Support/com.snapdeck.app/settings.json`. You can edit it by
hand; Snapdeck reads it on launch. A missing field is filled in from the default, a misspelled one
makes Snapdeck complain rather than quietly ignore what you meant, and a file it cannot parse at
all is reported in the log and replaced with the defaults, so a bad edit never stops the app from
starting.

## Architecture

Six parts, in one workspace, with the dependency arrow pointing one way.

```
┌─────────────────────────────────────────────────────────────────┐
│ apps/desktop                                the Tauri shell     │
│                                                                 │
│  src-tauri (Rust)          │  src (React 19)                    │
│  tray, global shortcuts,   │  overlay.html   the selection UI   │
│  overlay windows, editor   │  editor.html    hosts <Editor/>    │
│  windows, settings file,   │  settings.html  the settings form  │
│  file writing, clipboard,  │                                    │
│  updater, per-window ACL   │  each window gets only the ACL     │
│  capabilities              │  capability it needs               │
└───────────┬─────────────────────────────────┬───────────────────┘
            │                                 │
            ▼                                 ▼
┌───────────────────────────┐   ┌─────────────────────────────────┐
│ crates/capture            │   │ packages/editor                 │
│ snapdeck-capture          │   │ @snapdeck/editor                │
│                           │   │                                 │
│ ScreenCapturer trait,     │   │ layer model, undo stack, hit    │
│ frame and geometry types, │   │ testing, renderer, export       │
│ a mock for tests, and the │   │ ── no React below this line ──  │
│ macOS ScreenCaptureKit    │   │ <Editor/>, a React component    │
│ implementation            │   │ that knows nothing of its host  │
│                           │   │                                 │
│ no Tauri, no windows,     │   │ no Tauri: saving and copying    │
│ no files                  │   │ leave through props as a Blob   │
└─────────────┬─────────────┘   └─────────────────────────────────┘
              │
              ▼
┌───────────────────────────┐   ┌─────────────────────────────────┐
│ crates/frame              │   │ crates/stitch                   │
│ snapdeck-frame            │   │ snapdeck-stitch                 │
│                           │   │                                 │
│ Frame, PixelFormat, Rect, │◀──│ finds how far one frame         │
│ the capture error type:   │   │ scrolled past the last and      │
│ plain data, so a crate    │   │ joins a run of them into one    │
│ that only reasons about   │   │ picture                         │
│ pixels never links a      │   │                                 │
│ platform capture          │   │ pixels in, pixels out: no       │
│ framework                 │   │ screen, no window, no file      │
└───────────────────────────┘   └─────────────────────────────────┘

              the browser side, over a loopback bridge

┌───────────────────────────┐   ┌─────────────────────────────────┐
│ apps/extension            │──▶│ packages/protocol               │
│ @snapdeck/extension       │   │ @snapdeck/protocol              │
│                           │   │                                 │
│ MV3: measures the page,   │   │ the bridge contract as zod       │
│ plans the scroll, joins   │   │ schemas, imported by the        │
│ the layers on a canvas,   │   │ extension and mirrored by the   │
│ hands the PNG to the app  │   │ desktop app, which reads this   │
│                           │   │ file in its own tests so the    │
│ falls back to a download  │   │ two cannot drift apart          │
│ when the app is not there │   │                                 │
└───────────────────────────┘   └─────────────────────────────────┘
```

**`crates/capture`** is the capture core. It defines what a screen capturer is and what a frame is,
and the macOS implementation behind `cfg(target_os = "macos")` is the only one that exists. It hands
back pixels and the scale factor they were captured at, and knows nothing about where they go.

**`packages/editor`** is the annotation editor as a library, and it is free of Tauri on purpose.
Everything below the `Editor` component is plain TypeScript over the canvas 2D API, so the same
model, renderer and export could be driven by a browser extension. `Editor` itself takes its host
through props: a save is a `Blob` leaving through a callback, not a file being written.

**`crates/frame`** holds the picture types and nothing else. They live apart from
`crates/capture` because they are plain data: a crate that only reasons about pixels, such as
`crates/stitch`, can take a `Frame` without linking a platform's capture framework and the Swift
runtime behind it.

**`crates/stitch`** joins overlapping frames. It measures how far one frame scrolled past the one
before it, drops the rows they share and pastes the rest, and it knows about pixels and nothing
else, which is what lets every case in its tests be built by hand and compared byte for byte.

**`packages/protocol`** is the bridge contract, written once as zod schemas. The extension imports
it; the desktop app mirrors it in Rust and reads these very files in its tests, so the two
languages cannot quietly come to disagree about what a message is.

**`apps/extension`** is the Chrome extension that captures a whole page, described in
[docs/EXTENSION.md](docs/EXTENSION.md). It asks for no host permissions, hands the finished PNG to
the desktop app over a connection that never leaves the machine, and falls back to an ordinary
download when the app is not running.

**`apps/desktop`** is the only part that knows this is a Mac app. Rust owns the tray, the shortcuts,
the overlay and editor windows, the settings file, the writing and the updater; the React side is
three thin entry points around code that lives elsewhere.

[CONTRIBUTING.md](CONTRIBUTING.md) says which one to open for a given change, and how to build and
test them.

## License

MIT, see [LICENSE](LICENSE).

## Known limitations

- **Fullscreen Spaces.** Capturing while another app owns a fullscreen Space does not work yet. The
  overlay window is created with the frozen frame intact, but macOS keeps it on a normal Space, so
  nothing appears over the fullscreen app. Setting `canJoinAllSpaces | fullScreenAuxiliary`, raising the
  window level, ordering the window in on the same run loop turn and activating the app were all tried
  and none of them place the overlay above another app's fullscreen Space.
- **Multiple displays are unverified.** Every display is frozen and covered, and the code paths for
  it are there, but no part of it has been exercised on a real multi-display machine.
- **A capture writes a copy of your screen to disk.** Freezing the screen means saving a
  full-resolution, lossless PNG of every display under `~/Library/Caches`. They are deleted as soon
  as the capture ends, cancelled or not, but a crash or a force quit leaves them behind until the
  next capture overwrites them.
- **A window outline can disagree with the frozen pixels.** Window mode lists the windows after the
  screen has been frozen, so a window that moves in between is outlined where it now is rather than
  where the frozen frame shows it.
- **Closing an editor throws away unsaved annotations without asking.** The capture itself is
  never at risk, since the file and the clipboard are finished before the editor opens, but
  `Esc`, the Close button and the title bar's red button all discard whatever has been drawn and
  not saved. Installing an update does the same, because it restarts Snapdeck; the update dialog
  says so before you agree to it.
- **One shortcut another app holds takes all three down.** The three bindings are registered as a
  set or not at all, so a combination macOS refuses, usually because another app already holds it,
  takes the other two with it rather than leaving them half working. Snapdeck does not stop there:
  at launch it falls back to the built-in three and says so, and a rebind that is refused puts the
  bindings you had back. The keyboard is left empty only when there is nothing left to fall back
  on. The settings window says which combination was refused and shows what is actually bound, and
  the menu bar item captures in every mode either way.
- **No screen recording.** Snapdeck takes still captures; it does not record video.

