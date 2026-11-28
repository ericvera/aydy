# 01_02 Transcript activity

## Goal

Teach the transcript module one new fact: when the session last wrote anything (main transcript or any subagent transcript). The transcript's content is not read for progress — there is no TodoWrite checklist (goals decision 10).

## Files to modify/create

- `src-tauri/src/discovery/transcript.rs`

## Background

AYDY (Tauri v2 menu bar app, Rust core in `src-tauri/`) reads each live Claude Code session's transcript (`~/.claude/projects/<slug>/<session_id>.jsonl`, line-delimited JSON) on a 25 s scan, backwards from the end in windows, cached per file by size+mtime (`transcript.rs:1-13`, `TranscriptCache` `:136-206`, `read_within` `:228-262`, `interpret` `:351-403`). Parsing is free-form `serde_json::Value`, total by design; sidechain entries are skipped (`meaning`, `:309-342`).

Rule from the spec (shared with the VS Code extension, which mirrors it in task 03_02 — keep it exactly):

- **Activity** (`last_activity`): the newest mtime among the transcript itself and every `*.jsonl` file under `<transcript's parent dir>/<session_id>/subagents/`, searched recursively to depth 3 (real layout on this machine: `<id>/subagents/agent-*.jsonl` and `<id>/subagents/workflows/wf_*/agent-*.jsonl`). Subagents write there, not to the main transcript, so without this a session busy in a subagent reads as quiet. Missing transcript → `None`. Unreadable subagent dirs are ignored.

Nothing else in this file changes: `TranscriptFacts`, `read_within`, the status windows and the stamp cache stay as they are. Task 01_03 calls `last_activity` from the reconciler's scan.

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. Add `pub fn last_activity(transcript: &Path, session_id: &str) -> Option<SystemTime>`: `fs::metadata(transcript)?.modified()`, then walk `transcript.parent()?.join(session_id).join("subagents")` with a small recursive helper bounded at depth 3, taking the max mtime of files whose extension is `jsonl`. No caching (it is a handful of stats per scan, and the stamp cache watches only the main file).
2. Doc comment in the file's existing voice: explain _why_ subagent files count (subagents and workflow agents write there, so a session busy in a subagent would otherwise read as quiet) and why it is uncached.

## Gotchas

- Do not route this through `TranscriptCache`: its stamp is the main file's size+mtime, so a subagent write would not invalidate it.
- A missing or unreadable `subagents` dir is normal (most sessions have none); it must not turn the result into `None` when the main transcript exists.
- `last_activity` has no production caller until 01_03; it is `pub` in a `pub mod`, so the dead-code lint does not fire. Keep it `pub` rather than adding `#[allow(dead_code)]`.

## Verification

- Run `cargo test` and `cargo clippy --all-targets -- -D warnings` from `src-tauri/`.
- New tests in `transcript.rs`, `last_activity`: transcript only; a newer `subagents/agent-x.jsonl` wins; a newer `subagents/workflows/wf_1/agent-y.jsonl` wins; an older subagent file does not; non-`.jsonl` files are ignored; a file deeper than depth 3 is ignored; missing transcript → `None`. Set mtimes explicitly with `File::set_modified` (pattern in `src-tauri/src/discovery/status_files.rs` tests, `write_status`).
