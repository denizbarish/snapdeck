import { connectToBridge, testBridgeConnection } from '../background/bridge'
import { readPairingToken, tokenLooksValid, writePairingToken } from './token'

/**
 * The options page: read the stored token into the field, put an edited one
 * back, and say in one line what happened.
 *
 * Plain TypeScript against markup that is already in `options.html`, no
 * framework. Nothing on this page appears or disappears, so there is no tree to
 * keep in step, and a UI library would be this extension's only runtime
 * dependency.
 *
 * The `Test connection` button shakes hands and stops: it is pressed while the
 * user is looking at a token they may have pasted wrong, and a button that
 * uploads a page to answer that question would be a surprise.
 */

/**
 * Long enough for an app that is starting up to answer, short enough that a
 * user who pressed a button knows the answer is "no" rather than "wait".
 */
const TEST_TIMEOUT_MS = 3000

const WRONG_SHAPE =
  'A pairing token is 64 characters long, made of the digits 0-9 and the letters a-f. Copy it again in Snapdeck settings.'

type Tone = 'idle' | 'good' | 'bad'

function need<T extends Element>(id: string, type: new () => T): T {
  const found = document.getElementById(id)
  if (!(found instanceof type)) {
    throw new Error(`options.html has no ${type.name} with the id ${id}`)
  }
  return found
}

const form = need('pairing', HTMLFormElement)
const field = need('token', HTMLInputElement)
const save = need('save', HTMLButtonElement)
const test = need('test', HTMLButtonElement)
const status = need('status', HTMLParagraphElement)

function say(text: string, tone: Tone): void {
  status.textContent = text
  status.dataset.tone = tone
}

/**
 * The token as it will be stored, with the whitespace a paste from a text field
 * brings along taken off. The field is rewritten too, so what the user is
 * looking at is what was checked.
 */
function tokenInField(): string {
  const token = field.value.trim()
  field.value = token
  return token
}

/** Keeps a second press from starting a second connection attempt. */
async function whileBusy<T>(work: () => Promise<T>): Promise<T> {
  save.disabled = true
  test.disabled = true
  try {
    return await work()
  } finally {
    save.disabled = false
    test.disabled = false
  }
}

async function storeToken(): Promise<void> {
  const token = tokenInField()
  if (!tokenLooksValid(token)) {
    say(WRONG_SHAPE, 'bad')
    return
  }
  await writePairingToken(token)
  say('Token saved. Test the connection to check it against the app.', 'good')
}

async function testConnection(): Promise<void> {
  const token = tokenInField()
  if (!tokenLooksValid(token)) {
    say(WRONG_SHAPE, 'bad')
    return
  }

  say('Asking Snapdeck…', 'idle')
  const outcome = await testBridgeConnection(
    () => connectToBridge(),
    token,
    TEST_TIMEOUT_MS,
  )
  say(
    outcome.ok ? 'Snapdeck answered. This token is the one it is expecting.' : outcome.message,
    outcome.ok ? 'good' : 'bad',
  )
}

/**
 * Runs one of the two actions and shows whatever went wrong instead of leaving
 * it in a console nobody has open.
 */
function run(action: () => Promise<void>): void {
  void whileBusy(action).catch((cause: unknown) => {
    say(cause instanceof Error ? cause.message : String(cause), 'bad')
  })
}

form.addEventListener('submit', (event: SubmitEvent) => {
  // The form exists so that pressing Return in the field saves; the page never
  // navigates.
  event.preventDefault()
  run(storeToken)
})

test.addEventListener('click', () => {
  run(testConnection)
})

run(async () => {
  field.value = await readPairingToken()
  say(
    field.value.length > 0
      ? 'A token is saved. Test the connection to check it against the app.'
      : 'Not paired yet. Paste the token from Snapdeck settings.',
    'idle',
  )
})
