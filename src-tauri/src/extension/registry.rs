//! What AYDY knows about the VS Code windows that have registered with it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::clock::Clock;
use crate::listener::server::lock_recovering;

/// How often a registered window is expected to say it is still there.
///
/// Published to the extension at registration so the two cadences cannot drift
/// apart across releases: the extension is installed from the Marketplace and
/// updated on a cadence of its own, and a copy of this number compiled into an
/// older build would either flap or linger.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

/// How long a registration outlives the last heartbeat it sent.
///
/// Three intervals plus room for a machine that was asleep or busy. A window
/// that closes takes its extension host with it, so nothing announces the end;
/// silence is the only signal there is.
pub const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(35);

/// A VS Code window's extension host process id, which is how AYDY names the
/// window.
pub type ExtensionPid = u32;

/// One window's registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The extension host's own process id.
    pub pid: ExtensionPid,
    /// The loopback port this window's extension is listening on for commands.
    pub callback_port: u16,
    /// The folders open in this window, which is what tells AYDY a window
    /// holding a given repository exists.
    pub workspace_folders: Vec<PathBuf>,
    /// The version of the Claude Code extension in this window, when there is
    /// one. `None` means it is not installed, or its manifest did not name a
    /// version — neither of which stops this window registering.
    pub claude_code_version: Option<String>,
}

/// A registration and the moment it last said it was still there.
#[derive(Debug)]
struct Live {
    registration: Registration,
    last_seen: Instant,
}

/// Every window currently registered, keyed by its extension host PID.
///
/// Reads prune first, so an expired registration is never handed out and no
/// timer has to run to make that true.
pub struct ExtensionRegistry {
    clock: Arc<dyn Clock>,
    windows: Mutex<HashMap<ExtensionPid, Live>>,
}

impl ExtensionRegistry {
    /// An empty registry reading time from `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Records `registration`, replacing anything held for the same PID.
    ///
    /// A window that registers again is the same window: its extension host
    /// was reloaded, or it found AYDY after a restart, and either way the
    /// callback port it names now is the only one worth keeping.
    pub fn register(&self, registration: Registration) {
        let now = self.clock.now();
        let mut windows = lock_recovering(&self.windows);

        self.prune(&mut windows, now);
        windows.insert(
            registration.pid,
            Live {
                registration,
                last_seen: now,
            },
        );
    }

    /// Records that the window behind `pid` is still there, answering whether
    /// AYDY still knows it.
    ///
    /// A `false` is how an extension learns to register again — after AYDY
    /// restarted, or after its own registration expired — without needing
    /// anything else to tell it.
    pub fn heartbeat(&self, pid: ExtensionPid) -> bool {
        let now = self.clock.now();
        let mut windows = lock_recovering(&self.windows);

        self.prune(&mut windows, now);

        match windows.get_mut(&pid) {
            Some(live) => {
                live.last_seen = now;

                true
            }
            None => false,
        }
    }

    /// Drops the window behind `pid`.
    ///
    /// Called both when a window says it is closing and when a command sent to
    /// it cannot be delivered: a registration AYDY cannot reach is worse than
    /// no registration, because it makes a window look jumpable.
    pub fn forget(&self, pid: ExtensionPid) {
        lock_recovering(&self.windows).remove(&pid);
    }

    /// The registration for `pid`, if that window is still live.
    pub fn registration(&self, pid: ExtensionPid) -> Option<Registration> {
        let now = self.clock.now();
        let mut windows = lock_recovering(&self.windows);

        self.prune(&mut windows, now);

        windows.get(&pid).map(|live| live.registration.clone())
    }

    /// Every live registration, ordered by PID so callers see a stable list.
    pub fn registrations(&self) -> Vec<Registration> {
        let now = self.clock.now();
        let mut windows = lock_recovering(&self.windows);

        self.prune(&mut windows, now);

        let mut live: Vec<Registration> = windows
            .values()
            .map(|live| live.registration.clone())
            .collect();
        live.sort_by_key(|registration| registration.pid);

        live
    }

    /// How many windows are registered.
    pub fn window_count(&self) -> usize {
        self.registrations().len()
    }

    /// Whether any window is registered at all, which is what the settings pane
    /// reports.
    pub fn connected(&self) -> bool {
        self.window_count() > 0
    }

    /// Drops every registration that has gone quiet for longer than
    /// [`REGISTRATION_TIMEOUT`].
    fn prune(&self, windows: &mut HashMap<ExtensionPid, Live>, now: Instant) {
        windows.retain(|_, live| now.duration_since(live.last_seen) <= REGISTRATION_TIMEOUT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;

    fn registry() -> (Arc<TestClock>, ExtensionRegistry) {
        let clock = Arc::new(TestClock::new());

        (Arc::clone(&clock), ExtensionRegistry::new(clock))
    }

    fn window(pid: ExtensionPid, callback_port: u16) -> Registration {
        Registration {
            pid,
            callback_port,
            workspace_folders: vec![PathBuf::from("/Users/eric/Code/aydy")],
            claude_code_version: Some("2.1.238".to_string()),
        }
    }

    #[test]
    fn nothing_is_registered_before_a_window_says_anything() {
        let (_clock, registry) = registry();

        assert_eq!(registry.registrations(), Vec::new());
        assert!(!registry.connected());
    }

    #[test]
    fn a_registration_is_retrievable_by_its_process_id() {
        let (_clock, registry) = registry();

        registry.register(window(65_921, 55_123));

        assert_eq!(registry.registration(65_921), Some(window(65_921, 55_123)));
        assert!(registry.connected());
    }

    #[test]
    fn a_process_id_that_never_registered_has_no_registration() {
        let (_clock, registry) = registry();
        registry.register(window(65_921, 55_123));

        assert_eq!(registry.registration(17_804), None);
    }

    /// Three sessions in one window is the normal case, but three windows is
    /// what the identifier has to keep apart.
    #[test]
    fn every_window_is_held_separately_under_its_own_process_id() {
        let (_clock, registry) = registry();

        registry.register(window(65_921, 55_123));
        registry.register(window(17_804, 55_124));

        assert_eq!(
            registry.registrations(),
            vec![window(17_804, 55_124), window(65_921, 55_123)]
        );
        assert_eq!(registry.window_count(), 2);
    }

    /// The extension host was reloaded and bound a new port. Keeping the first
    /// one would leave AYDY posting commands into a socket nobody holds.
    #[test]
    fn registering_again_replaces_the_callback_port_rather_than_adding_a_window() {
        let (_clock, registry) = registry();
        registry.register(window(65_921, 55_123));

        registry.register(window(65_921, 55_999));

        assert_eq!(registry.window_count(), 1);
        assert_eq!(
            registry.registration(65_921).map(|it| it.callback_port),
            Some(55_999)
        );
    }

    #[test]
    fn a_window_that_says_it_is_closing_is_forgotten() {
        let (_clock, registry) = registry();
        registry.register(window(65_921, 55_123));

        registry.forget(65_921);

        assert_eq!(registry.registration(65_921), None);
        assert!(!registry.connected());
    }

    /// A closed window takes its extension host with it, so nothing announces
    /// the end and only the silence says so.
    #[test]
    fn a_window_that_goes_quiet_for_too_long_is_dropped() {
        let (clock, registry) = registry();
        registry.register(window(65_921, 55_123));

        clock.advance(REGISTRATION_TIMEOUT + Duration::from_secs(1));

        assert_eq!(registry.registration(65_921), None);
        assert_eq!(registry.registrations(), Vec::new());
        assert!(!registry.connected());
    }

    #[test]
    fn a_window_still_beating_is_kept_however_long_it_stays_open() {
        let (clock, registry) = registry();
        registry.register(window(65_921, 55_123));

        for _ in 0..10 {
            clock.advance(HEARTBEAT_INTERVAL);
            assert!(registry.heartbeat(65_921));
        }

        assert_eq!(registry.registration(65_921), Some(window(65_921, 55_123)));
    }

    /// One window closing must not take a window that is still open with it.
    #[test]
    fn one_window_going_quiet_leaves_the_others_registered() {
        let (clock, registry) = registry();
        registry.register(window(65_921, 55_123));
        registry.register(window(17_804, 55_124));

        clock.advance(REGISTRATION_TIMEOUT - Duration::from_secs(1));
        assert!(registry.heartbeat(65_921));
        clock.advance(Duration::from_secs(2));

        assert_eq!(registry.registrations(), vec![window(65_921, 55_123)]);
    }

    /// AYDY restarted, or this window's registration expired while the machine
    /// slept. Either way the extension has to be told to register again.
    #[test]
    fn a_heartbeat_from_a_window_aydy_does_not_know_is_refused() {
        let (clock, registry) = registry();

        assert!(!registry.heartbeat(65_921));

        registry.register(window(65_921, 55_123));
        clock.advance(REGISTRATION_TIMEOUT + Duration::from_secs(1));

        assert!(!registry.heartbeat(65_921));
    }

    /// The heartbeat has to reset the clock rather than merely answer, or every
    /// window would be dropped one timeout after it first registered.
    #[test]
    fn a_heartbeat_keeps_the_registration_alive_past_the_timeout() {
        let (clock, registry) = registry();
        registry.register(window(65_921, 55_123));

        clock.advance(REGISTRATION_TIMEOUT - Duration::from_secs(1));
        assert!(registry.heartbeat(65_921));
        clock.advance(REGISTRATION_TIMEOUT - Duration::from_secs(1));

        assert_eq!(registry.registration(65_921), Some(window(65_921, 55_123)));
    }

    /// The workspace folders are what Task 5.4's jump is gated on, so they have
    /// to survive registration rather than only the PID and the port.
    #[test]
    fn the_folders_and_the_claude_code_version_are_kept_with_the_window() {
        let (_clock, registry) = registry();

        registry.register(Registration {
            pid: 65_921,
            callback_port: 55_123,
            workspace_folders: vec![
                PathBuf::from("/Users/eric/Code/aydy"),
                PathBuf::from("/Users/eric/Code/okven"),
            ],
            claude_code_version: None,
        });

        let stored = registry.registration(65_921).expect("a registration");
        assert_eq!(
            stored.workspace_folders,
            vec![
                PathBuf::from("/Users/eric/Code/aydy"),
                PathBuf::from("/Users/eric/Code/okven")
            ]
        );
        assert_eq!(stored.claude_code_version, None);
    }
}
