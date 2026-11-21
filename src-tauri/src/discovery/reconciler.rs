//! The periodic scan, and the diff it hands the registry.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;

use crate::discovery::label;
use crate::discovery::process_tree::{ProcessParents, SystemParents};
use crate::discovery::repo::{self, RepoIdentity};
use crate::discovery::session_files::{self, ProcessTable, SessionFile};
use crate::discovery::transcript::{self, TranscriptCache, TranscriptFacts};
use crate::discovery::window_map::{self, WindowMap};
use crate::extension::registry::ExtensionRegistry;
use crate::listener::server::lock_recovering;
use crate::session::event::{
    DiscoveredSession, ReconciliationResult, SessionEnrichment, SessionId, WindowId,
};
use crate::session::registry::SessionRegistry;

/// How often the scan runs — and, because the scan is also what sweeps expired
/// sessions out of the registry, how finely the resolution window can be
/// served.
///
/// This number and [`UNKNOWN_RESOLUTION_WINDOW`] are one budget split in two,
/// not two independent settings. A stopped session has to leave the list within
/// two minutes, and a session whose status cannot be determined is allowed two
/// minutes to resolve; taken separately those two allowances add up to three.
///
/// The whole chain, worst case: a session stops immediately after a scan, so a
/// full interval passes before the scan that reports it gone; it then waits out
/// the resolution window, which is served in whole intervals because nothing
/// but a scan removes it. That is 25 s + 3 × 25 s = 100 s, twenty seconds
/// inside the two-minute ceiling. The interval is also well inside the
/// once-a-minute floor reconciliation itself has to meet.
///
/// **Neither number can move on its own, and the margin is small.** Raising the
/// interval or the window pushes worst-case detection out, and a window that is
/// not a whole number of intervals costs a whole extra interval.
///
/// [`UNKNOWN_RESOLUTION_WINDOW`]: crate::session::status::UNKNOWN_RESOLUTION_WINDOW
pub const SCAN_INTERVAL: Duration = Duration::from_secs(25);

/// The directory Claude Code keeps its per-session files in, unless
/// `CLAUDE_CONFIG_DIR` says otherwise.
const CONFIG_DIR: &str = ".claude";

/// The environment variable Claude Code reads its configuration directory from.
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// The directory holding one file per live session.
const SESSIONS_DIR: &str = "sessions";

/// Where this machine's session files live, or `None` when there is no home
/// directory to look in.
pub fn user_sessions_dir() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os(CONFIG_DIR_VARIABLE) {
        if !configured.is_empty() {
            return Some(PathBuf::from(configured).join(SESSIONS_DIR));
        }
    }

    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;

    Some(PathBuf::from(home).join(CONFIG_DIR).join(SESSIONS_DIR))
}

/// What one scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// The sessions seen, and the sessions positively gone.
    pub result: ReconciliationResult,
    /// The repository each session seen belongs to, for the sessions that
    /// belong to one.
    pub repos: Vec<(SessionId, RepoIdentity)>,
    /// What each session seen has in its transcript.
    pub transcripts: Vec<(SessionId, TranscriptFacts)>,
    /// The VS Code window running each session seen, for the ones the process
    /// tree places in a registered window. Empty without the extension.
    pub windows: WindowMap,
}

/// The periodic reconciliation scan.
///
/// It remembers which sessions it has seen, because that is what makes a
/// removal safe: a session it has never seen is a session it cannot speak for.
pub struct Reconciler {
    dir: PathBuf,
    /// Where the transcripts live, which is the only account of a session the
    /// scan discovers before any hook has reported on it.
    projects_dir: PathBuf,
    transcripts: TranscriptCache,
    processes: Arc<dyn ProcessTable>,
    /// Every session this scan has observed in a local session file, and the
    /// process it was last observed under.
    ///
    /// The process id is half the record on purpose. This machine reaps session
    /// files, so a session whose `SessionEnd` never arrived — a listener
    /// restart, a port move — leaves no file behind to judge it by. What is
    /// left is the process it was seen under, and a dead process for a session
    /// with no file is as positive as a dead process for a session with one.
    seen: HashMap<SessionId, u32>,
    /// The windows the companion extension has registered, or `None` when the
    /// extension is not part of this scan — which is the shape of every scan on
    /// a machine that has never installed it.
    extensions: Option<Arc<ExtensionRegistry>>,
    /// How a session's process is traced back to the extension host running it.
    parents: Arc<dyn ProcessParents>,
    /// Whether the scan is currently unable to see anything, so the fallback is
    /// reported when it starts rather than on every pass.
    degraded: bool,
}

impl Reconciler {
    /// A scan over `dir`, reading transcripts from `projects_dir` and checking
    /// process ids against `processes`.
    pub fn new(
        dir: impl Into<PathBuf>,
        projects_dir: impl Into<PathBuf>,
        processes: Arc<dyn ProcessTable>,
    ) -> Self {
        Self {
            dir: dir.into(),
            projects_dir: projects_dir.into(),
            transcripts: TranscriptCache::new(),
            processes,
            seen: HashMap::new(),
            extensions: None,
            parents: Arc::new(SystemParents),
            degraded: false,
        }
    }

    /// Joins the sessions this scan sees to the windows registered in
    /// `extensions`, tracing ancestry through `parents`.
    ///
    /// Left off, the scan places no session in any window, which is exactly
    /// what a machine without the companion extension produces — so the
    /// no-extension path is the default rather than a case to remember.
    pub fn with_windows(
        mut self,
        extensions: Arc<ExtensionRegistry>,
        parents: Arc<dyn ProcessParents>,
    ) -> Self {
        self.extensions = Some(extensions);
        self.parents = parents;
        self
    }

    /// Whether the scan can currently see nothing, leaving the app on hook-only
    /// tracking.
    ///
    /// Sessions still appear from live events while this holds; what is lost is
    /// self-healing.
    pub fn is_degraded(&self) -> bool {
        self.degraded
    }

    /// Runs one pass.
    ///
    /// Sessions with a live process are reported present. A session is reported
    /// **gone** only when this scan has seen it before and the process it was
    /// seen under is no longer alive — whether its file is still there or has
    /// been reaped since. Those are the cases the scan can speak to. A scan
    /// that can see nothing reports nothing gone, because absence from a scan
    /// is not evidence of death: a session AYDY has never seen in a session
    /// file has no process on record and can never be reported gone here.
    pub fn scan(&mut self) -> Scan {
        let files = match session_files::read_all(&self.dir) {
            Ok(files) => files,
            Err(error) => {
                self.degrade(&format!("cannot read {}: {error}", self.dir.display()));

                return Scan::default();
            }
        };

        if files.sessions.is_empty() && files.unrecognised > 0 {
            self.degrade(&format!(
                "none of the {} files in {} are in a shape it recognises",
                files.unrecognised,
                self.dir.display()
            ));

            return Scan::default();
        }

        self.recover();

        let (live, stale): (Vec<SessionFile>, Vec<SessionFile>) = files
            .sessions
            .into_iter()
            .partition(|session| self.processes.is_alive(session.pid));

        let mut scan = Scan::default();
        let mut read = HashSet::with_capacity(live.len());

        for session in &live {
            let facts = match transcript::locate(
                &self.projects_dir,
                session.cwd.as_deref(),
                &session.session_id,
            ) {
                Some(path) => {
                    let facts = self.transcripts.read(&path);
                    read.insert(path);

                    facts
                }
                None => TranscriptFacts::default(),
            };

            scan.result
                .present
                .push(discovered(session, facts.status.as_ref().copied()));
            scan.transcripts.push((session.session_id.clone(), facts));

            if let Some(identity) = session.cwd.as_deref().and_then(repo::identify) {
                scan.repos.push((session.session_id.clone(), identity));
            }
        }

        self.transcripts.retain(&read);
        scan.windows = self.associate(&live);

        let running: HashSet<&str> = live
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        let filed: HashSet<&str> = live
            .iter()
            .chain(stale.iter())
            .map(|session| session.session_id.as_str())
            .collect();

        for session in &stale {
            // The same session can hold a stale file from an earlier process
            // and a live one from the process it is running under now. A
            // session that is running is never reported gone.
            if running.contains(session.session_id.as_str()) {
                continue;
            }

            if self.seen.remove(&session.session_id).is_some() {
                scan.result.gone.push(session.session_id.clone());
            }
        }

        // A session with no file left at all is only judged by the process it
        // was seen under. A live process means the file was reaped early or
        // the session moved on to a process this scan can see, and either way
        // the session stays; a dead one means it ended without AYDY hearing
        // about it, which is exactly the case a missed `SessionEnd` leaves
        // behind.
        let vanished: Vec<SessionId> = self
            .seen
            .iter()
            .filter(|(session_id, pid)| {
                !filed.contains(session_id.as_str()) && !self.processes.is_alive(**pid)
            })
            .map(|(session_id, _)| session_id.clone())
            .collect();

        for session_id in vanished {
            self.seen.remove(&session_id);
            scan.result.gone.push(session_id);
        }

        for session in live {
            self.seen.insert(session.session_id, session.pid);
        }

        scan
    }

    /// Joins the sessions this pass can see to the windows running them.
    ///
    /// Only live sessions are offered: a dead process has no ancestry left to
    /// read, and a session on its way out of the list has no use for a window.
    fn associate(&self, live: &[SessionFile]) -> WindowMap {
        let Some(extensions) = &self.extensions else {
            return WindowMap::default();
        };

        let windows: Vec<WindowId> = extensions
            .registrations()
            .into_iter()
            .map(|registration| registration.pid)
            .collect();

        window_map::associate(live, &windows, &*self.parents)
    }

    /// Reports the fallback to hook-only tracking, once per spell of it.
    fn degrade(&mut self, reason: &str) {
        if self.degraded {
            return;
        }

        self.degraded = true;
        eprintln!("aydy: {reason}; tracking sessions from hook events only");
    }

    fn recover(&mut self) {
        self.degraded = false;
    }
}

/// The session a live session file describes, as the registry accepts it.
fn discovered(
    session: &SessionFile,
    status: Option<crate::session::status::SessionStatus>,
) -> DiscoveredSession {
    let mut discovered = DiscoveredSession::new(session.session_id.as_str());

    if let Some(cwd) = &session.cwd {
        discovered = discovered.with_cwd(cwd.clone());
    }
    if let Some(entrypoint) = &session.entrypoint {
        discovered = discovered.with_entrypoint(entrypoint.as_str());
    }
    if session.is_jumpable() {
        discovered = discovered.jumpable();
    }
    if let Some(status) = status {
        discovered = discovered.with_status(status);
    }

    discovered
}

/// Runs one scan, applies everything it found to `registry`, and sweeps the
/// sessions whose resolution window has run out.
///
/// The sweep belongs here because the scan is the only thing guaranteed to ask
/// for it: the popover may never be opened, and a removal that waits for the
/// user to look is not a removal within two minutes. It runs after the scan's
/// own findings so a session reported gone by this very pass starts its window
/// rather than being judged against a stale one.
///
/// Blocking: it reads the filesystem and waits on the registry lock, so it runs
/// on the blocking pool rather than on an async worker, where waiting for that
/// lock can starve the runtime's IO and timer driver.
pub fn scan_and_apply(reconciler: &mut Reconciler, registry: &Mutex<SessionRegistry>) {
    let scan = reconciler.scan();
    let enrichment = enrichment(&scan);
    let mut registry = lock_recovering(registry);

    registry.apply_reconciliation(scan.result);
    // After the reconciliation, so a session discovered by this very pass is
    // already in the registry to be placed in its window.
    registry.apply_window_map(&scan.windows);

    for (session_id, fields) in enrichment {
        registry.apply_enrichment(&session_id, fields);
    }

    registry.tick();
}

/// Everything one scan learned about each session it saw, as the registry
/// accepts it.
///
/// The repository comes from the working directory and the title and branch
/// from the transcript, but the row label is composed here because the fallback
/// chain needs all three at once — and because the directory it falls back to
/// is the session's own, not the worktree-aware repository name.
fn enrichment(scan: &Scan) -> Vec<(SessionId, SessionEnrichment)> {
    let repos: HashMap<&str, &RepoIdentity> = scan
        .repos
        .iter()
        .map(|(session_id, identity)| (session_id.as_str(), identity))
        .collect();
    let transcripts: HashMap<&str, &TranscriptFacts> = scan
        .transcripts
        .iter()
        .map(|(session_id, facts)| (session_id.as_str(), facts))
        .collect();

    scan.result
        .present
        .iter()
        .map(|session| {
            let identity = repos.get(session.session_id.as_str());
            let facts = transcripts.get(session.session_id.as_str());
            let title = facts.and_then(|facts| facts.title.as_deref());
            let branch = facts.and_then(|facts| facts.branch.as_deref());

            (
                session.session_id.clone(),
                SessionEnrichment {
                    label: Some(label::label(title, branch, session.cwd.as_deref())),
                    repo: identity.map(|identity| identity.name.clone()),
                    worktree: identity.and_then(|identity| identity.worktree.clone()),
                    branch: branch.map(str::to_string),
                    jumpable: None,
                },
            )
        })
        .collect()
}

/// Scans every [`SCAN_INTERVAL`] until `shutdown` fires.
///
/// The first pass runs immediately, which is what rebuilds the list after the
/// app relaunches into a machine that already has sessions running.
pub async fn run(
    reconciler: Reconciler,
    registry: Arc<Mutex<SessionRegistry>>,
    shutdown: oneshot::Receiver<()>,
) {
    run_every(SCAN_INTERVAL, reconciler, registry, shutdown).await;
}

/// The loop behind [`run`], with the period as a parameter so a test can drive
/// several passes without waiting minutes for them.
///
/// The period is an interval rather than a sleep between passes, so the time a
/// pass takes comes out of it instead of being added to it — the detection
/// budget is measured from one scan to the next, not from the end of one to the
/// start of another. A pass that overruns delays the next one by a full period
/// rather than firing a burst of catch-up scans, which would only queue more
/// work behind a machine that is already busy.
async fn run_every(
    period: Duration,
    reconciler: Reconciler,
    registry: Arc<Mutex<SessionRegistry>>,
    mut shutdown: oneshot::Receiver<()>,
) {
    // Held here rather than moved into the pass, so a pass that panics costs
    // that pass and nothing else. Nothing in this module is allowed to be
    // fatal, and self-healing a single panic disables for the lifetime of the
    // process is not self-healing.
    let reconciler = Arc::new(Mutex::new(reconciler));
    let mut passes = tokio::time::interval(period);
    passes.set_missed_tick_behavior(MissedTickBehavior::Delay);

    // The first tick is immediate; the pass below is that tick.
    passes.tick().await;

    loop {
        let scanning = tokio::task::spawn_blocking({
            let reconciler = Arc::clone(&reconciler);
            let registry = Arc::clone(&registry);

            move || scan_and_apply(&mut lock_recovering(&reconciler), &registry)
        });

        if let Err(error) = scanning.await {
            eprintln!("aydy: a session scan did not finish: {error}; the next one runs as usual");
        }

        tokio::select! {
            _ = passes.tick() => {}
            _ = &mut shutdown => return,
        }
    }
}

/// The stop signal for the running scan.
pub struct ScanShutdown {
    signal: Mutex<Option<oneshot::Sender<()>>>,
}

impl ScanShutdown {
    /// A trigger that fires `signal`.
    pub fn new(signal: oneshot::Sender<()>) -> Self {
        Self {
            signal: Mutex::new(Some(signal)),
        }
    }

    /// Asks the scan to stop. Firing twice does nothing.
    pub fn trigger(&self) {
        if let Some(signal) = lock_recovering(&self.signal).take() {
            let _ = signal.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::clock::TestClock;
    use crate::discovery::process_tree::FixedParents;
    use crate::discovery::session_files::FixedProcesses;
    use crate::discovery::test_support::{
        checkout, projects_dir, scratch_dir, session_file, sessions_dir, worktree,
        COMPLETED_TRANSCRIPT, NEVER_PROMPTED_TRANSCRIPT, WORKING_TRANSCRIPT,
    };
    use crate::extension::registry::Registration;
    use crate::session::event::{HookEvent, HookEventKind};
    use crate::session::status::{SessionStatus, UNKNOWN_RESOLUTION_WINDOW};

    /// The ceiling the whole detection chain has to stay inside.
    const DETECTION_BOUND: Duration = Duration::from_secs(120);

    /// The floor reconciliation itself has to meet.
    const RECONCILIATION_FLOOR: Duration = Duration::from_secs(60);

    /// A scan whose sessions have no transcripts at all, which is what most of
    /// these tests are about: what the session files and the process table say
    /// on their own.
    fn reconciler(dir: impl Into<PathBuf>, alive: impl IntoIterator<Item = u32>) -> Reconciler {
        Reconciler::new(
            dir,
            scratch_dir("no-projects"),
            Arc::new(FixedProcesses::new(alive)),
        )
    }

    fn reconciler_reading(
        dir: impl Into<PathBuf>,
        projects: impl Into<PathBuf>,
        alive: impl IntoIterator<Item = u32>,
    ) -> Reconciler {
        Reconciler::new(dir, projects, Arc::new(FixedProcesses::new(alive)))
    }

    fn registry(clock: &Arc<TestClock>) -> Mutex<SessionRegistry> {
        Mutex::new(SessionRegistry::new(clock.clone()))
    }

    fn listed(registry: &Mutex<SessionRegistry>, session_id: &str) -> bool {
        lock_recovering(registry)
            .snapshot()
            .groups
            .iter()
            .flat_map(|group| &group.sessions)
            .any(|session| session.id == session_id)
    }

    /// Whether the registry still holds the session, asked without taking a
    /// snapshot — a snapshot sweeps expired sessions itself, so asking through
    /// one cannot tell whether anything else ever does.
    fn tracked(registry: &Mutex<SessionRegistry>, session_id: &str) -> bool {
        lock_recovering(registry).session(session_id).is_some()
    }

    /// A process table that takes its time and counts how often it was asked,
    /// so a test can watch what a slow pass does to the period between passes.
    struct SlowProcesses {
        delay: Duration,
        passes: Arc<AtomicUsize>,
        processes: FixedProcesses,
    }

    impl SlowProcesses {
        fn new(delay: Duration, alive: impl IntoIterator<Item = u32>) -> Self {
            Self {
                delay,
                passes: Arc::new(AtomicUsize::new(0)),
                processes: FixedProcesses::new(alive),
            }
        }
    }

    impl ProcessTable for SlowProcesses {
        fn is_alive(&self, pid: u32) -> bool {
            std::thread::sleep(self.delay);
            self.passes.fetch_add(1, Ordering::Relaxed);

            self.processes.is_alive(pid)
        }
    }

    /// A process table that panics the first time it is asked, so one pass
    /// fails the way a real one would.
    struct PanicsOnce {
        used: AtomicBool,
        processes: FixedProcesses,
    }

    impl PanicsOnce {
        fn new(alive: impl IntoIterator<Item = u32>) -> Self {
            Self {
                used: AtomicBool::new(false),
                processes: FixedProcesses::new(alive),
            }
        }
    }

    impl ProcessTable for PanicsOnce {
        fn is_alive(&self, pid: u32) -> bool {
            if !self.used.swap(true, Ordering::Relaxed) {
                panic!("the process table failed");
            }

            self.processes.is_alive(pid)
        }
    }

    #[test]
    fn the_scan_interval_and_the_unknown_window_stay_inside_the_two_minute_bound() {
        // The whole chain, not the two headline numbers: a session that stops
        // immediately after a scan waits a full interval for the scan that
        // reports it gone, and then waits out the resolution window — which
        // only whole scans can serve, since nothing else sweeps the registry.
        let sweeps = UNKNOWN_RESOLUTION_WINDOW
            .as_secs()
            .div_ceil(SCAN_INTERVAL.as_secs());
        let sweeps = u32::try_from(sweeps).expect("a resolution window of a sane number of scans");
        let worst_case = SCAN_INTERVAL + SCAN_INTERVAL * sweeps;

        assert!(
            SCAN_INTERVAL <= RECONCILIATION_FLOOR,
            "the scan runs every {SCAN_INTERVAL:?}, less often than once a minute"
        );
        assert!(
            worst_case < DETECTION_BOUND,
            "a stopped session takes {worst_case:?} to leave the list at worst"
        );
    }

    #[test]
    fn a_session_whose_process_dies_leaves_the_list_inside_the_two_minute_bound() {
        let cwd = scratch_dir("bound");
        let dir = sessions_dir(
            "bound",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = reconciler(&dir, [11]);

        scan_and_apply(&mut reconciler, &registry);
        // A session in the state the bound is about: reported on by its hooks,
        // so it is resolved rather than already counting down an Unknown window
        // it was born with.
        lock_recovering(&registry)
            .apply_hook_event(HookEvent::new("session-a", HookEventKind::Stop));

        assert!(tracked(&registry, "session-a"));

        // The worst case: the process dies the instant after a scan, so the
        // whole of the next interval passes before anything can notice.
        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([])),
            ..reconciler
        };
        let mut elapsed = Duration::ZERO;

        while tracked(&registry, "session-a") {
            clock.advance(SCAN_INTERVAL);
            elapsed += SCAN_INTERVAL;

            assert!(
                elapsed <= DETECTION_BOUND,
                "the session was still listed {elapsed:?} after its process died"
            );

            scan_and_apply(&mut reconciler, &registry);
        }

        assert_eq!(elapsed, SCAN_INTERVAL + UNKNOWN_RESOLUTION_WINDOW);
    }

    #[test]
    fn a_session_whose_process_is_alive_is_reported_present() {
        let cwd = scratch_dir("present");
        let dir = sessions_dir(
            "present",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );

        let scan = reconciler(&dir, [11]).scan();

        assert_eq!(scan.result.present.len(), 1);
        assert_eq!(scan.result.present[0].session_id, "session-a");
        assert_eq!(scan.result.present[0].cwd, Some(cwd));
        assert_eq!(
            scan.result.present[0].entrypoint,
            Some("claude-vscode".to_string())
        );
        assert!(scan.result.present[0].jumpable);
    }

    #[test]
    fn a_file_whose_process_is_not_alive_is_ignored() {
        let cwd = scratch_dir("stale");
        let dir = sessions_dir(
            "stale",
            &[
                (11, &session_file(11, "session-a", &cwd, "claude-vscode")),
                (22, &session_file(22, "session-b", &cwd, "claude-vscode")),
            ],
        );

        let scan = reconciler(&dir, [11]).scan();

        assert_eq!(scan.result.present.len(), 1);
        assert_eq!(scan.result.present[0].session_id, "session-a");
        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_file_the_scan_cannot_recognise_is_skipped_and_the_rest_of_the_scan_stands() {
        let cwd = scratch_dir("malformed");
        let dir = sessions_dir(
            "malformed",
            &[
                (11, "{ half a file"),
                (22, r#"{ "peerProtocol": 9, "shape": "unrecognised" }"#),
                (33, &session_file(33, "session-c", &cwd, "claude-vscode")),
            ],
        );

        let scan = reconciler(&dir, [11, 22, 33]).scan();

        assert_eq!(scan.result.present.len(), 1);
        assert_eq!(scan.result.present[0].session_id, "session-c");
    }

    #[test]
    fn a_directory_of_files_none_of_which_are_recognised_degrades_to_hook_only_tracking() {
        let dir = sessions_dir(
            "all-malformed",
            &[(11, "{ half a file"), (22, "also not a session file")],
        );
        let mut reconciler = reconciler(&dir, [11, 22]);

        let scan = reconciler.scan();

        assert_eq!(scan, Scan::default());
        assert!(reconciler.is_degraded());
    }

    #[test]
    fn a_missing_sessions_directory_degrades_to_hook_only_tracking() {
        let dir = scratch_dir("missing").join("sessions");
        let mut reconciler = reconciler(&dir, [11]);

        let scan = reconciler.scan();

        assert_eq!(scan, Scan::default());
        assert!(reconciler.is_degraded());
    }

    #[test]
    fn a_sessions_directory_that_comes_back_ends_the_degraded_spell() {
        let dir = scratch_dir("returns").join("sessions");
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::create_dir_all(&dir).expect("the sessions directory to be creatable");
        let scan = reconciler.scan();

        assert!(!reconciler.is_degraded());
        assert_eq!(scan, Scan::default());
    }

    #[test]
    fn an_empty_sessions_directory_is_not_a_degraded_scan() {
        let dir = sessions_dir("empty", &[]);
        let mut reconciler = reconciler(&dir, []);

        let scan = reconciler.scan();

        assert_eq!(scan, Scan::default());
        assert!(!reconciler.is_degraded());
    }

    #[test]
    fn a_sessions_directory_that_disappears_reports_nothing_gone() {
        let cwd = scratch_dir("vanishing");
        let dir = sessions_dir(
            "vanishing",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::remove_dir_all(&dir).expect("the sessions directory to be removable");
        let scan = reconciler.scan();

        assert!(scan.result.gone.is_empty());
        assert!(reconciler.is_degraded());
    }

    #[test]
    fn a_directory_whose_files_stop_being_recognised_reports_nothing_gone() {
        let cwd = scratch_dir("shape-changed");
        let dir = sessions_dir(
            "shape-changed",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::write(dir.join("22.json"), "{ not a session file }")
            .expect("the unrecognised file to be writable");
        std::fs::remove_file(dir.join("11.json")).expect("the session file to be removable");
        let scan = reconciler.scan();

        assert!(scan.result.gone.is_empty());
        assert!(reconciler.is_degraded());
    }

    #[test]
    fn a_hook_only_session_the_scan_cannot_see_is_never_reported_gone() {
        let dir = sessions_dir("unseen", &[]);

        let scan = reconciler(&dir, []).scan();

        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_session_seen_before_whose_process_has_died_is_reported_gone() {
        let cwd = scratch_dir("died");
        let dir = sessions_dir(
            "died",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([])),
            ..reconciler
        };
        let scan = reconciler.scan();

        assert!(scan.result.present.is_empty());
        assert_eq!(scan.result.gone, vec!["session-a".to_string()]);
    }

    #[test]
    fn a_session_seen_before_whose_file_is_reaped_with_its_process_dead_is_reported_gone() {
        let cwd = scratch_dir("reaped");
        let dir = sessions_dir(
            "reaped",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        // The session ends without AYDY hearing its `SessionEnd`, and the file
        // goes with it — which is what this machine does.
        std::fs::remove_file(dir.join("11.json")).expect("the session file to be removable");
        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([])),
            ..reconciler
        };
        let scan = reconciler.scan();

        assert_eq!(scan.result.gone, vec!["session-a".to_string()]);
    }

    #[test]
    fn a_session_whose_file_is_reaped_while_its_process_lives_is_not_reported_gone() {
        let cwd = scratch_dir("reaped-early");
        let dir = sessions_dir(
            "reaped-early",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::remove_file(dir.join("11.json")).expect("the session file to be removable");
        let scan = reconciler.scan();

        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_session_whose_file_is_reaped_is_reported_gone_only_once() {
        let cwd = scratch_dir("reaped-once");
        let dir = sessions_dir(
            "reaped-once",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::remove_file(dir.join("11.json")).expect("the session file to be removable");
        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([])),
            ..reconciler
        };
        reconciler.scan();
        let scan = reconciler.scan();

        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_session_whose_file_is_reaped_while_it_runs_under_a_new_process_is_not_reported_gone() {
        let cwd = scratch_dir("reaped-restarted");
        let dir = sessions_dir(
            "reaped-restarted",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::remove_file(dir.join("11.json")).expect("the session file to be removable");
        std::fs::write(
            dir.join("22.json"),
            session_file(22, "session-a", &cwd, "claude-vscode"),
        )
        .expect("the new session file to be writable");
        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([22])),
            ..reconciler
        };
        let scan = reconciler.scan();

        assert_eq!(scan.result.present.len(), 1);
        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_stale_file_for_a_session_never_seen_is_not_reported_gone() {
        let cwd = scratch_dir("never-seen");
        let dir = sessions_dir(
            "never-seen",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );

        let scan = reconciler(&dir, []).scan();

        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_session_running_under_a_new_process_is_not_reported_gone_by_its_old_file() {
        let cwd = scratch_dir("restarted");
        let dir = sessions_dir(
            "restarted",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        std::fs::write(
            dir.join("22.json"),
            session_file(22, "session-a", &cwd, "claude-vscode"),
        )
        .expect("the new session file to be writable");
        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([22])),
            ..reconciler
        };
        let scan = reconciler.scan();

        assert_eq!(scan.result.present.len(), 1);
        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_session_reported_gone_once_is_not_reported_gone_again() {
        let cwd = scratch_dir("gone-once");
        let dir = sessions_dir(
            "gone-once",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let mut reconciler = reconciler(&dir, [11]);
        reconciler.scan();

        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([])),
            ..reconciler
        };
        reconciler.scan();
        let scan = reconciler.scan();

        assert!(scan.result.gone.is_empty());
    }

    #[test]
    fn a_session_in_a_linked_worktree_takes_the_repositorys_name() {
        let dir = scratch_dir("worktree-scan");
        let root = checkout(&dir, "okven");
        let cwd = worktree(&dir.join("okven.worktrees"), &root, "eric-attribution-2");
        let sessions = sessions_dir(
            "worktree-scan",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );

        let scan = reconciler(&sessions, [11]).scan();

        assert_eq!(scan.repos.len(), 1);
        assert_eq!(scan.repos[0].0, "session-a");
        assert_eq!(scan.repos[0].1.name, "okven");
        assert_eq!(
            scan.repos[0].1.worktree,
            Some("eric-attribution-2".to_string())
        );
    }

    #[test]
    fn a_session_outside_any_repository_reports_no_repository() {
        let cwd = scratch_dir("no-repo-scan");
        let dir = sessions_dir(
            "no-repo-scan",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );

        let scan = reconciler(&dir, [11]).scan();

        assert!(scan.repos.is_empty());
    }

    #[test]
    fn a_session_from_another_client_is_discovered_but_not_jumpable() {
        let cwd = scratch_dir("other-client-scan");
        let dir = sessions_dir(
            "other-client-scan",
            &[(11, &session_file(11, "session-a", &cwd, "claude-desktop"))],
        );

        let scan = reconciler(&dir, [11]).scan();

        assert_eq!(scan.result.present.len(), 1);
        assert!(!scan.result.present[0].jumpable);
    }

    #[test]
    fn a_session_whose_directory_is_not_on_this_machine_is_not_jumpable() {
        let cwd = scratch_dir("remote-scan").join("elsewhere");
        let dir = sessions_dir(
            "remote-scan",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );

        let scan = reconciler(&dir, [11]).scan();

        assert_eq!(scan.result.present.len(), 1);
        assert!(!scan.result.present[0].jumpable);
    }

    /// A registry holding one window, registered under `pid`.
    fn extension_registry(clock: &Arc<TestClock>, pid: WindowId) -> Arc<ExtensionRegistry> {
        let registry = Arc::new(ExtensionRegistry::new(clock.clone()));
        registry.register(Registration {
            pid,
            callback_port: 55_123,
            workspace_folders: vec![PathBuf::from("/Users/eric/Code/aydy")],
            claude_code_version: Some("2.1.220".to_string()),
        });

        registry
    }

    #[test]
    fn a_scan_places_each_session_it_sees_in_the_window_running_it() {
        let dir = scratch_dir("window-map");
        let cwd = checkout(&dir, "aydy");
        let sessions = sessions_dir(
            "window-map",
            &[
                (11, &session_file(11, "session-a", &cwd, "claude-vscode")),
                (12, &session_file(12, "session-b", &cwd, "claude-vscode")),
            ],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = reconciler(&sessions, [11, 12]).with_windows(
            extension_registry(&clock, 65_921),
            Arc::new(FixedParents::new([(11, 65_921), (12, 400)])),
        );

        scan_and_apply(&mut reconciler, &registry);

        let registry = lock_recovering(&registry);
        assert_eq!(registry.window_of("session-a"), Some(65_921));
        assert_eq!(registry.window_of("session-b"), None);
    }

    /// Without the extension the scan places nothing, which is the state every
    /// machine that has never installed it runs in.
    #[test]
    fn a_scan_with_no_extension_involved_places_no_session_in_a_window() {
        let dir = scratch_dir("no-window-map");
        let cwd = checkout(&dir, "aydy");
        let sessions = sessions_dir(
            "no-window-map",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);

        scan_and_apply(&mut reconciler(&sessions, [11]), &registry);

        assert_eq!(lock_recovering(&registry).window_of("session-a"), None);
    }

    #[test]
    fn a_scan_puts_the_repository_name_on_the_session_it_discovers() {
        let dir = scratch_dir("enrichment");
        let root = checkout(&dir, "okven");
        let cwd = worktree(&dir.join("okven.worktrees"), &root, "eric-attribution-2");
        let sessions = sessions_dir(
            "enrichment",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);

        scan_and_apply(&mut reconciler(&sessions, [11]), &registry);

        let registry = lock_recovering(&registry);
        let session = registry
            .session("session-a")
            .expect("the discovered session to be tracked");

        assert_eq!(session.repo.as_deref(), Some("okven"));
        assert_eq!(session.worktree.as_deref(), Some("eric-attribution-2"));
    }

    #[test]
    fn a_session_the_scan_cannot_verify_stays_in_the_list() {
        let dir = sessions_dir("unverifiable", &[]);
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        lock_recovering(&registry).apply_hook_event(
            HookEvent::new("desktop-session", HookEventKind::SessionStart)
                .with_cwd("/not/on/this/machine"),
        );

        for _ in 0..8 {
            scan_and_apply(&mut reconciler(&dir, []), &registry);
            clock.advance(SCAN_INTERVAL);
        }

        assert!(listed(&registry, "desktop-session"));
        assert_eq!(
            lock_recovering(&registry)
                .session("desktop-session")
                .map(|session| session.status()),
            Some(SessionStatus::Done)
        );
    }

    #[test]
    fn a_session_the_scan_has_seen_leaves_the_list_once_its_process_dies() {
        let cwd = scratch_dir("removed");
        let dir = sessions_dir(
            "removed",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = reconciler(&dir, [11]);

        scan_and_apply(&mut reconciler, &registry);
        lock_recovering(&registry)
            .apply_hook_event(HookEvent::new("session-a", HookEventKind::Stop));

        assert!(listed(&registry, "session-a"));

        clock.advance(SCAN_INTERVAL);
        let mut reconciler = Reconciler {
            processes: Arc::new(FixedProcesses::new([])),
            ..reconciler
        };
        scan_and_apply(&mut reconciler, &registry);

        assert_eq!(
            lock_recovering(&registry)
                .session("session-a")
                .map(|session| session.status()),
            Some(SessionStatus::Unknown)
        );

        // The scan is what sweeps it out. Nothing else in the app is guaranteed
        // to ask, so this is asserted without taking a snapshot.
        clock.advance(UNKNOWN_RESOLUTION_WINDOW);
        scan_and_apply(&mut reconciler, &registry);

        assert!(!tracked(&registry, "session-a"));
    }

    /// A scan over one live session in `cwd`, whose transcript holds
    /// `transcript`.
    fn scan_of(name: &str, cwd: &std::path::Path, transcript: &str) -> Reconciler {
        let sessions = sessions_dir(
            name,
            &[(11, &session_file(11, "session-a", cwd, "claude-vscode"))],
        );
        let projects = projects_dir(name, &[(cwd, "session-a", transcript)]);

        reconciler_reading(sessions, projects, [11])
    }

    #[test]
    fn a_session_whose_transcript_ends_in_a_completed_turn_is_discovered_as_done() {
        let cwd = scratch_dir("derived-done");
        let scan = scan_of("derived-done", &cwd, COMPLETED_TRANSCRIPT).scan();

        assert_eq!(scan.result.present[0].status, Some(SessionStatus::Done));
    }

    #[test]
    fn a_session_whose_transcript_ends_in_an_open_turn_is_discovered_as_working() {
        let cwd = scratch_dir("derived-working");
        let scan = scan_of("derived-working", &cwd, WORKING_TRANSCRIPT).scan();

        assert_eq!(scan.result.present[0].status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_session_with_no_transcript_is_discovered_with_no_status() {
        let cwd = scratch_dir("derived-none");
        let sessions = sessions_dir(
            "derived-none",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );

        let scan = reconciler_reading(sessions, scratch_dir("derived-none-projects"), [11]).scan();

        assert_eq!(scan.result.present[0].status, None);
    }

    #[test]
    fn a_scan_puts_the_title_and_the_branch_on_the_session_it_discovers() {
        let cwd = scratch_dir("titled");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);

        scan_and_apply(
            &mut scan_of("titled", &cwd, COMPLETED_TRANSCRIPT),
            &registry,
        );

        let registry = lock_recovering(&registry);
        let session = registry
            .session("session-a")
            .expect("the discovered session to be tracked");

        assert_eq!(
            session.label.as_deref(),
            Some("Review attribution mock data test file")
        );
        assert_eq!(session.branch.as_deref(), Some("eric-attribution"));
    }

    #[test]
    fn a_session_with_no_title_takes_its_branch_as_its_label() {
        let cwd = scratch_dir("untitled");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);

        scan_and_apply(
            &mut scan_of("untitled", &cwd, WORKING_TRANSCRIPT),
            &registry,
        );

        assert_eq!(
            lock_recovering(&registry)
                .session("session-a")
                .and_then(|session| session.label.clone())
                .as_deref(),
            Some("main")
        );
    }

    #[test]
    fn a_session_with_no_transcript_at_all_takes_its_directory_as_its_label() {
        let cwd = scratch_dir("unlabelled");
        let sessions = sessions_dir(
            "unlabelled",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);

        scan_and_apply(
            &mut reconciler_reading(sessions, scratch_dir("unlabelled-projects"), [11]),
            &registry,
        );

        let expected = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the scratch directory to have a name");

        assert_eq!(
            lock_recovering(&registry)
                .session("session-a")
                .and_then(|session| session.label.clone())
                .as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn a_session_sitting_at_done_survives_a_restart_without_flapping() {
        // The regression the derivation exists for: after a restart nothing but
        // the transcript knows what the session was doing, and a session that
        // falls to Unknown is evicted at the window and re-added by the next
        // scan, for as long as it lives.
        let cwd = scratch_dir("restart");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = scan_of("restart", &cwd, COMPLETED_TRANSCRIPT);

        for _ in 0..8 {
            scan_and_apply(&mut reconciler, &registry);

            assert_eq!(
                lock_recovering(&registry)
                    .session("session-a")
                    .map(|session| session.status()),
                Some(SessionStatus::Done),
                "the session left the list or lost its status"
            );

            clock.advance(SCAN_INTERVAL);
        }
    }

    #[test]
    fn a_session_that_has_never_been_prompted_survives_past_two_minutes() {
        let cwd = scratch_dir("never-prompted");
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = scan_of("never-prompted", &cwd, NEVER_PROMPTED_TRANSCRIPT);

        scan_and_apply(&mut reconciler, &registry);
        clock.advance(Duration::from_secs(150));
        scan_and_apply(&mut reconciler, &registry);

        assert_eq!(
            lock_recovering(&registry)
                .session("session-a")
                .map(|session| session.status()),
            Some(SessionStatus::Done)
        );
    }

    #[test]
    fn a_session_whose_transcript_cannot_be_read_settles_rather_than_flapping() {
        // Its file is on disk and its process is alive, and only its transcript
        // is silent. The scan positively confirms it exists on every pass, so
        // it must not be evicted and re-added forever.
        let cwd = scratch_dir("unreadable");
        let sessions = sessions_dir(
            "unreadable",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = reconciler_reading(sessions, scratch_dir("unreadable-projects"), [11]);

        for _ in 0..12 {
            scan_and_apply(&mut reconciler, &registry);

            assert!(
                tracked(&registry, "session-a"),
                "a session the scan can see was removed from the list"
            );

            clock.advance(SCAN_INTERVAL);
        }

        assert_eq!(
            lock_recovering(&registry)
                .session("session-a")
                .map(|session| session.status()),
            Some(SessionStatus::Done)
        );
    }

    #[test]
    fn a_title_that_appears_mid_session_reaches_the_row() {
        let cwd = scratch_dir("retitled");
        let sessions = sessions_dir(
            "retitled",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let projects = projects_dir("retitled", &[(&cwd, "session-a", WORKING_TRANSCRIPT)]);
        let clock = Arc::new(TestClock::new());
        let registry = registry(&clock);
        let mut reconciler = reconciler_reading(sessions, &projects, [11]);
        scan_and_apply(&mut reconciler, &registry);

        let transcript = projects
            .join(transcript::project_slug(&cwd))
            .join("session-a.jsonl");
        let mut grown =
            std::fs::read_to_string(&transcript).expect("the transcript to be readable");
        grown.push_str(r#"{"type":"ai-title","aiTitle":"The title it settled on"}"#);
        grown.push('\n');
        std::fs::write(&transcript, grown).expect("the transcript to be writable");

        clock.advance(SCAN_INTERVAL);
        scan_and_apply(&mut reconciler, &registry);

        assert_eq!(
            lock_recovering(&registry)
                .session("session-a")
                .and_then(|session| session.label.clone())
                .as_deref(),
            Some("The title it settled on")
        );
    }

    #[tokio::test]
    async fn the_running_scan_reconciles_once_before_it_waits_and_stops_when_asked() {
        let cwd = scratch_dir("running");
        let dir = sessions_dir(
            "running",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = Arc::new(Mutex::new(SessionRegistry::new(clock.clone())));
        let (signal, shutdown) = oneshot::channel();
        let scan = tokio::spawn(run(reconciler(&dir, [11]), Arc::clone(&registry), shutdown));

        ScanShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), scan)
            .await
            .expect("the scan to stop when asked")
            .expect("the scan not to panic");

        assert!(listed(&registry, "session-a"));
    }

    /// Runs on a multi-threaded runtime with a single worker, as a busy machine
    /// effectively is: one blocking wait on the wrong thread is enough to stop
    /// every timer and every socket the app has.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_scan_waiting_for_the_registry_does_not_block_the_runtime() {
        let cwd = scratch_dir("starvation");
        let dir = sessions_dir(
            "starvation",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = Arc::new(Mutex::new(SessionRegistry::new(clock.clone())));
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
        let scan = tokio::spawn(run(reconciler(&dir, [11]), Arc::clone(&registry), shutdown));

        let started = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let waited = started.elapsed();

        let _ = release.send(());
        holder.join().expect("the holding thread to finish");
        ScanShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), scan)
            .await
            .expect("the scan to stop when asked")
            .expect("the scan not to panic");

        assert!(
            waited < Duration::from_secs(1),
            "a fifty-millisecond timer took {waited:?} while a scan waited for the registry"
        );
    }

    #[tokio::test]
    async fn the_time_a_pass_takes_comes_out_of_the_period_rather_than_adding_to_it() {
        const PERIOD: Duration = Duration::from_millis(200);
        const PASS: Duration = Duration::from_millis(160);
        const PASSES: usize = 4;

        let cwd = scratch_dir("period");
        let dir = sessions_dir(
            "period",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = Arc::new(Mutex::new(SessionRegistry::new(clock.clone())));
        let processes = Arc::new(SlowProcesses::new(PASS, [11]));
        let counted = Arc::clone(&processes.passes);
        let (signal, shutdown) = oneshot::channel();
        let started = std::time::Instant::now();
        let scan = tokio::spawn(run_every(
            PERIOD,
            Reconciler::new(&dir, scratch_dir("no-projects"), processes),
            Arc::clone(&registry),
            shutdown,
        ));

        while counted.load(Ordering::Relaxed) < PASSES {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let took = started.elapsed();

        ScanShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), scan)
            .await
            .expect("the scan to stop when asked")
            .expect("the scan not to panic");

        // Four passes are three periods plus the last pass. Sleeping between
        // passes instead would make it three periods plus four passes, which
        // is what puts the detection budget over its ceiling.
        let bound = PERIOD * 3 + PASS + PASS * 3 / 2;

        assert!(
            took < bound,
            "{PASSES} passes {PERIOD:?} apart took {took:?}, so the pass itself is being added to the period"
        );
    }

    #[tokio::test]
    async fn a_pass_that_panics_does_not_stop_the_passes_after_it() {
        let cwd = scratch_dir("panicking");
        let dir = sessions_dir(
            "panicking",
            &[(11, &session_file(11, "session-a", &cwd, "claude-vscode"))],
        );
        let clock = Arc::new(TestClock::new());
        let registry = Arc::new(Mutex::new(SessionRegistry::new(clock.clone())));
        let (signal, shutdown) = oneshot::channel();
        let scan = tokio::spawn(run_every(
            Duration::from_millis(10),
            Reconciler::new(
                &dir,
                scratch_dir("no-projects"),
                Arc::new(PanicsOnce::new([11])),
            ),
            Arc::clone(&registry),
            shutdown,
        ));

        let mut healed = false;
        for _ in 0..200 {
            if tracked(&registry, "session-a") {
                healed = true;
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        ScanShutdown::new(signal).trigger();
        tokio::time::timeout(Duration::from_secs(5), scan)
            .await
            .expect("the scan to stop when asked")
            .expect("the scan not to panic");

        assert!(
            healed,
            "no pass after the panicking one ever reached the registry"
        );
    }
}
