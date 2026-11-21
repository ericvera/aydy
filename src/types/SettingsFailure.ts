/**
 * Why a settings command could not do what it was asked.
 *
 * A settings file that could not be written is the one failure where the change
 * did happen, and its reason says so — which is why the pane reports the reason
 * rather than a failure of its own wording.
 */
export interface SettingsFailure {
  reason: string
}
