import { invoke } from '@tauri-apps/api/core'
import type { Ref } from 'vue'
import { ref } from 'vue'

import type { JumpOutcome } from '../types/JumpOutcome'

/** The outcomes the user is told about, as opposed to acted on. */
type JumpFailure = Extract<JumpOutcome, 'unconfirmed' | 'duplicated'>

// Module scoped, like the other composables, so the notice and the list are
// looking at one answer.
const failure = ref<JumpFailure | null>(null)

// A jump retries, so the popover can still be up and still be clicked for a
// couple of seconds while one is in flight. A second click on the row that is
// already jumping is that same jump: asking again would reveal the tab a second
// time and raise the window twice over, for the one answer the first click is
// already waiting for.
const inFlight = new Map<string, Promise<boolean>>()

interface Jump {
  failure: Ref<JumpFailure | null>
  /** Takes the user to a session, answering whether the click navigated. */
  jump: (sessionId: string) => Promise<boolean>
  dismiss: () => void
}

/**
 * Going to the session behind a row.
 *
 * A jump is asked for and then found out about: Rust reveals the tab, raises
 * the window and waits for the window itself to say it came forward, so this
 * waits for an answer rather than firing and forgetting. The wait is invisible
 * in the ordinary case, because a window coming forward takes focus from the
 * popover and the popover hides on focus loss.
 *
 * A row clicked again while its jump is still waiting is answered by that jump
 * rather than by a second one.
 */
export const useJump = (): Jump => {
  const ask = async (sessionId: string): Promise<boolean> => {
    failure.value = null

    let outcome: JumpOutcome

    try {
      outcome = await invoke<JumpOutcome>('jump_to_session', { sessionId })
    } catch {
      // A jump that could not even be asked for is a jump nothing confirmed,
      // and claiming otherwise is the failure this whole path exists to avoid.
      outcome = 'unconfirmed'
    }

    if (outcome === 'unconfirmed' || outcome === 'duplicated') {
      failure.value = outcome

      return false
    }

    // A session no window could be found for goes nowhere and says nothing,
    // which is what a row that cannot be jumped to has always done.
    return outcome !== 'noWindow'
  }

  const jump = (sessionId: string): Promise<boolean> => {
    const asked = inFlight.get(sessionId)

    if (asked !== undefined) {
      return asked
    }

    const jumping = ask(sessionId).finally(() => {
      inFlight.delete(sessionId)
    })

    inFlight.set(sessionId, jumping)

    return jumping
  }

  const dismiss = (): void => {
    failure.value = null
  }

  return { failure, jump, dismiss }
}
