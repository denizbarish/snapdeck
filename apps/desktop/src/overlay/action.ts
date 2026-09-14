/** The action that starts a recording instead of taking a still. */
export const RECORD_ACTION = 'record'

/** The Tauri command a confirmed selection is handed to. */
export type ConfirmCommand = 'capture_region' | 'start_recording'

/**
 * Whether this action records, which is an exact match and never a prefix.
 *
 * One place rather than two, so the command and the hint can never disagree
 * about what the overlay is going to do.
 */
function isRecording(action: string): boolean {
  return action === RECORD_ACTION
}

/**
 * Which command `Enter` invokes.
 *
 * Anything that is not exactly `RECORD_ACTION` takes a still. The default is
 * deliberately the harmless one: an unrecognised action must never start a
 * recording the user did not ask for, and a URL is a thing that can be wrong.
 */
export function confirmCommand(action: string): ConfirmCommand {
  return isRecording(action) ? 'start_recording' : 'capture_region'
}

/** What the size readout tells the user to press. */
export function confirmHint(action: string, snapsToWindows: boolean): string {
  const key = snapsToWindows ? 'Click' : 'Enter'
  const outcome = isRecording(action) ? 'record' : 'capture'
  return `${key} to ${outcome}`
}
