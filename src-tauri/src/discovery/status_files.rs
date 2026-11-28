//! The per-session files under AYDY's data directory: the status files the
//! hook script writes, holding each session's latest raw hook payload, the
//! progress files the agent writes with its ETA, and the seen files recording
//! when a session was last looked at.
//!
//! Every file here is written by another process, so a file that cannot be
//! read or parsed is skipped rather than reported, and a missing directory
//! reads as empty.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::atomic_file::write_atomically;
use crate::session::event::HookEvent;
use crate::session::hook_payload::HookPayload;
use crate::session::signals::Step;

const JSON_EXTENSION: &str = "json";
const MAX_SESSION_ID_LEN: usize = 128;
const MAX_ETA: Duration = Duration::from_secs(24 * 60 * 60);

/// AYDY's data directory and the locations inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn status_dir(&self) -> PathBuf {
        self.root.join("status")
    }

    pub fn seen_dir(&self) -> PathBuf {
        self.root.join("seen")
    }

    pub fn progress_dir(&self) -> PathBuf {
        self.root.join("progress")
    }

    pub fn jump_dir(&self) -> PathBuf {
        self.root.join("jump")
    }

    pub fn hook_script(&self) -> PathBuf {
        self.root.join("bin").join("aydy-hook.sh")
    }
}

/// A session's latest hook event and when the hook script wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRecord {
    pub event: HookEvent,
    pub written_at_ms: u64,
}

/// A session's latest progress report and when the agent wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressRecord {
    pub session_id: String,
    pub eta: Duration,
    pub step: Option<Step>,
    pub written_at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressFile {
    eta_seconds: u64,
    step: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct StepFile {
    done: u32,
    total: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeenFile {
    seen_at: u64,
}

/// Reads every status file in `dir`.
///
/// A file whose payload names a session other than its own file stem is
/// skipped, so a renamed or stray file cannot conjure a phantom session.
pub fn read_statuses(dir: &Path) -> Vec<StatusRecord> {
    json_files(dir)
        .filter_map(|(stem, path)| {
            let written_at_ms = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(epoch_ms)?;
            let event = fs::read_to_string(&path)
                .ok()
                .and_then(|contents| serde_json::from_str::<HookPayload>(&contents).ok())
                .and_then(HookPayload::into_event)
                .filter(|event| event.session_id == stem)?;

            Some(StatusRecord {
                event,
                written_at_ms,
            })
        })
        .collect()
}

/// Reads every progress file in `dir`.
///
/// A malformed step costs the report its step only; the ETA still stands.
pub fn read_progress(dir: &Path) -> Vec<ProgressRecord> {
    json_files(dir)
        .filter(|(stem, _)| is_valid_session_id(stem))
        .filter_map(|(session_id, path)| {
            let written_at_ms = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(epoch_ms)?;
            let progress = fs::read_to_string(&path)
                .ok()
                .and_then(|contents| serde_json::from_str::<ProgressFile>(&contents).ok())?;
            let step = progress
                .step
                .and_then(|step| serde_json::from_value::<StepFile>(step).ok())
                .and_then(|step| Step::new(step.done, step.total));

            Some(ProgressRecord {
                session_id,
                eta: Duration::from_secs(progress.eta_seconds).min(MAX_ETA),
                step,
                written_at_ms,
            })
        })
        .collect()
}

/// Reads every seen file in `dir` as session id to the epoch ms it was seen at.
pub fn read_seen(dir: &Path) -> HashMap<String, u64> {
    json_files(dir)
        .filter_map(|(stem, path)| {
            let seen = fs::read_to_string(&path)
                .ok()
                .and_then(|contents| serde_json::from_str::<SeenFile>(&contents).ok())?;

            Some((stem, seen.seen_at))
        })
        .collect()
}

/// Records that `session_id` was seen at `seen_at_ms`.
///
/// The id becomes a file name, so anything but a plain id is refused rather
/// than allowed to name a path outside `dir`.
pub fn write_seen(dir: &Path, session_id: &str, seen_at_ms: u64) -> io::Result<()> {
    if !is_valid_session_id(session_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a session id: {session_id:?}"),
        ));
    }

    fs::create_dir_all(dir)?;
    let contents = serde_json::to_string(&SeenFile {
        seen_at: seen_at_ms,
    })
    .map_err(io::Error::other)?;

    write_atomically(
        &dir.join(format!("{session_id}.{JSON_EXTENSION}")),
        &contents,
    )
}

fn is_valid_session_id(session_id: &str) -> bool {
    (1..=MAX_SESSION_ID_LEN).contains(&session_id.len())
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// The `.json` files in `dir` that are not dotfiles, with their stems.
fn json_files(dir: &Path) -> impl Iterator<Item = (String, PathBuf)> {
    fs::read_dir(dir).into_iter().flatten().filter_map(|entry| {
        let path = entry.ok()?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some(JSON_EXTENSION) {
            return None;
        }
        let stem = path.file_stem()?.to_str()?;
        if stem.starts_with('.') {
            return None;
        }

        Some((stem.to_string(), path))
    })
}

fn epoch_ms(time: SystemTime) -> Option<u64> {
    let millis = time.duration_since(UNIX_EPOCH).ok()?.as_millis();

    u64::try_from(millis).ok()
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::time::Duration;

    use super::*;
    use crate::session::event::HookEventKind;
    use crate::test_support::scratch_dir;

    fn write_status(dir: &Path, name: &str, contents: &str, written_at_ms: u64) {
        let path = dir.join(name);
        fs::write(&path, contents).expect("the status file to be writable");
        File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(UNIX_EPOCH + Duration::from_millis(written_at_ms)))
            .expect("the mtime to be settable");
    }

    fn by_session(records: Vec<StatusRecord>) -> HashMap<String, StatusRecord> {
        records
            .into_iter()
            .map(|record| (record.event.session_id.clone(), record))
            .collect()
    }

    #[test]
    fn each_tracked_event_parses_with_the_time_its_file_was_written() {
        let dir = scratch_dir("status-kinds");
        let cases = [
            ("start", "SessionStart", 1_785_526_308_001),
            ("prompt", "UserPromptSubmit", 1_785_526_308_002),
            ("wait", "Notification", 1_785_526_308_003),
            ("stop", "Stop", 1_785_526_308_004),
            ("end", "SessionEnd", 1_785_526_308_005),
        ];
        for (id, event, at) in cases {
            write_status(
                &dir,
                &format!("{id}.json"),
                &format!(
                    r#"{{"session_id":"{id}","hook_event_name":"{event}","message":"Claude is waiting for your input"}}"#
                ),
                at,
            );
        }

        let records = by_session(read_statuses(&dir));

        assert_eq!(records.len(), 5);
        assert_eq!(records["start"].event.kind, HookEventKind::SessionStart);
        assert_eq!(records["start"].written_at_ms, 1_785_526_308_001);
        assert_eq!(
            records["prompt"].event.kind,
            HookEventKind::UserPromptSubmit
        );
        assert_eq!(records["prompt"].written_at_ms, 1_785_526_308_002);
        assert_eq!(
            records["wait"].event.kind,
            HookEventKind::Notification {
                message: Some("Claude is waiting for your input".to_string())
            }
        );
        assert_eq!(records["wait"].written_at_ms, 1_785_526_308_003);
        assert_eq!(records["stop"].event.kind, HookEventKind::Stop);
        assert_eq!(records["stop"].written_at_ms, 1_785_526_308_004);
        assert_eq!(records["end"].event.kind, HookEventKind::SessionEnd);
        assert_eq!(records["end"].written_at_ms, 1_785_526_308_005);
    }

    #[test]
    fn temp_files_bad_files_and_mismatched_stems_are_skipped() {
        let dir = scratch_dir("status-skips");
        let stop = |id: &str| format!(r#"{{"session_id":"{id}","hook_event_name":"Stop"}}"#);
        write_status(&dir, "good.json", &stop("good"), 1_000);
        write_status(&dir, ".good.json.aydy-1-0.tmp", &stop("good"), 1_000);
        write_status(&dir, ".hidden.json", &stop(".hidden"), 1_000);
        write_status(&dir, "broken.json", "{not json", 1_000);
        write_status(
            &dir,
            "untracked.json",
            r#"{"session_id":"untracked","hook_event_name":"PreToolUse"}"#,
            1_000,
        );
        write_status(&dir, "renamed.json", &stop("original"), 1_000);
        write_status(&dir, "notes.txt", &stop("notes"), 1_000);
        fs::create_dir(dir.join("folder.json")).expect("the directory to be creatable");

        let records = read_statuses(&dir);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].event.session_id, "good");
    }

    #[test]
    fn a_missing_directory_reads_as_empty() {
        let dir = scratch_dir("status-missing").join("absent");

        assert!(read_statuses(&dir).is_empty());
        assert!(read_seen(&dir).is_empty());
    }

    #[test]
    fn a_seen_time_written_reads_back() {
        let dir = scratch_dir("seen-round-trip").join("seen");

        write_seen(
            &dir,
            "9026fc28-2bc7-47cb-a4bd-d20882da64f2",
            1_785_526_308_250,
        )
        .expect("the seen file to be written");
        write_seen(&dir, "other_id", 7).expect("the seen file to be written");
        fs::write(dir.join("broken.json"), "{}").expect("the bad file to be writable");

        let seen = read_seen(&dir);

        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen["9026fc28-2bc7-47cb-a4bd-d20882da64f2"],
            1_785_526_308_250
        );
        assert_eq!(seen["other_id"], 7);
        assert_eq!(
            fs::read_to_string(dir.join("other_id.json")).expect("the file to exist"),
            r#"{"seenAt":7}"#
        );
    }

    #[test]
    fn an_id_that_is_not_a_plain_session_id_is_refused() {
        let dir = scratch_dir("seen-refused");
        let too_long = "a".repeat(MAX_SESSION_ID_LEN + 1);

        for id in ["../x", "", "a/b", "a.b", too_long.as_str()] {
            let error = write_seen(&dir, id, 1).expect_err("the id to be refused");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert!(write_seen(&dir, &"a".repeat(MAX_SESSION_ID_LEN), 1).is_ok());
        assert!(!dir.parent().expect("a parent").join("x.json").exists());
    }

    #[test]
    fn the_data_directory_names_each_location_inside_it() {
        let data = DataDir::new(PathBuf::from("/data"));

        assert_eq!(data.status_dir(), PathBuf::from("/data/status"));
        assert_eq!(data.seen_dir(), PathBuf::from("/data/seen"));
        assert_eq!(data.progress_dir(), PathBuf::from("/data/progress"));
        assert_eq!(data.jump_dir(), PathBuf::from("/data/jump"));
        assert_eq!(data.hook_script(), PathBuf::from("/data/bin/aydy-hook.sh"));
    }

    fn progress_by_session(dir: &Path) -> HashMap<String, ProgressRecord> {
        read_progress(dir)
            .into_iter()
            .map(|record| (record.session_id.clone(), record))
            .collect()
    }

    #[test]
    fn a_progress_file_reads_its_eta_and_step_with_the_time_it_was_written() {
        let dir = scratch_dir("progress-read");
        write_status(
            &dir,
            "a.json",
            r#"{"etaSeconds":360,"step":{"done":4,"total":7}}"#,
            1_785_526_308_001,
        );
        write_status(&dir, "b.json", r#"{"etaSeconds":0}"#, 1_785_526_308_002);

        let records = progress_by_session(&dir);

        assert_eq!(
            records["a"],
            ProgressRecord {
                session_id: "a".to_string(),
                eta: Duration::from_secs(360),
                step: Step::new(4, 7),
                written_at_ms: 1_785_526_308_001,
            }
        );
        assert_eq!(records["b"].eta, Duration::ZERO);
        assert_eq!(records["b"].step, None);
        assert_eq!(records["b"].written_at_ms, 1_785_526_308_002);
    }

    #[test]
    fn an_invalid_step_is_dropped_but_the_report_kept() {
        let dir = scratch_dir("progress-bad-step");
        let cases = [
            ("over", r#"{"etaSeconds":60,"step":{"done":5,"total":4}}"#),
            ("zero", r#"{"etaSeconds":60,"step":{"done":0,"total":0}}"#),
            (
                "negative",
                r#"{"etaSeconds":60,"step":{"done":-1,"total":4}}"#,
            ),
            ("shape", r#"{"etaSeconds":60,"step":"halfway"}"#),
        ];
        for (id, body) in cases {
            write_status(&dir, &format!("{id}.json"), body, 1);
        }

        let records = progress_by_session(&dir);

        assert_eq!(records.len(), cases.len());
        for (id, _) in cases {
            assert_eq!(records[id].eta, Duration::from_secs(60));
            assert_eq!(records[id].step, None);
        }
    }

    #[test]
    fn a_progress_file_without_a_usable_eta_or_session_id_is_skipped() {
        let dir = scratch_dir("progress-skipped");
        write_status(&dir, "missing.json", r#"{"step":{"done":1,"total":2}}"#, 1);
        write_status(&dir, "negative.json", r#"{"etaSeconds":-5}"#, 1);
        write_status(&dir, "fraction.json", r#"{"etaSeconds":1.5}"#, 1);
        write_status(&dir, "broken.json", "{not json", 1);
        write_status(&dir, "a.b.json", r#"{"etaSeconds":5}"#, 1);
        write_status(&dir, "kept.json", r#"{"etaSeconds":5}"#, 1);

        let records = progress_by_session(&dir);

        assert_eq!(records.keys().collect::<Vec<_>>(), ["kept"]);
    }

    #[test]
    fn an_eta_above_a_day_is_capped() {
        let dir = scratch_dir("progress-cap");
        write_status(
            &dir,
            "a.json",
            &format!(r#"{{"etaSeconds":{}}}"#, u64::MAX),
            1,
        );

        assert_eq!(progress_by_session(&dir)["a"].eta, MAX_ETA);
    }
}
