import { MAX_IMAGE_PIXELS, MAX_TITLE_CHARS, MAX_URL_CHARS } from '@snapdeck/protocol'

import type { ContentRequest, ContentResponse } from '../content/main'
import { readPairingToken } from '../options/token'
import { connectToBridge, sendFullPage, toBase64, type SendOutcome } from './bridge'
import { captureFullPage, type CaptureDeps, type FullPageCapture } from './capture'
import { badgeText, downloadInstead, fallbackFilename, unreachableBadge } from './fallback'

/**
 * The service worker: what happens between the user pressing the button and
 * the page ending up somewhere.
 *
 * Every decision it makes belongs somewhere else. The order of the capture is
 * `capture`, the arithmetic is `plan` and `composite`, the handshake is
 * `bridge` and the file name is `fallback`. What is left here is the wiring to
 * `chrome`, which is the one part no test can arrange, and it is deliberately
 * the thinnest layer in the extension.
 *
 * Only two things can go wrong from the user's side and both end the same way:
 * the page is never lost. If the app is not there, or refuses it, the capture
 * goes to the downloads folder and the action badge says so. No `notifications`
 * permission is asked for, so the badge and its tooltip are the whole
 * vocabulary this extension has.
 */

/**
 * How long lazy-loaded content is given to arrive after a scroll. It is free in
 * practice: Chrome's own quota already keeps captures further apart than this.
 */
const SETTLE_MS = 250

/**
 * Long enough for an app that is busy writing the last capture to answer, short
 * enough that a user watching a badge is not watching it for a minute.
 */
const BRIDGE_TIMEOUT_MS = 10_000

/** The badge's ground. The same red the options page says a refusal in. */
const BADGE_COLOUR = '#b3261e'

/** What a page with no address of its own is called on the wire. */
const UNKNOWN_URL = 'about:blank'

/**
 * The tabs a capture is already running in. A second press would drive the same
 * page from two loops at once, each scrolling out from under the other's
 * captures.
 */
const busy = new Set<number>()

function clip(text: string, limit: number): string {
  return text.length > limit ? text.slice(0, limit) : text
}

function reasonOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause)
}

/** One round trip to the content script. */
function ask(tabId: number, request: ContentRequest): Promise<ContentResponse> {
  return chrome.tabs.sendMessage<ContentRequest, ContentResponse>(tabId, request)
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms)
  })
}

/**
 * The page's metrics, injecting the content script first if nobody answers.
 *
 * The measurement doubles as the check that the script is there, because it is
 * the first thing the loop asks for anyway. Injecting unconditionally would add
 * a second copy of the listener on every press, and the two copies would each
 * keep their own list of the elements they hid.
 */
async function measureThrough(tabId: number, maxPixels: number): Promise<ContentResponse> {
  try {
    return await ask(tabId, { command: 'measure', maxPixels })
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ['content.js'] })
    return await ask(tabId, { command: 'measure', maxPixels })
  }
}

/**
 * Where the page was before any of this started, so it can be put back there.
 *
 * Read with a one-line injection rather than a message, because the content
 * script's `scrollTo` answers with where the page ended up and there is no
 * question it can be asked that does not move it first.
 */
async function readScrollY(tabId: number): Promise<number> {
  const [frame] = await chrome.scripting.executeScript({
    target: { tabId },
    func: () => window.scrollY,
  })
  return typeof frame?.result === 'number' ? frame.result : 0
}

async function captureVisible(windowId: number): Promise<ImageBitmap> {
  const dataUrl = await chrome.tabs.captureVisibleTab(windowId, { format: 'png' })
  // `fetch` on a `data:` URL needs no host permission, and it is the only way
  // to get from what Chrome hands back to something a canvas can draw.
  const response = await fetch(dataUrl)
  return createImageBitmap(await response.blob())
}

function depsFor(tabId: number, windowId: number, startedAt: number): CaptureDeps {
  return {
    measure: async () => {
      const answer = await measureThrough(tabId, MAX_IMAGE_PIXELS)
      if (answer.command !== 'measure') {
        throw new Error(`the page answered a measurement with a ${answer.command}`)
      }
      return answer.metrics
    },
    setPinnedHidden: async (hidden: boolean) => {
      await ask(tabId, { command: hidden ? 'prepare' : 'restore' })
    },
    scrollTo: async (y: number) => {
      await ask(tabId, { command: 'scrollTo', scrollY: y })
    },
    settle: () => sleep(SETTLE_MS),
    captureVisible: () => captureVisible(windowId),
    restoreScroll: async () => {
      await ask(tabId, { command: 'scrollTo', scrollY: startedAt })
    },
  }
}

/** The title the manifest gives the action, for putting back after a failure. */
function defaultActionTitle(): string {
  const manifest = chrome.runtime.getManifest()
  // An empty title is not a blank tooltip: Chrome falls back to the extension's
  // name, which is what an unpressed action shows anyway.
  return 'action' in manifest ? (manifest.action?.default_title ?? '') : ''
}

async function wearBadge(tabId: number, badge: { text: string; title: string }): Promise<void> {
  await chrome.action.setBadgeBackgroundColor({ tabId, color: BADGE_COLOUR })
  await chrome.action.setBadgeText({ tabId, text: badge.text })
  await chrome.action.setTitle({ tabId, title: badge.title })
}

async function clearBadge(tabId: number): Promise<void> {
  await chrome.action.setBadgeText({ tabId, text: '' })
  await chrome.action.setTitle({ tabId, title: defaultActionTitle() })
}

/**
 * What the action says about a refusal.
 *
 * An app that is not running gets the sentence `fallback` owns, because that is
 * the case the badge exists for. Everything else carries the sentence the
 * bridge wrote for it: a token that was refused, a version mismatch and a port
 * held by something that could not prove it is Snapdeck need different things
 * from the user, and "Snapdeck is not running" would send them looking for a
 * window that is already open - or, for the last of the three, to open the app
 * and leave the impostor holding the port.
 */
function badgeFor(outcome: Extract<SendOutcome, { ok: false }>): {
  text: string
  title: string
} {
  return outcome.code === 'unreachable'
    ? unreachableBadge()
    : { text: badgeText(), title: outcome.message }
}

/** The token has to exist before there is any point opening a socket. */
const NOT_PAIRED: Extract<SendOutcome, { ok: false }> = {
  ok: false,
  code: 'unauthorized',
  message:
    'Snapdeck and this extension are not paired yet, so the capture went to your downloads. Paste the pairing token in the extension’s options.',
}

async function handOver(
  tab: chrome.tabs.Tab,
  capture: FullPageCapture,
): Promise<SendOutcome> {
  const token = await readPairingToken()
  if (token.length === 0) return NOT_PAIRED

  const { devicePixelRatio } = capture.metrics
  return await sendFullPage(
    () => connectToBridge(),
    token,
    {
      requestId: crypto.randomUUID(),
      page: {
        // `fullPageSchema` refuses anything longer, and a `data:` page's
        // address goes well past it. Where the picture's own numbers are never
        // altered to fit - a clamped width describes the same picture wrongly -
        // an address is metadata, and clipping one costs a truncated URL in a
        // file name where not clipping costs the whole capture.
        url: clip(tab.url ?? UNKNOWN_URL, MAX_URL_CHARS),
        title: clip(tab.title ?? '', MAX_TITLE_CHARS),
      },
      image: {
        pngBase64: await toBase64(capture.blob),
        // The plan is in CSS pixels and the PNG is in device pixels. Truncated
        // rather than rounded, because that is what a canvas does to a size it
        // is given.
        width: Math.trunc(capture.plan.compositeWidth * devicePixelRatio),
        height: Math.trunc(capture.plan.compositeHeight * devicePixelRatio),
        devicePixelRatio,
      },
      truncated: capture.plan.truncated,
    },
    BRIDGE_TIMEOUT_MS,
  )
}

async function deliver(
  tabId: number,
  tab: chrome.tabs.Tab,
  capture: FullPageCapture,
): Promise<void> {
  const outcome = await handOver(tab, capture)
  if (outcome.ok) {
    await clearBadge(tabId)
    return
  }

  // The capture cost the user a scroll down their whole page. Whatever went
  // wrong between here and the app, it is not thrown away.
  await downloadInstead(
    capture.blob,
    fallbackFilename(tab.url ?? UNKNOWN_URL, new Date()),
    (options) => chrome.downloads.download(options),
  )
  await wearBadge(tabId, badgeFor(outcome))
}

async function onClicked(tab: chrome.tabs.Tab): Promise<void> {
  const tabId = tab.id
  if (tabId === undefined || busy.has(tabId)) return

  busy.add(tabId)
  try {
    await clearBadge(tabId)
    const startedAt = await readScrollY(tabId)
    const capture = await captureFullPage(
      depsFor(tabId, tab.windowId, startedAt),
      MAX_IMAGE_PIXELS,
    )
    await deliver(tabId, tab, capture)
  } catch (cause) {
    // A page the extension is not allowed to touch, a tab that closed halfway
    // down, a canvas the browser refused. There is nothing to download, so all
    // that is left is to say so.
    await wearBadge(tabId, {
      text: badgeText(),
      title: `Snapdeck could not capture this page. (${reasonOf(cause)})`,
    })
  } finally {
    busy.delete(tabId)
  }
}

chrome.action.onClicked.addListener((tab: chrome.tabs.Tab) => {
  // The last resort. Everything worth reporting is reported on the badge, and
  // what is left is a tab that closed while the extension was writing to it.
  void onClicked(tab).catch((cause: unknown) => {
    console.error('snapdeck: the capture could not even be reported', cause)
  })
})
