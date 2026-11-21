import { invoke } from '@tauri-apps/api/core'

interface Popover {
  close: () => Promise<void>
  quit: () => Promise<void>
}

/**
 * The popover window itself: taking it down, and quitting the app.
 *
 * Both go through Rust rather than the window API, so that every hide passes
 * the same bookkeeping the tray icon's click-to-close depends on.
 */
export const usePopover = (): Popover => {
  const close = async (): Promise<void> => {
    await invoke('close_popover')
  }

  const quit = async (): Promise<void> => {
    await invoke('quit')
  }

  return { close, quit }
}
