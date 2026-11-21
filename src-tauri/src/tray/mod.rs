//! The menu bar presence: the tray icon and the popover hanging under it.

pub mod icon;
pub mod popover;

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconEvent};
use tauri::AppHandle;

use crate::tray::icon::IconIndication;

/// The id of the tray icon, as declared in `tauri.conf.json`.
pub const TRAY_ID: &str = "aydy";

/// Puts an indication on the tray icon.
///
/// The image and the template flag are set together: setting them one after the
/// other renders the icon twice and flickers.
pub fn apply(tray: &TrayIcon, indication: IconIndication) -> tauri::Result<()> {
    tray.set_icon_with_as_template(Some(Image::from_bytes(indication.state.image())?), true)?;
    tray.set_title(indication.count.map(|count| count.to_string()))
}

/// Puts an indication on the app's tray icon, if it has one.
pub fn update(app: &AppHandle, indication: IconIndication) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    if let Err(error) = apply(&tray, indication) {
        eprintln!("aydy: cannot update the menu bar icon: {error}");
    }
}

/// Handles a tray icon event.
///
/// Left click toggles the popover; there is no menu to open.
pub fn on_tray_event(app: &AppHandle, event: &TrayIconEvent) {
    // Without this the positioner plugin never learns where the tray icon is,
    // and tray-relative positioning silently does nothing at all.
    tauri_plugin_positioner::on_tray_event(app, event);

    if matches!(
        event,
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
    ) {
        popover::toggle(app);
    }
}
