# 01_02 Transcript checklist and activity

## Goal

Teach the transcript reader two new facts: the current turn's TodoWrite checklist progress, read from exactly the last 256 KiB of the transcript, and when the session last wrote anything (main transcript or any subagent transcript).

## Files to modify/create

- `src-tauri/src/discovery/transcript.rs`

## Background

AYDY (Tauri v2 menu bar app, Rust core in `src-tauri/`) reads each live Claude Code session's transcript (`~/.claude/projects/<slug>/<session_id>.jsonl`, line-delimited JSON) on a 25 s scan, backwards from the end in windows, cached per file by size+mtime (`transcript.rs:1-13`, `TranscriptCache` `:136-206`, `read_within` `:228-262`, `interpret` `:351-403`). Parsing is free-form `serde_json::Value`, total by design; sidechain entries are skipped (`meaning`, `:309-342`).

Task 01_01 added `crate::session::signals::Step { done, total }` with `Step::new(done, total) -> Option<Step>` (`src-tauri/src/session/signals.rs`). Use it for the checklist.

Rules from the spec (shared with the VS Code extension, which mirrors them in a later task — keep them exactly):

- **Checklist** = the newest non-sidechain `type: "assistant"` entry whose `message.content` array has a `{"type":"tool_use","name":"TodoWrite","input":{"todos":[…]}}` element, provided it comes _after_ the newest genuine user prompt. `done` = todos whose `status` is the string `"completed"`; `total` = number of todos; `Step::new` rejects total 0. A **genuine prompt** is a `type: "user"` entry that is not `isSidechain: true`, not `isMeta: true`, and whose `message.content` is a string, or an array containing no element with `type == "tool_result"`. Tool results are `type: "user"` entries too — they do not end the search.
- **Read rule (identical in the extension, task 03_02):** the checklist is read from exactly the last `CHECKLIST_WINDOW` = 256 KiB of the transcript (the whole file when it is smaller), dropping a leading partial line when the window does not reach the start of the file — what the existing `tail(path, window)` (`transcript.rs:263-289`) already does. It is **not** read from whichever status window `read_within` stopped at: that window can be as small as 32 KiB (`WINDOWS`, `:46`; early return `:243`) or as large as the whole file (`:252-257`), and the extension cannot reproduce that choice because it does not derive title/branch/status. A window holding no checklist before reaching a genuine prompt (or its own start) gives `None`. The checklist never takes part in `read_within`'s widening decision.
- **Activity** (`last_activity`): the newest mtime among the transcript itself and every `*.jsonl` file under `<transcript's parent dir>/<session_id>/subagents/`, searched recursively to depth 3 (real layout on this machine: `<id>/subagents/agent-*.jsonl` and `<id>/subagents/workflows/wf_*/agent-*.jsonl`). Subagents write there, not to the main transcript, so without this a session busy in a subagent reads as quiet. Missing transcript → `None`. Unreadable subagent dirs are ignored.

TodoWrite shape (Claude Code): `{"type":"assistant","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"TodoWrite","input":{"todos":[{"content":"A","status":"completed","activeForm":"Doing A"},{"content":"B","status":"in_progress","activeForm":"Doing B"}]}}],"stop_reason":"tool_use"}}`. No transcript on this machine actually calls TodoWrite, so tests use synthetic lines built with the existing helpers (`line`, `user_line`, `assistant_line` at `:442-470`).

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `TranscriptFacts` gains `pub checklist: Option<Step>` (doc comment: the current turn's TodoWrite progress; `None` when there is none in the last `CHECKLIST_WINDOW`). It stays `Default`/`Eq`.
2. `const CHECKLIST_WINDOW: u64 = 256 * 1024;` with a doc comment: the one window both the app and the extension read the checklist from, so the two surfaces agree (the extension's `CHECKLIST_WINDOW_BYTES`).
3. `fn checklist(text: &str) -> Option<Step>` (pure): walk lines newest-first, skipping unparseable lines; for each entry, if it is a non-sidechain assistant with a TodoWrite `tool_use`, return `Step::new(done, total)` from its todos (the newest TodoWrite wins even if empty — `Step::new` returning `None` ends the search); if it is a genuine prompt, return `None`. End of text → `None`. Keep helpers small (`todo_write_step(entry) -> Option<Option<Step>>`, `is_genuine_prompt(entry) -> bool`), free-form JSON like `meaning`. `interpret` does not compute the checklist.
4. In `read_within`, on every path that returns facts from a readable file, set `facts.checklist = tail(path, CHECKLIST_WINDOW).and_then(|text| checklist(&text))`. You may reuse a text the loop already read when that window was exactly `CHECKLIST_WINDOW` or covered the whole file and the file is no larger than `CHECKLIST_WINDOW`; otherwise read it separately. The extra read happens only when the stamp cache misses (the transcript changed), at most 256 KiB per changed transcript per 25 s scan. Do not change the widening condition.
5. Add `pub fn last_activity(transcript: &Path, session_id: &str) -> Option<SystemTime>`: `fs::metadata(transcript)?.modified()`, then walk `transcript.parent()?.join(session_id).join("subagents")` with a small recursive helper bounded at depth 3, taking the max mtime of files whose extension is `jsonl`. No caching (it is a handful of stats per scan, and the stamp cache watches only the main file).
6. Doc comments in the file's existing voice: explain _why_ subagent files count and why the checklist has its own fixed window (agreement with the extension) instead of riding the status windows.

## Gotchas

- An assistant entry with `stop_reason: "tool_use"` is TurnOpen for status; the checklist logic is independent of `meaning`.
- A `user` entry carrying `tool_result` blocks is mid-turn; treating it as a prompt would always hide the checklist (every TodoWrite is followed by its tool_result).
- Content may be a string or an array; any other shape is not a genuine prompt, so an unrecognised entry never hides a checklist.
- Keep `TranscriptFacts` comparisons in existing tests compiling — tests that build expected `TranscriptFacts` literals need `checklist: None` or `..Default::default()`.

## Verification

- Run `cargo test` and `cargo clippy --all-targets -- -D warnings` from `src-tauri/`.
- New tests in `transcript.rs`:
  - a TodoWrite after the last prompt yields `done/total` (2 completed of 5 → `Step {2,5}`);
  - the newest TodoWrite wins over an older one in the same turn;
  - a TodoWrite before the newest genuine prompt yields `None`;
  - tool_result user entries between the TodoWrite and the end do not hide it;
  - an `isMeta` user entry does not end the turn;
  - a sidechain TodoWrite is ignored;
  - an empty todos list yields `None`;
  - `checklist` unit cases on text directly (the bullets above);
  - a TodoWrite between 32 KiB and 256 KiB from the end of a transcript whose status/title/branch are answered in the first 32 KiB window is still found (`read` returns it);
  - a TodoWrite more than 256 KiB from the end (pad with tool_result user lines after it, ~300 KiB file) yields `None`, while the status is still read;
  - `last_activity`: transcript only; a newer `subagents/agent-x.jsonl` wins; a newer `subagents/workflows/wf_1/agent-y.jsonl` wins; non-`.jsonl` files are ignored; missing transcript → `None`. Set mtimes explicitly with `File::set_modified` (pattern in `src-tauri/src/discovery/status_files.rs` tests, `write_status`).
