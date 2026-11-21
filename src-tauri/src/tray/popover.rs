//! Showing, hiding and positioning the popover window.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::sync::lock_recovering;

/// The label of the popover window, as declared in `tauri.conf.json`.
pub const POPOVER_LABEL: &str = "popover";

/// How long after the popover hides a tray click still counts as the click that
/// hid it.
///
/// Clicking the tray icon while the popover is up may take key status away from
/// it first, and that focus loss hides the popover before the click is ever
/// delivered here. Long enough to cover that ordering; short enough that a
/// click a person makes after seeing the popover close still reopens it.
const CLOSING_CLICK_WINDOW: Duration = Duration::from_millis(300);

/// How long after an action that opens something in front of the popover a
/// focus loss is still that action's own.
///
/// Long enough for the Finder to come forward on a busy machine, short enough
/// that a suppression nothing consumed is gone before the user's next click.
const FOCUS_SUPPRESSION_WINDOW: Duration = Duration::from_secs(2);

/// What a click on the tray icon should do to the popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    Show,
    Hide,
    /// The popover is already down because this very click took it down.
    Nothing,
}

/// When the popover was last hidden.
///
/// This is what tells the click that closed the popover apart from a click that
/// should reopen it. Without it, a macOS that resigns key status before
/// delivering the click leaves the toggle looking at a hidden window, showing it
/// straight back, and the popover cannot be dismissed from its own icon.
pub struct PopoverVisibility {
    hidden_at: Mutex<Option<Instant>>,
}

impl Default for PopoverVisibility {
    fn default() -> Self {
        Self::new()
    }
}

impl PopoverVisibility {
    pub const fn new() -> Self {
        Self {
            hidden_at: Mutex::new(None),
        }
    }

    /// Records that the popover went down at `now`.
    pub fn hidden(&self, now: Instant) {
        *lock_recovering(&self.hidden_at) = Some(now);
    }

    /// Records that the popover came up.
    pub fn shown(&self) {
        *lock_recovering(&self.hidden_at) = None;
    }

    /// What a launch of AYDY that found it already running should do.
    ///
    /// Never a toggle, and never nothing on account of a hide that has just
    /// happened: a launch is not the click that closed the popover, and someone
    /// who launched AYDY is asking to see it. A popover that is already up is
    /// the one case with nothing left to do.
    ///
    /// The record of the hide is consumed all the same, so the tray click after
    /// a launch is read against the launch rather than against a hide the user
    /// has already had answered.
    pub fn launched(&self, visible: bool) -> ClickAction {
        lock_recovering(&self.hidden_at).take();

        if visible {
            ClickAction::Nothing
        } else {
            ClickAction::Show
        }
    }

    /// What a click at `now` should do, given whether the popover is visible.
    ///
    /// The record of the hide is consumed either way, so a second click always
    /// reopens the popover however fast it follows the first.
    pub fn click(&self, visible: bool, now: Instant) -> ClickAction {
        let hidden_at = lock_recovering(&self.hidden_at).take();

        if visible {
            return ClickAction::Hide;
        }

        match hidden_at {
            Some(at) if now.duration_since(at) < CLOSING_CLICK_WINDOW => ClickAction::Nothing,
            _ => ClickAction::Show,
        }
    }
}

/// One pending permission for the popover to lose focus without going down.
///
/// The popover hides as soon as it loses focus, which is how clicking anywhere
/// else dismisses it — but opening the Finder or a native dialog takes focus
/// too, and that would take the popover down underneath the thing it just
/// opened. An action that opens one arms this first.
///
/// It is armed for **one** focus loss and expires on its own, rather than
/// suppressing everything inside a window: a suppression that outlived the
/// action it belongs to would swallow a real dismissal, and there is no second
/// focus loss to fall back on once the popover is already unfocused.
pub struct FocusSuppression {
    until: Mutex<Option<Instant>>,
}

impl Default for FocusSuppression {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusSuppression {
    pub const fn new() -> Self {
        Self {
            until: Mutex::new(None),
        }
    }

    /// Arms the next focus loss to be ignored.
    pub fn arm(&self, now: Instant) {
        *lock_recovering(&self.until) = Some(now + FOCUS_SUPPRESSION_WINDOW);
    }

    /// Whether this focus loss is a suppressed one, consuming the permission
    /// either way — an expired suppression is not carried forward.
    pub fn consume(&self, now: Instant) -> bool {
        matches!(lock_recovering(&self.until).take(), Some(until) if now < until)
    }
}

/// The popover's own record of when it was last hidden.
///
/// One process, one popover, and both the tray handler and the window event
/// handler reach it from a `&AppHandle` they are handed, so it lives here
/// rather than in managed state.
static VISIBILITY: PopoverVisibility = PopoverVisibility::new();

/// The popover's pending permission to lose focus, held here for the same
/// reason [`VISIBILITY`] is.
static SUPPRESSION: FocusSuppression = FocusSuppression::new();

/// Lets the popover lose focus once without hiding.
///
/// Called by the actions that open something in front of it — revealing the
/// settings file in the Finder is the first — immediately before the thing
/// that takes focus is opened.
pub fn suppress_next_focus_loss() {
    SUPPRESSION.arm(Instant::now());
}

/// Shows the popover if it is hidden, hides it if it is showing.
pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(POPOVER_LABEL) else {
        return;
    };

    let visible = match window.is_visible() {
        Ok(visible) => visible,
        Err(error) => {
            eprintln!("aydy: cannot tell whether the popover is showing: {error}");
            return;
        }
    };

    match VISIBILITY.click(visible, Instant::now()) {
        ClickAction::Show => show(&window),
        ClickAction::Hide => hide(&window),
        ClickAction::Nothing => {}
    }
}

/// Answers a launch of AYDY that found it already running.
///
/// macOS answers launching an app that is already running by activating it and
/// telling it so, rather than by starting a second process for the
/// single-instance socket to turn back — so this is the path a Spotlight or a
/// Finder launch takes, and the socket is only for a copy macOS did not
/// recognise as the same app. An accessory app has neither a Dock icon nor a
/// window for that activation to put in front of anyone, so the popover is the
/// whole of what AYDY can show for it.
///
/// A popover whose visibility cannot be read is treated as down, because a
/// launch that shows nothing is the failure this exists to prevent.
pub fn reveal(app: &AppHandle) {
    let Some(window) = app.get_webview_window(POPOVER_LABEL) else {
        return;
    };

    if VISIBILITY.launched(window.is_visible().unwrap_or(false)) == ClickAction::Show {
        show(&window);
    }
}

/// Anchors the popover under the tray icon and brings it up.
///
/// Positioning is relative to the tray icon's last known rect, which the
/// positioner plugin only learns from forwarded tray events — see the tray
/// event handler.
pub fn show(window: &WebviewWindow) {
    if window.move_window(Position::TrayCenter).is_err() {
        // The plugin only knows where the tray icon is once a tray event has
        // been forwarded to it, and a click always forwards one. This is the
        // case where something other than a click opened the popover; the top
        // right corner is where the tray icon lives, so it is the closest
        // honest guess.
        if let Err(error) = window.move_window(Position::TopRight) {
            eprintln!("aydy: cannot position the popover: {error}");
        }
    }

    keep_on_screen(window);

    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        eprintln!("aydy: cannot show the popover: {error}");
        return;
    }

    VISIBILITY.shown();
}

/// Pulls the popover back onto the monitor it is on, if centring it under the
/// tray icon has hung it over an edge.
///
/// The tray icon sits at the right of the menu bar, so a 320px popover centred
/// under it runs off the screen on most machines. The plugin has a constrained
/// move of its own, but it looks the monitor up from the tray icon's *physical*
/// position, which lands outside the logical desktop on a scaled display and
/// leaves the window unconstrained — measured, not assumed.
fn keep_on_screen(window: &WebviewWindow) {
    let (Ok(Some(monitor)), Ok(position), Ok(size)) = (
        window.current_monitor(),
        window.outer_position(),
        window.outer_size(),
    ) else {
        return;
    };

    let origin = monitor.position();
    let area = monitor.size();
    let overhang = i32::try_from(size.width).unwrap_or(i32::MAX);
    let leftmost = origin.x;
    let rightmost =
        (origin.x + i32::try_from(area.width).unwrap_or(i32::MAX) - overhang).max(leftmost);
    let x = position.x.clamp(leftmost, rightmost);

    if x != position.x {
        if let Err(error) = window.set_position(PhysicalPosition::new(x, position.y)) {
            eprintln!("aydy: cannot pull the popover back onto the screen: {error}");
        }
    }
}

/// Takes the popover down.
///
/// The moment is recorded whichever path took it down, because the click that
/// hides it through focus loss is the same click the tray handler then sees.
pub fn hide(window: &WebviewWindow) {
    if let Err(error) = window.hide() {
        eprintln!("aydy: cannot hide the popover: {error}");
        return;
    }

    VISIBILITY.hidden(Instant::now());
}

/// Hides the popover when it loses focus, which is how clicking anywhere else
/// dismisses it.
///
/// This depends on `WindowEvent::Focused`, which Tauri's `unstable` feature
/// stops delivering (tauri#12568). That feature must stay off.
///
/// A focus loss the popover asked for — the Finder coming forward for
/// **Reveal file** — is ignored instead, and nothing is recorded for it,
/// because the popover did not go down.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != POPOVER_LABEL {
        return;
    }

    if hides_the_popover(event, Instant::now()) {
        if let Some(popover) = window.app_handle().get_webview_window(POPOVER_LABEL) {
            hide(&popover);
        }
    }
}

/// Whether this window event takes the popover down, consuming the app's one
/// pending permission to lose focus.
///
/// This is the whole of the decision [`on_window_event`] makes — the handler
/// itself only has the window to hide — so it is what lets an action that armed
/// the suppression and the hide it prevents be driven together, without a
/// running app to raise the event on.
pub(crate) fn hides_the_popover(event: &WindowEvent, now: Instant) -> bool {
    takes_the_popover_down(event, &SUPPRESSION, now)
}

/// Whether a window event should take the popover down.
///
/// Only a focus loss does, and only one the popover did not ask for.
fn takes_the_popover_down(
    event: &WindowEvent,
    suppression: &FocusSuppression,
    now: Instant,
) -> bool {
    matches!(event, WindowEvent::Focused(false)) && !suppression.consume(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_while_the_popover_is_up_takes_it_down() {
        let visibility = PopoverVisibility::new();

        assert_eq!(
            visibility.click(true, Instant::now()),
            ClickAction::Hide,
            "a click on the tray icon should close an open popover"
        );
    }

    /// The whole of the reported failure: AYDY launched from Spotlight while it
    /// was already running, and nothing appeared.
    #[test]
    fn a_launch_that_finds_aydy_running_brings_the_popover_up() {
        let visibility = PopoverVisibility::new();

        assert_eq!(visibility.launched(false), ClickAction::Show);
    }

    /// A launch is never the click that closed the popover, so the guard that
    /// exists to stop a tray click reopening what it just closed must not
    /// swallow one — which would leave the launch showing nothing at all.
    #[test]
    fn a_launch_just_after_the_popover_was_clicked_shut_still_brings_it_up() {
        let visibility = PopoverVisibility::new();
        let hidden = Instant::now();

        visibility.hidden(hidden);

        assert_eq!(visibility.launched(false), ClickAction::Show);
    }

    /// Launching AYDY asks to see it, so a popover already up stays up. Reading
    /// the launch as a toggle would close the very thing it asked for.
    #[test]
    fn a_launch_while_the_popover_is_up_leaves_it_up() {
        let visibility = PopoverVisibility::new();

        assert_eq!(visibility.launched(true), ClickAction::Nothing);
    }

    /// The launch answered the hide, so the click after it is a fresh one.
    #[test]
    fn a_click_after_a_launch_is_read_against_the_launch() {
        let visibility = PopoverVisibility::new();
        let hidden = Instant::now();

        visibility.hidden(hidden);
        visibility.launched(false);

        assert_eq!(
            visibility.click(false, hidden + Duration::from_millis(5)),
            ClickAction::Show
        );
    }

    #[test]
    fn a_click_while_the_popover_is_down_brings_it_up() {
        let visibility = PopoverVisibility::new();

        assert_eq!(visibility.click(false, Instant::now()), ClickAction::Show);
    }

    /// Clicking the status item can take key status away from the popover
    /// first, so the hide lands before the click does. Reading that as "it is
    /// down, put it up" is what would make the popover impossible to dismiss
    /// from its own icon.
    #[test]
    fn a_click_that_lands_just_after_the_popover_hid_leaves_it_down() {
        let visibility = PopoverVisibility::new();
        let hidden = Instant::now();

        visibility.hidden(hidden);

        assert_eq!(
            visibility.click(false, hidden + Duration::from_millis(5)),
            ClickAction::Nothing
        );
    }

    #[test]
    fn the_next_click_brings_the_popover_back_however_fast_it_follows() {
        let visibility = PopoverVisibility::new();
        let hidden = Instant::now();

        visibility.hidden(hidden);
        visibility.click(false, hidden + Duration::from_millis(5));

        assert_eq!(
            visibility.click(false, hidden + Duration::from_millis(10)),
            ClickAction::Show
        );
    }

    #[test]
    fn a_click_long_after_the_popover_hid_brings_it_up() {
        let visibility = PopoverVisibility::new();
        let hidden = Instant::now();

        visibility.hidden(hidden);

        assert_eq!(
            visibility.click(false, hidden + Duration::from_secs(2)),
            ClickAction::Show
        );
    }

    /// Opening the Finder for **Reveal file** takes focus, and an unsuppressed
    /// focus loss takes the popover down underneath it.
    #[test]
    fn a_focus_loss_the_popover_asked_for_is_not_one_that_hides_it() {
        let suppression = FocusSuppression::new();
        let armed = Instant::now();

        suppression.arm(armed);

        assert!(suppression.consume(armed + Duration::from_millis(200)));
    }

    #[test]
    fn a_focus_loss_nothing_asked_for_still_hides_the_popover() {
        let suppression = FocusSuppression::new();

        assert!(!suppression.consume(Instant::now()));
    }

    /// One action, one focus loss. A suppression that stayed armed would
    /// swallow the click the user dismisses the popover with, and there is no
    /// second focus loss to fall back on once it is already unfocused.
    #[test]
    fn only_the_first_focus_loss_after_an_action_is_suppressed() {
        let suppression = FocusSuppression::new();
        let armed = Instant::now();

        suppression.arm(armed);
        suppression.consume(armed + Duration::from_millis(200));

        assert!(!suppression.consume(armed + Duration::from_millis(400)));
    }

    /// Losing focus is how clicking anywhere else dismisses the popover, so
    /// this is the rule the whole dismissal rides on.
    #[test]
    fn losing_focus_takes_the_popover_down() {
        let suppression = FocusSuppression::new();

        assert!(takes_the_popover_down(
            &WindowEvent::Focused(false),
            &suppression,
            Instant::now()
        ));
    }

    /// The Finder coming forward for **Reveal file** is a focus loss the
    /// popover asked for, and hiding underneath what it just opened is exactly
    /// what the suppression exists to stop.
    #[test]
    fn losing_focus_to_something_the_popover_opened_leaves_it_up() {
        let suppression = FocusSuppression::new();
        let now = Instant::now();

        suppression.arm(now);

        assert!(!takes_the_popover_down(
            &WindowEvent::Focused(false),
            &suppression,
            now + Duration::from_millis(200)
        ));
    }

    #[test]
    fn gaining_focus_and_everything_else_leave_the_popover_alone() {
        let suppression = FocusSuppression::new();

        assert!(!takes_the_popover_down(
            &WindowEvent::Focused(true),
            &suppression,
            Instant::now()
        ));
        assert!(!takes_the_popover_down(
            &WindowEvent::Destroyed,
            &suppression,
            Instant::now()
        ));
    }

    /// An action that opened nothing, or one whose focus loss never arrived,
    /// must not leave the popover undismissable.
    #[test]
    fn a_suppression_nothing_consumed_expires() {
        let suppression = FocusSuppression::new();
        let armed = Instant::now();

        suppression.arm(armed);

        assert!(!suppression.consume(armed + FOCUS_SUPPRESSION_WINDOW));
    }

    #[test]
    fn showing_the_popover_forgets_the_hide_before_it() {
        let visibility = PopoverVisibility::new();
        let hidden = Instant::now();

        visibility.hidden(hidden);
        visibility.shown();

        assert_eq!(
            visibility.click(false, hidden + Duration::from_millis(5)),
            ClickAction::Show
        );
    }
}
