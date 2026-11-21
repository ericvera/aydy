//! The session snapshot on its way to the popover.
//!
//! Snapshots travel over a `tauri::ipc::Channel` rather than an event: event
//! payloads are JSON strings the docs call unsuitable for bigger messages, and
//! the event path has open crash, panic and ordering bugs at high emission
//! rates. A channel is created by the frontend and passed into a command, so
//! the interface subscribes and Rust writes to what it was handed.
//!
//! The same loop that pushes a snapshot updates the menu bar icon and plays the
//! session sounds, so all three are derived from the same observation and
//! cannot disagree about what just happened.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;

use crate::extension::registry::ExtensionRegistry;
use crate::listener::port::{PortMove, PortState};
use crate::listener::server::{lock_recovering, ListenerHealth};
use crate::notify::sound::SoundNotifier;
use crate::session::registry::{
    RegistrySnapshot, SessionGroup, SessionGroupKind, SessionRegistry, SessionSnapshot,
};
use crate::session::status::SessionStatus;
use crate::tray;
use crate::tray::icon::{indication, IconIndication};

/// How often the registry is observed.
///
/// Ten times a second is the ceiling on how often the interface can hear from
/// the app, not a promise of a message that often: an observation that matches
/// the last one sent is not sent again.
pub const PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

/// A session's status, as the interface names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusPayload {
    NeedsInput,
    Working,
    Done,
    Unknown,
}

impl From<SessionStatus> for StatusPayload {
    fn from(status: SessionStatus) -> Self {
        match status {
            SessionStatus::NeedsInput => Self::NeedsInput,
            SessionStatus::Working => Self::Working,
            SessionStatus::Done => Self::Done,
            SessionStatus::Unknown => Self::Unknown,
        }
    }
}

/// What a group of sessions is grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKindPayload {
    Project,
    Unidentified,
    NotJumpable,
}

impl From<SessionGroupKind> for GroupKindPayload {
    fn from(kind: SessionGroupKind) -> Self {
        match kind {
            SessionGroupKind::Project => Self::Project,
            SessionGroupKind::Unidentified => Self::Unidentified,
            SessionGroupKind::NotJumpable => Self::NotJumpable,
        }
    }
}

/// One session row.
///
/// Durations cross as whole milliseconds: JavaScript has no duration type, and
/// serde's own `Duration` shape is a seconds-and-nanoseconds pair the interface
/// would only have to reassemble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPayload {
    pub id: String,
    pub label: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub status: StatusPayload,
    pub unread: bool,
    pub jumpable: bool,
    pub is_current: bool,
    pub working_time_ms: u64,
    pub longest_turn_ms: u64,
    pub notification_message: Option<String>,
}

impl From<&SessionSnapshot> for SessionPayload {
    fn from(session: &SessionSnapshot) -> Self {
        Self {
            id: session.id.clone(),
            label: session.label.clone(),
            repo: session.repo.clone(),
            branch: session.branch.clone(),
            status: session.status.into(),
            unread: session.unread,
            jumpable: session.jumpable,
            is_current: session.is_current,
            working_time_ms: milliseconds(session.working_time),
            longest_turn_ms: milliseconds(session.longest_turn),
            notification_message: session.notification_message.clone(),
        }
    }
}

/// One group of sessions, already ordered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupPayload {
    pub kind: GroupKindPayload,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub sessions: Vec<SessionPayload>,
}

impl From<&SessionGroup> for GroupPayload {
    fn from(group: &SessionGroup) -> Self {
        Self {
            kind: group.kind.into(),
            repo: group.repo.clone(),
            branch: group.branch.clone(),
            sessions: group.sessions.iter().map(SessionPayload::from).collect(),
        }
    }
}

/// A move the listener already made for itself.
///
/// The port is diagnostic and appears nowhere else in the interface; the user
/// is never asked to choose one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortMovePayload {
    pub from: u16,
    pub to: u16,
}

impl From<PortMove> for PortMovePayload {
    fn from(moved: PortMove) -> Self {
        Self {
            from: moved.from,
            to: moved.to,
        }
    }
}

/// What the list was when the app stopped being able to receive events.
///
/// The age is whole seconds rather than the observation's own resolution, so a
/// stale list costs one message a second rather than ten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StalePayload {
    pub session_count: usize,
    pub last_seen_ago_ms: u64,
}

/// When the app stopped being able to receive events, and what it could see at
/// the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Staleness {
    session_count: usize,
    since: Instant,
}

impl Staleness {
    /// The staleness after an observation.
    ///
    /// The count and the moment are taken once, when the app stops being able
    /// to receive events, because that is what "last seen" means — a count
    /// re-read afterwards is a claim about a list the app has just said it
    /// cannot trust. Recovery clears it, so the next spell is measured from its
    /// own start.
    pub fn observe(
        current: Option<Self>,
        cannot_receive_events: bool,
        session_count: usize,
        now: Instant,
    ) -> Option<Self> {
        if !cannot_receive_events {
            return None;
        }

        Some(current.unwrap_or(Self {
            session_count,
            since: now,
        }))
    }

    fn payload(self, now: Instant) -> StalePayload {
        StalePayload {
            session_count: self.session_count,
            last_seen_ago_ms: now.duration_since(self.since).as_secs() * 1_000,
        }
    }
}

/// The whole list, grouped, ordered and counted by the registry, with what the
/// popover needs to know about the app's own health.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPayload {
    pub groups: Vec<GroupPayload>,
    pub session_count: usize,
    pub unread_count: usize,
    /// The port move notice, once the listener has made one.
    pub port_move: Option<PortMovePayload>,
    /// Set only while the app cannot receive events, which cannot happen before
    /// its hooks exist.
    pub stale: Option<StalePayload>,
    /// Whether any VS Code window's companion extension is registered.
    ///
    /// It rides on the snapshot rather than being asked for, because the one
    /// thing that reads it is a prompt offering the extension to a user who has
    /// not got it: windows register seconds after the editor starts and stop
    /// registering when it closes, and a prompt that asked once at startup
    /// would offer the extension to people who already have it.
    pub extension_connected: bool,
}

impl From<&RegistrySnapshot> for SnapshotPayload {
    fn from(snapshot: &RegistrySnapshot) -> Self {
        Self {
            groups: snapshot.groups.iter().map(GroupPayload::from).collect(),
            session_count: snapshot.session_count,
            unread_count: snapshot.unread_count,
            port_move: None,
            stale: None,
            extension_connected: false,
        }
    }
}

/// The payload for one observation.
pub fn payload(
    snapshot: &RegistrySnapshot,
    moved: Option<PortMove>,
    stale: Option<Staleness>,
    now: Instant,
) -> SnapshotPayload {
    SnapshotPayload {
        port_move: moved.map(PortMovePayload::from),
        stale: stale.map(|stale| stale.payload(now)),
        ..SnapshotPayload::from(snapshot)
    }
}

/// Everything one observation of the registry decides: what the popover is
/// sent, what the menu bar icon shows, and the staleness the next observation
/// measures against.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Observation {
    payload: SnapshotPayload,
    indication: IconIndication,
    stale: Option<Staleness>,
}

/// One observation, decided without a running app.
///
/// The loop below needs an `AppHandle` and cannot be driven from a test, so
/// everything it decides lives here instead — including the two lines that
/// carry the port move and the stale list into the payload, and the one that
/// hands the snapshot to the sound notifier. Each of those is a line whose
/// absence would leave every test green and the user with nothing.
fn observe(
    snapshot: &RegistrySnapshot,
    cannot_receive_events: bool,
    extension_connected: bool,
    moved: Option<PortMove>,
    stale: Option<Staleness>,
    now: Instant,
    sounds: &SoundNotifier,
) -> Observation {
    let stale = Staleness::observe(stale, cannot_receive_events, snapshot.session_count, now);

    sounds.observe(snapshot);

    Observation {
        payload: SnapshotPayload {
            extension_connected,
            ..payload(snapshot, moved, stale, now)
        },
        indication: indication(snapshot, cannot_receive_events),
        stale,
    }
}

/// A duration in whole milliseconds, saturating rather than wrapping.
fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The channel the interface is currently listening on, and the last payload
/// written to it.
#[derive(Default)]
struct Subscriber {
    channel: Option<Channel<SnapshotPayload>>,
    sent: Option<SnapshotPayload>,
}

/// The popover's subscription to session snapshots.
#[derive(Default)]
pub struct SessionSubscription {
    subscriber: Mutex<Subscriber>,
}

impl SessionSubscription {
    /// Takes the interface's channel, replacing any earlier one.
    ///
    /// A reload creates a new channel, and the one it replaces is dropped here,
    /// so channels belonging to webviews that are gone cannot accumulate.
    ///
    /// The record of what was last sent is cleared with it: the new subscriber
    /// has seen nothing, so the next observation reaches it even if the
    /// registry has not changed.
    pub fn subscribe(&self, channel: Channel<SnapshotPayload>) {
        let mut subscriber = lock_recovering(&self.subscriber);

        subscriber.channel = Some(channel);
        subscriber.sent = None;
    }

    /// Writes a payload to the channel unless it matches the last one written.
    ///
    /// Returns whether anything was written.
    pub fn publish(&self, payload: &SnapshotPayload) -> bool {
        let mut subscriber = lock_recovering(&self.subscriber);

        if subscriber.sent.as_ref() == Some(payload) {
            return false;
        }

        let Some(channel) = subscriber.channel.as_ref() else {
            return false;
        };

        match channel.send(payload.clone()) {
            Ok(()) => {
                subscriber.sent = Some(payload.clone());
                true
            }
            Err(error) => {
                // A channel that cannot be written to belongs to a webview that
                // is gone. Dropping it here is what keeps a reload from leaving
                // one behind.
                eprintln!(
                    "aydy: dropping the session channel, which cannot be written to: {error}"
                );
                subscriber.channel = None;
                subscriber.sent = None;
                false
            }
        }
    }
}

/// Subscribes the popover to session snapshots.
#[tauri::command]
pub fn subscribe_sessions(
    channel: Channel<SnapshotPayload>,
    subscription: State<'_, Arc<SessionSubscription>>,
) {
    subscription.subscribe(channel);
}

/// Observes the registry on a fixed cadence, updates the menu bar icon, plays
/// the sounds a transition earns, and pushes the snapshot to the popover when it has changed.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    app: AppHandle,
    registry: Arc<Mutex<SessionRegistry>>,
    health: Arc<ListenerHealth>,
    ports: Arc<PortState>,
    windows: Arc<ExtensionRegistry>,
    subscription: Arc<SessionSubscription>,
    sounds: Arc<SoundNotifier>,
    shutdown: oneshot::Receiver<()>,
) {
    run_every(
        PUBLISH_INTERVAL,
        app,
        registry,
        health,
        ports,
        windows,
        subscription,
        sounds,
        shutdown,
    )
    .await;
}

/// The loop behind [`run`], with the period as a parameter.
#[allow(clippy::too_many_arguments)]
async fn run_every(
    period: Duration,
    app: AppHandle,
    registry: Arc<Mutex<SessionRegistry>>,
    health: Arc<ListenerHealth>,
    ports: Arc<PortState>,
    windows: Arc<ExtensionRegistry>,
    subscription: Arc<SessionSubscription>,
    sounds: Arc<SoundNotifier>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut observations = tokio::time::interval(period);
    observations.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut shown: Option<IconIndication> = None;
    let mut stale: Option<Staleness> = None;

    loop {
        // The registry is behind a blocking mutex, and waiting on one from an
        // async worker starves the runtime's timers and sockets.
        let observing = tokio::task::spawn_blocking({
            let registry = Arc::clone(&registry);
            let health = Arc::clone(&health);
            let windows = Arc::clone(&windows);

            move || {
                let snapshot = lock_recovering(&registry).snapshot();
                let cannot_receive_events = health.cannot_receive_events();
                // Read here with the rest: it takes a lock of its own, and
                // every other lock this loop needs is taken on this thread.
                let extension_connected = windows.connected();

                (snapshot, cannot_receive_events, extension_connected)
            }
        });

        match observing.await {
            Ok((snapshot, cannot_receive_events, extension_connected)) => {
                let observed = observe(
                    &snapshot,
                    cannot_receive_events,
                    extension_connected,
                    ports.moved(),
                    stale,
                    Instant::now(),
                    &sounds,
                );

                if shown != Some(observed.indication) {
                    tray::update(&app, observed.indication);
                    shown = Some(observed.indication);
                }

                stale = observed.stale;

                subscription.publish(&observed.payload);
            }
            Err(error) => {
                eprintln!("aydy: an observation of the session registry did not finish: {error}");
            }
        }

        tokio::select! {
            _ = observations.tick() => {}
            _ = &mut shutdown => return,
        }
    }
}

/// The stop signal for the running publisher.
pub struct PublisherShutdown {
    signal: Mutex<Option<oneshot::Sender<()>>>,
}

impl PublisherShutdown {
    /// A trigger that fires `signal`.
    pub fn new(signal: oneshot::Sender<()>) -> Self {
        Self {
            signal: Mutex::new(Some(signal)),
        }
    }

    /// Asks the publisher to stop. Firing twice does nothing.
    pub fn trigger(&self) {
        if let Some(signal) = lock_recovering(&self.signal).take() {
            let _ = signal.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use tauri::ipc::InvokeResponseBody;
    use tokio::sync::watch;

    use super::*;
    use crate::clock::SystemClock;
    use crate::notify::sound::test_support::RecordingPlayer;
    use crate::notify::sound::{Sound, SoundPlayer};
    use crate::settings::store::Settings;

    /// A channel that records what was written to it.
    fn recording() -> (Channel<SnapshotPayload>, Arc<Mutex<Vec<String>>>) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&written);

        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                lock_recovering(&recorder).push(json);
            }

            Ok(())
        });

        (channel, written)
    }

    /// A channel that fails every write, as one belonging to a webview that has
    /// gone away does.
    fn broken() -> Channel<SnapshotPayload> {
        Channel::new(|_| Err(tauri::Error::WebviewNotFound))
    }

    /// A notifier over a player that records what it was asked for.
    fn sounds() -> (SoundNotifier, Arc<RecordingPlayer>) {
        let player = Arc::new(RecordingPlayer::new());
        // The sender is dropped: a receiver keeps answering with the last value
        // it was sent, and no test here changes a setting.
        let receiver = watch::channel(Settings::default()).1;

        (
            SoundNotifier::new(
                Arc::clone(&player) as Arc<dyn SoundPlayer>,
                receiver,
                Arc::new(SystemClock),
            ),
            player,
        )
    }

    /// A snapshot carrying `sessions` working sessions, `unread` of them
    /// unread.
    fn snapshot(sessions: usize, unread: usize) -> RegistrySnapshot {
        snapshot_of(sessions, unread, SessionStatus::Working)
    }

    /// A snapshot carrying `sessions` sessions in `status`, `unread` of them
    /// unread.
    fn snapshot_of(sessions: usize, unread: usize, status: SessionStatus) -> RegistrySnapshot {
        let rows = (0..sessions)
            .map(|index| SessionSnapshot {
                id: format!("session-{index}"),
                label: format!("Session {index}"),
                repo: Some("aydy".to_string()),
                branch: Some("main".to_string()),
                status,
                unread: index < unread,
                jumpable: true,
                is_current: false,
                working_time: Duration::from_secs(90),
                longest_turn: Duration::from_millis(1_500),
                notification_message: None,
            })
            .collect();

        RegistrySnapshot {
            groups: vec![SessionGroup {
                kind: SessionGroupKind::Project,
                repo: Some("aydy".to_string()),
                branch: Some("main".to_string()),
                sessions: rows,
            }],
            session_count: sessions,
            unread_count: unread,
        }
    }

    /// The move has already been made and the hooks already rewritten, so this
    /// is a notice; nothing about it asks the user to choose a port.
    #[test]
    fn a_port_move_crosses_naming_the_port_it_left_and_the_one_it_took() {
        let moved = PortMove {
            from: 47_823,
            to: 47_824,
        };

        let json =
            serde_json::to_value(payload(&snapshot(0, 0), Some(moved), None, Instant::now()))
                .expect("the payload to serialize");

        assert_eq!(json["portMove"]["from"], 47_823);
        assert_eq!(json["portMove"]["to"], 47_824);
        assert_eq!(json["stale"], serde_json::Value::Null);
    }

    #[test]
    fn a_listener_that_never_moved_sends_no_notice() {
        let payload = payload(&snapshot(0, 0), None, None, Instant::now());

        assert_eq!(payload.port_move, None);
    }

    /// With no hooks registered nothing is being sent, so nothing is being
    /// lost, and a freshly installed app must not present itself as broken.
    /// `ListenerHealth` is what decides that, and it answers false.
    #[test]
    fn an_app_that_can_receive_events_is_never_stale() {
        assert_eq!(Staleness::observe(None, false, 3, Instant::now()), None);
    }

    #[test]
    fn an_app_that_cannot_receive_events_records_what_it_last_saw() {
        let now = Instant::now();

        let stale = Staleness::observe(None, true, 3, now).expect("a stale list");

        assert_eq!(
            stale.payload(now + Duration::from_secs(360)),
            StalePayload {
                session_count: 3,
                last_seen_ago_ms: 360_000,
            }
        );
    }

    /// "Last seen" is what the app could see when it stopped hearing from
    /// anything. Re-reading the count while stale would make it a claim about a
    /// list the app has just said it cannot trust, and would restart the clock
    /// on every observation.
    #[test]
    fn the_count_and_the_moment_are_taken_once_and_then_left_alone() {
        let started = Instant::now();
        let stale = Staleness::observe(None, true, 3, started);

        let later = Staleness::observe(stale, true, 0, started + Duration::from_secs(60))
            .expect("a stale list");

        assert_eq!(
            later.payload(started + Duration::from_secs(60)),
            StalePayload {
                session_count: 3,
                last_seen_ago_ms: 60_000,
            }
        );
    }

    #[test]
    fn recovering_ends_the_spell_and_the_next_one_is_measured_from_its_own_start() {
        let started = Instant::now();
        let stale = Staleness::observe(None, true, 3, started);
        let recovered = Staleness::observe(stale, false, 3, started + Duration::from_secs(60));

        let again = Staleness::observe(recovered, true, 5, started + Duration::from_secs(120))
            .expect("a stale list");

        assert_eq!(
            again.payload(started + Duration::from_secs(150)),
            StalePayload {
                session_count: 5,
                last_seen_ago_ms: 30_000,
            }
        );
    }

    /// The age is what changes while nothing else does, so its resolution is
    /// what decides how often a stale list costs a message. A tenth of a second
    /// would defeat the dedupe entirely.
    #[test]
    fn the_age_of_a_stale_list_changes_no_more_than_once_a_second() {
        let started = Instant::now();
        let stale = Staleness::observe(None, true, 3, started).expect("a stale list");

        assert_eq!(
            stale.payload(started + Duration::from_millis(1_100)),
            stale.payload(started + Duration::from_millis(1_900))
        );
        assert_ne!(
            stale.payload(started + Duration::from_millis(1_900)),
            stale.payload(started + Duration::from_millis(2_000))
        );
    }

    #[test]
    fn a_stale_list_crosses_with_its_count_and_its_age() {
        let started = Instant::now();
        let stale = Staleness::observe(None, true, 3, started);

        let json = serde_json::to_value(payload(
            &snapshot(3, 0),
            None,
            stale,
            started + Duration::from_secs(360),
        ))
        .expect("the payload to serialize");

        assert_eq!(json["stale"]["sessionCount"], 3);
        assert_eq!(json["stale"]["lastSeenAgoMs"], 360_000);
    }

    /// Both notices reach the popover through one line of the publishing loop
    /// each, and the loop needs a running app. This is that wiring: a move the
    /// listener already made and a spell it cannot receive events in are both
    /// carried into the payload, and the icon is derived from the same
    /// observation.
    #[test]
    fn an_observation_carries_the_port_move_and_the_stale_list_to_the_popover() {
        let now = Instant::now();
        let moved = PortMove {
            from: 47_823,
            to: 47_824,
        };

        let observed = observe(
            &snapshot(3, 0),
            true,
            false,
            Some(moved),
            None,
            now,
            &sounds().0,
        );

        assert_eq!(
            observed.payload.port_move,
            Some(PortMovePayload {
                from: 47_823,
                to: 47_824,
            })
        );
        assert_eq!(
            observed.payload.stale,
            Some(StalePayload {
                session_count: 3,
                last_seen_ago_ms: 0,
            })
        );
        assert_eq!(observed.indication, indication(&snapshot(3, 0), true));
    }

    /// The prompt offering the companion extension reads this, and a window
    /// registers seconds after the editor starts. Asking once when the popover
    /// was built would offer the extension to people who already have it.
    #[test]
    fn an_observation_carries_whether_a_window_has_registered() {
        let connected = observe(
            &snapshot(1, 0),
            false,
            true,
            None,
            None,
            Instant::now(),
            &sounds().0,
        );
        let alone = observe(
            &snapshot(1, 0),
            false,
            false,
            None,
            None,
            Instant::now(),
            &sounds().0,
        );

        assert!(connected.payload.extension_connected);
        assert!(!alone.payload.extension_connected);
    }

    #[test]
    fn an_observation_of_an_app_that_never_moved_and_can_hear_carries_neither_notice() {
        let observed = observe(
            &snapshot(2, 1),
            false,
            false,
            None,
            None,
            Instant::now(),
            &sounds().0,
        );

        assert_eq!(observed.payload.port_move, None);
        assert_eq!(observed.payload.stale, None);
        assert_eq!(observed.stale, None);
        assert_eq!(observed.payload.session_count, 2);
        assert_eq!(observed.indication, indication(&snapshot(2, 1), false));
    }

    /// The sounds reach the user through one line of the publishing loop, and
    /// the loop needs a running app. This is that wiring: the same observation
    /// that fills the payload and the icon is the one the notifier is handed.
    #[test]
    fn an_observation_hands_the_snapshot_to_the_sound_notifier() {
        let (notifier, player) = sounds();
        let now = Instant::now();

        observe(&snapshot(1, 0), false, false, None, None, now, &notifier);
        observe(
            &snapshot_of(1, 1, SessionStatus::Done),
            false,
            false,
            None,
            None,
            now,
            &notifier,
        );

        assert_eq!(player.asked(), vec![Sound::Finished]);
    }

    /// The spell one observation records is the one the next observation ages,
    /// so what the loop carries forward has to be what it was handed back.
    #[test]
    fn the_staleness_an_observation_records_is_the_one_the_next_observation_ages() {
        let started = Instant::now();
        let notifier = sounds().0;
        let first = observe(&snapshot(3, 0), true, false, None, None, started, &notifier);

        let later = observe(
            &snapshot(0, 0),
            true,
            false,
            None,
            first.stale,
            started + Duration::from_secs(60),
            &notifier,
        );

        assert_eq!(
            later.payload.stale,
            Some(StalePayload {
                session_count: 3,
                last_seen_ago_ms: 60_000,
            })
        );
    }

    #[test]
    fn a_snapshot_crosses_as_camel_cased_json_with_durations_in_milliseconds() {
        let payload = SnapshotPayload::from(&snapshot(1, 1));
        let json = serde_json::to_value(&payload).expect("the payload to serialize");

        assert_eq!(json["sessionCount"], 1);
        assert_eq!(json["unreadCount"], 1);

        let session = &json["groups"][0]["sessions"][0];

        assert_eq!(session["status"], "working");
        assert_eq!(session["workingTimeMs"], 90_000);
        assert_eq!(session["longestTurnMs"], 1_500);
        assert_eq!(session["isCurrent"], false);
        assert_eq!(session["notificationMessage"], serde_json::Value::Null);
        assert_eq!(json["groups"][0]["kind"], "project");
    }

    /// Every field of a row, with a value of its own so a pair of them swapped
    /// is a failure rather than a coincidence. This mapping is the contract the
    /// popover renders, and `unread` alone carries the whole premise of the
    /// app, so none of it is left to a spot check.
    #[test]
    fn every_field_of_a_session_crosses_the_boundary() {
        let session = SessionSnapshot {
            id: "0d4b9a1c".to_string(),
            label: "Tray icon and popover".to_string(),
            repo: Some("aydy".to_string()),
            branch: Some("feat/session-monitor".to_string()),
            status: SessionStatus::NeedsInput,
            unread: true,
            jumpable: true,
            is_current: true,
            working_time: Duration::from_secs(90),
            longest_turn: Duration::from_millis(1_500),
            notification_message: Some("Claude needs your permission".to_string()),
        };

        let payload = SessionPayload::from(&session);

        assert_eq!(payload.id, "0d4b9a1c");
        assert_eq!(payload.label, "Tray icon and popover");
        assert_eq!(payload.repo, Some("aydy".to_string()));
        assert_eq!(payload.branch, Some("feat/session-monitor".to_string()));
        assert_eq!(payload.status, StatusPayload::NeedsInput);
        assert!(payload.unread);
        assert!(payload.jumpable);
        assert!(payload.is_current);
        assert_eq!(payload.working_time_ms, 90_000);
        assert_eq!(payload.longest_turn_ms, 1_500);
        assert_eq!(
            payload.notification_message,
            Some("Claude needs your permission".to_string())
        );
    }

    /// The other direction of every flag and every optional, so a mapping that
    /// inverts one or hard-codes it cannot pass on the test above alone.
    #[test]
    fn a_read_session_with_nothing_to_say_crosses_that_way_too() {
        let session = SessionSnapshot {
            id: "7f2e5b60".to_string(),
            label: "Untitled session".to_string(),
            repo: None,
            branch: None,
            status: SessionStatus::Done,
            unread: false,
            jumpable: false,
            is_current: false,
            working_time: Duration::ZERO,
            longest_turn: Duration::ZERO,
            notification_message: None,
        };

        let payload = SessionPayload::from(&session);

        assert_eq!(payload.id, "7f2e5b60");
        assert_eq!(payload.label, "Untitled session");
        assert_eq!(payload.repo, None);
        assert_eq!(payload.branch, None);
        assert_eq!(payload.status, StatusPayload::Done);
        assert!(!payload.unread);
        assert!(!payload.jumpable);
        assert!(!payload.is_current);
        assert_eq!(payload.working_time_ms, 0);
        assert_eq!(payload.longest_turn_ms, 0);
        assert_eq!(payload.notification_message, None);
    }

    #[test]
    fn every_status_and_group_kind_has_a_name_of_its_own() {
        let statuses = [
            SessionStatus::NeedsInput,
            SessionStatus::Working,
            SessionStatus::Done,
            SessionStatus::Unknown,
        ]
        .map(|status| serde_json::to_string(&StatusPayload::from(status)).expect("a name"));

        assert_eq!(
            statuses.to_vec(),
            vec![
                "\"needsInput\"".to_string(),
                "\"working\"".to_string(),
                "\"done\"".to_string(),
                "\"unknown\"".to_string(),
            ]
        );

        let kinds = [
            SessionGroupKind::Project,
            SessionGroupKind::Unidentified,
            SessionGroupKind::NotJumpable,
        ]
        .map(|kind| serde_json::to_string(&GroupKindPayload::from(kind)).expect("a name"));

        assert_eq!(
            kinds.to_vec(),
            vec![
                "\"project\"".to_string(),
                "\"unidentified\"".to_string(),
                "\"notJumpable\"".to_string(),
            ]
        );
    }

    #[test]
    fn nothing_is_published_before_the_interface_subscribes() {
        let subscription = SessionSubscription::default();

        assert!(!subscription.publish(&SnapshotPayload::from(&snapshot(1, 0))));
    }

    #[test]
    fn an_unchanged_snapshot_is_not_published_twice() {
        let subscription = SessionSubscription::default();
        let (channel, written) = recording();
        subscription.subscribe(channel);

        let payload = SnapshotPayload::from(&snapshot(2, 1));

        assert!(subscription.publish(&payload));
        assert!(!subscription.publish(&payload));
        assert_eq!(lock_recovering(&written).len(), 1);
    }

    #[test]
    fn a_changed_snapshot_is_published() {
        let subscription = SessionSubscription::default();
        let (channel, written) = recording();
        subscription.subscribe(channel);

        assert!(subscription.publish(&SnapshotPayload::from(&snapshot(2, 1))));
        assert!(subscription.publish(&SnapshotPayload::from(&snapshot(2, 0))));
        assert_eq!(lock_recovering(&written).len(), 2);
    }

    #[test]
    fn subscribing_again_replaces_the_channel_rather_than_adding_one() {
        let subscription = SessionSubscription::default();
        let (first, before_reload) = recording();
        subscription.subscribe(first);
        subscription.publish(&SnapshotPayload::from(&snapshot(1, 0)));

        let (second, after_reload) = recording();
        subscription.subscribe(second);
        subscription.publish(&SnapshotPayload::from(&snapshot(2, 0)));

        assert_eq!(lock_recovering(&before_reload).len(), 1);
        assert_eq!(lock_recovering(&after_reload).len(), 1);
    }

    #[test]
    fn a_reloaded_interface_is_sent_the_snapshot_it_has_not_seen_even_though_nothing_changed() {
        let subscription = SessionSubscription::default();
        let (first, _) = recording();
        subscription.subscribe(first);
        subscription.publish(&SnapshotPayload::from(&snapshot(1, 0)));

        let (second, after_reload) = recording();
        subscription.subscribe(second);

        assert!(subscription.publish(&SnapshotPayload::from(&snapshot(1, 0))));
        assert_eq!(lock_recovering(&after_reload).len(), 1);
    }

    #[test]
    fn a_channel_that_cannot_be_written_to_is_dropped() {
        let subscription = SessionSubscription::default();
        subscription.subscribe(broken());

        assert!(!subscription.publish(&SnapshotPayload::from(&snapshot(1, 0))));
        assert!(lock_recovering(&subscription.subscriber).channel.is_none());
    }
}
