//! Noticing that a session has resumed work after a mid-turn wait.
//!
//! No hook event fires when a permission prompt is answered, so the signal is
//! the session's transcript moving on: once a session is waiting, the next byte
//! Claude Code appends to its transcript is evidence that the wait is over.
//!
//! **This runs on its own cadence, not on the reconciliation scan.** The scan
//! runs every 25 seconds and a resume has to be reflected within 5, so wiring
//! the two together would measure a failure the requirement expects to pass.
//! The poll is cheap because it is scoped to the sessions currently waiting,
//! which is a handful at most, and each one costs a single `stat`.
//!
//! ## Why the transcript and not a sixth hook
//!
//! Measured on this machine against Claude Code 2.1.220:
//!
//! - The transcript is appended to roughly 130 ms after a tool call finishes,
//!   and **not at all while one runs** — 22 seconds of a running tool produced
//!   no write.
//! - Nothing whatsoever is written when a permission prompt is *answered*. Real
//!   transcripts on this machine hold `assistant`/`tool_use` to `user`/tool
//!   result gaps of up to 12 hours with no entry in between, and a permission
//!   wait lives inside exactly that gap.
//! - Claude Code's own hook table places `PermissionRequest` *before* the
//!   prompt and `PostToolUse` *after* a successful tool, so a sixth hook would
//!   fire at the same moment the transcript grows and no earlier.
//!
//! So the poll interval is the only thing a sixth hook would buy, and it would
//! cost the deny case: a refused prompt fires `PermissionDenied`, never
//! `PostToolUse`, while the transcript grows either way.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;

use crate::discovery::reconciler::ScanShutdown;
use crate::discovery::transcript::{self, Stamp};
use crate::listener::server::lock_recovering;
use crate::session::event::SessionId;
use crate::session::registry::SessionRegistry;
use crate::session::status::SessionStatus;

/// How often the transcripts of waiting sessions are checked.
///
/// A session has to be back to Working within 5 seconds of resuming. The first
/// evidence Claude Code produces is the transcript write that follows the
/// resumed tool call, so the budget is that write plus this interval, and one
/// second leaves the bound four-fifths unspent.
pub const RESUME_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Watches the transcripts of the sessions that are waiting on the user.
#[derive(Debug, Default)]
pub struct ResumeWatcher {
    /// What each waiting session's transcript looked like when the wait was
    /// first observed. A session is only reported resumed once its transcript
    /// differs from this, so the observation that starts the watch can never be
    /// the one that ends it.
    waiting: HashMap<SessionId, Stamp>,
}

impl ResumeWatcher {
    /// A watcher tracking nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many sessions are currently being watched.
    pub fn watching(&self) -> usize {
        self.waiting.len()
    }

    /// Checks every waiting session once, and reports the ones whose
    /// transcripts have moved on.
    ///
    /// Blocking: it stats files and waits on the registry lock, so it belongs
    /// on the blocking pool rather than on an async worker.
    pub fn poll(&mut self, registry: &Mutex<SessionRegistry>) {
        let watched: Vec<(SessionId, PathBuf)> = lock_recovering(registry)
            .sessions()
            .filter(|session| session.status() == SessionStatus::NeedsInput)
            .filter_map(|session| Some((session.id.clone(), session.transcript_path.clone()?)))
            .collect();

        let mut resumed = Vec::new();
        let mut still_waiting = HashMap::with_capacity(watched.len());

        for (session_id, path) in watched {
            let Some(stamp) = transcript::stamp(&path) else {
                // A transcript that cannot be looked at yet is not evidence of
                // a resume; the wait stands until it can be.
                continue;
            };

            match self.waiting.get(&session_id) {
                Some(observed) if observed != &stamp => resumed.push(session_id),
                Some(observed) => {
                    still_waiting.insert(session_id, *observed);
                }
                None => {
                    still_waiting.insert(session_id, stamp);
                }
            }
        }

        // Sessions that stopped waiting for any other reason — the turn ended,
        // a new prompt arrived, the session left — drop out here rather than
        // being carried into the next pass.
        self.waiting = still_waiting;

        if resumed.is_empty() {
            return;
        }

        let mut registry = lock_recovering(registry);

        for session_id in resumed {
            registry.apply_resume(&session_id);
        }
    }
}

/// Polls every [`RESUME_POLL_INTERVAL`] until `shutdown` fires.
pub async fn run(registry: Arc<Mutex<SessionRegistry>>, shutdown: oneshot::Receiver<()>) {
    run_every(
        RESUME_POLL_INTERVAL,
        ResumeWatcher::new(),
        registry,
        shutdown,
    )
    .await;
}

/// The loop behind [`run`], with the period and the watcher as parameters so a
/// test can drive it without waiting seconds for each pass.
async fn run_every(
    period: Duration,
    watcher: ResumeWatcher,
    registry: Arc<Mutex<SessionRegistry>>,
    mut shutdown: oneshot::Receiver<()>,
) {
    // Held outside the loop, so a pass that panics costs that pass and not the
    // watching that follows it.
    let watcher = Arc::new(Mutex::new(watcher));
    let mut passes = tokio::time::interval(period);
    passes.set_missed_tick_behavior(MissedTickBehavior::Delay);

    // The first tick is immediate; the pass below is that tick.
    passes.tick().await;

    loop {
        let polling = tokio::task::spawn_blocking({
            let watcher = Arc::clone(&watcher);
            let registry = Arc::clone(&registry);

            move || lock_recovering(&watcher).poll(&registry)
        });

        if let Err(error) = polling.await {
            eprintln!("aydy: a resume check did not finish: {error}; the next one runs as usual");
        }

        tokio::select! {
            _ = passes.tick() => {}
            _ = &mut shutdown => return,
        }
    }
}

/// The stop signal for the running resume watch.
///
/// A type of its own rather than a second [`ScanShutdown`], because the app
/// holds each of its shutdown triggers as managed state and two of one type
/// would be one.
pub struct ResumeShutdown(ScanShutdown);

impl ResumeShutdown {
    /// A trigger that fires `signal`.
    pub fn new(signal: oneshot::Sender<()>) -> Self {
        Self(ScanShutdown::new(signal))
    }

    /// Asks the watch to stop. Firing twice does nothing.
    pub fn trigger(&self) {
        self.0.trigger();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::clock::TestClock;
    use crate::discovery::test_support::scratch_dir;
    use crate::session::event::{HookEvent, HookEventKind};

    /// The transcript entry Claude Code appends when a tool call finishes,
    /// which is the first thing it writes after a prompt has been answered.
    const TOOL_RESULT: &str = r#"{"type":"user","gitBranch":"main","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#;

    fn transcript(name: &str) -> PathBuf {
        let path = scratch_dir(name).join("s-1.jsonl");
        fs::write(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"stop_reason\":\"tool_use\"}}\n",
        )
        .expect("the transcript to be writable");

        path
    }

    fn grow(path: &Path) {
        let mut contents = fs::read_to_string(path).expect("the transcript to be readable");
        contents.push_str(TOOL_RESULT);
        contents.push('\n');
        fs::write(path, contents).expect("the transcript to be writable");
    }

    fn registry(clock: &Arc<TestClock>) -> Arc<Mutex<SessionRegistry>> {
        Arc::new(Mutex::new(SessionRegistry::new(clock.clone())))
    }

    fn waiting(registry: &Mutex<SessionRegistry>, path: &Path) {
        let mut registry = lock_recovering(registry);
        registry.apply_hook_event(
            HookEvent::new("s-1", HookEventKind::UserPromptSubmit).with_transcript_path(path),
        );
        registry.apply_hook_event(HookEvent::new(
            "s-1",
            HookEventKind::Notification {
                message: Some("Needs permission for Bash".to_string()),
            },
        ));
    }

    fn status(registry: &Mutex<SessionRegistry>) -> Option<SessionStatus> {
        lock_recovering(registry)
            .session("s-1")
            .map(|session| session.status())
    }

    #[test]
    fn a_waiting_session_whose_transcript_moves_on_returns_to_working() {
        let path = transcript("resumed");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        let mut watcher = ResumeWatcher::new();

        watcher.poll(&registry);

        assert_eq!(status(&registry), Some(SessionStatus::NeedsInput));

        grow(&path);
        watcher.poll(&registry);

        assert_eq!(status(&registry), Some(SessionStatus::Working));
    }

    #[test]
    fn a_waiting_session_whose_transcript_stands_still_keeps_waiting() {
        let path = transcript("still");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        let mut watcher = ResumeWatcher::new();

        for _ in 0..5 {
            watcher.poll(&registry);
        }

        assert_eq!(status(&registry), Some(SessionStatus::NeedsInput));
    }

    #[test]
    fn the_pass_that_starts_watching_a_session_never_reports_it_resumed() {
        let path = transcript("baseline");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        // The transcript already grew before anything looked at it, which is
        // what a session that has been waiting since before AYDY started looks
        // like.
        grow(&path);
        let mut watcher = ResumeWatcher::new();

        watcher.poll(&registry);

        assert_eq!(status(&registry), Some(SessionStatus::NeedsInput));
    }

    #[test]
    fn a_session_that_is_not_waiting_is_not_watched() {
        let path = transcript("working");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        lock_recovering(&registry).apply_hook_event(
            HookEvent::new("s-1", HookEventKind::UserPromptSubmit).with_transcript_path(&path),
        );
        let mut watcher = ResumeWatcher::new();

        watcher.poll(&registry);

        assert_eq!(watcher.watching(), 0);
    }

    #[test]
    fn a_session_that_stops_waiting_is_forgotten() {
        let path = transcript("forgotten");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        let mut watcher = ResumeWatcher::new();
        watcher.poll(&registry);

        assert_eq!(watcher.watching(), 1);

        lock_recovering(&registry).apply_hook_event(HookEvent::new("s-1", HookEventKind::Stop));
        watcher.poll(&registry);

        assert_eq!(watcher.watching(), 0);
    }

    #[test]
    fn a_waiting_session_with_no_transcript_yet_is_not_reported_resumed() {
        let path = scratch_dir("no-transcript").join("s-1.jsonl");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        let mut watcher = ResumeWatcher::new();

        watcher.poll(&registry);
        fs::write(&path, TOOL_RESULT).expect("the transcript to be writable");
        watcher.poll(&registry);

        assert_eq!(status(&registry), Some(SessionStatus::NeedsInput));
    }

    #[test]
    fn the_resume_poll_is_far_faster_than_the_five_second_bound() {
        const BOUND: Duration = Duration::from_secs(5);

        assert!(
            RESUME_POLL_INTERVAL * 2 < BOUND,
            "a resume takes up to {RESUME_POLL_INTERVAL:?} to be noticed"
        );
    }

    #[tokio::test]
    async fn the_running_watch_notices_a_resume_and_stops_when_asked() {
        let path = transcript("running");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        let (signal, shutdown) = oneshot::channel();
        let watching = tokio::spawn(run_every(
            Duration::from_millis(10),
            ResumeWatcher::new(),
            Arc::clone(&registry),
            shutdown,
        ));

        tokio::time::sleep(Duration::from_millis(50)).await;
        grow(&path);

        let mut resumed = false;
        for _ in 0..200 {
            if status(&registry) == Some(SessionStatus::Working) {
                resumed = true;
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        ResumeShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), watching)
            .await
            .expect("the watch to stop when asked")
            .expect("the watch not to panic");

        assert!(resumed, "the running watch never noticed the resume");
    }

    /// Runs on a multi-threaded runtime with a single worker, as a busy machine
    /// effectively is: one blocking wait on the wrong thread stops every timer
    /// and every socket the app has.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_watch_waiting_for_the_registry_does_not_block_the_runtime() {
        let path = transcript("starvation");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        waiting(&registry, &path);
        let busy = Arc::clone(&registry);
        let (locked, is_locked) = oneshot::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _guard = lock_recovering(&busy);
            locked.send(()).expect("the test to be waiting");
            // Bounded, so a failing assertion below cannot deadlock the test.
            let _ = released.recv_timeout(Duration::from_secs(5));
        });
        is_locked.await.expect("the registry to be held");

        let (signal, shutdown) = oneshot::channel();
        let watching = tokio::spawn(run_every(
            Duration::from_millis(10),
            ResumeWatcher::new(),
            Arc::clone(&registry),
            shutdown,
        ));

        let started = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let waited = started.elapsed();

        let _ = release.send(());
        holder.join().expect("the holding thread to finish");
        ResumeShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), watching)
            .await
            .expect("the watch to stop when asked")
            .expect("the watch not to panic");

        assert!(
            waited < Duration::from_secs(1),
            "a fifty-millisecond timer took {waited:?} while a resume check waited for the registry"
        );
    }
}
