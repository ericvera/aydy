//! Taking the user to a session: the tab first, then the window it is in.
//!
//! Every mechanism here is undocumented, best effort, or both, and each one
//! fails *silently* rather than raising anything. So the shape of this module
//! is not "do the thing" but "prove a window exists, ask, and then find out
//! whether anything happened":
//!
//! - **The reveal is guarded twice.** Asking Claude Code to open a session in a
//!   window that is not running it does not fail — it opens a second, live copy
//!   of that session. AYDY only ever addresses the window the process-tree join
//!   placed the session in, and the window checks its own tabs before it calls.
//! - **The raise is gated on a window that is known to exist.** `code <folder>`
//!   focuses a window already holding that folder and **opens a new one** when
//!   none does, so it is never run unless a registration or a Claude Code lock
//!   file says such a window is open.
//! - **Nothing is believed until the window says so.** The extension already
//!   reports focus and its active tab; a jump waits for that report, tries once
//!   more without it, and then says it could not rather than pretending.
//!
//! What is deliberately absent: `vscode://` with a `windowId=` query parameter
//! targets one window, but a wrong id silently lands on the active window,
//! which for Claude Code means duplicating a session. It is not implemented
//! here and should not be.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::extension::command::{ExtensionCommands, RevealOutcome};
use crate::extension::registry::{ExtensionPid, Registration};
use crate::listener::server::lock_recovering;
use crate::session::event::WindowId;
use crate::session::session::Session;
use crate::session::tab_match::{tab_label, WindowFocus, PLACEHOLDER_TAB_LABEL};

/// How long a jump waits for the window to report that it came forward.
///
/// The window reports on a 150 ms debounce and `code` has to reach a running
/// editor first, so this is generous by comparison; it is still short enough
/// that a jump nothing answered is retried while the user is still looking at
/// the popover.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_millis(1_500);

/// How many times a jump is attempted before it is reported as unconfirmed.
///
/// One retry, not a loop: every mechanism here is best effort, and a second
/// attempt covers an editor that was busy, while a third would only make the
/// user wait longer to be told the same thing.
const ATTEMPTS: u32 = 2;

/// The environment variable Claude Code takes its configuration directory from.
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// Claude Code's configuration directory inside the user's home.
const CONFIG_DIR: &str = ".claude";

/// The directory Claude Code writes one lock file per open editor window into.
const IDE_DIR: &str = "ide";

/// The extension of the files in that directory.
const LOCK_EXTENSION: &str = "lock";

/// Where the `code` command line lives, in the order it is looked for.
///
/// **Absolute paths, never the bare name.** A GUI app launched by launchd —
/// which is what launch-at-login makes the normal case — inherits launchd's
/// `PATH`, and that does not include `/usr/local/bin`. `Command::new("code")`
/// therefore resolves under `yarn tauri dev` from a terminal and fails in the
/// shipped app, which is exactly the silent failure the rest of this module is
/// built to avoid.
const CODE_PROGRAMS: [&str; 3] = [
    "/usr/local/bin/code",
    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
    "/opt/homebrew/bin/code",
];

/// What came of a click on a session row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JumpOutcome {
    /// The window reported itself in front, with the expected tab in it where
    /// one was revealed.
    Jumped,
    /// The window was asked to come forward and there is nothing in it to
    /// confirm that it did — the state of a machine without the companion
    /// extension, where the raise is the whole of what jumping can be.
    Raised,
    /// No window could be shown to be open on this session, so nothing was
    /// run. The click marks the session read and goes nowhere, which is what a
    /// row that cannot be jumped to has always done.
    NoWindow,
    /// The window was asked twice and never said it came forward.
    Unconfirmed,
    /// A tab opened when the reveal was called, so a second live copy of the
    /// session may now exist.
    Duplicated,
}

/// Everything a jump needs to know about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTarget {
    pub session_id: String,
    /// The window the process-tree join placed this session in. `None` means
    /// nothing proved which window runs it, and it is the whole guard on the
    /// reveal: no window, no reveal, ever.
    pub window: Option<WindowId>,
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
            window: session.window,
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
}

/// The folders VS Code windows are known to have open, from a source other
/// than the companion extension.
pub trait WindowFolders: Send + Sync {
    /// Every folder some editor window currently has open.
    fn folders(&self) -> Vec<PathBuf>;
}

/// Claude Code's own record of which folders are open in which editor window.
///
/// One file per window, each naming that window's workspace folders. Its `pid`
/// is the *main* editor process and is identical across every file, so this
/// cannot say which window is which — but "does some window have this folder
/// open?" is exactly what `code <folder>` needs, and this answers it **without
/// the companion extension**, which is what makes clicking still raise a window
/// on a machine that never installed it.
///
/// These are undocumented Claude Code internals, so a directory that is missing
/// or unreadable is read as "cannot confirm" rather than as "no windows".
pub struct IdeLocks {
    directory: Option<PathBuf>,
}

impl IdeLocks {
    /// The locks in `directory`.
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self { directory }
    }

    /// The locks in the directory Claude Code writes them to.
    pub fn user() -> Self {
        Self::new(user_ide_dir())
    }
}

impl WindowFolders for IdeLocks {
    fn folders(&self) -> Vec<PathBuf> {
        let Some(directory) = self.directory.as_ref() else {
            return Vec::new();
        };

        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };

        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|kind| kind == LOCK_EXTENSION))
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .filter_map(|held| serde_json::from_str::<LockFile>(&held).ok())
            .flat_map(|lock| lock.workspace_folders)
            .collect()
    }
}

/// One editor window's lock file, of which only the folders are read.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LockFile {
    workspace_folders: Vec<PathBuf>,
}

/// Claude Code's `ide` directory, wherever the user's configuration lives.
pub fn user_ide_dir() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os(CONFIG_DIR_VARIABLE) {
        if !configured.is_empty() {
            return Some(PathBuf::from(configured).join(IDE_DIR));
        }
    }

    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;

    Some(PathBuf::from(home).join(CONFIG_DIR).join(IDE_DIR))
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

/// One jump waiting to hear that it worked.
#[derive(Debug)]
struct Pending {
    window: WindowId,
    /// The tab the window is expected to have in front, when a reveal put one
    /// there. `None` when the jump only asked for the window.
    tab: Option<String>,
    confirmed: bool,
}

/// What tells one waiting jump from another.
type Ticket = u64;

/// Every jump currently waiting, each under a ticket of its own.
#[derive(Debug, Default)]
struct Waiters {
    next: Ticket,
    held: Vec<(Ticket, Pending)>,
}

impl Waiters {
    fn pending(&self, ticket: Ticket) -> Option<&Pending> {
        self.held
            .iter()
            .find(|(held, _)| *held == ticket)
            .map(|(_, pending)| pending)
    }
}

/// What a jump waits on: the window itself saying it came forward.
///
/// Nothing else can say it. `code` answers as soon as it has handed the folder
/// over, the reveal answers about a tab rather than about focus, and macOS says
/// nothing to an app about another app's windows. The companion extension
/// already reports focus and its active tab on every change, so a jump listens
/// for that report and gives up on it rather than assuming.
///
/// **More than one jump can be waiting at a time.** A jump retries, so the
/// popover is still up and still clickable while one is in flight — another
/// row, or the same row again. So a wait is not a single slot: each is held
/// under its own ticket, a report settles every jump that asked for exactly it
/// and no others, and a jump that ends takes only its own ticket away. Without
/// that, one jump finishing would answer or silence another still waiting, and
/// a window that did come forward would be reported as one that never did.
#[derive(Debug, Default)]
pub struct JumpWatch {
    waiting: Mutex<Waiters>,
    settled: Condvar,
}

impl JumpWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts waiting for `window` to report itself in front, showing `tab`
    /// where one was revealed.
    ///
    /// Armed before the raise is run, so a window quick enough to report before
    /// the call returns is still heard. The wait lasts as long as the
    /// [`Waiting`] this answers with.
    pub fn watch(&self, window: WindowId, tab: Option<String>) -> Waiting<'_> {
        let mut waiters = lock_recovering(&self.waiting);
        let ticket = waiters.next;

        waiters.next += 1;
        waiters.held.push((
            ticket,
            Pending {
                window,
                tab,
                confirmed: false,
            },
        ));

        Waiting {
            watch: self,
            ticket,
        }
    }

    /// Takes what a window reported about itself, waking the jumps waiting on
    /// it.
    ///
    /// A report from another window, a window reporting that it does *not*
    /// have focus, and a window in front showing some other tab all say the
    /// jump has not happened yet, so each is ignored rather than answered.
    /// Every jump the report does match is settled by it: one window coming
    /// forward is the answer to each click that asked for exactly that, and a
    /// row clicked twice has asked for it twice.
    pub fn confirm(&self, focus: &WindowFocus) {
        if !focus.focused {
            return;
        }

        let mut waiters = lock_recovering(&self.waiting);
        let mut woken = false;

        for (_, watching) in waiters.held.iter_mut() {
            if watching.window != focus.window_id {
                continue;
            }

            if let Some(tab) = watching.tab.as_deref() {
                if focus.active_tab_label.as_deref() != Some(tab) {
                    continue;
                }
            }

            watching.confirmed = true;
            woken = true;
        }

        if woken {
            self.settled.notify_all();
        }
    }

    /// Waits up to `timeout` for the window one ticket is watching to report
    /// itself in front.
    fn wait_on(&self, ticket: Ticket, timeout: Duration) -> bool {
        let waiters = lock_recovering(&self.waiting);

        let (waiters, _) = self
            .settled
            .wait_timeout_while(
                waiters,
                timeout,
                |waiters| matches!(waiters.pending(ticket), Some(watching) if !watching.confirmed),
            )
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        matches!(waiters.pending(ticket), Some(watching) if watching.confirmed)
    }

    /// Gives one ticket up, leaving every other jump's where it is.
    fn forget(&self, ticket: Ticket) {
        lock_recovering(&self.waiting)
            .held
            .retain(|(held, _)| *held != ticket);
    }
}

/// One jump's place in the watch, given up when the jump ends.
///
/// A guard rather than a `stop` anything can call: the only wait a jump is able
/// to end is its own, which is what keeps two jumps in flight at once from
/// taking each other's answer away.
#[derive(Debug)]
#[must_use = "the wait lasts only as long as this does"]
pub struct Waiting<'watch> {
    watch: &'watch JumpWatch,
    ticket: Ticket,
}

impl Waiting<'_> {
    /// Waits up to `timeout` for the window this jump asked for to report
    /// itself in front.
    pub fn wait(&self, timeout: Duration) -> bool {
        self.watch.wait_on(self.ticket, timeout)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.watch.forget(self.ticket);
    }
}

/// Everything a jump is made of, so that the sequence can be driven without an
/// editor, a socket or a running app.
pub struct Jumper {
    extensions: Arc<ExtensionCommands>,
    raiser: Arc<dyn WindowRaiser>,
    folders: Arc<dyn WindowFolders>,
    watch: Arc<JumpWatch>,
    confirm_timeout: Duration,
}

impl Jumper {
    pub fn new(
        extensions: Arc<ExtensionCommands>,
        raiser: Arc<dyn WindowRaiser>,
        folders: Arc<dyn WindowFolders>,
        watch: Arc<JumpWatch>,
    ) -> Self {
        Self {
            extensions,
            raiser,
            folders,
            watch,
            confirm_timeout: CONFIRM_TIMEOUT,
        }
    }

    /// The same jumper, waiting `timeout` for each confirmation.
    pub fn confirming_within(mut self, timeout: Duration) -> Self {
        self.confirm_timeout = timeout;

        self
    }

    /// Takes the user to `target`, and answers honestly about whether it
    /// worked.
    ///
    /// Blocking: it posts to a window over loopback, runs a command line and
    /// waits for a report to arrive. It belongs on a thread that is allowed to
    /// wait.
    pub fn jump(&self, target: &JumpTarget) -> JumpOutcome {
        let windows = self.extensions.registry().registrations();

        // Decided before anything is asked for. A reveal that cannot be
        // followed by a raise leaves the user watching a window they did not
        // ask to look at change its tab, and a `code` that is not installed
        // cannot raise anything at all.
        let Some(folder) = folder_of(target, &windows, self.folders.as_ref()) else {
            return JumpOutcome::NoWindow;
        };

        if !self.raiser.available() {
            return JumpOutcome::NoWindow;
        }

        // Only a window that is registered can answer, and only the window the
        // join placed this session in is ever addressed.
        let confirming = target
            .window
            .filter(|window| windows.iter().any(|held| held.pid == *window));

        for _ in 0..ATTEMPTS {
            let revealed = self.reveal(target, confirming);

            if revealed == Some(RevealOutcome::Duplicated) {
                // Nothing more is attempted: asking again would ask for a
                // second duplicate, and the window is deliberately not raised
                // either. Raising it takes focus from the popover, the popover
                // hides on focus loss, and the one message telling the user a
                // second copy of their session may be running would go unread.
                return JumpOutcome::Duplicated;
            }

            // Only a tab that was actually brought forward is waited for.
            // Expecting one that was refused would turn every raise-only jump
            // into a failure.
            let expected_tab = match revealed {
                Some(RevealOutcome::Revealed) => target.tab_label.clone(),
                _ => None,
            };

            // A window that could not be reached has just been dropped by the
            // command channel, and a window that has gone confirms nothing.
            // Waiting on it anyway would report a raise that worked as a
            // failure.
            let confirming = confirming
                .filter(|window| self.extensions.registry().registration(*window).is_some());

            // Armed before the raise is run, and given up when this attempt
            // ends. The wait belongs to this jump alone: another click while
            // this one is still in flight is another jump, and neither is
            // allowed to take the other's answer or to end its wait.
            let waiting = confirming.map(|window| self.watch.watch(window, expected_tab));

            let raised = self.raiser.raise(&folder).is_ok();

            match &waiting {
                // Nothing in that window can say whether it came forward, so
                // the raise having run is the whole of the answer there is.
                None if raised => return JumpOutcome::Raised,
                Some(waiting) if raised && waiting.wait(self.confirm_timeout) => {
                    return JumpOutcome::Jumped
                }
                _ => {}
            }
        }

        JumpOutcome::Unconfirmed
    }

    /// Asks `window` to bring the session's tab forward, when everything about
    /// the session says that is safe to ask.
    ///
    /// `None` means nothing was sent. There are three ways to get one, and all
    /// three are the guard doing its job: the join placed the session in no
    /// window, that window is not registered, or AYDY holds no title that
    /// could be on a tab and so has nothing for the window to check against.
    fn reveal(&self, target: &JumpTarget, window: Option<ExtensionPid>) -> Option<RevealOutcome> {
        let window = window?;
        let tab_label = target.tab_label.as_deref()?;

        self.extensions
            .reveal(window, &target.session_id, tab_label)
            .ok()
    }
}

/// The folder to raise a window on, or `None` when no window can be shown to be
/// open on this session.
///
/// Two independent sources answer, and the second is what makes a machine
/// without the companion extension still raise a window: the registrations when
/// the extension is installed, Claude Code's own lock files when it is not.
/// Neither answering means `code` is not run at all — running it would open a
/// new window, and a click that opens an empty editor is worse than a click
/// that does nothing.
fn folder_of(
    target: &JumpTarget,
    windows: &[Registration],
    locks: &dyn WindowFolders,
) -> Option<PathBuf> {
    if let Some(window) = target.window {
        if let Some(held) = windows.iter().find(|held| held.pid == window) {
            // This window is known to run the session, so any folder it has
            // open raises the right window — including when the session's own
            // directory sits outside all of them. A window with no folder at
            // all answers nothing rather than falling through: the sources
            // below would name some other window holding this session's
            // directory, and raising that one takes the user somewhere their
            // session is not.
            return holding(&held.workspace_folders, target.cwd.as_deref())
                .or_else(|| held.workspace_folders.first().cloned());
        }
    }

    let cwd = target.cwd.as_deref()?;

    windows
        .iter()
        .find_map(|held| holding(&held.workspace_folders, Some(cwd)))
        .or_else(|| holding(&locks.folders(), Some(cwd)))
}

/// The folder in `folders` that `cwd` sits in, preferring the closest one.
///
/// Two windows open on the same folder cannot be told apart by this, and that
/// is accepted: raising one of them and having the reveal put the right tab
/// forward in whichever comes up beats refusing to act.
fn holding(folders: &[PathBuf], cwd: Option<&Path>) -> Option<PathBuf> {
    let cwd = cwd?;

    folders
        .iter()
        .filter(|folder| cwd.starts_with(folder))
        .max_by_key(|folder| folder.components().count())
        .cloned()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::Instant;

    use super::*;
    use crate::clock::TestClock;
    use crate::extension::command::ExtensionChannel;
    use crate::extension::registry::ExtensionRegistry;
    use crate::session::status::SessionStatus;

    /// Short enough that a jump nothing answers costs milliseconds rather than
    /// three seconds of suite time.
    const CONFIRM_WITHIN: Duration = Duration::from_millis(20);

    /// How long a stepped test waits for a jump to reach its raise.
    ///
    /// Far longer than it takes, and bounded all the same: a jump that never
    /// gets there should fail the test rather than hang the suite on it.
    const STEP_WITHIN: Duration = Duration::from_secs(5);

    static SCRATCH: AtomicU32 = AtomicU32::new(0);

    /// The answer of a window whose callback port cannot be reached at all,
    /// which is what a window that closed since the last scan looks like.
    const UNREACHABLE: &str = "unreachable";

    /// A window's side of the command channel, recording what it was asked and
    /// answering the outcome it was given.
    struct FakeWindow {
        asked: Mutex<Vec<String>>,
        reveal: Option<&'static str>,
    }

    impl FakeWindow {
        fn answering(reveal: Option<&'static str>) -> Arc<Self> {
            Arc::new(Self {
                asked: Mutex::new(Vec::new()),
                reveal,
            })
        }

        fn asked(&self) -> Vec<String> {
            lock_recovering(&self.asked).clone()
        }
    }

    impl ExtensionChannel for FakeWindow {
        fn post(&self, _port: u16, body: &str) -> Result<String, String> {
            lock_recovering(&self.asked).push(body.to_string());

            if self.reveal == Some(UNREACHABLE) {
                return Err("connection refused".to_string());
            }

            let request: serde_json::Value = serde_json::from_str(body).expect("a command");
            let pid = &request["pid"];

            Ok(match self.reveal {
                Some(outcome) => format!(
                    r#"{{"pid":{pid},"ok":{},"reveal":"{outcome}"}}"#,
                    outcome == "revealed"
                ),
                None => format!(r#"{{"pid":{pid},"ok":true}}"#),
            })
        }
    }

    /// A stand-in for the `code` command line, which reports the window in
    /// front the way an editor coming forward does.
    struct FakeRaiser {
        raised: Mutex<Vec<PathBuf>>,
        installed: bool,
        watch: Arc<JumpWatch>,
        /// One entry per raise: what the editor reports, or nothing at all.
        reports: Mutex<VecDeque<Option<WindowFocus>>>,
    }

    impl FakeRaiser {
        fn new(watch: &Arc<JumpWatch>, reports: Vec<Option<WindowFocus>>) -> Arc<Self> {
            Arc::new(Self {
                raised: Mutex::new(Vec::new()),
                installed: true,
                watch: Arc::clone(watch),
                reports: Mutex::new(reports.into()),
            })
        }

        /// A machine with no `code` on it at all.
        fn missing(watch: &Arc<JumpWatch>) -> Arc<Self> {
            Arc::new(Self {
                raised: Mutex::new(Vec::new()),
                installed: false,
                watch: Arc::clone(watch),
                reports: Mutex::new(VecDeque::new()),
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

            if let Some(Some(report)) = lock_recovering(&self.reports).pop_front() {
                self.watch.confirm(&report);
            }

            Ok(())
        }
    }

    /// Claude Code's lock files, stood in for by the folders they would name.
    struct FakeLocks {
        folders: Vec<PathBuf>,
    }

    impl FakeLocks {
        fn holding(folders: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                folders: folders.iter().map(PathBuf::from).collect(),
            })
        }
    }

    impl WindowFolders for FakeLocks {
        fn folders(&self) -> Vec<PathBuf> {
            self.folders.clone()
        }
    }

    /// One raise the test is driving, and the report the editor makes before
    /// that raise returns.
    struct Raising {
        resume: Sender<Option<WindowFocus>>,
    }

    impl Raising {
        /// Lets the raise return, with `report` made to the watch first — or
        /// with nothing reported at all, which is a window that has already
        /// said everything it has to say.
        fn reports(self, report: Option<WindowFocus>) {
            self.resume
                .send(report)
                .expect("a raise to be waiting on the report");
        }
    }

    /// A `code` stand-in that hands the test control at each raise it was told
    /// to drive, so two jumps can be put in flight at once and stepped past
    /// each other.
    ///
    /// A raise beyond those is answered at once and reported by nothing, which
    /// is what a retry looks like from here — the retry being the failure these
    /// tests are about, that is what makes them fail rather than hang.
    struct SteppedRaiser {
        raised: Mutex<Vec<PathBuf>>,
        watch: Arc<JumpWatch>,
        driven: AtomicU32,
        raises: Mutex<Sender<Raising>>,
    }

    impl SteppedRaiser {
        fn raised(&self) -> Vec<PathBuf> {
            lock_recovering(&self.raised).clone()
        }
    }

    impl WindowRaiser for SteppedRaiser {
        fn available(&self) -> bool {
            true
        }

        fn raise(&self, folder: &Path) -> Result<(), String> {
            lock_recovering(&self.raised).push(folder.to_path_buf());

            let driven = self
                .driven
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    left.checked_sub(1)
                })
                .is_ok();

            if !driven {
                return Ok(());
            }

            let (resume, told) = mpsc::channel();
            lock_recovering(&self.raises)
                .send(Raising { resume })
                .expect("the test to be watching the raises");

            if let Some(report) = told.recv().expect("the test to answer the raise") {
                self.watch.confirm(&report);
            }

            Ok(())
        }
    }

    struct Stepped {
        jumper: Jumper,
        windows: Arc<ExtensionRegistry>,
        raiser: Arc<SteppedRaiser>,
    }

    /// A jumper whose editor hands the test the first `driven` raises, one at a
    /// time, and answers anything beyond them without a report.
    fn stepping(driven: u32) -> (Stepped, Receiver<Raising>) {
        let watch = Arc::new(JumpWatch::new());
        let channel = FakeWindow::answering(Some("revealed"));
        let (raises, stepped) = mpsc::channel();
        let raiser = Arc::new(SteppedRaiser {
            raised: Mutex::new(Vec::new()),
            watch: Arc::clone(&watch),
            driven: AtomicU32::new(driven),
            raises: Mutex::new(raises),
        });
        let windows = Arc::new(ExtensionRegistry::new(Arc::new(TestClock::new())));
        let extensions = Arc::new(ExtensionCommands::new(
            Arc::clone(&windows),
            Arc::clone(&channel) as Arc<dyn ExtensionChannel>,
        ));

        (
            Stepped {
                jumper: Jumper::new(
                    extensions,
                    Arc::clone(&raiser) as Arc<dyn WindowRaiser>,
                    FakeLocks::holding(&[]) as Arc<dyn WindowFolders>,
                    watch,
                )
                .confirming_within(CONFIRM_WITHIN),
                windows,
                raiser,
            },
            stepped,
        )
    }

    fn window(pid: ExtensionPid, folders: &[&str]) -> Registration {
        Registration {
            pid,
            callback_port: 55_123,
            workspace_folders: folders.iter().map(PathBuf::from).collect(),
            claude_code_version: Some("2.1.238".to_string()),
        }
    }

    fn target(window: Option<WindowId>, tab_label: Option<&str>, cwd: Option<&str>) -> JumpTarget {
        JumpTarget {
            session_id: "s1".to_string(),
            window,
            tab_label: tab_label.map(str::to_string),
            cwd: cwd.map(PathBuf::from),
        }
    }

    /// A second session, so that two jumps in flight at once are two jumps.
    fn second_target(
        window: Option<WindowId>,
        tab_label: Option<&str>,
        cwd: Option<&str>,
    ) -> JumpTarget {
        JumpTarget {
            session_id: "s2".to_string(),
            ..target(window, tab_label, cwd)
        }
    }

    fn reported(window: WindowId, focused: bool, active_tab_label: Option<&str>) -> WindowFocus {
        WindowFocus {
            window_id: window,
            focused,
            active_tab_label: active_tab_label.map(str::to_string),
        }
    }

    struct Harness {
        jumper: Jumper,
        windows: Arc<ExtensionRegistry>,
        channel: Arc<FakeWindow>,
        raiser: Arc<FakeRaiser>,
    }

    /// A jumper whose window answers `reveal`, whose editor makes `reports`
    /// when it is raised, and which knows about `locks` besides.
    fn jumping(
        reveal: Option<&'static str>,
        reports: Vec<Option<WindowFocus>>,
        locks: &[&str],
    ) -> Harness {
        let watch = Arc::new(JumpWatch::new());
        let channel = FakeWindow::answering(reveal);
        let raiser = FakeRaiser::new(&watch, reports);
        let windows = Arc::new(ExtensionRegistry::new(Arc::new(TestClock::new())));
        let extensions = Arc::new(ExtensionCommands::new(
            Arc::clone(&windows),
            Arc::clone(&channel) as Arc<dyn ExtensionChannel>,
        ));

        Harness {
            jumper: Jumper::new(
                extensions,
                Arc::clone(&raiser) as Arc<dyn WindowRaiser>,
                FakeLocks::holding(locks) as Arc<dyn WindowFolders>,
                watch,
            )
            .confirming_within(CONFIRM_WITHIN),
            windows,
            channel,
            raiser,
        }
    }

    /// The guard the whole task turns on: the reveal goes to the window the
    /// process-tree join placed the session in, and to no other.
    #[test]
    fn a_reveal_is_sent_only_to_the_window_that_runs_the_session() {
        let harness = jumping(
            Some("revealed"),
            vec![Some(reported(65_921, true, Some("Fix the auth flow")))],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));
        harness
            .windows
            .register(window(17_804, &["/Users/eric/Code/okven"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy/src"),
        ));

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(
            harness.channel.asked(),
            vec![
                r#"{"pid":65921,"command":"reveal","sessionId":"s1","tabLabel":"Fix the auth flow"}"#
                    .to_string()
            ]
        );
        assert_eq!(
            harness.raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
    }

    /// The join placed this session nowhere, so nothing proves which window
    /// runs it — and a reveal sent on a guess is what opens a second live copy
    /// of the user's session.
    #[test]
    fn a_session_no_window_owns_is_raised_but_never_revealed() {
        let harness = jumping(Some("revealed"), vec![], &[]);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            None,
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy/src"),
        ));

        assert_eq!(outcome, JumpOutcome::Raised);
        assert_eq!(harness.channel.asked(), Vec::<String>::new());
        assert_eq!(
            harness.raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
    }

    /// The window closed since the last scan placed the session in it. There is
    /// nothing left to address, and nothing left to confirm with either.
    #[test]
    fn a_session_in_a_window_that_is_no_longer_registered_is_not_revealed() {
        let harness = jumping(Some("revealed"), vec![], &["/Users/eric/Code/okven"]);

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/okven/app"),
        ));

        assert_eq!(outcome, JumpOutcome::Raised);
        assert_eq!(harness.channel.asked(), Vec::<String>::new());
    }

    /// Every mechanism in a jump fails silently, so a jump nobody confirmed is
    /// tried once more and then said out loud rather than reported as success.
    #[test]
    fn a_jump_nothing_confirms_is_tried_once_more_and_then_reported() {
        let harness = jumping(Some("revealed"), vec![], &[]);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Unconfirmed);
        assert_eq!(harness.channel.asked().len(), 2);
        assert_eq!(harness.raiser.raised().len(), 2);
    }

    /// A jump retries, so the popover is still up and still clickable while
    /// one is in flight — and a second row clicked there is a second jump,
    /// running beside the first. The one that finishes first must not take the
    /// other's confirmation with it: the window the first click asked for did
    /// come forward, and telling the user it did not is the whole failure this
    /// module exists to avoid.
    #[test]
    fn a_jump_that_finishes_leaves_one_still_in_flight_waiting_for_its_own_window() {
        let (harness, raises) = stepping(2);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));
        harness
            .windows
            .register(window(17_804, &["/Users/eric/Code/okven"]));
        let clicked = target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        );
        let clicked_next = second_target(
            Some(17_804),
            Some("Ship the release"),
            Some("/Users/eric/Code/okven"),
        );

        thread::scope(|jumps| {
            let jumping = jumps.spawn(|| harness.jumper.jump(&clicked));
            let raising = raises
                .recv_timeout(STEP_WITHIN)
                .expect("the first jump to reach its raise");
            let jumping_next = jumps.spawn(|| harness.jumper.jump(&clicked_next));
            let raising_next = raises
                .recv_timeout(STEP_WITHIN)
                .expect("the second jump to reach its raise");

            raising_next.reports(Some(reported(17_804, true, Some("Ship the release"))));
            assert_eq!(
                jumping_next.join().expect("the second jump"),
                JumpOutcome::Jumped
            );

            raising.reports(Some(reported(65_921, true, Some("Fix the auth flow"))));
            assert_eq!(jumping.join().expect("the first jump"), JumpOutcome::Jumped);
        });

        // One raise each: a jump whose confirmation was taken from it would
        // have gone round again.
        assert_eq!(harness.raiser.raised().len(), 2);
    }

    /// The same row clicked twice before the first click answered. Both asked
    /// for exactly one thing — that window in front with that tab — and the
    /// window coming forward once is the answer to both, so neither is left
    /// waiting on a second report that is never coming.
    #[test]
    fn a_row_clicked_twice_while_the_first_click_waits_has_both_clicks_answered() {
        let (harness, raises) = stepping(2);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));
        let clicked = target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        );

        thread::scope(|jumps| {
            let jumping = jumps.spawn(|| harness.jumper.jump(&clicked));
            let raising = raises
                .recv_timeout(STEP_WITHIN)
                .expect("the first click to reach its raise");
            let jumping_again = jumps.spawn(|| harness.jumper.jump(&clicked));
            let raising_again = raises
                .recv_timeout(STEP_WITHIN)
                .expect("the second click to reach its raise");

            raising_again.reports(Some(reported(65_921, true, Some("Fix the auth flow"))));
            assert_eq!(
                jumping_again.join().expect("the second click"),
                JumpOutcome::Jumped
            );

            // The window is already in front with that tab showing, so it has
            // nothing further to report — and the report it did make is the
            // one the first click was waiting for.
            raising.reports(None);
            assert_eq!(
                jumping.join().expect("the first click"),
                JumpOutcome::Jumped
            );
        });

        assert_eq!(harness.raiser.raised().len(), 2);
    }

    /// The second attempt is what a busy editor gets, and it is enough.
    #[test]
    fn a_jump_confirmed_on_the_second_attempt_is_a_jump() {
        let harness = jumping(
            Some("revealed"),
            vec![
                None,
                Some(reported(65_921, true, Some("Fix the auth flow"))),
            ],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(harness.raiser.raised().len(), 2);
    }

    /// The most damaging thing this app can do, and the one outcome that must
    /// never be attempted twice.
    #[test]
    fn a_reveal_that_opened_a_tab_is_reported_and_never_retried() {
        let harness = jumping(Some("duplicated"), vec![], &[]);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Duplicated);
        assert_eq!(harness.channel.asked().len(), 1);
        assert_eq!(harness.raiser.raised(), Vec::<PathBuf>::new());
    }

    /// The window's own guard said the session's tab is not there. That is not
    /// a failure to jump: the window still comes forward, and the tab it has in
    /// front is whatever it already had.
    #[test]
    fn a_refused_reveal_still_raises_and_waits_only_for_the_window() {
        let harness = jumping(
            Some("notPresent"),
            vec![Some(reported(65_921, true, Some("Some other session")))],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(harness.raiser.raised().len(), 1);
    }

    /// A window in front showing something else is not the tab that was asked
    /// for, so the jump has not happened yet.
    #[test]
    fn a_window_in_front_showing_another_tab_does_not_confirm_a_revealed_one() {
        let harness = jumping(
            Some("revealed"),
            vec![
                Some(reported(65_921, true, Some("Some other session"))),
                Some(reported(65_921, true, Some("Some other session"))),
            ],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Unconfirmed);
    }

    #[test]
    fn a_report_from_another_window_confirms_nothing() {
        let harness = jumping(
            Some("revealed"),
            vec![
                Some(reported(17_804, true, Some("Fix the auth flow"))),
                Some(reported(17_804, true, Some("Fix the auth flow"))),
            ],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));
        harness
            .windows
            .register(window(17_804, &["/Users/eric/Code/okven"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Unconfirmed);
    }

    /// A window can report its tab without having come forward at all, which
    /// is the background-window case the whole app is built around.
    #[test]
    fn a_window_that_is_not_in_front_confirms_nothing() {
        let harness = jumping(
            Some("revealed"),
            vec![
                Some(reported(65_921, false, Some("Fix the auth flow"))),
                Some(reported(65_921, false, Some("Fix the auth flow"))),
            ],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Unconfirmed);
    }

    /// The window closed between the scan that placed the session and the
    /// click. The command channel drops it on the spot, and a window that has
    /// gone confirms nothing — so the raise that did run is the answer, rather
    /// than a failure reported over a window that came forward anyway.
    #[test]
    fn a_window_that_has_gone_by_the_time_it_is_asked_leaves_the_raise_as_the_answer() {
        let harness = jumping(Some(UNREACHABLE), vec![], &[]);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness.jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::Raised);
        assert_eq!(harness.raiser.raised().len(), 1);
    }

    /// The degraded path: no extension anywhere, and the click still brings a
    /// window forward.
    #[test]
    fn a_folder_named_by_a_lock_file_raises_a_window_with_no_extension_at_all() {
        let harness = jumping(None, vec![], &["/Users/eric/Code/okven"]);

        let outcome = harness
            .jumper
            .jump(&target(None, None, Some("/Users/eric/Code/okven/app")));

        assert_eq!(outcome, JumpOutcome::Raised);
        assert_eq!(
            harness.raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/okven")]
        );
    }

    /// `code` on a folder no window has open opens a new one, so a session
    /// neither source can place must not run it at all.
    #[test]
    fn a_folder_in_neither_source_runs_no_code_at_all() {
        let harness = jumping(None, vec![], &["/Users/eric/Code/okven"]);

        let outcome = harness
            .jumper
            .jump(&target(None, None, Some("/Users/eric/Code/aydy")));

        assert_eq!(outcome, JumpOutcome::NoWindow);
        assert_eq!(harness.raiser.raised(), Vec::<PathBuf>::new());
        assert_eq!(harness.channel.asked(), Vec::<String>::new());
    }

    /// With nothing to bring the window forward with, revealing a tab would
    /// switch what the user sees in a window they are not looking at and then
    /// leave it there.
    #[test]
    fn a_machine_with_no_code_command_line_goes_nowhere_and_reveals_nothing() {
        let watch = Arc::new(JumpWatch::new());
        let channel = FakeWindow::answering(Some("revealed"));
        let raiser = FakeRaiser::missing(&watch);
        let windows = Arc::new(ExtensionRegistry::new(Arc::new(TestClock::new())));
        let extensions = Arc::new(ExtensionCommands::new(
            Arc::clone(&windows),
            Arc::clone(&channel) as Arc<dyn ExtensionChannel>,
        ));
        windows.register(window(65_921, &["/Users/eric/Code/aydy"]));
        let jumper = Jumper::new(
            extensions,
            raiser as Arc<dyn WindowRaiser>,
            FakeLocks::holding(&[]) as Arc<dyn WindowFolders>,
            watch,
        )
        .confirming_within(CONFIRM_WITHIN);

        let outcome = jumper.jump(&target(
            Some(65_921),
            Some("Fix the auth flow"),
            Some("/Users/eric/Code/aydy"),
        ));

        assert_eq!(outcome, JumpOutcome::NoWindow);
        assert_eq!(channel.asked(), Vec::<String>::new());
    }

    /// AYDY holds no title for this session, so it has nothing for the window
    /// to check a tab against. The window still comes forward.
    #[test]
    fn a_session_with_no_title_is_raised_without_a_reveal() {
        let harness = jumping(
            Some("revealed"),
            vec![Some(reported(65_921, true, Some("Something else")))],
            &[],
        );
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome =
            harness
                .jumper
                .jump(&target(Some(65_921), None, Some("/Users/eric/Code/aydy")));

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(harness.channel.asked(), Vec::<String>::new());
    }

    /// The session's own directory is outside every folder the window has
    /// open, which is ordinary — a session started in a subdirectory of
    /// nothing. The window is still known, so it is still raised.
    #[test]
    fn the_owning_windows_own_folder_is_raised_when_the_session_sits_outside_it() {
        let harness = jumping(None, vec![Some(reported(65_921, true, None))], &[]);
        harness
            .windows
            .register(window(65_921, &["/Users/eric/Code/aydy"]));

        let outcome = harness
            .jumper
            .jump(&target(Some(65_921), None, Some("/tmp/scratch")));

        assert_eq!(outcome, JumpOutcome::Jumped);
        assert_eq!(
            harness.raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
    }

    #[test]
    fn the_closest_folder_holding_the_session_is_the_one_raised() {
        let harness = jumping(None, vec![], &[]);
        harness.windows.register(window(
            65_921,
            &["/Users/eric/Code", "/Users/eric/Code/okven"],
        ));

        let outcome = harness.jumper.jump(&target(
            None,
            None,
            Some("/Users/eric/Code/okven/app/models"),
        ));

        assert_eq!(outcome, JumpOutcome::Raised);
        assert_eq!(
            harness.raiser.raised(),
            vec![PathBuf::from("/Users/eric/Code/okven")]
        );
    }

    #[test]
    fn a_session_that_cannot_be_jumped_to_has_nothing_to_jump_to() {
        let mut session = Session::new("s1".to_string(), Instant::now());
        session.jumpable = false;
        session.window = Some(65_921);

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

        assert_eq!(target.window, None);
        assert_eq!(target.cwd, None);
        assert_eq!(session.status(), SessionStatus::Unknown);
    }

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!(
            "aydy-ide-{}-{}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// The real reader against real files, because the shape being parsed is
    /// undocumented Claude Code internals rather than something AYDY writes.
    #[test]
    fn lock_files_name_the_folders_their_windows_have_open() {
        let directory = scratch();
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        std::fs::write(
            directory.join("22720.lock"),
            r#"{"pid":89576,"workspaceFolders":["/Users/eric/Code/aydy"],"ideName":"Visual Studio Code"}"#,
        )
        .expect("a lock file");
        std::fs::write(
            directory.join("31455.lock"),
            r#"{"pid":89576,"workspaceFolders":["/Users/eric/Code/okven"]}"#,
        )
        .expect("a lock file");
        std::fs::write(directory.join("notes.txt"), "not a lock file").expect("a stray file");
        std::fs::write(directory.join("broken.lock"), "{").expect("a broken lock file");

        let mut folders = IdeLocks::new(Some(directory.clone())).folders();
        folders.sort();

        std::fs::remove_dir_all(&directory).ok();

        assert_eq!(
            folders,
            vec![
                PathBuf::from("/Users/eric/Code/aydy"),
                PathBuf::from("/Users/eric/Code/okven")
            ]
        );
    }

    /// Undocumented internals that may not be there at all. Nothing to read
    /// means nothing is confirmed, never "no windows are open".
    #[test]
    fn a_lock_directory_that_is_not_there_confirms_no_windows() {
        assert_eq!(
            IdeLocks::new(Some(scratch())).folders(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(IdeLocks::new(None).folders(), Vec::<PathBuf>::new());
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
