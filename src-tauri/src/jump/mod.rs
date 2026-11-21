//! Taking the user to a session: the tab first, then the window it is in.
//!
//! A jump does not address any one window. It writes a request file into the
//! jump directory, and every VS Code window watching that directory decides for
//! itself whether it owns the session. The owner claims the request by
//! unlinking it, reveals the tab, and writes a reply naming the folder to
//! raise. The app then raises that folder with `code`.
//!
//! - **Only the owner answers.** Asking Claude Code to open a session in a
//!   window that is not running it opens a second, live copy of that session,
//!   so the decision belongs to the windows, which know their own folders.
//! - **The raise is gated on a reply.** `code <folder>` opens a new window when
//!   none holds that folder, so it only ever runs on a folder a window named.
//! - **Nothing is believed without a reply.** A request nobody claims within
//!   the timeout is withdrawn and reported as unanswered.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::atomic_file::write_atomically;
use crate::discovery::session_files::{self, ProcessTable};
use crate::session::session::Session;
use crate::session::tab_match::{tab_label, PLACEHOLDER_TAB_LABEL};

/// How long a jump waits for a window to answer its request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

/// How long a jump keeps waiting after a window claimed its request but had
/// not replied by the timeout.
const REPLY_GRACE: Duration = Duration::from_secs(1);

/// How often the jump directory is looked at for a reply.
const REPLY_POLL: Duration = Duration::from_millis(50);

/// The suffix of a reply file, `<id>.reply.json`.
const REPLY_SUFFIX: &str = ".reply.json";

/// Where the `code` command line lives, in the order it is looked for.
///
/// **Absolute paths, never the bare name.** A GUI app launched by launchd —
/// which is what launch-at-login makes the normal case — inherits launchd's
/// `PATH`, and that does not include `/usr/local/bin`. `Command::new("code")`
/// therefore resolves under `yarn tauri dev` from a terminal and fails in the
/// shipped app.
const CODE_PROGRAMS: [&str; 3] = [
    "/usr/local/bin/code",
    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
    "/opt/homebrew/bin/code",
];

/// What came of a click on a session row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JumpOutcome {
    /// The owning window answered and was raised.
    Jumped,
    /// There is nothing to raise a window with, or nothing to find the owner
    /// by, so no request was written. The click marks the session read and
    /// goes nowhere, which is what a row that cannot be jumped to has always
    /// done.
    NoWindow,
    /// No window answered the request, or the one that did could not be
    /// brought forward.
    Unanswered,
    /// A tab opened when the reveal was called, so a second live copy of the
    /// session may now exist.
    Duplicated,
}

/// Everything a jump needs to know about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTarget {
    pub session_id: String,
    /// What the session's tab is expected to read, cut the way the editor cuts
    /// it. `None` when AYDY holds nothing that could be on a tab.
    pub tab_label: Option<String>,
    pub cwd: Option<PathBuf>,
}

impl JumpTarget {
    /// What jumping to `session` would have to do, or `None` when the session
    /// is not one that can be jumped to at all.
    pub fn of(session: &Session) -> Option<Self> {
        if !session.jumpable {
            return None;
        }

        Some(Self {
            session_id: session.id.clone(),
            // The placeholder is what *every* unprompted session shows, in
            // every window, so it identifies nothing — and the window matches
            // this label against its own tabs before it calls Claude Code.
            // Sending it would turn that guard into a coin toss.
            tab_label: session
                .label
                .as_deref()
                .map(tab_label)
                .filter(|label| label != PLACEHOLDER_TAB_LABEL),
            cwd: session.cwd.clone(),
        })
    }

    /// The same target, with `cwd` replaced by the session's launch directory
    /// from its live session file in `sessions_dir`.
    ///
    /// The registry's `cwd` follows every hook event, so a session that moved
    /// from `/a` into `/a/.claude/worktrees/x` would let the worktree's window
    /// claim a session whose tab lives in the `/a` window. Windows decide
    /// ownership by the launch directory, so the request has to carry it. When
    /// no live file names one, the registry's `cwd` stays as the fallback.
    pub fn with_launch_cwd(
        self,
        sessions_dir: Option<&Path>,
        processes: &dyn ProcessTable,
    ) -> Self {
        let Some(files) = sessions_dir.and_then(|dir| session_files::read_all(dir).ok()) else {
            return self;
        };

        let launch = files.sessions.into_iter().find_map(|file| {
            (file.session_id == self.session_id && processes.is_alive(file.pid))
                .then_some(file.cwd)
                .flatten()
        });

        match launch {
            Some(cwd) => Self {
                cwd: Some(cwd),
                ..self
            },
            None => self,
        }
    }
}

/// Bringing an editor window to the front.
pub trait WindowRaiser: Send + Sync {
    /// Whether there is anything to raise a window with.
    ///
    /// Asked before anything is attempted: with no `code` to run there is no
    /// raise, and a jump that cannot end with the window in front should reveal
    /// nothing and report that it went nowhere.
    fn available(&self) -> bool;

    /// Brings the window holding `folder` to the front.
    fn raise(&self, folder: &Path) -> Result<(), String>;
}

/// The `code` command line, resolved by absolute path.
pub struct CodeCli {
    programs: Vec<PathBuf>,
}

impl CodeCli {
    /// A raiser that looks for `code` in `programs`, in order.
    pub fn new(programs: Vec<PathBuf>) -> Self {
        Self { programs }
    }

    /// The `code` installed on this machine, or `None` when there is none.
    fn program(&self) -> Option<&Path> {
        self.programs
            .iter()
            .map(PathBuf::as_path)
            .find(|program| program.exists())
    }
}

impl Default for CodeCli {
    fn default() -> Self {
        Self::new(CODE_PROGRAMS.iter().map(PathBuf::from).collect())
    }
}

impl WindowRaiser for CodeCli {
    fn available(&self) -> bool {
        self.program().is_some()
    }

    fn raise(&self, folder: &Path) -> Result<(), String> {
        let Some(program) = self.program() else {
            return Err("no code command line is installed".to_string());
        };

        // The status is waited for rather than the child abandoned: `code`
        // hands the folder to the running editor and exits, and its exit code
        // is the only thing that distinguishes a raise from a command that
        // never ran.
        match Command::new(program)
            .arg(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("code answered {status}")),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Distinguishes requests written by this process in the same millisecond.
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The jump directory, and how long a jump waits on it.
pub struct JumpRequests {
    dir: PathBuf,
    timeout: Duration,
    grace: Duration,
    poll: Duration,
}

impl JumpRequests {
    /// Requests written into `dir`, with the production timings.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            timeout: REPLY_TIMEOUT,
            grace: REPLY_GRACE,
            poll: REPLY_POLL,
        }
    }

    /// The longest the app ever waits for a reply. A window skips replying to
    /// a request older than this, and a reply older than this is swept.
    pub fn budget(&self) -> Duration {
        self.timeout + self.grace
    }

    /// Deletes every reply older than the budget: nobody is waiting for it.
    fn sweep(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };

        for entry in entries.flatten() {
            let is_reply = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(REPLY_SUFFIX));

            if !is_reply {
                continue;
            }

            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > self.budget());

            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Polls for `reply` until `wait` has passed.
    fn wait_for(&self, reply: &Path, wait: Duration) -> Option<String> {
        let deadline = Instant::now() + wait;

        loop {
            if let Ok(contents) = fs::read_to_string(reply) {
                let _ = fs::remove_file(reply);

                return Some(contents);
            }

            let now = Instant::now();

            if now >= deadline {
                return None;
            }

            thread::sleep(self.poll.min(deadline - now));
        }
    }
}

/// The request a jump writes, `<id>.json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JumpRequest<'a> {
    id: &'a str,
    session_id: &'a str,
    cwd: &'a Path,
    tab_label: Option<&'a str>,
    requested_at: u64,
}

/// The reply the owning window writes, `<id>.reply.json`.
#[derive(Debug, Deserialize)]
struct JumpReply {
    open: PathBuf,
    reveal: String,
}

/// Everything a jump is made of, so that the sequence can be driven without an
/// editor or a running app.
pub struct Jumper {
    raiser: Arc<dyn WindowRaiser>,
    requests: JumpRequests,
}

impl Jumper {
    pub fn new(raiser: Arc<dyn WindowRaiser>, requests: JumpRequests) -> Self {
        Self { raiser, requests }
    }

    /// Takes the user to `target`, and answers honestly about whether it
    /// worked.
    ///
    /// Blocking: it writes a request and polls for its reply for up to the
    /// budget. It belongs on a thread that is allowed to wait.
    pub fn jump(&self, target: &JumpTarget) -> JumpOutcome {
        let Some(cwd) = target.cwd.as_deref() else {
            return JumpOutcome::NoWindow;
        };

        if !self.raiser.available() {
            return JumpOutcome::NoWindow;
        }

        self.requests.sweep();

        let requested_at = epoch_ms();
        let id = format!(
            "{}-{}-{requested_at}",
            std::process::id(),
            REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let request = self.requests.dir.join(format!("{id}.json"));
        let reply = self.requests.dir.join(format!("{id}{REPLY_SUFFIX}"));
        let written = serde_json::to_string(&JumpRequest {
            id: &id,
            session_id: &target.session_id,
            cwd,
            tab_label: target.tab_label.as_deref(),
            requested_at,
        })
        .map_err(io::Error::other)
        .and_then(|contents| write_atomically(&request, &contents));

        if let Err(error) = written {
            eprintln!("aydy: could not write the jump request: {error}");

            return JumpOutcome::Unanswered;
        }

        if let Some(contents) = self.requests.wait_for(&reply, self.requests.timeout) {
            return self.answer(&contents);
        }

        match fs::remove_file(&request) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match self.requests.wait_for(&reply, self.requests.grace) {
                    Some(contents) => self.answer(&contents),
                    None => JumpOutcome::Unanswered,
                }
            }
            _ => JumpOutcome::Unanswered,
        }
    }

    /// What a reply's `contents` come to.
    fn answer(&self, contents: &str) -> JumpOutcome {
        let Ok(reply) = serde_json::from_str::<JumpReply>(contents) else {
            return JumpOutcome::Unanswered;
        };

        if reply.reveal == "duplicated" {
            // The window is deliberately not raised: raising it takes focus
            // from the popover, the popover hides on focus loss, and the one
            // message telling the user a second copy of their session may be
            // running would go unread.
            return JumpOutcome::Duplicated;
        }

        // An absolute path cannot be read by `code` as one of its options.
        if !reply.open.is_absolute() {
            return JumpOutcome::Unanswered;
        }

        match self.raiser.raise(&reply.open) {
            Ok(()) => JumpOutcome::Jumped,
            Err(_) => JumpOutcome::Unanswered,
        }
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;
    use std::sync::mpsc;
    use std::sync::Mutex;

    use super::*;
    use crate::discovery::session_files::FixedProcesses;
    use crate::discovery::test_support::{scratch_dir, session_file, sessions_dir};
    use crate::session::status::SessionStatus;
    use crate::sync::lock_recovering;

    static SCRATCH: AtomicU32 = AtomicU32::new(0);

    /// Long enough that a responder thread always answers inside it; a jump
    /// that is answered returns as soon as the reply lands.
    const ANSWERED_WITHIN: Duration = Duration::from_secs(5);

    /// Short enough that a jump nothing answers costs milliseconds.
    const UNANSWERED_WITHIN: Duration = Duration::from_millis(50);

    struct FakeRaiser {
        raised: Mutex<Vec<PathBuf>>,
        installed: bool,
        fails: bool,
    }

    impl FakeRaiser {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                raised: Mutex::new(Vec::new()),
                installed: true,
                fails: false,
            })
        }

        fn raised(&self) -> Vec<PathBuf> {
            lock_recovering(&self.raised).clone()
        }
    }

    impl WindowRaiser for FakeRaiser {
        fn available(&self) -> bool {
            self.installed
        }

        fn raise(&self, folder: &Path) -> Result<(), String> {
            lock_recovering(&self.raised).push(folder.to_path_buf());

            if self.fails {
                return Err("code answered 1".to_string());
            }

            Ok(())
        }
    }

    fn jump_dir(name: &str) -> PathBuf {
        let dir = scratch_dir(name).join("jump");
        fs::create_dir_all(&dir).expect("the jump directory to be creatable");

        dir
    }

    fn requests(dir: &Path, timeout: Duration, grace: Duration) -> JumpRequests {
        JumpRequests {
            dir: dir.to_path_buf(),
            timeout,
            grace,
            poll: Duration::from_millis(5),
        }
    }

    fn jumper(raiser: &Arc<FakeRaiser>, requests: JumpRequests) -> Jumper {
        Jumper::new(Arc::clone(raiser) as Arc<dyn WindowRaiser>, requests)
    }

    fn target(cwd: Option<&str>) -> JumpTarget {
        JumpTarget {
            session_id: "s1".to_string(),
            tab_label: Some("Fix the auth flow".to_string()),
            cwd: cwd.map(PathBuf::from),
        }
    }

    /// The request file in `dir`, when one is there.
    fn request_in(dir: &Path) -> Option<PathBuf> {
        fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");

                !name.starts_with('.') && name.ends_with(".json") && !name.ends_with(REPLY_SUFFIX)
            })
    }

    fn json_files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("the jump directory to be readable")
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| name.ends_with(".json"))
            .collect();
        names.sort();

        names
    }

    /// A window that claims the first request in `dir`, waits `delay`, and
    /// replies with `reply`, handing the request it read back to the test.
    fn respond(
        dir: &Path,
        delay: Duration,
        reply: &'static str,
    ) -> (thread::JoinHandle<()>, mpsc::Receiver<serde_json::Value>) {
        let dir = dir.to_path_buf();
        let (sent, read) = mpsc::channel();
        let responder = thread::spawn(move || {
            let deadline = Instant::now() + ANSWERED_WITHIN;
            let request = loop {
                if let Some(request) = request_in(&dir) {
                    break request;
                }

                assert!(Instant::now() < deadline, "a request to be written");
                thread::sleep(Duration::from_millis(2));
            };
            let contents = fs::read_to_string(&request).expect("the request to be readable");
            fs::remove_file(&request).expect("the request to be claimed");
            let parsed: serde_json::Value =
                serde_json::from_str(&contents).expect("the request to be JSON");
            let id = parsed["id"].as_str().expect("an id").to_string();
            let _ = sent.send(parsed);

            thread::sleep(delay);
            write_atomically(&dir.join(format!("{id}{REPLY_SUFFIX}")), reply)
                .expect("the reply to be written");
        });

        (responder, read)
    }

    #[test]
    fn a_revealed_reply_raises_the_folder_it_names() {
        let dir = jump_dir("revealed");
        let raiser = FakeRaiser::new();
        let jumper = jumper(&raiser, requests(&dir, ANSWERED_WITHIN, UNANSWERED_WITHIN));
        let (responder, read) = respond(
            &dir,
            Duration::ZERO,
            r#"{"open":"/Users/eric/Code/aydy","reveal":"revealed"}"#,
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy/src")));
        responder.join().expect("the responder");
        let request = read.recv().expect("the request the window read");

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(
            raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
        assert_eq!(request["sessionId"], "s1");
        assert_eq!(request["cwd"], "/Users/eric/Code/aydy/src");
        assert_eq!(request["tabLabel"], "Fix the auth flow");
        assert!(request["requestedAt"].as_u64().is_some());
        assert_eq!(json_files_in(&dir), Vec::<String>::new());
    }

    /// The most damaging thing this app can do. The window is not raised, so
    /// the popover stays up to say so.
    #[test]
    fn a_duplicated_reply_is_reported_and_raises_nothing() {
        let dir = jump_dir("duplicated");
        let raiser = FakeRaiser::new();
        let jumper = jumper(&raiser, requests(&dir, ANSWERED_WITHIN, UNANSWERED_WITHIN));
        let (responder, _read) = respond(
            &dir,
            Duration::ZERO,
            r#"{"open":"/Users/eric/Code/aydy","reveal":"duplicated"}"#,
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));
        responder.join().expect("the responder");

        assert_eq!(outcome, JumpOutcome::Duplicated);
        assert_eq!(raiser.raised(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_request_nobody_answers_is_unanswered_and_withdrawn() {
        let dir = jump_dir("unanswered");
        let raiser = FakeRaiser::new();
        let jumper = jumper(
            &raiser,
            requests(&dir, UNANSWERED_WITHIN, UNANSWERED_WITHIN),
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));

        assert_eq!(outcome, JumpOutcome::Unanswered);
        assert_eq!(json_files_in(&dir), Vec::<String>::new());
        assert_eq!(raiser.raised(), Vec::<PathBuf>::new());
    }

    /// The window claimed the request before the timeout but replied after it,
    /// which the grace period exists for.
    #[test]
    fn a_request_claimed_in_time_and_answered_in_the_grace_period_is_a_jump() {
        let dir = jump_dir("late");
        let raiser = FakeRaiser::new();
        let jumper = jumper(
            &raiser,
            requests(
                &dir,
                Duration::from_millis(200),
                Duration::from_millis(2_000),
            ),
        );
        let (responder, _read) = respond(
            &dir,
            Duration::from_millis(400),
            r#"{"open":"/Users/eric/Code/aydy","reveal":"notPresent"}"#,
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));
        responder.join().expect("the responder");

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(
            raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
    }

    #[test]
    fn a_machine_with_no_code_command_line_goes_nowhere_and_writes_nothing() {
        let dir = scratch_dir("no-code").join("jump");
        let raiser = Arc::new(FakeRaiser {
            raised: Mutex::new(Vec::new()),
            installed: false,
            fails: false,
        });
        let jumper = jumper(
            &raiser,
            requests(&dir, UNANSWERED_WITHIN, UNANSWERED_WITHIN),
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));

        assert_eq!(outcome, JumpOutcome::NoWindow);
        assert!(!dir.exists());
    }

    #[test]
    fn a_session_with_no_directory_goes_nowhere_and_writes_nothing() {
        let dir = scratch_dir("no-cwd").join("jump");
        let raiser = FakeRaiser::new();
        let jumper = jumper(
            &raiser,
            requests(&dir, UNANSWERED_WITHIN, UNANSWERED_WITHIN),
        );

        assert_eq!(jumper.jump(&target(None)), JumpOutcome::NoWindow);
        assert!(!dir.exists());
    }

    #[test]
    fn a_malformed_reply_is_unanswered() {
        let dir = jump_dir("malformed");
        let raiser = FakeRaiser::new();
        let jumper = jumper(&raiser, requests(&dir, ANSWERED_WITHIN, UNANSWERED_WITHIN));
        let (responder, _read) = respond(&dir, Duration::ZERO, "{");

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));
        responder.join().expect("the responder");

        assert_eq!(outcome, JumpOutcome::Unanswered);
        assert_eq!(raiser.raised(), Vec::<PathBuf>::new());
        assert_eq!(json_files_in(&dir), Vec::<String>::new());
    }

    /// A path that does not start at the root could be read by `code` as one
    /// of its own options.
    #[test]
    fn a_reply_naming_a_relative_path_raises_nothing() {
        let dir = jump_dir("relative");
        let raiser = FakeRaiser::new();
        let jumper = jumper(&raiser, requests(&dir, ANSWERED_WITHIN, UNANSWERED_WITHIN));
        let (responder, _read) = respond(
            &dir,
            Duration::ZERO,
            r#"{"open":"--install-extension","reveal":"revealed"}"#,
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));
        responder.join().expect("the responder");

        assert_eq!(outcome, JumpOutcome::Unanswered);
        assert_eq!(raiser.raised(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_window_that_cannot_be_raised_is_unanswered() {
        let dir = jump_dir("raise-fails");
        let raiser = Arc::new(FakeRaiser {
            raised: Mutex::new(Vec::new()),
            installed: true,
            fails: true,
        });
        let jumper = jumper(&raiser, requests(&dir, ANSWERED_WITHIN, UNANSWERED_WITHIN));
        let (responder, _read) = respond(
            &dir,
            Duration::ZERO,
            r#"{"open":"/Users/eric/Code/aydy","reveal":"revealed"}"#,
        );

        let outcome = jumper.jump(&target(Some("/Users/eric/Code/aydy")));
        responder.join().expect("the responder");

        assert_eq!(outcome, JumpOutcome::Unanswered);
    }

    #[test]
    fn a_jump_sweeps_replies_older_than_the_budget_and_nothing_else() {
        let dir = jump_dir("sweep");
        let stale = dir.join("old-1.reply.json");
        let fresh = dir.join("new-1.reply.json");
        let other = dir.join("other-1.json");
        for path in [&stale, &fresh, &other] {
            fs::write(path, "{}").expect("a fixture file");
        }
        fs::File::options()
            .write(true)
            .open(&stale)
            .expect("the stale reply")
            .set_modified(SystemTime::now() - Duration::from_secs(60))
            .expect("the stale reply to be backdated");
        let raiser = FakeRaiser::new();
        let jumper = jumper(
            &raiser,
            requests(&dir, UNANSWERED_WITHIN, UNANSWERED_WITHIN),
        );

        jumper.jump(&target(Some("/Users/eric/Code/aydy")));

        assert_eq!(
            json_files_in(&dir),
            vec!["new-1.reply.json".to_string(), "other-1.json".to_string()]
        );
    }

    #[test]
    fn the_budget_is_the_timeout_and_the_grace_period() {
        assert_eq!(
            JumpRequests::new(PathBuf::from("/data/jump")).budget(),
            Duration::from_secs(4)
        );
    }

    const LAUNCH_PID: u32 = 37_648;

    fn moved_target() -> JumpTarget {
        JumpTarget {
            session_id: "s1".to_string(),
            tab_label: None,
            cwd: Some(PathBuf::from("/a/.claude/worktrees/x")),
        }
    }

    #[test]
    fn a_target_carries_the_launch_directory_of_its_live_session_file() {
        let contents = session_file(LAUNCH_PID, "s1", Path::new("/a"), "claude-vscode");
        let dir = sessions_dir("launch-cwd", &[(LAUNCH_PID, &contents)]);

        let target = moved_target().with_launch_cwd(Some(&dir), &FixedProcesses::new([LAUNCH_PID]));

        assert_eq!(target.cwd, Some(PathBuf::from("/a")));
    }

    #[test]
    fn a_target_without_a_live_launch_directory_keeps_its_own() {
        let live = FixedProcesses::new([LAUNCH_PID, 41_000, 42_000]);
        let dead = session_file(LAUNCH_PID, "s1", Path::new("/a"), "claude-vscode");
        let other = session_file(41_000, "s2", Path::new("/a"), "claude-vscode");
        let without_cwd = r#"{"pid":42000,"sessionId":"s1"}"#;
        let dead_dir = sessions_dir("launch-dead", &[(LAUNCH_PID, &dead)]);
        let other_dir = sessions_dir("launch-other", &[(41_000, &other)]);
        let without_cwd_dir = sessions_dir("launch-no-cwd", &[(42_000, without_cwd)]);
        let missing_dir = scratch_dir("launch-missing").join("absent");

        let cases: [(Option<&Path>, &dyn ProcessTable); 5] = [
            (Some(&dead_dir), &FixedProcesses::new([])),
            (Some(&other_dir), &live),
            (Some(&without_cwd_dir), &live),
            (None, &live),
            (Some(&missing_dir), &live),
        ];

        for (dir, processes) in cases {
            assert_eq!(
                moved_target().with_launch_cwd(dir, processes),
                moved_target(),
                "{dir:?}"
            );
        }
    }

    #[test]
    fn a_session_that_cannot_be_jumped_to_has_nothing_to_jump_to() {
        let mut session = Session::new("s1".to_string(), Instant::now());
        session.jumpable = false;

        assert_eq!(JumpTarget::of(&session), None);
    }

    #[test]
    fn a_targets_tab_label_is_the_title_cut_the_way_the_editor_cuts_it() {
        let mut session = Session::new("s1".to_string(), Instant::now());
        session.jumpable = true;
        session.label = Some("Models section configuration options".to_string());

        assert_eq!(
            JumpTarget::of(&session).expect("a target").tab_label,
            Some("Models section configura…".to_string())
        );
    }

    /// Every session that has not been prompted yet shows this, in every
    /// window, so sending it would have the window's guard match a tab
    /// belonging to some other session entirely.
    #[test]
    fn the_placeholder_every_unprompted_session_shows_is_never_sent_as_a_tab() {
        let mut session = Session::new("s1".to_string(), Instant::now());
        session.jumpable = true;
        session.label = Some(PLACEHOLDER_TAB_LABEL.to_string());

        assert_eq!(JumpTarget::of(&session).expect("a target").tab_label, None);
    }

    /// A session the registry holds but nothing else: it still has to answer
    /// something rather than panicking on a missing field.
    #[test]
    fn a_session_with_nothing_known_about_it_still_produces_a_target() {
        let mut session = Session::new("s1".to_string(), Instant::now());
        session.jumpable = true;

        let target = JumpTarget::of(&session).expect("a target");

        assert_eq!(target.cwd, None);
        assert_eq!(session.status(), SessionStatus::Unknown);
    }

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!(
            "aydy-jump-{}-{}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// The whole point of the list: a bare `code` would resolve from a
    /// terminal and not from the shipped app.
    #[test]
    fn every_place_code_is_looked_for_is_an_absolute_path() {
        assert!(CODE_PROGRAMS
            .iter()
            .all(|program| Path::new(program).is_absolute()));
    }

    #[test]
    fn the_code_command_line_is_found_by_absolute_path_or_not_at_all() {
        let directory = scratch();
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let installed = directory.join("code");
        std::fs::write(&installed, "#!/bin/sh\n").expect("a code command line");

        let found = CodeCli::new(vec![directory.join("missing"), installed.clone()]);
        let absent = CodeCli::new(vec![directory.join("missing")]);

        assert!(found.available());
        assert!(!absent.available());
        assert_eq!(found.program(), Some(installed.as_path()));

        std::fs::remove_dir_all(&directory).ok();
    }
}
