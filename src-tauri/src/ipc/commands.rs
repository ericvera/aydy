//! What the popover asks the app to do.
//!
//! Every command here is a thin wrapper: the registry owns unread, the tray
//! module owns the popover window, and Tauri owns exiting. Each body that has
//! anything to decide is a plain function beside its command, so it can be
//! driven without a running app.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::discovery::reconciler::user_sessions_dir;
use crate::discovery::session_files::LiveProcesses;
use crate::discovery::status_files::{write_seen, DataDir};
use crate::hooks::settings_file::SettingsFile;
use crate::jump::{JumpOutcome, JumpTarget, Jumper};
use crate::session::registry::SessionRegistry;
use crate::sync::lock_recovering;
use crate::tracking::{tracking_state, TrackingState};
use crate::tray::popover::{hide, POPOVER_LABEL};

/// Marks a session read, which is what clicking its row does.
///
/// This is the only way unread clears without the companion extension, so it
/// runs for every row — a row that cannot be jumped to still clears.
#[tauri::command]
pub fn mark_read(
    session_id: String,
    registry: State<'_, Arc<Mutex<SessionRegistry>>>,
    data: State<'_, DataDir>,
) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        });

    mark_session_read(&registry, &data, &session_id, now_ms);
}

/// The body of [`mark_read`], reachable without a Tauri app to hold state.
///
/// The seen file is written as well as the registry updated, so the VS Code
/// tab count agrees with the popover. A seen file that cannot be written
/// leaves the row read here and is not a reason to fail the click.
///
/// A session the registry no longer holds is not an error: the row was
/// rendered from a snapshot that can be a fraction of a second old, and the
/// session it named may have ended in between.
pub fn mark_session_read(
    registry: &Mutex<SessionRegistry>,
    data: &DataDir,
    session_id: &str,
    now_ms: u64,
) {
    lock_recovering(registry).mark_read(session_id, now_ms);

    if let Err(error) = write_seen(&data.seen_dir(), session_id, now_ms) {
        eprintln!("aydy: could not record {session_id} as seen: {error}");
    }
}

/// Takes the user to a session, which is what clicking a jumpable row does.
///
/// It answers what happened rather than acknowledging the click: whether the
/// owning window answered and came forward, whether there was nothing to raise
/// a window with (a click that goes nowhere and says nothing, not an error),
/// whether no window answered, and whether the reveal opened a tab it should
/// not have.
#[tauri::command]
pub async fn jump_to_session(
    session_id: String,
    registry: State<'_, Arc<Mutex<SessionRegistry>>>,
    jumper: State<'_, Arc<Jumper>>,
) -> Result<JumpOutcome, String> {
    let Some(target) = jump_target(&registry, &session_id) else {
        return Ok(JumpOutcome::NoWindow);
    };

    let jumper = Arc::clone(&jumper);

    // A jump reads session files, writes a request and polls for its reply,
    // so it runs on a thread that is allowed to wait, and never while the
    // registry lock is held.
    Ok(tauri::async_runtime::spawn_blocking(move || {
        let target = target.with_launch_cwd(user_sessions_dir().as_deref(), &LiveProcesses);

        jumper.jump(&target)
    })
    .await
    .unwrap_or(JumpOutcome::Unanswered))
}

/// What jumping to `session_id` would have to do, reachable without a Tauri app
/// to hold state.
///
/// A session the registry no longer holds, and one that was never jumpable,
/// both answer `None`: the row was rendered from a snapshot that can be a
/// fraction of a second old.
pub fn jump_target(registry: &Mutex<SessionRegistry>, session_id: &str) -> Option<JumpTarget> {
    JumpTarget::of(lock_recovering(registry).session(session_id)?)
}

/// Takes the popover down.
///
/// A click that navigates closes the popover; a click that only marks a
/// session read leaves it open. The hide goes through the tray module rather
/// than the window API so it is recorded the same way every other hide is, and
/// the tray icon's click-to-close cannot be thrown by it.
#[tauri::command]
pub fn close_popover(app: AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        hide(&window);
    }
}

/// Quits the app.
///
/// Exiting through the app handle is what raises the run events the listener,
/// the scan, the resume watch and the publisher stop on.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// Whether the extension's session tracking is installed, as the popover reads
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingStatusPayload {
    pub state: TrackingState,
}

/// Whether the extension's hook script and its hooks are in place.
///
/// The popover polls this while tracking is not installed, so it costs one
/// stat and one read of the settings file.
#[tauri::command]
pub fn tracking_status(data: State<'_, DataDir>) -> TrackingStatusPayload {
    let script = data.hook_script();
    let state = match SettingsFile::user() {
        Ok(settings) => tracking_state(&script, &settings),
        Err(_) if script.is_file() => TrackingState::HooksMissing,
        Err(_) => TrackingState::NotInstalled,
    };

    TrackingStatusPayload { state }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::clock::TestClock;
    use crate::discovery::status_files::{read_seen, StatusRecord};
    use crate::session::event::{HookEvent, HookEventKind, SessionEnrichment};
    use crate::test_support::scratch_dir;

    /// The wall-clock ms the click tests are dated against.
    const NOW_MS: u64 = 1_790_000_000_000;

    fn stop(session_id: &str, written_at_ms: u64) -> StatusRecord {
        StatusRecord {
            event: HookEvent::new(session_id, HookEventKind::Stop),
            written_at_ms,
        }
    }

    /// A registry holding one session that a `Stop` record has left unread.
    fn unread_session() -> Mutex<SessionRegistry> {
        let mut registry = SessionRegistry::new(Arc::new(TestClock::new()));

        registry.apply_status(stop("session-1", NOW_MS - 1_000), NOW_MS);

        Mutex::new(registry)
    }

    /// A data directory of the test's own, so no test writes the real one.
    fn scratch_data(name: &str) -> DataDir {
        DataDir::new(scratch_dir(name))
    }

    /// A session the scan found in VS Code and the transcript pass titled,
    /// built through the entry points the running app builds it through. A
    /// jump needs no window placement.
    fn jumpable_session() -> Mutex<SessionRegistry> {
        let mut registry = SessionRegistry::new(Arc::new(TestClock::new()));

        registry.apply_hook_event(
            HookEvent::new("session-1", HookEventKind::UserPromptSubmit)
                .with_cwd("/Users/eric/Code/aydy"),
        );
        registry.apply_enrichment(
            "session-1",
            SessionEnrichment {
                label: Some("Models section configuration options".to_string()),
                jumpable: Some(true),
                ..SessionEnrichment::default()
            },
        );

        Mutex::new(registry)
    }

    fn is_unread(registry: &Mutex<SessionRegistry>, session_id: &str) -> bool {
        lock_recovering(registry)
            .session(session_id)
            .expect("the session to be tracked")
            .unread()
    }

    /// A session found by the scan, labelled by the transcript pass, and read
    /// back out the way the command reads it.
    #[test]
    fn a_jumpable_session_carries_its_tab_and_its_directory() {
        let registry = jumpable_session();

        let target = jump_target(&registry, "session-1").expect("a target");

        assert_eq!(target.session_id, "session-1");
        assert_eq!(
            target.tab_label,
            Some("Models section configura\u{2026}".to_string())
        );
        assert_eq!(target.cwd, Some(PathBuf::from("/Users/eric/Code/aydy")));
    }

    /// The click still marks the row read; there is simply nowhere to send it.
    #[test]
    fn a_session_that_cannot_be_jumped_to_has_no_target() {
        let registry = unread_session();

        assert_eq!(jump_target(&registry, "session-1"), None);
    }

    /// The row was rendered from a snapshot that can be a fraction of a second
    /// old, and the session it named may have ended in between.
    #[test]
    fn a_session_the_registry_no_longer_holds_has_no_target() {
        let registry = unread_session();

        assert_eq!(jump_target(&registry, "session-gone"), None);
    }

    #[test]
    fn clicking_a_row_marks_its_session_read() {
        let registry = unread_session();
        assert!(is_unread(&registry, "session-1"));

        mark_session_read(&registry, &scratch_data("click"), "session-1", NOW_MS);

        assert!(!is_unread(&registry, "session-1"));
    }

    /// The seen file is what the VS Code tab count reads, so a click that only
    /// updated the registry would leave the two disagreeing.
    #[test]
    fn clicking_a_row_writes_its_seen_file() {
        let registry = unread_session();
        let data = scratch_data("seen-file");

        mark_session_read(&registry, &data, "session-1", NOW_MS);

        assert_eq!(read_seen(&data.seen_dir()).get("session-1"), Some(&NOW_MS));
        assert!(data.seen_dir().join("session-1.json").is_file());
    }

    /// A seen file that cannot be written is logged, never a failed click.
    #[test]
    fn a_seen_file_that_cannot_be_written_still_marks_the_row_read() {
        let registry = unread_session();
        let data = scratch_data("seen-blocked");
        fs::write(data.seen_dir(), "not a directory").expect("the blocking file");

        mark_session_read(&registry, &data, "session-1", NOW_MS);

        assert!(!is_unread(&registry, "session-1"));
    }

    #[test]
    fn clicking_one_row_leaves_every_other_session_unread() {
        let registry = unread_session();
        lock_recovering(&registry).apply_status(stop("session-2", NOW_MS - 1_000), NOW_MS);

        mark_session_read(&registry, &scratch_data("one-row"), "session-1", NOW_MS);

        assert!(!is_unread(&registry, "session-1"));
        assert!(is_unread(&registry, "session-2"));
    }

    /// The snapshot a row was rendered from can be a fraction of a second old,
    /// so the session it names may already have ended. That click must do
    /// nothing rather than panic, because a panic here takes the command out.
    #[test]
    fn clicking_a_row_for_a_session_that_has_gone_does_nothing() {
        let registry = unread_session();

        mark_session_read(
            &registry,
            &scratch_data("ended"),
            "session-that-ended",
            NOW_MS,
        );

        assert!(is_unread(&registry, "session-1"));
    }

    /// A poisoned registry lock is recovered from rather than propagated: a
    /// panic somewhere else must not leave clicking a row dead for the rest of
    /// the session.
    #[test]
    fn a_click_still_lands_after_a_panic_poisoned_the_registry() {
        let registry = Arc::new(unread_session());
        let poisoning = Arc::clone(&registry);

        let panicked = std::thread::spawn(move || {
            let _guard = poisoning.lock().expect("the lock");

            panic!("something near the registry gave up");
        })
        .join();

        assert!(panicked.is_err());
        assert!(registry.is_poisoned());

        mark_session_read(&registry, &scratch_data("poisoned"), "session-1", NOW_MS);

        assert!(!is_unread(&registry, "session-1"));
    }

    /// Working time keeps accruing while a session is Working, so the reading
    /// the popover shows is a live one rather than a value frozen at the last
    /// hook event.
    #[test]
    fn a_session_marked_read_keeps_the_rest_of_its_state() {
        let clock = Arc::new(TestClock::new());
        let mut registry = SessionRegistry::new(clock.clone());

        registry.apply_hook_event(HookEvent::new("session-1", HookEventKind::UserPromptSubmit));
        clock.advance(Duration::from_secs(60));

        let registry = Mutex::new(registry);
        mark_session_read(&registry, &scratch_data("keeps-state"), "session-1", NOW_MS);

        let snapshot = lock_recovering(&registry).snapshot();
        let session = snapshot.groups[0].sessions[0].clone();

        assert!(!session.unread);
        assert_eq!(session.working_time, Duration::from_secs(60));
    }
}
