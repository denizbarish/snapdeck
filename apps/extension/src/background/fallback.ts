import { toBase64 } from './bridge'

/**
 * What happens to a capture when the desktop app is not there to take it.
 *
 * The capture already cost the user a scroll down their whole page, so it is
 * never thrown away: it goes to their downloads folder instead, and the action
 * badge is how they find out that is where it went. No `notifications`
 * permission is asked for, so the badge and its tooltip are the whole
 * vocabulary this extension has for saying anything.
 */

/** The only format the extension produces. */
const PNG_MIME_TYPE = 'image/png'

const FILE_EXTENSION = '.png'

/** Every downloaded capture is named for its source, so they sort together. */
const NAME_PREFIX = 'snapdeck'

/** Used when a page's address has no host to name it after. */
const UNNAMED_PAGE = 'page'

/**
 * Long enough for a host and a path segment or two, short enough to leave room
 * for the prefix, the timestamp and the extension inside what a file system
 * will take.
 */
const MAX_SLUG_LENGTH = 60

/** Anything a file name cannot hold, or that a shell would read as syntax. */
const UNSAFE_CHARACTERS = /[^a-z0-9.-]+/gi

/** The badge text is one character wide; anything longer is truncated by Chrome. */
const BADGE_TEXT = '!'

/**
 * Where the capture came from, as something a file system will take.
 *
 * The host and the path both go in, because a name that carries only the host
 * turns a morning of captures into `snapdeck-example.com` twelve times over.
 *
 * The query string stays out. It is where password reset tokens, signed URL
 * parameters and session ids live, and a file name carries them into the
 * download folder, the browser's download history and whatever syncs that
 * folder to a server. Two captures of the same page are told apart by the
 * timestamp instead.
 */
function slugOf(url: string): string {
  let readable: string
  try {
    const parsed = new URL(url)
    readable = `${parsed.hostname}${parsed.pathname}`
  } catch {
    // `chrome://`, a `data:` URL, or anything else the parser will not take.
    // A capture that succeeded is not lost over the name of its source.
    readable = ''
  }

  const slug = readable
    .replace(UNSAFE_CHARACTERS, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, MAX_SLUG_LENGTH)
    // The slice can leave a trailing separator behind.
    .replace(/-$/, '')

  return slug.length > 0 ? slug : UNNAMED_PAGE
}

/**
 * UTC rather than local time: it is the same instant everywhere, it sorts as a
 * string, and it never repeats itself for an hour every autumn.
 */
function stampOf(at: Date): string {
  return at.toISOString().slice(0, 19).replace(/[:T]/g, '-')
}

export function fallbackFilename(url: string, at: Date): string {
  return `${NAME_PREFIX}-${slugOf(url)}-${stampOf(at)}${FILE_EXTENSION}`
}

export async function downloadInstead(
  blob: Blob,
  filename: string,
  download: (options: { url: string; filename: string }) => Promise<number>,
): Promise<void> {
  // `URL.createObjectURL` is a document's API and a Manifest V3 service worker
  // is not a document, so the bytes travel in the URL itself.
  const url = `data:${PNG_MIME_TYPE};base64,${await toBase64(blob)}`
  await download({ url, filename })
}

/** The badge and title the action wears when the desktop app is not there. */
export function unreachableBadge(): { text: string; title: string } {
  return {
    text: BADGE_TEXT,
    title: 'Snapdeck is not running, so the capture went to your downloads instead.',
  }
}

/** The same one character, for the badges that carry another reason. */
export function badgeText(): string {
  return BADGE_TEXT
}
