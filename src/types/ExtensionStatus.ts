/**
 * Whether the companion VS Code extension has registered with AYDY.
 *
 * A window that closes says nothing about it — its extension host goes with it
 * — so this is what AYDY still holds at the moment it was asked, rather than
 * what it was ever told.
 */
export interface ExtensionStatus {
  connected: boolean
  /**
   * How many VS Code windows are registered. One extension host per window, so
   * this counts windows rather than installations.
   */
  windowCount: number
}
