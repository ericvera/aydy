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
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;

use crate::notify::sound::SoundNotifier;
use crate::session::registry::{
    RegistrySnapshot, SessionGroup, SessionGroupKind, SessionRegistry, SessionSnapshot,
};
use crate::session::status::SessionStatus;
use crate::sync::lock_recovering;
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

/// The whole list, grouped, ordered and counted by the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPayload {
    pub groups: Vec<GroupPayload>,
    pub session_count: usize,
    pub unread_count: usize,
}

impl From<&RegistrySnapshot> for SnapshotPayload {
    fn from(snapshot: &RegistrySnapshot) -> Self {
        Self {
            groups: snapshot.groups.iter().map(GroupPayload::from).collect(),
            session_count: snapshot.session_count,
            unread_count: snapshot.unread_count,
        }
    }
}

/// Everything one observation of the registry decides: what the popover is
/// sent and what the menu bar icon shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Observation {
    payload: SnapshotPayload,
    indication: IconIndication,
}

/// One observation, decided without a running app.
///
/// The loop below needs an `AppHandle` and cannot be driven from a test, so
/// everything it decides lives here instead — including the line that hands
/// the snapshot to the sound notifier, whose absence would leave every test
/// green and the user with nothing.
fn observe(snapshot: &RegistrySnapshot, sounds: &SoundNotifier) -> Observation {
    sounds.observe(snapshot);

    Observation {
        payload: SnapshotPayload::from(snapshot),
        indication: indication(snapshot),
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
pub async fn run(
    app: AppHandle,
    registry: Arc<Mutex<SessionRegistry>>,
    subscription: Arc<SessionSubscription>,
    sounds: Arc<SoundNotifier>,
    shutdown: oneshot::Receiver<()>,
) {
    run_every(
        PUBLISH_INTERVAL,
        app,
        registry,
        subscription,
        sounds,
        shutdown,
    )
    .await;
}

/// The loop behind [`run`], with the period as a parameter.
async fn run_every(
    period: Duration,
    app: AppHandle,
    registry: Arc<Mutex<SessionRegistry>>,
    subscription: Arc<SessionSubscription>,
    sounds: Arc<SoundNotifier>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut observations = tokio::time::interval(period);
    observations.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut shown: Option<IconIndication> = None;

    loop {
        // The registry is behind a blocking mutex, and waiting on one from an
        // async worker starves the runtime's timers and sockets.
        let observing = tokio::task::spawn_blocking({
            let registry = Arc::clone(&registry);

            move || lock_recovering(&registry).snapshot()
        });

        match observing.await {
            Ok(snapshot) => {
                let observed = observe(&snapshot, &sounds);

                if shown != Some(observed.indication) {
                    tray::update(&app, observed.indication);
                    shown = Some(observed.indication);
                }

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

    /// The sounds reach the user through one line of the publishing loop, and
    /// the loop needs a running app. This is that wiring: the same observation
    /// that fills the payload and the icon is the one the notifier is handed.
    #[test]
    fn an_observation_hands_the_snapshot_to_the_sound_notifier() {
        let (notifier, player) = sounds();

        observe(&snapshot(1, 0), &notifier);
        observe(&snapshot_of(1, 1, SessionStatus::Done), &notifier);

        assert_eq!(player.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn an_observation_sends_the_snapshot_and_derives_the_icon_from_it() {
        let observed = observe(&snapshot(2, 1), &sounds().0);

        assert_eq!(observed.payload, SnapshotPayload::from(&snapshot(2, 1)));
        assert_eq!(observed.indication, indication(&snapshot(2, 1)));
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
        assert_eq!(session["notificationMessage"], serde_json::Value::Null);
        assert_eq!(json["groups"][0]["kind"], "project");
        assert!(session.get("isCurrent").is_none());

        for retired in ["portMove", "stale", "extensionConnected"] {
            assert!(json.get(retired).is_none(), "{retired} is still sent");
        }
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
