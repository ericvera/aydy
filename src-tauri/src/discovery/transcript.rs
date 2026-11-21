//! Reading a session's transcript for its title, its branch, and the status its
//! terminal entries imply.
//!
//! Transcripts are line-delimited JSON at
//! `~/.claude/projects/<slug>/<session_id>.jsonl` and grow to megabytes, so they
//! are read backwards from the end in windows and cached per file, invalidated
//! on size or mtime. Reading whole files on a scan that runs every 25 seconds
//! would be the app's dominant cost.
//!
//! **Unknown means cannot determine, not nothing has happened yet.** A session
//! that has started and never been prompted is Done, and collapsing that into
//! Unknown puts every freshly opened session into the eviction loop
//! reconciliation's resolution window exists to end.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::session::status::SessionStatus;

/// The directory holding one subdirectory of transcripts per working directory.
const PROJECTS_DIR: &str = "projects";

/// The directory Claude Code keeps its configuration in, unless
/// `CLAUDE_CONFIG_DIR` says otherwise.
const CONFIG_DIR: &str = ".claude";

/// The environment variable Claude Code reads its configuration directory from.
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// The extension every transcript carries.
const TRANSCRIPT_EXTENSION: &str = "jsonl";

/// The `gitBranch` value a session outside a repository reports. It is not a
/// branch name, and treating it as one puts a group called `HEAD` in the list.
const DETACHED_BRANCH: &str = "HEAD";

/// The tail windows tried in turn, largest last.
///
/// The first covers the end of every transcript seen on this machine. The
/// larger ones exist for the pathological case — a turn whose last tool result
/// was itself hundreds of kilobytes — and are only ever read when the smaller
/// window answered nothing.
const WINDOWS: [u64; 3] = [32 * 1024, 256 * 1024, 1024 * 1024];

/// The largest transcript this app will fall back to reading in full.
///
/// No window can answer a transcript whose **last** entry is larger than the
/// window itself: the window then lands inside that entry and holds no complete
/// line at all. That is not hypothetical — the largest single entry among the
/// 1470 transcripts on this machine is 1 098 528 bytes, already past the top
/// window, and two entries exceed it. Such a transcript reads as Unknown, and a
/// session the scan keeps confirming then settles at Done when its resolution
/// window runs out, while it is in fact working: a wrong row rather than a
/// missing one, which is the worse of the two failures.
///
/// So when the windows leave the **status** unanswered — the only fact with a
/// wrong answer rather than a fallback behind it — the whole file is read, up to
/// this ceiling. The ceiling is what keeps a pathological transcript from
/// turning a 25-second scan into an unbounded read; the largest transcript on
/// this machine is 19 MiB, so it is not a limit real data reaches. Past it, the
/// status stays Unknown, which is the honest answer for a file this app has
/// declined to read.
const MAX_WHOLE_READ: u64 = 32 * 1024 * 1024;

/// What a transcript says about its session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptFacts {
    /// The title Claude Code generated for the session, newest first.
    pub title: Option<String>,
    /// The branch the session is on, when it is on one.
    pub branch: Option<String>,
    /// The status the terminal entries imply, or `None` when the transcript is
    /// missing, unreadable, or in a shape this app does not recognise.
    pub status: Option<SessionStatus>,
}

/// Where this machine's transcripts live, or `None` when there is no home
/// directory to look in.
pub fn user_projects_dir() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os(CONFIG_DIR_VARIABLE) {
        if !configured.is_empty() {
            return Some(PathBuf::from(configured).join(PROJECTS_DIR));
        }
    }

    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;

    Some(PathBuf::from(home).join(CONFIG_DIR).join(PROJECTS_DIR))
}

/// The directory name Claude Code gives a working directory's transcripts.
///
/// Every character outside `[A-Za-z0-9]` becomes a dash, which is why
/// `/Users/eric/Code/okven.worktrees/eric-fab-snippet` and
/// `/Users/eric/.sculptor/workspaces/…` both round-trip to the names actually
/// on disk.
pub fn project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// Finds a session's transcript.
///
/// Hook payloads carry the path, but a session reconciliation discovers after a
/// restart has no hook behind it yet, and its status is exactly what the
/// transcript is needed for. The slug is derived from the working directory
/// first; when that misses — an undocumented naming rule is not a contract —
/// the projects directory is searched for the session's own file.
pub fn locate(projects_dir: &Path, cwd: Option<&Path>, session_id: &str) -> Option<PathBuf> {
    let file = format!("{session_id}.{TRANSCRIPT_EXTENSION}");

    if let Some(cwd) = cwd {
        let direct = projects_dir.join(project_slug(cwd)).join(&file);

        if direct.is_file() {
            return Some(direct);
        }
    }

    fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(&file))
        .find(|candidate| candidate.is_file())
}

/// Transcript facts, remembered per file until the file changes.
#[derive(Debug, Default)]
pub struct TranscriptCache {
    entries: HashMap<PathBuf, (Stamp, TranscriptFacts)>,
}

/// What identifies a version of a transcript. Transcripts are only ever
/// appended to, so size alone would nearly always do; the modification time is
/// carried as well so a rewrite of the same length is not mistaken for no
/// change at all.
///
/// It is the one definition of "this file has moved on", shared by the cache
/// and by the resume watcher, which is watching for exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}

impl TranscriptCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The facts `path` currently reports, reading the file only when it has
    /// changed since the last read.
    pub fn read(&mut self, path: &Path) -> TranscriptFacts {
        let Some(stamp) = stamp(path) else {
            self.entries.remove(path);

            return TranscriptFacts::default();
        };

        if let Some((cached, facts)) = self.entries.get(path) {
            if cached == &stamp {
                return facts.clone();
            }
        }

        let facts = read(path);
        self.entries
            .insert(path.to_path_buf(), (stamp, facts.clone()));

        facts
    }

    /// Forgets every transcript outside `keep`, so the cache tracks the sessions
    /// that exist rather than every session that ever existed.
    pub fn retain(&mut self, keep: &HashSet<PathBuf>) {
        self.entries.retain(|path, _| keep.contains(path));
    }

    /// How many transcripts are currently remembered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is currently remembered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What `path` currently looks like, or `None` when it is not a file that can
/// be looked at.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;

    if !metadata.is_file() {
        return None;
    }

    Some(Stamp {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// Reads `path` from the end, widening the window until every fact it can
/// answer is answered or the whole file has been read.
pub fn read(path: &Path) -> TranscriptFacts {
    read_within(path, MAX_WHOLE_READ)
}

/// [`read`], with the whole-file ceiling supplied.
///
/// The ceiling is only ever reached by a transcript larger than the top window,
/// so a test that pins it would otherwise have to write a file the size of the
/// real [`MAX_WHOLE_READ`].
fn read_within(path: &Path, ceiling: u64) -> TranscriptFacts {
    let Some(stamp) = stamp(path) else {
        return TranscriptFacts::default();
    };

    let mut facts = TranscriptFacts::default();

    for window in WINDOWS {
        let whole = window >= stamp.size;
        let Some(text) = tail(path, window) else {
            return TranscriptFacts::default();
        };

        facts = interpret(&text, whole);

        if whole || (facts.status.is_some() && facts.title.is_some() && facts.branch.is_some()) {
            return facts;
        }
    }

    // Every window landed inside a single entry larger than itself, so none of
    // them held a complete line. Only the status is worth reading the whole file
    // for: the title and the branch have a fallback behind them, while an
    // unanswered status settles a working session at Done.
    if facts.status.is_none() && stamp.size <= ceiling {
        if let Some(text) = tail(path, stamp.size) {
            facts = interpret(&text, true);
        }
    }

    facts
}

/// The last `window` bytes of `path`, dropping a leading partial line when the
/// window does not reach the start of the file.
fn tail(path: &Path, window: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let from = size.saturating_sub(window);

    file.seek(SeekFrom::Start(from)).ok()?;

    let mut bytes = Vec::new();
    file.take(window).read_to_end(&mut bytes).ok()?;

    let text = String::from_utf8_lossy(&bytes).into_owned();

    if from == 0 {
        return Some(text);
    }

    // The window landed mid-line; that line is already accounted for by the
    // next window, and a half-line is not a transcript entry.
    Some(match text.find('\n') {
        Some(newline) => text[newline + 1..].to_string(),
        None => String::new(),
    })
}

/// What an entry says about whether the session's turn is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meaning {
    /// The turn is still open — Claude owes a reply.
    TurnOpen,
    /// The turn has completed.
    TurnComplete,
    /// The entry says nothing about status. `ai-title`,
    /// `file-history-snapshot`, `queue-operation`, `last-prompt`, `mode` and
    /// `attachment` entries all trail a turn without ending it, so the literal
    /// last line of a transcript is regularly one of them.
    None,
}

/// The stop reason an assistant entry carries while its turn continues: the
/// model asked for a tool and is waiting for the result.
const TOOL_USE: &str = "tool_use";

const USER_ENTRY: &str = "user";
const ASSISTANT_ENTRY: &str = "assistant";
const TITLE_ENTRY: &str = "ai-title";

/// What one entry says about the session's turn.
///
/// Entries are read as free-form JSON rather than into a typed shape, so a
/// field that arrives with an unexpected type costs that field rather than the
/// whole line. These files are undocumented Claude Code internals; parsing them
/// is total by design, exactly as session files are.
fn meaning(entry: &serde_json::Value, kind: &str) -> Meaning {
    // A subagent's turn is not the session's turn: a subagent finishing while
    // the session waits on its result must not read as the session finishing.
    if entry
        .get("isSidechain")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Meaning::None;
    }

    match kind {
        USER_ENTRY => Meaning::TurnOpen,
        ASSISTANT_ENTRY => {
            let stop_reason = entry
                .get("message")
                .and_then(|message| message.get("stop_reason"))
                .and_then(serde_json::Value::as_str);

            if stop_reason == Some(TOOL_USE) {
                Meaning::TurnOpen
            } else {
                Meaning::TurnComplete
            }
        }
        _ => Meaning::None,
    }
}

/// Derives the facts a window of transcript text carries.
///
/// `whole` says whether the window reaches the start of the file, which is what
/// separates the two cases that must never be collapsed: a transcript with no
/// `user` entry **anywhere** is a session that has started and never been
/// prompted, which is Done, while a window that simply has not reached one yet
/// is not evidence of anything.
fn interpret(text: &str, whole: bool) -> TranscriptFacts {
    let mut facts = TranscriptFacts::default();
    let mut recognised = false;

    for line in text.lines().rev() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(kind) = entry.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };

        recognised = true;

        if facts.title.is_none() && kind == TITLE_ENTRY {
            facts.title = entry
                .get("aiTitle")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_string);
        }

        if facts.branch.is_none() {
            facts.branch = entry
                .get("gitBranch")
                .and_then(serde_json::Value::as_str)
                .and_then(branch);
        }

        if facts.status.is_none() {
            facts.status = match meaning(&entry, kind) {
                Meaning::TurnOpen => Some(SessionStatus::Working),
                Meaning::TurnComplete => Some(SessionStatus::Done),
                Meaning::None => None,
            };
        }
    }

    // Every entry in the whole file, and not one of them a turn: the session
    // has started and is sitting at the prompt. A file with nothing in it at
    // all is a different fact — there is nothing there to recognise.
    if facts.status.is_none() && whole && recognised {
        facts.status = Some(SessionStatus::Done);
    }

    facts
}

/// The branch a `gitBranch` value names, or `None` when it names none.
fn branch(value: &str) -> Option<String> {
    let value = value.trim();

    if value.is_empty() || value == DETACHED_BRANCH {
        return None;
    }

    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;
    use crate::discovery::test_support::scratch_dir;

    /// The tail of a real transcript from this machine, verbatim apart from
    /// shortened message bodies: an assistant turn that ended, followed by the
    /// status-free entries Claude Code writes after one.
    const REAL_COMPLETED_TAIL: &str = r#"{"parentUuid":"7d1c","isSidechain":false,"userType":"external","cwd":"/Users/eric/Code/okven","sessionId":"s-1","version":"2.1.220","gitBranch":"eric-attribution","entrypoint":"claude-vscode","type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":"ok"}]},"uuid":"a1","timestamp":"2026-08-02T18:20:00.000Z","toolUseResult":{"stdout":"ok"}}
{"parentUuid":"a1","isSidechain":false,"userType":"external","cwd":"/Users/eric/Code/okven","sessionId":"s-1","version":"2.1.220","gitBranch":"eric-attribution","entrypoint":"claude-vscode","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-4","content":[{"type":"text","text":"Done."}],"stop_reason":"end_turn","usage":{"input_tokens":10}},"requestId":"req_1","type":"assistant","uuid":"a2","timestamp":"2026-08-02T18:20:04.000Z"}
{"type":"last-prompt","lastPrompt":"check the tests","leafUuid":"a2","sessionId":"s-1"}
{"type":"ai-title","aiTitle":"Review attribution mock data test file","sessionId":"s-1"}
"#;

    fn transcript(name: &str, contents: &str) -> PathBuf {
        let path = scratch_dir(name).join("s-1.jsonl");
        fs::write(&path, contents).expect("the transcript to be writable");

        path
    }

    fn line(kind: &str, extra: &str) -> String {
        format!(r#"{{"type":"{kind}"{extra}}}"#)
    }

    fn user_line(branch: &str) -> String {
        line(
            "user",
            &format!(r#","gitBranch":"{branch}","message":{{"role":"user","content":"hello"}}"#),
        )
    }

    fn assistant_line(stop_reason: &str) -> String {
        line(
            "assistant",
            &format!(r#","message":{{"role":"assistant","stop_reason":"{stop_reason}"}}"#),
        )
    }

    fn title_line(title: &str) -> String {
        line("ai-title", &format!(r#","aiTitle":"{title}""#))
    }

    /// At least `bytes` of entries that say nothing about the status, so a fact
    /// placed before them is genuinely out of a narrower window's reach rather
    /// than merely further back.
    fn padding(extra: &str, bytes: usize) -> String {
        let entry = line(
            "file-history-snapshot",
            &format!(r#"{extra},"snapshot":"{}""#, "x".repeat(4096)),
        );
        let count = bytes.div_ceil(entry.len() + 1);

        vec![entry; count].join("\n")
    }

    /// A transcript whose last entry is larger than the top window, the shape
    /// two real entries on this machine already have.
    fn transcript_with_a_giant_last_entry() -> String {
        let huge = line(
            "user",
            &format!(
                r#","gitBranch":"main","message":{{"role":"user","content":[{{"type":"tool_result","content":"{}"}}]}}"#,
                "x".repeat(WINDOWS[2] as usize + 64 * 1024)
            ),
        );

        [assistant_line("end_turn"), huge].join("\n")
    }

    #[test]
    fn the_last_ai_title_is_the_one_that_wins() {
        let path = transcript(
            "titles",
            &[
                title_line("An early guess"),
                assistant_line("end_turn"),
                title_line("The current title"),
            ]
            .join("\n"),
        );

        assert_eq!(read(&path).title, Some("The current title".to_string()));
    }

    #[test]
    fn a_transcript_with_no_ai_title_yields_no_title() {
        let path = transcript(
            "no-title",
            &[user_line("main"), assistant_line("end_turn")].join("\n"),
        );

        assert_eq!(read(&path).title, None);
    }

    #[test]
    fn a_blank_ai_title_yields_no_title() {
        let path = transcript(
            "blank-title",
            &[title_line("   "), assistant_line("end_turn")].join("\n"),
        );

        assert_eq!(read(&path).title, None);
    }

    #[test]
    fn the_branch_comes_from_the_entries_that_carry_one() {
        let path = transcript(
            "branch",
            &[user_line("eric-attribution"), assistant_line("end_turn")].join("\n"),
        );

        assert_eq!(read(&path).branch, Some("eric-attribution".to_string()));
    }

    #[test]
    fn a_detached_head_is_not_a_branch() {
        let path = transcript(
            "detached",
            &[user_line("HEAD"), assistant_line("end_turn")].join("\n"),
        );

        assert_eq!(read(&path).branch, None);
    }

    #[test]
    fn a_transcript_with_no_git_branch_yields_no_branch() {
        let path = transcript(
            "no-branch",
            &[
                line("user", r#","message":{"role":"user","content":"hi"}"#),
                assistant_line("end_turn"),
            ]
            .join("\n"),
        );

        assert_eq!(read(&path).branch, None);
    }

    #[test]
    fn a_transcript_ending_in_an_open_turn_is_working() {
        let path = transcript(
            "open-turn",
            &[assistant_line("end_turn"), user_line("main")].join("\n"),
        );

        assert_eq!(read(&path).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_transcript_ending_in_a_tool_call_is_working() {
        let path = transcript(
            "tool-call",
            &[user_line("main"), assistant_line("tool_use")].join("\n"),
        );

        assert_eq!(read(&path).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_transcript_ending_in_a_completed_turn_is_done() {
        let path = transcript(
            "completed",
            &[user_line("main"), assistant_line("end_turn")].join("\n"),
        );

        assert_eq!(read(&path).status, Some(SessionStatus::Done));
    }

    #[test]
    fn the_tail_of_a_real_transcript_reads_as_a_completed_turn() {
        let path = transcript("real-tail", REAL_COMPLETED_TAIL);

        let facts = read(&path);

        assert_eq!(facts.status, Some(SessionStatus::Done));
        assert_eq!(
            facts.title,
            Some("Review attribution mock data test file".to_string())
        );
        assert_eq!(facts.branch, Some("eric-attribution".to_string()));
    }

    #[test]
    fn status_free_trailing_entries_do_not_change_the_derived_status() {
        let trailing = [
            title_line("A title written after the turn"),
            line("file-history-snapshot", r#","snapshot":{}"#),
            line("queue-operation", r#","operation":"add""#),
            line("last-prompt", r#","lastPrompt":"go on""#),
            line("mode", r#","mode":"acceptEdits""#),
            line("attachment", r#","attachment":{}"#),
        ];

        let done = transcript(
            "trailing-done",
            &[
                vec![user_line("main"), assistant_line("end_turn")],
                trailing.to_vec(),
            ]
            .concat()
            .join("\n"),
        );
        let working = transcript(
            "trailing-working",
            &[
                vec![assistant_line("end_turn"), user_line("main")],
                trailing.to_vec(),
            ]
            .concat()
            .join("\n"),
        );

        assert_eq!(read(&done).status, Some(SessionStatus::Done));
        assert_eq!(read(&working).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_subagents_completed_turn_does_not_complete_the_sessions_turn() {
        let path = transcript(
            "sidechain",
            &[
                user_line("main"),
                assistant_line("tool_use"),
                line(
                    "assistant",
                    r#","isSidechain":true,"message":{"role":"assistant","stop_reason":"end_turn"}"#,
                ),
            ]
            .join("\n"),
        );

        assert_eq!(read(&path).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_transcript_with_no_user_entry_at_all_is_done_rather_than_unknown() {
        let path = transcript(
            "never-prompted",
            &[
                line("queue-operation", r#","operation":"add""#),
                line("system", r#","content":"session started""#),
            ]
            .join("\n"),
        );

        assert_eq!(read(&path).status, Some(SessionStatus::Done));
    }

    #[test]
    fn a_transcript_that_does_not_exist_says_nothing() {
        let path = scratch_dir("missing").join("s-1.jsonl");

        assert_eq!(read(&path), TranscriptFacts::default());
    }

    #[test]
    fn an_empty_transcript_says_nothing() {
        let path = transcript("empty", "");

        assert_eq!(read(&path).status, None);
    }

    #[test]
    fn a_transcript_in_a_shape_this_app_does_not_recognise_says_nothing() {
        let path = transcript("unrecognised", "half a line\nnot json either\n");

        assert_eq!(read(&path).status, None);
    }

    #[test]
    fn a_single_unparseable_line_does_not_stop_the_rest_being_read() {
        let path = transcript(
            "one-bad-line",
            &[
                user_line("main"),
                "{ truncated".to_string(),
                assistant_line("end_turn"),
            ]
            .join("\n"),
        );

        assert_eq!(read(&path).status, Some(SessionStatus::Done));
    }

    #[test]
    fn a_transcript_longer_than_the_first_window_is_still_read_from_its_end() {
        let filler = line(
            "user",
            &format!(
                r#","gitBranch":"main","message":{{"role":"user","content":"{}"}}"#,
                "x".repeat(4096)
            ),
        );
        let mut lines: Vec<String> = (0..40).map(|_| filler.clone()).collect();
        lines.push(title_line("A title past the first window"));
        lines.push(assistant_line("end_turn"));
        let path = transcript("large", &lines.join("\n"));

        let facts = read(&path);

        assert!(
            fs::metadata(&path).expect("the transcript to exist").len() > WINDOWS[0],
            "the fixture has to be larger than the first window to exercise it"
        );
        assert_eq!(facts.status, Some(SessionStatus::Done));
        assert_eq!(
            facts.title,
            Some("A title past the first window".to_string())
        );
    }

    #[test]
    fn a_title_out_of_the_first_windows_reach_is_found_by_widening_it() {
        const TITLE: &str = "A title the first window cannot see";

        let path = transcript(
            "title-past-the-first-window",
            &[
                title_line(TITLE),
                padding("", WINDOWS[0] as usize),
                user_line("main"),
                assistant_line("end_turn"),
            ]
            .join("\n"),
        );

        let first_window = tail(&path, WINDOWS[0]).expect("the first window to be readable");
        assert!(
            !first_window.contains(TITLE),
            "the fixture has to put the title out of the first window's reach"
        );

        let facts = read(&path);

        // The status and the branch are both answered by the first window, so
        // answering here is only possible if the window widened rather than
        // returning as soon as it had a status.
        assert_eq!(facts.status, Some(SessionStatus::Done));
        assert_eq!(facts.branch, Some("main".to_string()));
        assert_eq!(facts.title, Some(TITLE.to_string()));
    }

    #[test]
    fn a_branch_out_of_the_second_windows_reach_is_found_by_widening_it() {
        const BRANCH: &str = "eric/item-schedule-ui";

        let path = transcript(
            "branch-past-the-second-window",
            &[
                user_line(BRANCH),
                padding("", WINDOWS[1] as usize),
                title_line("A title the first window can see"),
                assistant_line("end_turn"),
            ]
            .join("\n"),
        );

        let second_window = tail(&path, WINDOWS[1]).expect("the second window to be readable");
        assert!(
            !second_window.contains(BRANCH),
            "the fixture has to put the branch out of the second window's reach"
        );

        let facts = read(&path);

        // The status and the title are both answered by the first window, so
        // only a window that keeps widening past the second one answers this.
        assert_eq!(facts.status, Some(SessionStatus::Done));
        assert_eq!(
            facts.title,
            Some("A title the first window can see".to_string())
        );
        assert_eq!(facts.branch, Some(BRANCH.to_string()));
    }

    #[test]
    fn a_window_that_has_not_reached_a_turn_is_not_a_transcript_that_was_never_prompted() {
        let tail_entries = [
            padding(r#","gitBranch":"main""#, WINDOWS[0] as usize),
            title_line("A title the first window can see"),
        ]
        .join("\n");

        // Identical from the first window's point of view: status-free entries,
        // a branch and a title, and no turn in sight. They differ only in
        // whether a turn exists anywhere in the file at all.
        let prompted = transcript(
            "prompted-out-of-the-first-window",
            &[user_line("main"), tail_entries.clone()].join("\n"),
        );
        let never_prompted = transcript("never-prompted-at-all", &tail_entries);

        assert_eq!(read(&prompted).status, Some(SessionStatus::Working));
        assert_eq!(read(&never_prompted).status, Some(SessionStatus::Done));
    }

    #[test]
    fn a_last_entry_larger_than_the_top_window_still_yields_a_status() {
        let path = transcript("giant-last-entry", &transcript_with_a_giant_last_entry());

        assert!(
            tail(&path, WINDOWS[2])
                .expect("the top window to be readable")
                .is_empty(),
            "the fixture has to leave even the top window holding no complete entry"
        );

        // Not Done, which is what an unanswered status settles at and what would
        // put a working session on the wrong row.
        assert_eq!(read(&path).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_transcript_past_the_whole_read_ceiling_is_not_read_in_full() {
        let path = transcript("past-the-ceiling", &transcript_with_a_giant_last_entry());

        // A ceiling the fixture is larger than, so the whole-file fallback is
        // declined and Unknown is the documented answer rather than an
        // unbounded read.
        assert_eq!(read_within(&path, WINDOWS[2]).status, None);
    }

    #[test]
    fn a_window_that_lands_mid_line_does_not_read_half_an_entry() {
        let path = transcript(
            "mid-line",
            &[
                user_line("main"),
                assistant_line("end_turn"),
                title_line("The current title"),
            ]
            .join("\n"),
        );
        let whole = fs::read_to_string(&path).expect("the transcript to be readable");
        // A window ending inside the assistant entry. Keeping its trailing
        // fragment would parse as an entry that is not there.
        let window = (whole.len() - whole.find("end_turn").expect("the entry to be there")) as u64;

        let text = tail(&path, window).expect("the tail to be readable");

        assert!(!text.contains("end_turn"));
        assert!(text.starts_with(&title_line("The current title")));
    }

    #[test]
    fn the_cache_returns_the_same_facts_until_the_transcript_changes() {
        let path = transcript(
            "cached",
            &[user_line("main"), assistant_line("end_turn")].join("\n"),
        );
        let mut cache = TranscriptCache::new();

        let first = cache.read(&path);
        fs::remove_file(&path).expect("the transcript to be removable");
        let second = cache.read(&path);

        assert_eq!(first.status, Some(SessionStatus::Done));
        // Removing the file changes the stamp, so the cache must not be
        // answering from what it read before.
        assert_eq!(second, TranscriptFacts::default());
    }

    #[test]
    fn the_cache_is_invalidated_when_the_transcript_grows() {
        let path = transcript(
            "growing",
            &[user_line("main"), assistant_line("end_turn")].join("\n"),
        );
        let mut cache = TranscriptCache::new();

        assert_eq!(cache.read(&path).status, Some(SessionStatus::Done));

        let grown = [
            user_line("main"),
            assistant_line("end_turn"),
            user_line("main"),
        ]
        .join("\n");
        fs::write(&path, grown).expect("the transcript to be writable");

        assert_eq!(cache.read(&path).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_rewrite_of_the_same_length_is_not_mistaken_for_no_change() {
        let done = [user_line("main"), assistant_line("end_turn")].join("\n");
        let working = [assistant_line("end_turn"), user_line("main")].join("\n");
        assert_eq!(
            done.len(),
            working.len(),
            "the two versions have to be the same size for the size alone to be no evidence"
        );

        let path = transcript("rewritten", &done);
        let mut cache = TranscriptCache::new();

        assert_eq!(cache.read(&path).status, Some(SessionStatus::Done));

        fs::write(&path, &working).expect("the transcript to be writable");
        File::options()
            .write(true)
            .open(&path)
            .expect("the transcript to be openable")
            .set_modified(SystemTime::now() + Duration::from_secs(60))
            .expect("the modification time to be settable");

        assert_eq!(cache.read(&path).status, Some(SessionStatus::Working));
    }

    #[test]
    fn a_title_that_changes_mid_session_is_re_read() {
        let path = transcript(
            "retitled",
            &[title_line("The first guess"), assistant_line("end_turn")].join("\n"),
        );
        let mut cache = TranscriptCache::new();

        assert_eq!(cache.read(&path).title, Some("The first guess".to_string()));

        let retitled = [
            title_line("The first guess"),
            assistant_line("end_turn"),
            title_line("The better title"),
        ]
        .join("\n");
        fs::write(&path, retitled).expect("the transcript to be writable");

        assert_eq!(
            cache.read(&path).title,
            Some("The better title".to_string())
        );
    }

    #[test]
    fn the_cache_forgets_transcripts_it_is_not_asked_to_keep() {
        let kept = transcript("kept", &assistant_line("end_turn"));
        let dropped = transcript("dropped", &assistant_line("end_turn"));
        let mut cache = TranscriptCache::new();
        cache.read(&kept);
        cache.read(&dropped);

        assert_eq!(cache.len(), 2);

        cache.retain(&HashSet::from([kept.clone()]));

        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn a_working_directory_becomes_the_directory_name_claude_code_uses() {
        assert_eq!(
            project_slug(Path::new("/Users/eric/Code/aydy")),
            "-Users-eric-Code-aydy"
        );
        assert_eq!(
            project_slug(Path::new(
                "/Users/eric/Code/okven.worktrees/eric-fab-snippet"
            )),
            "-Users-eric-Code-okven-worktrees-eric-fab-snippet"
        );
        assert_eq!(
            project_slug(Path::new("/Users/eric/.sculptor/workspaces/abc/code")),
            "-Users-eric--sculptor-workspaces-abc-code"
        );
    }

    #[test]
    fn a_transcript_is_found_from_the_working_directory() {
        let root = scratch_dir("locate");
        let cwd = Path::new("/Users/eric/Code/aydy");
        let project = root.join(project_slug(cwd));
        fs::create_dir_all(&project).expect("the project directory to be creatable");
        fs::write(project.join("s-1.jsonl"), assistant_line("end_turn"))
            .expect("the transcript to be writable");

        assert_eq!(
            locate(&root, Some(cwd), "s-1"),
            Some(project.join("s-1.jsonl"))
        );
    }

    #[test]
    fn a_transcript_whose_directory_is_not_named_after_the_working_directory_is_still_found() {
        let root = scratch_dir("locate-elsewhere");
        let project = root.join("-somewhere-else-entirely");
        fs::create_dir_all(&project).expect("the project directory to be creatable");
        fs::write(project.join("s-1.jsonl"), assistant_line("end_turn"))
            .expect("the transcript to be writable");

        assert_eq!(
            locate(&root, Some(Path::new("/Users/eric/Code/aydy")), "s-1"),
            Some(project.join("s-1.jsonl"))
        );
    }

    #[test]
    fn a_session_with_no_transcript_anywhere_is_not_located() {
        let root = scratch_dir("locate-missing");

        assert_eq!(
            locate(&root, Some(Path::new("/Users/eric/Code/aydy")), "s-1"),
            None
        );
    }
}
