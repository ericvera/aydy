//! Reading `~/.claude/sessions/<pid>.json` and checking the processes behind
//! those files are still alive.
//!
//! The files are undocumented Claude Code internals, so every field is optional
//! here, unknown fields are ignored, and a file that cannot be recognised is
//! skipped rather than failing the scan.
//!
//! **A file is not evidence that its session is running.** Whether Claude Code
//! reliably deletes these files on exit was never established, and a session
//! killed outright cannot delete anything, so the process id is checked against
//! the process table before a file is believed. Trusting the file alone
//! produces phantom sessions that never leave the list.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The entrypoint a session running inside VS Code reports.
pub const VS_CODE_ENTRYPOINT: &str = "claude-vscode";

/// The extension every session file carries.
const SESSION_FILE_EXTENSION: &str = "json";

/// One session file, reduced to the fields AYDY uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    /// The Claude process the file describes.
    pub pid: u32,
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    /// The client the session belongs to, `claude-vscode` for VS Code.
    pub entrypoint: Option<String>,
}

impl SessionFile {
    /// Whether this session can be jumped to.
    pub fn is_jumpable(&self) -> bool {
        is_jumpable(self.entrypoint.as_deref(), self.cwd.as_deref())
    }
}

/// Everything one pass over the sessions directory found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFiles {
    /// The files that were recognised.
    pub sessions: Vec<SessionFile>,
    /// How many files could not be read or recognised. A release that changes
    /// the shape of these files shows up here rather than as a crash.
    pub unrecognised: usize,
}

/// The shape read off disk. Every field is optional so that a file missing one
/// of them still parses and can be judged, and unknown fields are ignored so a
/// field added by a later Claude Code release is not an error.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSessionFile {
    session_id: Option<String>,
    pid: Option<u32>,
    cwd: Option<PathBuf>,
    entrypoint: Option<String>,
}

/// Reads every session file in `dir`.
///
/// A directory that is missing or unreadable is an error for the caller to
/// degrade on; a single unreadable or unrecognised file inside it is not.
pub fn read_all(dir: &Path) -> io::Result<SessionFiles> {
    let mut files = SessionFiles::default();

    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else {
            files.unrecognised += 1;
            continue;
        };
        let path = entry.path();

        if path.extension().and_then(|extension| extension.to_str()) != Some(SESSION_FILE_EXTENSION)
        {
            continue;
        }

        match fs::read_to_string(&path)
            .ok()
            .and_then(|contents| parse(&contents, &path))
        {
            Some(session) => files.sessions.push(session),
            None => files.unrecognised += 1,
        }
    }

    Ok(files)
}

/// Reads one session file's `contents`, using `path` for the process id when
/// the document does not carry one.
///
/// `None` means the file is not a session file AYDY recognises: it is skipped,
/// never fatal.
pub fn parse(contents: &str, path: &Path) -> Option<SessionFile> {
    let raw: RawSessionFile = serde_json::from_str(contents).ok()?;
    let session_id = raw.session_id.filter(|id| !id.is_empty())?;
    let pid = raw.pid.or_else(|| pid_from_name(path))?;

    Some(SessionFile {
        pid,
        session_id,
        cwd: raw.cwd.filter(|cwd| !cwd.as_os_str().is_empty()),
        entrypoint: raw.entrypoint.filter(|entrypoint| !entrypoint.is_empty()),
    })
}

/// The process id in a `<pid>.json` file name.
fn pid_from_name(path: &Path) -> Option<u32> {
    path.file_stem()?.to_str()?.parse().ok()
}

/// Whether a session belongs to VS Code and sits on this machine, which is what
/// makes it jumpable.
///
/// This is deliberately independent of the companion extension. Installing or
/// removing the extension must never move a session from one group to another,
/// so nothing the extension reports takes part in the decision.
pub fn is_jumpable(entrypoint: Option<&str>, cwd: Option<&Path>) -> bool {
    entrypoint == Some(VS_CODE_ENTRYPOINT) && cwd.is_some_and(is_local)
}

/// Whether `cwd` names a directory on this machine.
pub fn is_local(cwd: &Path) -> bool {
    cwd.is_dir()
}

/// Which process ids are alive.
///
/// Injected rather than called directly so the scan can be tested against a
/// process table the test controls.
pub trait ProcessTable: Send + Sync {
    /// Whether a process with `pid` is currently running.
    fn is_alive(&self, pid: u32) -> bool;
}

/// The process table of this machine.
pub struct LiveProcesses;

impl ProcessTable for LiveProcesses {
    /// Signal 0 performs the error checking of a signal without sending one,
    /// which is the cheapest existence check there is.
    ///
    /// A process owned by another user answers `EPERM`, and that is still a
    /// live process. Process id 0 is never asked about: it addresses the whole
    /// process group rather than one process.
    fn is_alive(&self, pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };

        if pid <= 0 {
            return false;
        }

        // SAFETY: `kill` with signal 0 sends nothing and only reads the
        // process table.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }

        io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

/// A process table holding a fixed set of live process ids.
pub struct FixedProcesses {
    alive: HashSet<u32>,
}

impl FixedProcesses {
    /// A table reporting exactly `alive` as running.
    pub fn new(alive: impl IntoIterator<Item = u32>) -> Self {
        Self {
            alive: alive.into_iter().collect(),
        }
    }
}

impl ProcessTable for FixedProcesses {
    fn is_alive(&self, pid: u32) -> bool {
        self.alive.contains(&pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::test_support::{scratch_dir, sessions_dir, REAL_SESSION_FILE};

    #[test]
    fn the_session_file_this_machine_writes_is_parsed() {
        let session = parse(REAL_SESSION_FILE, Path::new("/tmp/sessions/37648.json"))
            .expect("the real session file shape to be recognised");

        assert_eq!(session.pid, 37648);
        assert_eq!(session.session_id, "9026fc28-2bc7-47cb-a4bd-d20882da64f2");
        assert_eq!(session.cwd, Some(PathBuf::from("/Users/eric/Code/aydy")));
        assert_eq!(session.entrypoint, Some(VS_CODE_ENTRYPOINT.to_string()));
    }

    #[test]
    fn a_field_a_later_release_adds_is_ignored() {
        let contents = r#"{
          "pid": 42,
          "sessionId": "session-1",
          "cwd": "/Users/eric/Code/aydy",
          "peerProtocol": 2,
          "somethingNew": { "nested": true }
        }"#;

        let session = parse(contents, Path::new("/tmp/sessions/42.json"))
            .expect("an unknown field to be ignored rather than fatal");

        assert_eq!(session.session_id, "session-1");
    }

    #[test]
    fn a_file_that_is_not_json_is_not_recognised() {
        assert_eq!(
            parse("not json at all", Path::new("/tmp/sessions/42.json")),
            None
        );
    }

    #[test]
    fn a_file_with_no_session_id_is_not_recognised() {
        let contents = r#"{ "pid": 42, "cwd": "/Users/eric/Code/aydy" }"#;

        assert_eq!(parse(contents, Path::new("/tmp/sessions/42.json")), None);
    }

    #[test]
    fn a_file_whose_session_id_is_the_wrong_type_is_not_recognised() {
        let contents = r#"{ "pid": 42, "sessionId": 7 }"#;

        assert_eq!(parse(contents, Path::new("/tmp/sessions/42.json")), None);
    }

    #[test]
    fn the_process_id_falls_back_to_the_file_name() {
        let contents = r#"{ "sessionId": "session-1" }"#;

        let session = parse(contents, Path::new("/tmp/sessions/1234.json"))
            .expect("the file name to supply the process id");

        assert_eq!(session.pid, 1234);
    }

    #[test]
    fn a_file_with_no_process_id_anywhere_is_not_recognised() {
        let contents = r#"{ "sessionId": "session-1" }"#;

        assert_eq!(
            parse(contents, Path::new("/tmp/sessions/current.json")),
            None
        );
    }

    #[test]
    fn every_recognised_file_in_the_directory_is_read() {
        let dir = sessions_dir(
            "read-all",
            &[
                (11, r#"{ "pid": 11, "sessionId": "session-a" }"#),
                (22, r#"{ "pid": 22, "sessionId": "session-b" }"#),
            ],
        );

        let files = read_all(&dir).expect("the directory to be readable");
        let mut ids: Vec<&str> = files
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        ids.sort_unstable();

        assert_eq!(ids, ["session-a", "session-b"]);
        assert_eq!(files.unrecognised, 0);
    }

    #[test]
    fn an_unrecognised_file_is_counted_and_the_rest_are_still_read() {
        let dir = sessions_dir(
            "unrecognised",
            &[
                (11, "{ this is not json"),
                (22, r#"{ "pid": 22, "sessionId": "session-b" }"#),
            ],
        );

        let files = read_all(&dir).expect("the directory to be readable");

        assert_eq!(files.sessions.len(), 1);
        assert_eq!(files.unrecognised, 1);
    }

    #[test]
    fn a_missing_directory_is_reported_as_an_error() {
        let dir = scratch_dir("missing").join("sessions");

        assert!(read_all(&dir).is_err());
    }

    #[test]
    fn a_vs_code_session_on_this_machine_is_jumpable() {
        let cwd = scratch_dir("jumpable");

        assert!(is_jumpable(Some(VS_CODE_ENTRYPOINT), Some(&cwd)));
    }

    #[test]
    fn a_session_from_another_client_is_not_jumpable() {
        let cwd = scratch_dir("other-client");

        assert!(!is_jumpable(Some("claude-desktop"), Some(&cwd)));
        assert!(!is_jumpable(None, Some(&cwd)));
    }

    #[test]
    fn a_vs_code_session_whose_directory_is_not_on_this_machine_is_not_jumpable() {
        let cwd = scratch_dir("not-local").join("elsewhere");

        assert!(!is_jumpable(Some(VS_CODE_ENTRYPOINT), Some(&cwd)));
        assert!(!is_jumpable(Some(VS_CODE_ENTRYPOINT), None));
    }

    #[test]
    fn this_process_is_alive_and_an_impossible_one_is_not() {
        let processes = LiveProcesses;

        assert!(processes.is_alive(std::process::id()));
        assert!(!processes.is_alive(0));
        assert!(!processes.is_alive(u32::MAX));
    }
}
