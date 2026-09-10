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
one it makes at all, and it is off until you turn it on.

- **Check for Updates…** in the menu bar asks the releases page whether there is a newer version.
  If there is, Snapdeck shows what it is and what the release says about it, and downloads nothing
  until you press **Update and Restart**. That one button does the whole thing: download, replace,
  restart.
- **Settings > Updates** has a checkbox for the same check at launch. It is **off by default**, and
  the menu item works whether or not you turn it on.

Every update is signed. The archive Snapdeck downloads carries a signature made by this project's
release workflow, checked against a public key built into the copy you are running. A download whose
signature does not verify is thrown away rather than installed, and Snapdeck puts the reason in the
menu bar and in its log instead of failing quietly.

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
own time zone, and is put on the clipboard at the same time, ready to paste. The region is
captured again at the display's own pixel density rather than cropped out of the frozen frame, so
on a Retina display the file is twice the size in pixels that you selected in points.

That second capture happens the moment you press `Enter`, not when the screen froze, so the file
holds the screen as it is then rather than the frozen frame you selected against. For still
content the two are the same picture; over a video, an animation or anything else that moves, the
saved pixels are the ones from the instant you confirmed.

## The editor

Every capture that produced a file opens in an editor window: the title bar carries the file's
name, and the picture is shown at its own size or fitted down if it does not fit the screen.

The editor is an offer, not a step. By the time it appears the capture is already finished: the
PNG is in `~/Pictures` and the image is on the clipboard. Closing the window without touching
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

`PNG` and `JPEG` beside `Save` choose the format, and `PNG` is where it starts.

Saving as PNG writes the edited picture back over the capture in `~/Pictures`, under the same
name, by writing a temporary file beside it and renaming it into place: a failed save leaves the
picture you already had rather than half of a new one. Saving as JPEG cannot overwrite a PNG, so
it writes a second file beside the capture, same name, `.jpg` instead of `.png`, and leaves the
original untouched. Either way the status bar names the file that was written.

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

Saving as PNG replaces the capture by renaming a new file over it, which replaces the file itself
rather than its contents. Finder tags, other extended attributes and the original creation date
belong to the file that is replaced and do not survive the save; the name, the location and the
pixels do.

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

**Saving as JPEG.** A JPEG cannot overwrite a PNG, so that save writes a second file and leaves the
capture where it is: the unredacted PNG stays in `~/Pictures` beside the redacted JPEG, under the
same name. If the point of the redaction was that the original should not exist, save as PNG, which
replaces the capture, or delete the PNG yourself.

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
  not saved.
- **A partly failed shortcut registration leaves some shortcuts dead.** Registration stops at the
  first shortcut macOS refuses, usually because another app already holds it, and the ones after it
  are never registered. The menu bar item captures in every mode either way.

