# The browser extension

Snapdeck captures what is on screen. A web page is usually taller than the screen, so the part of
it you would have to scroll to is not something a screen capture can reach. The extension is how
that part gets captured: Chrome scrolls the page for it, photographs each viewport, joins the
pictures into one PNG, and hands that PNG to Snapdeck over a connection that never leaves this
machine.

What arrives is not a second kind of capture. It goes through the same save path a region capture
does, so the **Folder**, **File name** and **Format** settings, and **Open the editor after a
capture**, all apply to it exactly as they apply to a region capture.

The extension is not on the Chrome Web Store. It is loaded from a folder you build, which is why
the first two sections below exist.

## Installing

Build it, then load the folder the build produced:

```bash
pnpm --filter @snapdeck/extension build
```

The build is two Vite passes, one for the extension's pages and one for the content script it
injects, followed by a check that every file `manifest.json` names is actually there. It writes
`apps/extension/dist`.

Then, in Chrome:

1. Open `chrome://extensions`.
2. Turn on **Developer mode**, top right.
3. Press **Load unpacked** and choose `apps/extension/dist`.

Chrome keeps the folder, not a copy of it, so rebuilding and pressing the reload arrow on the
extension's card is the whole update loop. Chrome assigns the extension an id when you load it, and
that id survives rebuilds of the same folder.

## Pairing

The extension and the app are paired by a token, and until they are, a capture goes to your
downloads folder rather than to Snapdeck.

1. In Snapdeck, open **Settings**, then **Browser Extension**, and press **Copy** next to the
   pairing token. The token is 64 characters: 32 bytes of system randomness written as hexadecimal.
2. Open the extension's options page, either from its card on `chrome://extensions` or by
   right-clicking its toolbar icon and choosing **Options**.
3. Paste the token into **Token** and press **Save**. The page checks the shape before it stores
   anything, so a paste that lost a character or brought a newline along is a sentence next to the
   field rather than a refusal from another window later.
4. Press **Test connection**. It performs the handshake and stops there. A button pressed to check
   a token should not upload a page, so this one does not.

**Regenerate** in Snapdeck's settings mints a new token and invalidates the old one. Every paired
browser has to be given the new one.

The token is kept in `chrome.storage.local`, not `sync`: it pairs this browser with the app on this
Mac, and syncing it would push a secret to machines where it pairs nothing.

## What the extension is allowed to do

`manifest.json` asks for four permissions and no host permissions at all.

- **`activeTab`.** The permission to touch the page in the tab whose toolbar button you just
  pressed, and only for as long as that press lasts. It is what lets the extension photograph the
  visible part of the tab and read that tab's address and title.
- **`scripting`.** The measuring and scrolling are done by a content script the extension injects
  into the page at the moment of a capture, rather than by a script standing in every page you
  visit waiting for one. The injection needs this.
- **`storage`.** One value: the pairing token.
- **`downloads`.** Where a capture goes when Snapdeck is not there to take it.

There is deliberately no **`host_permissions`** entry. A `host_permissions` list is a standing
grant: `"<all_urls>"` would let this extension read every page you open, for as long as it is
installed, whether or not you ever press its button. `activeTab` is the same power narrowed to one
tab and one press, granted by that press and gone afterwards, and it is enough for everything the
extension does. So the standing grant is not asked for: an extension that can only act on the tab
you pointed it at cannot read the ones you did not.

There is also no **`notifications`** permission. The action's badge and its tooltip are the whole
vocabulary the extension has for telling you anything, which is why the failure cases below are
described as badges.

## What the bridge does, and what it does not

The bridge is a WebSocket server inside Snapdeck. It listens on `127.0.0.1:51837` and nowhere else.

What it does:

- **Binds loopback only.** The listener is bound to `127.0.0.1`, not to every interface, so there
  is no socket for another machine on your network to reach. This is the first and strongest of the
  gates, because it is not a check that can be got past by sending the right bytes.
- **Refuses a handshake that is not from an extension.** The upgrade request has to carry an
  `Origin` of the form `chrome-extension://` followed by a 32-character extension id. A web page's
  `Origin` is its own site, and a page cannot forge that header, so a script on an ordinary page
  that opens `ws://127.0.0.1:51837` is answered with a 403 and never becomes a session.
- **Checks `Host` as well.** `Origin` already keeps pages off the socket, but a name that an
  attacker points at `127.0.0.1` would arrive with that name in `Host`, and a server that never
  looked would answer it. The bridge accepts only `127.0.0.1` or `localhost` followed by the port
  it actually bound. Two locks on the same door, because DNS rebinding is the attack that gets past
  one of them.
- **Checks the token.** The first frame of a session is a `hello` carrying the pairing token. A
  wrong one is an `unauthorized` and the connection closes.
- **Proves itself to the extension.** This is the half that runs the other way, and it is there
  because anything running as you can bind a loopback port. Suppose something took port 51837
  before Snapdeck did. The extension connects, sends its token, and a program that only had to
  invent a plausible-looking answer would then be handed a picture of your logged-in pages. So the
  extension's `hello` carries a fresh 16-byte challenge, and the app's `ready` has to answer it
  with `HMAC-SHA256(key = the token, message = the challenge)`. The extension recomputes that
  proof and compares it in constant time, and **no page is sent** until it matches. Something that
  does not know the token cannot produce the answer.
- **Reads a bounded frame.** Before a token has been checked the socket accepts at most 4 KiB per
  message, which is more than any `hello` needs; the full limit is raised only afterwards. Every
  field of every frame has a maximum, on both sides: the PNG, the pixel count, the URL, the title,
  the request id and the device pixel ratio are all checked against the same numbers by the zod
  schema in `packages/protocol` and by the serde mirror of it in Rust.

What it does not do:

- **It does not go to the network.** Nothing here reaches off the machine. Snapdeck's only outbound
  connection is the update check, which is a separate setting and is off unless you turn it on.
- **It does not carry anything but a captured page.** The protocol has exactly two frames the
  extension can send, `hello` and `fullPage`. There is no message for triggering a capture, listing
  files, reading settings or opening a window.
- **It does not pin the extension's identity.** Any extension id of the right shape passes the
  `Origin` gate, because a development install gets a different id on every machine and pinning one
  would make the extension unloadable. The token is what makes a session yours; the `Origin` check
  is what keeps web pages out.
- **It is not encrypted.** `ws://`, not `wss://`, on loopback. TLS on a connection that never
  leaves the machine buys a certificate problem and no security: anything that could read this
  socket could read Snapdeck's memory.

## When Snapdeck is not running

The capture cost you a scroll down the whole page, so it is never thrown away. If it cannot be
handed over, it is written to your downloads folder as
`snapdeck-<host-and-path>-<UTC timestamp>.png`, and the toolbar icon wears a red **!** badge whose
tooltip says where the capture went.

The query string is deliberately left out of that name. It is where password reset tokens and
signed URL parameters live, and a file name carries them into the downloads folder, the browser's
download history, and whatever syncs that folder somewhere else.

The badge's tooltip is the whole message, and it is not the same sentence every time, because the
cases need different things from you:

| What the tooltip says | What happened |
| --- | --- |
| Snapdeck is not running, so the capture went to your downloads instead. | Nothing accepted a connection on the port. |
| Snapdeck and this extension are not paired yet… | No token has been saved in the options page. |
| Snapdeck did not accept the pairing token… | The app is running and the token is wrong. Copy it again. |
| Snapdeck and this extension were built for different bridge protocol versions… | One of the two is older. Pairing again will not help. |
| Something is listening on Snapdeck's bridge port (51837) but could not prove it is Snapdeck… | See below. |

### "Something is listening on Snapdeck's bridge port"

This one is not a variation on "the app is closed", and it is why the two have separate wording.
Something accepted the connection and then failed to answer the challenge, which means it does not
hold your pairing token, which means it is not Snapdeck. The capture was **not** sent to it; it
went to your downloads folder instead.

Telling you the app was not running would be the wrong advice here: you would open Snapdeck, find
that the port is already taken, and the program that wants your pages would still be holding it.
Quit whatever else is on port 51837, then try again.

Snapdeck says the same thing from its own side. If it could not bind the port at startup, the
**Browser Extension** section of its settings window carries a warning instead of the usual
"Snapdeck is listening on port 51837" line, and it names the reason it was given.

## Known limits

### Lazy-loaded images

The capture loop scrolls one viewport at a time, waits 250 ms for whatever the scroll started to
finish, and then photographs what is on screen. An image that has not finished loading by then is
photographed as whatever the page is showing in its place, and 250 ms is a wait, not a guarantee.
The design spec says as much; this is what it measures out to.

Measured on a synthetic fixture: 5 viewport-tall sections, one `loading="lazy"` image each, served
from a local server that answers after a fixed delay, driven through the same scroll-settle-look
order the capture loop uses, in headless Chromium at a 1280x800 viewport.

| Settle | Images in view but not yet decoded when the capture would fire |
| --- | --- |
| 0 ms | 4 of 5 (the 100 ms, 300 ms, 800 ms and 2000 ms images) |
| **250 ms**, what ships | **1 of 5** (the 2000 ms image) |
| 1000 ms | 0 of 5 |
| 3000 ms | 0 of 5 |

Two things are worth reading off that. The wait does most of the work: without it, an image that
answers in a tenth of a second is already missed. And it buys more than its own length, because
Chrome starts fetching a lazy image well before it scrolls into view, so the 800 ms image had been
in flight for several steps by the time its own step arrived. What 250 ms does not survive is a
resource that takes seconds, and there is no wait that would make that a promise rather than a bet.

This was measured against the scroll loop, not through the extension itself: driving a real
`chrome.tabs.captureVisibleTab` needs a real toolbar press. It says when the pixels are ready, not
what the composite looked like.

### Virtualized lists

A virtualized list keeps only the rows near its own viewport in the DOM and never makes the page
taller. The capture measures the page with
`max(documentElement.scrollHeight, body.scrollHeight)`, so what it plans to capture is what the
document says it is, and the list's undrawn rows are in neither number.

On the same fixture, a 500-row list inside a 400 px scroller: the inner content is 20000 px tall,
**13 of the 500 rows exist in the DOM** at any moment, and the page's own `documentHeight` of
5044 px does not include the other 19600 px. Scrolling the page does not scroll the list, so the
capture contains the list's visible 400 px slice and nothing else.

This is not a wait that can be lengthened. Anything that scrolls in its own container rather than
with the document has the same shape, and the honest statement is that a full-page capture of it is
not something the extension can promise.

### Pages that are too long

There is a ceiling of 64 million pixels on the composite, which at a typical width and a Retina
device pixel ratio is a very long page. Past it the capture stops early rather than failing, and
Snapdeck says so when it arrives: the page was too long to capture in full, so it saved as much of
it as the browser could give.
