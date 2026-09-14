import { describe, expect, it } from 'vitest'
import { confirmCommand, confirmHint, RECORD_ACTION } from './action'

describe('confirmCommand', () => {
  it('hands a confirmed selection to the recording command', () => {
    expect(confirmCommand('record')).toBe('start_recording')
  })

  // The whole reason this is an exact match rather than a prefix or a
  // substring test. An overlay URL is a thing that can be wrong, and starting a
  // recording nobody asked for is far worse than taking a screenshot nobody
  // asked for: one is a file, the other is a camera the user did not turn on.
  it('takes a still for every action that is not exactly the recording one', () => {
    expect(confirmCommand('capture')).toBe('capture_region')
    expect(confirmCommand('')).toBe('capture_region')
    expect(confirmCommand('Record')).toBe('capture_region')
    expect(confirmCommand('recording')).toBe('capture_region')
    expect(confirmCommand('RECORD')).toBe('capture_region')
  })
})

describe('confirmHint', () => {
  // The menu bar is the only other place that says what is about to happen,
  // and it is not on screen while the overlay is. These four strings are the
  // whole of the instruction the user gets.
  it('names the key and the outcome for every combination', () => {
    expect(confirmHint('capture', false)).toBe('Enter to capture')
    expect(confirmHint('capture', true)).toBe('Click to capture')
    expect(confirmHint('record', false)).toBe('Enter to record')
    expect(confirmHint('record', true)).toBe('Click to record')
  })
})

describe('RECORD_ACTION', () => {
  // Written into the overlay URL by Rust and read back here, so the value is a
  // contract between two languages rather than a local detail.
  it('is the action Rust writes into the overlay URL', () => {
    expect(RECORD_ACTION).toBe('record')
  })
})
