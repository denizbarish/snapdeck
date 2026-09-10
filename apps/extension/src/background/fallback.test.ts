import { describe, expect, it } from 'vitest'

import { downloadInstead, fallbackFilename, unreachableBadge } from './fallback'

/**
 * The `node` project. The file name is string arithmetic, and the download is a
 * claim about which kind of URL is handed to Chrome, which the injected
 * `download` function records without a browser being involved.
 */

/** A fixed instant, so the name a test reads is the name every run reads. */
const AT = new Date(Date.UTC(2026, 8, 10, 21, 30, 5))

/** The four bytes every PNG starts with, enough to check the encoding. */
const PNG_SIGNATURE = new Uint8Array([0x89, 0x50, 0x4e, 0x47])

describe('fallbackFilename', () => {
  it('keeps the host and the date, and nothing a file name cannot hold', () => {
    // F1. `chrome.downloads` reads a slash as a directory and refuses the rest,
    // so a name carrying the URL as it stands is a download that never happens.
    const name = fallbackFilename('https://a.example/x?y=1', AT)

    expect(name).toContain('a.example')
    expect(name).toContain('2026-09-10')
    expect(name).not.toMatch(/[/:?]/)
    expect(name.endsWith('.png')).toBe(true)
  })

  it('still names a file when the url cannot be parsed', () => {
    // F2. A capture that succeeded is not lost because the page it came from
    // had an address the URL parser would not take.
    for (const url of ['about:blank', 'not a url']) {
      const name = fallbackFilename(url, AT)

      expect(name.length).toBeGreaterThan(0)
      expect(name).not.toMatch(/[/:?]/)
      expect(name.endsWith('.png')).toBe(true)
    }
  })
})

describe('downloadInstead', () => {
  it('hands the downloader a data url, because a service worker has none of the other kind', async () => {
    // F3. `URL.createObjectURL` is a document's API and a Manifest V3 service
    // worker is not a document. The failure mode is a `TypeError` at exactly
    // the moment the user's capture had nowhere else to go.
    const seen: { url: string; filename: string }[] = []

    await downloadInstead(new Blob([PNG_SIGNATURE], { type: 'image/png' }), 'x.png', (options) => {
      seen.push(options)
      return Promise.resolve(1)
    })

    expect(seen).toHaveLength(1)
    expect(seen[0]?.filename).toBe('x.png')
    expect(seen[0]?.url).toBe(`data:image/png;base64,${btoa('\x89PNG')}`)
  })
})

describe('unreachableBadge', () => {
  it('says the app is not there, in the only place the extension can say it', () => {
    // F4. No `notifications` permission is asked for, so the action's badge and
    // its tooltip are the whole vocabulary. Silence here is a capture the user
    // believes went to Snapdeck and cannot find.
    const badge = unreachableBadge()

    expect(badge.text).toBe('!')
    expect(badge.title).toMatch(/Snapdeck/)
    expect(badge.title).toMatch(/not running/i)
  })
})
