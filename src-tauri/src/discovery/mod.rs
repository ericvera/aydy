//! Discovering sessions from the outside, so the list heals itself.
//!
//! Hooks report transitions, but they cannot report what happened while AYDY
//! was not running, and a session that crashes never sends `SessionEnd`. The
//! second source is `~/.claude/sessions/<pid>.json`: one file per live Claude
//! process, cross-referenced against the process table. [`status_files`]
//! reads another: the per-session status and seen files under AYDY's data
//! directory, written by the hook script and the companion extension.
//!
//! Two rules hold this module together.
//!
//! **Only what the scan can see may be removed.** A session is positively gone
//! when it is one this scan can see — a session AYDY has already observed in a
//! local session file — and its process is no longer alive. Everything the scan
//! cannot see at all is unverified, not dead: Claude desktop sessions, remote
//! sessions and anything on a working directory that is not on this machine
//! never appear in a scan, and removing them for being unverifiable would empty
//! exactly the part of the list the product exists to show.
//!
//! **The repository name is not the last component of the working directory.**
//! A session in a linked worktree sits in a directory named after the branch,
//! not after the repository, and its `.git` is a file pointing at the real
//! repository. [`repo`] resolves that pointer.
//!
//! The data source is undocumented Claude Code internals — the `peerProtocol`
//! field suggests Anthropic versions it, but there is no contract — so a file
//! whose shape is not recognised is skipped and a directory that is missing
//! degrades to hook-only tracking. Nothing here is allowed to be fatal.

pub mod file_feed;
pub mod label;
pub mod reconciler;
pub mod repo;
pub mod resume;
pub mod session_files;
pub mod status_files;
pub mod transcript;

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SCRATCH: AtomicU32 = AtomicU32::new(0);

    /// A `~/.claude/sessions/<pid>.json` file exactly as this machine writes
    /// it, so the parser is measured against the real shape rather than
    /// against one written to suit it.
    pub const REAL_SESSION_FILE: &str = r#"{
  "pid": 37648,
  "sessionId": "9026fc28-2bc7-47cb-a4bd-d20882da64f2",
  "cwd": "/Users/eric/Code/aydy",
  "startedAt": 1785526308250,
  "procStart": "Fri Jul 31 19:31:44 2026",
  "version": "2.1.220",
  "peerProtocol": 1,
  "kind": "interactive",
  "entrypoint": "claude-vscode",
  "name": "aydy-83",
  "nameSource": "derived"
}
"#;

    /// A directory of this test's own, shared with no other test, so no test
    /// ever reads the developer's real `~/.claude`.
    pub fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aydy-discovery-{}-{name}-{}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the scratch directory to be creatable");

        dir
    }

    /// A sessions directory holding one file per `(pid, contents)` pair.
    pub fn sessions_dir(name: &str, files: &[(u32, &str)]) -> PathBuf {
        let dir = scratch_dir(name).join("sessions");
        fs::create_dir_all(&dir).expect("the sessions directory to be creatable");

        for (pid, contents) in files {
            fs::write(dir.join(format!("{pid}.json")), contents)
                .expect("the session file to be writable");
        }

        dir
    }

    /// The session file this machine writes for a session.
    pub fn session_file(pid: u32, session_id: &str, cwd: &Path, entrypoint: &str) -> String {
        format!(
            r#"{{
  "pid": {pid},
  "sessionId": "{session_id}",
  "cwd": "{}",
  "startedAt": 1785526308250,
  "version": "2.1.220",
  "peerProtocol": 1,
  "kind": "interactive",
  "entrypoint": "{entrypoint}",
  "name": "{session_id}",
  "nameSource": "derived"
}}
"#,
            cwd.display()
        )
    }

    /// A normal checkout at `<dir>/<name>`, whose `.git` is a directory.
    pub fn checkout(dir: &Path, name: &str) -> PathBuf {
        let root = dir.join(name);
        fs::create_dir_all(root.join(".git")).expect("the checkout to be creatable");

        root
    }

    /// A projects directory holding one transcript per
    /// `(cwd, session id, contents)` triple, named the way Claude Code names
    /// them.
    pub fn projects_dir(name: &str, transcripts: &[(&Path, &str, &str)]) -> PathBuf {
        let dir = scratch_dir(name).join("projects");
        fs::create_dir_all(&dir).expect("the projects directory to be creatable");

        for (cwd, session_id, contents) in transcripts {
            let project = dir.join(crate::discovery::transcript::project_slug(cwd));
            fs::create_dir_all(&project).expect("the project directory to be creatable");
            fs::write(project.join(format!("{session_id}.jsonl")), contents)
                .expect("the transcript to be writable");
        }

        dir
    }

    /// A transcript whose last turn completed, as a session sitting at Done
    /// leaves one.
    pub const COMPLETED_TRANSCRIPT: &str = concat!(
        r#"{"type":"user","gitBranch":"eric-attribution","message":{"role":"user","content":"go"}}"#,
        "\n",
        r#"{"type":"assistant","gitBranch":"eric-attribution","message":{"role":"assistant","stop_reason":"end_turn"}}"#,
        "\n",
        r#"{"type":"ai-title","aiTitle":"Review attribution mock data test file"}"#,
        "\n",
    );

    /// A transcript whose last turn is still running.
    pub const WORKING_TRANSCRIPT: &str = concat!(
        r#"{"type":"assistant","gitBranch":"main","message":{"role":"assistant","stop_reason":"end_turn"}}"#,
        "\n",
        r#"{"type":"user","gitBranch":"main","message":{"role":"user","content":"and again"}}"#,
        "\n",
    );

    /// A transcript for a session that has started and has never been prompted.
    pub const NEVER_PROMPTED_TRANSCRIPT: &str = concat!(
        r#"{"type":"queue-operation","operation":"add"}"#,
        "\n",
        r#"{"type":"system","content":"session started"}"#,
        "\n",
    );

    /// A linked worktree of `root` at `<dir>/<name>`, whose `.git` is a **file**
    /// pointing back at the repository, the way git actually writes one.
    pub fn worktree(dir: &Path, root: &Path, name: &str) -> PathBuf {
        let gitdir = root.join(".git").join("worktrees").join(name);
        fs::create_dir_all(&gitdir).expect("the worktree's git directory to be creatable");

        let linked = dir.join(name);
        fs::create_dir_all(&linked).expect("the worktree to be creatable");
        fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .expect("the worktree's git file to be writable");

        linked
    }
}
