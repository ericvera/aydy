/**
 * Why a hook action could not do what it was asked.
 *
 * Every one of these leaves the settings file exactly as it was, which is what
 * the screen reporting it leads with.
 */
export interface HookFailure {
  reason: string
}
