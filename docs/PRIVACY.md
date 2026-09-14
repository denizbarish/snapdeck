# Privacy policy

**This is the privacy policy for the Snapdeck browser extension, and it is the document the Chrome
Web Store listing points at.** It also covers the Snapdeck desktop app, because the extension hands
its captures to that app and the two are only honest about privacy together.

Last updated: 14 September 2026. It applies to Snapdeck 0.2.0 and to the extension built from the
same repository.

## The short version

Snapdeck collects nothing. There is no account, no analytics, no telemetry, no crash reporting, and
no server of ours for anything to be sent to. A captured page goes from the browser to the app on
the same machine and stops there.

## What the extension handles

When you press the toolbar button, the extension captures the tab you pressed it on: a PNG of the
whole page, that tab's address, and that tab's title. Those three things are what a capture is made
of, and they exist so the app can save the picture and name the file.

They are handed to Snapdeck over a WebSocket to `127.0.0.1`, the loopback address of your own
machine. That is the only socket the extension opens, and loopback traffic does not reach a network
interface, so it cannot leave the computer it started on.

The extension asks for `activeTab` rather than host permissions, so this ability exists for the one
tab you pointed it at and for the length of that one press. It has no standing permission to read
any page.

## What the extension stores

One value, in `chrome.storage.local`, under the key `bridgeToken`: the 64-character pairing token
you copy out of Snapdeck's settings. That is the whole of it. Nothing else is written, and no
history of what you captured is kept.

`local` rather than `sync` on purpose: the token pairs this browser with the app on this machine, so
syncing it would push a secret to machines where it pairs nothing.

## Where a capture ends up

- **When Snapdeck is running and paired:** the app saves the PNG into the folder chosen in its
  settings, `~/Pictures` unless you picked another one, under the file name template you set.
- **When it is not:** the extension writes the PNG to your browser's downloads folder as
  `snapdeck-<host-and-path>-<UTC timestamp>.png`. The page's query string is deliberately left out
  of that name, because it is where password reset tokens and signed URL parameters live and a file
  name would carry them into the downloads folder and the browser's download history.

Either way the file is on your disk and nowhere else.

## What the desktop app does with the network

Snapdeck's settings live in `~/Library/Application Support/com.snapdeck.app/settings.json`, on your
machine. Captures are files in the folder you chose.

The app makes exactly one kind of outbound connection: the update check, which asks
`https://github.com/denizbarish/snapdeck/releases/latest/download/latest.json` whether there is a
newer release. **Check for updates at launch is off by default**, so this happens only if you turn
that on or press Check for Updates in the menu bar. It sends nothing about you: it is a request for
a public file, and GitHub sees it the way it sees any download of a public release asset. No
capture, no page address, and no setting is ever part of it.

## What is not done

- No data is sold, shared, or transferred to anyone.
- No data is used for advertising, profiling, or creditworthiness.
- No browsing activity is collected. The extension reads a page only when you press its button on
  that page, and what it reads goes to your own machine.
- The extension executes no remote code. Everything it runs is in the package you installed.

## Questions

Snapdeck is open source under the MIT license. The code that backs every sentence above is at
<https://github.com/denizbarish/snapdeck>, and questions belong in that repository's issues.
