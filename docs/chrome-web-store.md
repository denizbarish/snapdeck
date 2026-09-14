# Publishing the extension to the Chrome Web Store

Everything below is meant to be copied into the developer dashboard as it stands. Each claim is
taken from the extension's own code, not from what the extension might plausibly do, and the store
rules quoted here are from Google's documentation, cited at the end.

## 1. The package

```bash
pnpm --filter @snapdeck/extension package
```

That runs the ordinary build (two Vite passes and `check-manifest.mjs`) and then writes
`apps/extension/dist-zip/snapdeck-extension-<version>.zip`. The archive has `manifest.json` at its
root, which is what the store looks for: "Make sure you place the manifest file in the **root
directory**, not in a folder."

The ZIP is ten files and about 39 KB:

```
background.js      chunks/token.js    content.js
icons/16.png       icons/32.png       icons/48.png     icons/128.png
manifest.json      options.html       options.js
```

No source maps, no `.DS_Store`, no wrapper folder. The script prints both what it packed and what it
left out, so a stray file in `dist/` is something you read rather than something you ship.
`dist-zip/` is in `.gitignore`; the ZIP is a build artefact and is not committed.

## 2. Store listing

### Name (75 character limit)

```
Snapdeck
```

8 characters.

### Short description (132 character limit)

```
Capture a whole web page and hand it to the Snapdeck app.
```

57 characters.

Both of these come from `manifest.json` and are not editable in the dashboard after upload. To
change either one you edit `apps/extension/public/manifest.json`, raise the version, and upload a
new ZIP.

### Detailed description

```
Snapdeck captures a whole web page, not just the part that fits on screen, and hands the picture to the Snapdeck app on your Mac.

Press the toolbar button. The extension measures the page, scrolls it one viewport at a time, photographs each viewport, and joins the results into a single PNG. That PNG goes to Snapdeck over a connection that never leaves your machine, and Snapdeck saves it exactly the way it saves any other capture: same folder, same file name template, same format, and the editor opens on it if you have that turned on.

If Snapdeck is not running, or the two have not been paired yet, the capture is not thrown away. It is written to your downloads folder instead, and the toolbar icon wears a badge whose tooltip says what happened and why.

PAIRING

Open Snapdeck's settings, find the Browser Extension section, and copy the pairing token. Open this extension's options page, paste the token, press Save, then press Test connection. Test connection performs the handshake and stops there: a button pressed to check a token should not upload a page.

That token is the only thing this extension stores.

WHAT IT DOES NOT DO

- No account and no sign-in.
- No analytics, no telemetry, no tracking of any kind.
- No server. The only connection the extension opens is to 127.0.0.1, your own machine.
- No host permissions. It can act on the tab whose button you pressed, for as long as that press lasts, and on no other tab. An extension that can only touch the tab you pointed it at cannot read the ones you did not.

KNOWN LIMITS, HONESTLY

- Images that load as you scroll get 250 ms per step to arrive. Most make it; one that takes seconds will not, and no wait would turn that into a promise.
- A list that swaps its rows in and out as you scroll inside its own container is captured as the slice that was on screen. The rest of it is not in the page's height, so there is nothing to scroll to.
- There is a ceiling of 64 million pixels on the finished image. A page past it is captured up to the ceiling rather than failing, and Snapdeck says so.

Snapdeck is free and open source under the MIT license. The desktop app is a macOS build; the extension works without it, with captures landing in your downloads folder.

Source, documentation and issues: https://github.com/denizbarish/snapdeck
```

Roughly 2,200 characters. The dashboard's detailed description field is far larger than that, so the
limit is not a constraint here; note that Google's public documentation describes the field without
stating a number, so treat the dashboard's own counter as the authority rather than any figure
quoted from memory.

### Category

**Art & Design.** Google's own description of that category is the reason: "These extensions provide
tools for viewing, editing, organizing, and sharing images and photos. They may also offer features
for capturing screenshots, image searching, and integrating with popular image hosting or editing
services." A screenshot tool is named in it explicitly.

The runner-up is **Tools**, which is where a utility with no better home goes. Art & Design is the
better fit because the extension's output is an image and its companion is an image editor, and
because a category that names the thing you do is worth more in search than a generic one.

### Language

English.

## 3. Privacy practices tab

### Single purpose description

```
Snapdeck captures the full length of the web page in the active tab as one PNG image and delivers that image to the user's Snapdeck desktop application over a local loopback connection, falling back to the browser's downloads folder when the application is not available. That is its only function.
```

### Permission justifications

**`activeTab`**

```
The extension captures the page in the tab whose toolbar button the user just pressed. activeTab is what grants the access needed to do that, scoped to that one tab and that one press: the extension photographs the visible area with chrome.tabs.captureVisibleTab and reads the tab's address and title so the resulting file can be named. activeTab is requested instead of host permissions precisely so that the extension holds no standing access to any site; it can act only on a tab the user has pointed it at, and only for the duration of that action.
```

**`scripting`**

```
A full-page capture needs the page measured and scrolled one viewport at a time. That work is done by a content script injected with chrome.scripting.executeScript at the moment of a capture, rather than by a content script declared in the manifest that would be present in every page the user opens. The scripting permission is what allows that on-demand injection. The injected script only measures the document, hides position-fixed elements during the scroll, and scrolls; it reads no page content and sends nothing anywhere.
```

**`storage`**

```
One value is stored: the 64-character pairing token that links this browser to the user's Snapdeck desktop application, written to chrome.storage.local under the key bridgeToken. Nothing else is stored. No captured image, no page address, and no history of captures is written to storage. chrome.storage.local rather than sync, because the token pairs this browser with an application on this one machine.
```

**`downloads`**

```
A full-page capture costs the user a scroll of the entire page, so it is never discarded. When the Snapdeck desktop application is not running, or the two have not been paired, the finished PNG is written to the user's downloads folder with chrome.downloads.download instead of being lost. This is the extension's fallback path and its only use of the permission.
```

**Host permissions:** none are requested. The manifest has no `host_permissions` key, so this field
should not appear. If a reviewer asks, the answer is that `activeTab` covers every case.

### Remote code

Select **"No, I am not using remote code."** If a justification field is offered:

```
The extension executes only the JavaScript contained in the uploaded package. It loads no remotely hosted script, evaluates no fetched string as code, and references no CDN. The only network socket it opens is a WebSocket to ws://127.0.0.1:51837, the loopback address of the user's own machine, and the only messages on it are the two defined frames of the local pairing protocol, neither of which carries code.
```

### Data usage

Leave every "what data does your extension collect" box **unchecked**, and tick all of the
compliance certifications.

The reasoning to have ready: Google's disclosure asks what the extension *collects*, which its
policies define as transferring data off the user's device. Snapdeck transfers nothing off the
device. The captured image travels from the browser to a process on the same machine over loopback,
or to that machine's downloads folder, and either way never reaches a network interface. There is no
server, no analytics endpoint, and no account. If a reviewer raises "website content" because the
extension does photograph a page, the one-line answer is that the content is handled on-device only
and is never transmitted anywhere.

### Privacy policy URL

```
https://github.com/denizbarish/snapdeck/blob/main/docs/PRIVACY.md
```

The file is [`docs/PRIVACY.md`](PRIVACY.md) in this repository. The URL works once that file is on
`main` and the repository is public, which are both conditions the store checks by fetching it.

## 4. Note for reviewers

Paste this into the "Notes for reviewers" / testing instructions field:

```
This extension is the browser half of Snapdeck, a free and open source screenshot tool for macOS. Full source, including the source of this extension, is at https://github.com/denizbarish/snapdeck and the extension is documented at https://github.com/denizbarish/snapdeck/blob/main/docs/EXTENSION.md

You do not need the desktop application to review the extension, and you do not need a macOS machine.

Without the application:
1. Install the extension and open any long web page.
2. Press the Snapdeck toolbar button.
3. The page scrolls down one viewport at a time and is then returned to where it was. A full-page PNG is saved to your downloads folder as snapdeck-<host-and-path>-<timestamp>.png, and the toolbar icon shows a red "!" badge whose tooltip explains that Snapdeck is not running so the capture went to downloads.

That is the extension's complete, intended behaviour when it is unpaired, and it exercises every permission the manifest asks for except the loopback delivery.

With the application, for completeness:
1. Snapdeck's Settings window has a Browser Extension section containing a pairing token, which is 32 bytes of randomness as 64 hexadecimal characters.
2. Copy it, open this extension's options page, paste it into the Token field, press Save.
3. Press Test connection. It performs the handshake against 127.0.0.1:51837 and reports the result without uploading anything.
4. Press the toolbar button on any page. The capture is delivered to the application instead of the downloads folder.

The extension opens no connection other than that loopback WebSocket, and stores nothing other than the pairing token.
```

## 5. Store images

What the store asks for, and what this repository already has.

| Asset | Requirement | Status |
| --- | --- | --- |
| Store icon | 128x128 PNG | **Ready.** `apps/extension/public/icons/128.png` is exactly 128x128 and ships in the ZIP as `icons/128.png`. Upload that same file. |
| Screenshots | At least 1, at most 5. 1280x800 or 640x400, square corners, full bleed | **Ready.** `docs/images/store/screenshot-editor-1280x800.png` and `docs/images/store/screenshot-overlay-1280x800.png`, both exactly 1280x800. |
| Small promotional tile | 440x280 PNG | **Ready.** `docs/images/store/promo-440x280.png`, exactly 440x280. |
| Marquee tile | 1400x560 PNG, optional | Missing, and optional. |

Google's advice on the icon is worth knowing before anyone redraws it: the artwork should occupy
96x96 with 16 pixels of transparent padding on each side, adding up to 128x128. The current file is
the right size; whether its artwork sits inside that inset is a design question, not a blocker.

### Where the store images come from

All three are written by the harness in `tools/screenshots`, in the same run as the README's
pictures:

```bash
pnpm screenshots
```

They are rendered at the store's sizes rather than cropped down from the README's images, and the
difference matters. A centre crop of `editor.png` to 16:10 cuts off the toolbar or the status bar,
which are the two things a picture of an editor is of; rendered at 1280x800, the interface lays
itself out at that size and the capture inside it is still shown at 1:1. The overlay gets a second
render of the demo page at 1280x800 for a plainer reason: its backdrop fills the window, so a frozen
frame of any other shape would be stretched. Both are taken at a device pixel ratio of 1, because
the store asks for 1280x800 and means pixels; a 2x capture is 2560x1600 and is refused.

The promotional tile is the one image here that is not the product running. It draws no interface
and mocks up no screen: it is `apps/extension/public/icons/128.png`, the name, and the description
already in `apps/extension/public/manifest.json`, on the accent colour the editor and the settings
window use for a pressed control, full bleed with no padding and no white border. The text is not
written for the store; changing the manifest's description and re-running the harness changes the
tile with it.

Both screenshots are taken on `docs/images/demo-page.html`, a fictional analytics page with invented
customers and `sk_live_` strings that are keys to nothing. No real screen, and nobody's data, is in
any of them.

One thing worth weighing before uploading them: both show the desktop app, not the extension. The
store asks that screenshots "demonstrate the actual user experience", and a reviewer comparing a
browser extension's listing against pictures of a Mac application may reasonably ask why. A shot of
the options page and a shot of a page mid-capture with the toolbar badge would represent this
listing better, but both would be new images and neither exists yet.

## 6. How the version moves

The store's rule is that every upload must carry a version strictly larger than the last one, and
`manifest.json` is where it reads that. This repository keeps a version in four places that
`release.yml` compares against the tag (`tauri.conf.json`, the workspace `Cargo.toml`,
`apps/desktop/package.json`, and the tag itself), and the extension's `manifest.json` is deliberately
not one of them: no CI job fails if it falls behind, so the discipline has to be yours. The rule to
follow is that a store upload rides on a release. When a release raises the repository to `0.3.0`,
raise `apps/extension/public/manifest.json` and `apps/extension/package.json` to `0.3.0` in the same
commit, run `pnpm --filter @snapdeck/extension package`, and upload the ZIP that produces. When the
store needs a second upload without a release behind it, a rejected review fixed by a listing change
or a one-line manifest correction, add a fourth component instead of inventing a release:
`0.3.0.1`, then `0.3.0.2`. Chrome accepts one to four dot-separated integers between 0 and 65535, so
those sort after `0.3.0` and satisfy the store without disturbing the three-part number the release
workflow checks.

## Sources

The rules quoted above, and where each one comes from.

- ZIP with the manifest at the root; the 132-character description limit; every upload needs a
  larger version: [Prepare your extension](https://developer.chrome.com/docs/webstore/prepare)
- 75-character name limit:
  [Manifest - name](https://developer.chrome.com/docs/extensions/reference/manifest/name)
- Version format, one to four integers from 0 to 65535:
  [Manifest - version](https://developer.chrome.com/docs/extensions/reference/manifest/version)
- Screenshots at 1280x800 or 640x400, one to five of them, square corners and full bleed; the
  128x128 store icon and its 96x96 artwork inset; the 440x280 small promotional tile:
  [Supplying images](https://developer.chrome.com/docs/webstore/images)
- Single purpose, permission justifications, remote code declaration, data use certification and
  the privacy policy link:
  [Fill out the privacy fields](https://developer.chrome.com/docs/webstore/cws-dashboard-privacy)
- Category descriptions:
  [Best practices](https://developer.chrome.com/docs/webstore/best_practices)
- What "collect" means for the data disclosure:
  [Limited use](https://developer.chrome.com/docs/webstore/program-policies/limited-use)

The detailed description's character limit is the one figure here with no public documentation
behind it. The dashboard shows a live counter for that field; trust it.
