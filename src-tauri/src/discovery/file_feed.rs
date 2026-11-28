//! Feeding the registry from the status, progress and seen files, every half
//! second.
//!
//! A status file is only believed while its session's Claude process is alive,
//! checked against the same sessions directory and process table the
//! reconciler uses, so a file left behind by a crashed session never shows.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;

use crate::discovery::session_files::{read_all, ProcessTable};
use crate::discovery::status_files::{
    read_progress, read_seen, read_statuses, DataDir, ProgressRecord, StatusRecord,
};
use crate::session::event::{HookEvent, HookEventKind};
use crate::session::registry::SessionRegistry;
use crate::sync::lock_recovering;

/// How often the status, progress and seen files are read.
pub const FEED_INTERVAL: Duration = Duration::from_millis(500);

/// How long the set of live sessions is trusted before it is read again.
pub const LIVENESS_INTERVAL: Duration = Duration::from_secs(5);

/// What one pass found that the registry has not been told yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedChanges {
    pub apply: Vec<StatusRecord>,
    /// Sessions applied before whose status file has since vanished.
    pub ended: Vec<String>,
    pub seen: HashMap<String, u64>,
    pub progress: Vec<ProgressRecord>,
}

/// The state carried from one pass over the files to the next.
pub struct FileFeed {
    data: DataDir,
    sessions_dir: Option<PathBuf>,
    processes: Arc<dyn ProcessTable>,
    /// The mtime of the status record last applied for each session.
    applied: HashMap<String, u64>,
    /// The mtime of the attention record each session has waiting one pass.
    held: HashMap<String, u64>,
    /// The mtime of the progress file last forwarded for each session.
    progress_applied: HashMap<String, u64>,
    first_pass: bool,
    live: HashSet<String>,
    live_read_at: Option<Instant>,
}

impl FileFeed {
    /// A feed over the files in `data`, believing a session only while its
    /// process in `sessions_dir` is alive in `processes`. With no sessions
    /// directory every session is believed.
    pub fn new(
        data: DataDir,
        sessions_dir: Option<PathBuf>,
        processes: Arc<dyn ProcessTable>,
    ) -> Self {
        Self {
            data,
            sessions_dir,
            processes,
            applied: HashMap::new(),
            held: HashMap::new(),
            progress_applied: HashMap::new(),
            first_pass: true,
            live: HashSet::new(),
            live_read_at: None,
        }
    }

    /// Reads the files once and returns what changed since the last pass.
    ///
    /// A new `Stop` or `Notification` waits one pass, so the seen write the
    /// extension makes about 150 ms after it, when the session finished in the
    /// window the user is looking at, is applied together with it and the
    /// session never reads as unread. The first pass holds nothing: sounds are
    /// not armed yet and the initial load has to land at once.
    pub fn pass(&mut self, now: Instant) -> FeedChanges {
        self.refresh_live(now);

        let records = read_statuses(&self.data.status_dir());
        let seen = read_seen(&self.data.seen_dir());
        let present: HashSet<String> = records
            .iter()
            .map(|record| record.event.session_id.clone())
            .collect();
        let mut apply = Vec::new();

        for record in records {
            let session_id = record.event.session_id.clone();
            let written_at_ms = record.written_at_ms;

            if !self.is_live(&session_id) || self.applied.get(&session_id) == Some(&written_at_ms) {
                continue;
            }

            let attention = matches!(
                record.event.kind,
                HookEventKind::Stop | HookEventKind::Notification { .. }
            );

            if attention && !self.first_pass && self.held.get(&session_id) != Some(&written_at_ms) {
                self.held.insert(session_id, written_at_ms);
                continue;
            }

            self.held.remove(&session_id);
            self.applied.insert(session_id, written_at_ms);
            apply.push(record);
        }

        let held: Vec<String> = self.held.keys().cloned().collect();
        for session_id in held {
            if !present.contains(&session_id) || !self.is_live(&session_id) {
                self.held.remove(&session_id);
            }
        }

        let ended: Vec<String> = self
            .applied
            .keys()
            .filter(|session_id| !present.contains(*session_id))
            .cloned()
            .collect();
        for session_id in &ended {
            self.applied.remove(session_id);
        }

        let seen = seen
            .into_iter()
            .filter(|(session_id, _)| present.contains(session_id))
            .collect();

        let progress = self.new_progress();

        self.first_pass = false;

        FeedChanges {
            apply,
            ended,
            seen,
            progress,
        }
    }

    /// The live sessions' progress files not forwarded since they last changed.
    /// Progress asks for no attention, so nothing is held.
    fn new_progress(&mut self) -> Vec<ProgressRecord> {
        let records = read_progress(&self.data.progress_dir());
        let present: HashSet<&str> = records
            .iter()
            .map(|record| record.session_id.as_str())
            .collect();
        self.progress_applied
            .retain(|session_id, _| present.contains(session_id.as_str()));

        let mut progress = Vec::new();
        for record in records {
            if !self.is_live(&record.session_id)
                || self.progress_applied.get(&record.session_id) == Some(&record.written_at_ms)
            {
                continue;
            }

            self.progress_applied
                .insert(record.session_id.clone(), record.written_at_ms);
            progress.push(record);
        }

        progress
    }

    /// Forgets that these sessions' progress was forwarded, so the next pass
    /// forwards the same files again.
    pub fn forget_progress(&mut self, session_ids: &[String]) {
        for session_id in session_ids {
            self.progress_applied.remove(session_id);
        }
    }

    fn is_live(&self, session_id: &str) -> bool {
        self.sessions_dir.is_none() || self.live.contains(session_id)
    }

    /// Rereads the live sessions once the set is older than
    /// [`LIVENESS_INTERVAL`], keeping the previous set when the directory
    /// cannot be read.
    fn refresh_live(&mut self, now: Instant) {
        let Some(dir) = &self.sessions_dir else {
            return;
        };

        if self
            .live_read_at
            .is_some_and(|read_at| now.saturating_duration_since(read_at) < LIVENESS_INTERVAL)
        {
            return;
        }

        let Ok(files) = read_all(dir) else {
            return;
        };

        self.live = files
            .sessions
            .into_iter()
            .filter(|session| self.processes.is_alive(session.pid))
            .map(|session| session.session_id)
            .collect();
        self.live_read_at = Some(now);
    }
}

/// Applies one pass's changes under a single lock: the records as of when they
/// were recorded, then the progress reports, then the sessions whose files
/// vanished, then the seen times, so a snapshot never holds a new attention
/// event without the seen time read alongside it.
///
/// Progress follows the status records so a turn opened in this pass can
/// reject a report written before it. Returns the sessions whose progress found
/// no session to land on.
pub fn apply_changes(
    registry: &Mutex<SessionRegistry>,
    changes: FeedChanges,
    now_ms: u64,
) -> Vec<String> {
    let mut registry = lock_recovering(registry);

    for record in changes.apply {
        registry.apply_status(record, now_ms);
    }

    let mut unapplied = Vec::new();
    for record in changes.progress {
        let session_id = record.session_id.clone();
        if !registry.apply_progress(record, now_ms) {
            unapplied.push(session_id);
        }
    }

    for session_id in changes.ended {
        registry.apply_hook_event(HookEvent::new(session_id, HookEventKind::SessionEnd));
    }

    for (session_id, seen_at_ms) in changes.seen {
        registry.apply_seen(&session_id, seen_at_ms);
    }

    unapplied
}

/// Feeds the registry every [`FEED_INTERVAL`] until `shutdown` fires, calling
/// `on_loaded` once the first pass has been applied.
pub async fn run(
    feed: FileFeed,
    registry: Arc<Mutex<SessionRegistry>>,
    on_loaded: Box<dyn FnOnce() + Send>,
    shutdown: oneshot::Receiver<()>,
) {
    run_every(FEED_INTERVAL, feed, registry, on_loaded, shutdown).await;
}

async fn run_every(
    period: Duration,
    feed: FileFeed,
    registry: Arc<Mutex<SessionRegistry>>,
    on_loaded: Box<dyn FnOnce() + Send>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let feed = Arc::new(Mutex::new(feed));
    let mut on_loaded = Some(on_loaded);
    let mut passes = tokio::time::interval(period);
    passes.set_missed_tick_behavior(MissedTickBehavior::Delay);

    passes.tick().await;

    loop {
        let feeding = tokio::task::spawn_blocking({
            let feed = Arc::clone(&feed);
            let registry = Arc::clone(&registry);

            move || {
                let changes = lock_recovering(&feed).pass(Instant::now());
                let unapplied = apply_changes(&registry, changes, epoch_now_ms());
                if !unapplied.is_empty() {
                    lock_recovering(&feed).forget_progress(&unapplied);
                }
            }
        });

        if let Err(error) = feeding.await {
            eprintln!(
                "aydy: a status file pass did not finish: {error}; the next one runs as usual"
            );
        }

        // Called even when the first pass failed or found nothing, or sounds
        // would never arm.
        if let Some(loaded) = on_loaded.take() {
            loaded();
        }

        tokio::select! {
            _ = passes.tick() => {}
            _ = &mut shutdown => return,
        }
    }
}

fn epoch_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The stop signal for the running feed.
pub struct FeedShutdown {
    signal: Mutex<Option<oneshot::Sender<()>>>,
}

impl FeedShutdown {
    /// A trigger that fires `signal`.
    pub fn new(signal: oneshot::Sender<()>) -> Self {
        Self {
            signal: Mutex::new(Some(signal)),
        }
    }

    /// Asks the feed to stop. Firing twice does nothing.
    pub fn trigger(&self) {
        if let Some(signal) = lock_recovering(&self.signal).take() {
            let _ = signal.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tokio::sync::watch;

    use super::*;
    use crate::clock::TestClock;
    use crate::discovery::session_files::FixedProcesses;
    use crate::discovery::status_files::write_seen;
    use crate::discovery::test_support::{scratch_dir, session_file};
    use crate::notify::sound::test_support::RecordingPlayer;
    use crate::notify::sound::{Sound, SoundNotifier, SoundPlayer};
    use crate::session::registry::{RegistrySnapshot, SessionSnapshot};
    use crate::session::signals::{Step, WorkSignals};
    use crate::settings::store::Settings;

    const BASE_MS: u64 = 1_785_526_308_000;
    const NOW_MS: u64 = BASE_MS + 60_000;

    /// A process table whose one process can be brought to life mid-test.
    struct Switchable {
        pid: u32,
        alive: AtomicBool,
    }

    impl ProcessTable for Switchable {
        fn is_alive(&self, pid: u32) -> bool {
            pid == self.pid && self.alive.load(Ordering::Relaxed)
        }
    }

    struct Fixture {
        root: PathBuf,
        data: DataDir,
        sessions: PathBuf,
    }

    impl Fixture {
        /// A data directory and a sessions directory holding a file for each
        /// `(pid, session id)` pair.
        fn new(name: &str, sessions: &[(u32, &str)]) -> Self {
            let root = scratch_dir(name);
            let data = DataDir::new(root.join("data"));
            fs::create_dir_all(data.status_dir()).expect("the status directory to be creatable");
            let dir = root.join("sessions");
            fs::create_dir_all(&dir).expect("the sessions directory to be creatable");

            for (pid, session_id) in sessions {
                fs::write(
                    dir.join(format!("{pid}.json")),
                    session_file(*pid, session_id, &root, "claude-vscode"),
                )
                .expect("the session file to be writable");
            }

            Self {
                root,
                data,
                sessions: dir,
            }
        }

        fn feed(&self, processes: Arc<dyn ProcessTable>) -> FileFeed {
            FileFeed::new(self.data.clone(), Some(self.sessions.clone()), processes)
        }

        fn live(&self, pids: impl IntoIterator<Item = u32>) -> FileFeed {
            self.feed(Arc::new(FixedProcesses::new(pids)))
        }

        fn status(&self, session_id: &str, event: &str, written_at_ms: u64) {
            write_status(&self.data.status_dir(), session_id, event, written_at_ms);
        }

        fn seen(&self, session_id: &str, seen_at_ms: u64) {
            write_seen(&self.data.seen_dir(), session_id, seen_at_ms)
                .expect("the seen file to be writable");
        }

        fn progress(&self, session_id: &str, body: &str, written_at_ms: u64) {
            let dir = self.data.progress_dir();
            fs::create_dir_all(&dir).expect("the progress directory to be creatable");
            let path = dir.join(format!("{session_id}.json"));
            fs::write(&path, body).expect("the progress file to be writable");
            set_mtime(&path, written_at_ms);
        }

        fn remove_status(&self, session_id: &str) {
            fs::remove_file(self.data.status_dir().join(format!("{session_id}.json")))
                .expect("the status file to be removable");
        }
    }

    fn write_status(dir: &Path, session_id: &str, event: &str, written_at_ms: u64) {
        let path = dir.join(format!("{session_id}.json"));
        fs::write(
            &path,
            format!(r#"{{"session_id":"{session_id}","hook_event_name":"{event}"}}"#),
        )
        .expect("the status file to be writable");
        set_mtime(&path, written_at_ms);
    }

    fn set_mtime(path: &Path, written_at_ms: u64) {
        File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(UNIX_EPOCH + Duration::from_millis(written_at_ms)))
            .expect("the mtime to be settable");
    }

    fn progress_ids(changes: &FeedChanges) -> Vec<&str> {
        changes
            .progress
            .iter()
            .map(|record| record.session_id.as_str())
            .collect()
    }

    fn signals(registry: &Mutex<SessionRegistry>, session_id: &str) -> WorkSignals {
        let snapshot = lock_recovering(registry).snapshot();

        rows(&snapshot)
            .iter()
            .find(|row| row.id == session_id)
            .expect("the session to be listed")
            .signals
            .expect("a Working session to carry signals")
    }

    fn applied_ids(changes: &FeedChanges) -> Vec<&str> {
        changes
            .apply
            .iter()
            .map(|record| record.event.session_id.as_str())
            .collect()
    }

    fn registry() -> Mutex<SessionRegistry> {
        Mutex::new(SessionRegistry::new(Arc::new(TestClock::new())))
    }

    fn rows(snapshot: &RegistrySnapshot) -> Vec<&SessionSnapshot> {
        snapshot
            .groups
            .iter()
            .flat_map(|group| group.sessions.iter())
            .collect()
    }

    fn unread(registry: &Mutex<SessionRegistry>, session_id: &str) -> bool {
        let snapshot = lock_recovering(registry).snapshot();

        rows(&snapshot)
            .iter()
            .find(|row| row.id == session_id)
            .expect("the session to be listed")
            .unread
    }

    #[test]
    fn a_live_sessions_record_applies_once_and_again_only_after_it_changes() {
        let fixture = Fixture::new("feed-once", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let now = Instant::now();
        fixture.status("a", "UserPromptSubmit", BASE_MS);

        assert_eq!(applied_ids(&feed.pass(now)), ["a"]);
        assert!(feed.pass(now).apply.is_empty());

        fixture.status("a", "UserPromptSubmit", BASE_MS + 1);

        assert_eq!(applied_ids(&feed.pass(now)), ["a"]);
        assert!(feed.pass(now).apply.is_empty());
    }

    #[test]
    fn a_dead_sessions_record_is_applied_once_its_process_is_alive() {
        let fixture = Fixture::new("feed-dead", &[(11, "a")]);
        let processes = Arc::new(Switchable {
            pid: 11,
            alive: AtomicBool::new(false),
        });
        let mut feed = fixture.feed(Arc::clone(&processes) as Arc<dyn ProcessTable>);
        let now = Instant::now();
        fixture.status("a", "UserPromptSubmit", BASE_MS);

        assert!(feed.pass(now).apply.is_empty());

        processes.alive.store(true, Ordering::Relaxed);

        assert!(feed.pass(now + Duration::from_secs(1)).apply.is_empty());
        assert_eq!(applied_ids(&feed.pass(now + LIVENESS_INTERVAL)), ["a"]);
    }

    #[test]
    fn a_deleted_status_file_ends_its_session_once() {
        let fixture = Fixture::new("feed-ended", &[(11, "a"), (12, "b")]);
        let mut feed = fixture.live([11]);
        let now = Instant::now();
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        fixture.status("b", "UserPromptSubmit", BASE_MS);
        feed.pass(now);

        fixture.remove_status("a");
        fixture.remove_status("b");

        assert_eq!(feed.pass(now).ended, ["a"]);
        assert!(feed.pass(now).ended.is_empty());
    }

    #[test]
    fn seen_times_are_passed_through_for_the_sessions_present() {
        let fixture = Fixture::new("feed-seen", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        fixture.seen("a", BASE_MS + 5);
        fixture.seen("gone", BASE_MS + 6);

        let changes = feed.pass(Instant::now());

        assert_eq!(
            changes.seen,
            HashMap::from([("a".to_string(), BASE_MS + 5)])
        );
    }

    #[test]
    fn an_unreadable_sessions_directory_keeps_the_previous_live_set() {
        let fixture = Fixture::new("feed-unreadable", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let now = Instant::now();
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        feed.pass(now);

        fs::remove_dir_all(&fixture.sessions).expect("the sessions directory to be removable");
        fixture.status("a", "UserPromptSubmit", BASE_MS + 1);

        assert_eq!(applied_ids(&feed.pass(now + LIVENESS_INTERVAL)), ["a"]);
    }

    #[test]
    fn with_no_sessions_directory_every_status_is_live() {
        let fixture = Fixture::new("feed-no-dir", &[]);
        let mut feed = FileFeed::new(
            fixture.data.clone(),
            None,
            Arc::new(FixedProcesses::new([])),
        );
        fixture.status("a", "UserPromptSubmit", BASE_MS);

        assert_eq!(applied_ids(&feed.pass(Instant::now())), ["a"]);
    }

    #[test]
    fn applied_changes_leave_a_finished_session_unread() {
        let fixture = Fixture::new("feed-unread", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        fixture.status("a", "Stop", BASE_MS);

        apply_changes(&registry, feed.pass(Instant::now()), NOW_MS);

        assert!(unread(&registry, "a"));
    }

    #[test]
    fn applied_changes_place_each_record_at_the_time_it_was_recorded() {
        let fixture = Fixture::new("feed-age", &[(11, "old"), (12, "recent")]);
        let mut feed = fixture.live([11, 12]);
        let registry = registry();
        fixture.status("old", "Stop", NOW_MS - 3_600_000);
        fixture.status("recent", "Stop", NOW_MS - 60_000);

        apply_changes(&registry, feed.pass(Instant::now()), NOW_MS);

        let snapshot = lock_recovering(&registry).snapshot();
        let order: Vec<&str> = rows(&snapshot).iter().map(|row| row.id.as_str()).collect();
        assert_eq!(order, ["recent", "old"]);
    }

    #[test]
    fn a_new_attention_record_is_held_for_one_pass() {
        let fixture = Fixture::new("feed-hold", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let now = Instant::now();
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        feed.pass(now);

        fixture.status("a", "Stop", BASE_MS + 1_000);

        assert!(feed.pass(now).apply.is_empty());
        assert_eq!(applied_ids(&feed.pass(now)), ["a"]);
        assert!(feed.pass(now).apply.is_empty());
    }

    #[test]
    fn a_seen_write_landing_while_a_stop_is_held_keeps_the_session_read() {
        let fixture = Fixture::new("feed-hold-seen", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        let now = Instant::now();
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        apply_changes(&registry, feed.pass(now), NOW_MS);
        fixture.status("a", "Stop", BASE_MS + 1_000);
        apply_changes(&registry, feed.pass(now), NOW_MS);

        fixture.seen("a", BASE_MS + 1_150);
        let changes = feed.pass(now);
        assert_eq!(applied_ids(&changes), ["a"]);
        apply_changes(&registry, changes, NOW_MS);

        assert!(!unread(&registry, "a"));
    }

    #[test]
    fn an_event_that_asks_for_no_attention_is_applied_on_the_pass_that_reads_it() {
        let fixture = Fixture::new("feed-no-hold", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let now = Instant::now();
        fixture.status("a", "Stop", BASE_MS);
        feed.pass(now);
        fixture.status("a", "Stop", BASE_MS + 1_000);
        feed.pass(now);

        fixture.status("a", "UserPromptSubmit", BASE_MS + 1_200);

        assert_eq!(applied_ids(&feed.pass(now)), ["a"]);
        assert!(feed.held.is_empty());
    }

    #[test]
    fn the_first_pass_applies_an_attention_record_at_once() {
        let fixture = Fixture::new("feed-first", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        fixture.status("a", "Stop", BASE_MS);

        assert_eq!(applied_ids(&feed.pass(Instant::now())), ["a"]);
    }

    const PROGRESS: &str = r#"{"etaSeconds":360,"step":{"done":4,"total":7}}"#;

    #[test]
    fn a_live_sessions_progress_is_forwarded_once_and_again_only_after_it_changes() {
        let fixture = Fixture::new("feed-progress-once", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let now = Instant::now();
        fixture.progress("a", PROGRESS, BASE_MS);

        assert_eq!(progress_ids(&feed.pass(now)), ["a"]);
        assert!(feed.pass(now).progress.is_empty());

        fixture.progress("a", PROGRESS, BASE_MS + 1);

        assert_eq!(progress_ids(&feed.pass(now)), ["a"]);
        assert!(feed.pass(now).progress.is_empty());
    }

    #[test]
    fn a_dead_sessions_progress_is_not_forwarded() {
        let fixture = Fixture::new("feed-progress-dead", &[(11, "a")]);
        let mut feed = fixture.live([]);
        fixture.progress("a", PROGRESS, BASE_MS);

        assert!(feed.pass(Instant::now()).progress.is_empty());
    }

    #[test]
    fn applied_progress_puts_the_eta_and_step_on_a_working_session() {
        let fixture = Fixture::new("feed-progress-applied", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        fixture.status("a", "UserPromptSubmit", NOW_MS - 2_000);
        fixture.progress("a", PROGRESS, NOW_MS - 1_000);

        let unapplied = apply_changes(&registry, feed.pass(Instant::now()), NOW_MS);

        assert!(unapplied.is_empty());
        let signals = signals(&registry, "a");
        assert!(signals.eta.is_some());
        assert_eq!(signals.step, Step::new(4, 7));
    }

    #[test]
    fn progress_older_than_a_prompt_applied_in_the_same_pass_yields_no_eta() {
        let fixture = Fixture::new("feed-progress-stale", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        fixture.status("a", "UserPromptSubmit", NOW_MS - 1_000);
        fixture.progress("a", PROGRESS, NOW_MS - 2_000);

        apply_changes(&registry, feed.pass(Instant::now()), NOW_MS);

        assert_eq!(signals(&registry, "a").eta, None);
    }

    #[test]
    fn progress_for_a_session_not_yet_tracked_lands_once_it_is() {
        let fixture = Fixture::new("feed-progress-retry", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        let now = Instant::now();
        fixture.progress("a", PROGRESS, NOW_MS - 1_000);

        let unapplied = apply_changes(&registry, feed.pass(now), NOW_MS);
        assert_eq!(unapplied, ["a"]);
        assert!(lock_recovering(&registry).session("a").is_none());

        feed.forget_progress(&unapplied);
        let changes = feed.pass(now);
        assert_eq!(progress_ids(&changes), ["a"]);

        lock_recovering(&registry).apply_status(
            StatusRecord {
                event: HookEvent::new("a", HookEventKind::UserPromptSubmit),
                written_at_ms: NOW_MS - 2_000,
            },
            NOW_MS,
        );
        assert!(apply_changes(&registry, changes, NOW_MS).is_empty());
        assert!(signals(&registry, "a").eta.is_some());
        assert!(feed.pass(now).progress.is_empty());
    }

    fn armed_sounds() -> (SoundNotifier, Arc<RecordingPlayer>) {
        let player = Arc::new(RecordingPlayer::new());
        let notifier = SoundNotifier::new(
            Arc::clone(&player) as Arc<dyn SoundPlayer>,
            watch::channel(Settings::default()).1,
            Arc::new(TestClock::new()),
        );

        (notifier, player)
    }

    fn feed_and_observe(
        feed: &mut FileFeed,
        registry: &Mutex<SessionRegistry>,
        sounds: &SoundNotifier,
    ) {
        apply_changes(registry, feed.pass(Instant::now()), NOW_MS);
        sounds.observe(&lock_recovering(registry).snapshot());
    }

    #[test]
    fn a_stop_in_the_window_the_user_is_looking_at_plays_nothing() {
        let fixture = Fixture::new("feed-sound-seen", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        let (sounds, player) = armed_sounds();
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        feed_and_observe(&mut feed, &registry, &sounds);

        fixture.status("a", "Stop", BASE_MS + 1_000);
        feed_and_observe(&mut feed, &registry, &sounds);
        fixture.seen("a", BASE_MS + 1_150);
        feed_and_observe(&mut feed, &registry, &sounds);

        assert!(player.asked().is_empty());
    }

    #[test]
    fn a_stop_nobody_saw_plays_once_on_the_next_pass() {
        let fixture = Fixture::new("feed-sound-unseen", &[(11, "a")]);
        let mut feed = fixture.live([11]);
        let registry = registry();
        let (sounds, player) = armed_sounds();
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        feed_and_observe(&mut feed, &registry, &sounds);

        fixture.status("a", "Stop", BASE_MS + 1_000);
        feed_and_observe(&mut feed, &registry, &sounds);
        assert!(player.asked().is_empty());

        feed_and_observe(&mut feed, &registry, &sounds);
        feed_and_observe(&mut feed, &registry, &sounds);

        assert_eq!(player.asked(), [Sound::Finished]);
    }

    #[tokio::test]
    async fn the_running_feed_reports_loaded_once_after_the_first_pass_is_applied() {
        let fixture = Fixture::new("feed-run", &[(11, "a")]);
        fixture.status("a", "UserPromptSubmit", BASE_MS);
        let registry = Arc::new(registry());
        let calls = Arc::new(AtomicUsize::new(0));
        let loaded_with_session = Arc::new(AtomicBool::new(false));
        let on_loaded: Box<dyn FnOnce() + Send> = Box::new({
            let registry = Arc::clone(&registry);
            let calls = Arc::clone(&calls);
            let loaded_with_session = Arc::clone(&loaded_with_session);

            move || {
                calls.fetch_add(1, Ordering::Relaxed);
                loaded_with_session.store(
                    lock_recovering(&registry).session("a").is_some(),
                    Ordering::Relaxed,
                );
            }
        });
        let (signal, shutdown) = oneshot::channel();
        let feeding = tokio::spawn(run_every(
            Duration::from_millis(10),
            fixture.live([11]),
            Arc::clone(&registry),
            on_loaded,
            shutdown,
        ));

        tokio::time::sleep(Duration::from_millis(80)).await;
        FeedShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), feeding)
            .await
            .expect("the feed to stop when asked")
            .expect("the feed not to panic");

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(loaded_with_session.load(Ordering::Relaxed));
        let _ = fs::remove_dir_all(&fixture.root);
    }
}
