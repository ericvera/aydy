# 01_02 Transcript checklist and activity

## Goal

Teach the transcript reader two new facts: the current turn's TodoWrite checklist progress, and when the session last wrote anything (main transcript or any subagent transcript).

## Files to modify/create

- `src-tauri/src/discovery/transcript.rs`

## Background

AYDY (Tauri v2 menu bar app, Rust core in `src-tauri/`) reads each live Claude Code session's transcript (`~/.claude/projects/<slug>/<session_id>.jsonl`, line-delimited JSON) on a 25 s scan, backwards from the end in windows, cached per file by size+mtime (`transcript.rs:1-13`, `TranscriptCache` `:136-206`, `read_within` `:228-262`, `interpret` `:351-403`). Parsing is free-form `serde_json::Value`, total by design; sidechain entries are skipped (`meaning`, `:309-342`).

Task 01_01 added `crate::session::signals::Step { done, total }` with `Step::new(done, total) -> Option<Step>` (`src-tauri/src/session/signals.rs`). Use it for the checklist.

Rules from the spec (shared with the VS Code extension, which mirrors them in a later task — keep them exactly):

- **Checklist** = the newest non-sidechain `type: "assistant"` entry whose `message.content` array has a `{"type":"tool_use","name":"TodoWrite","input":{"todos":[…]}}` element, provided it comes _after_ the newest genuine user prompt. `done` = todos whose `status` is the string `"completed"`; `total` = number of todos; `Step::new` rejects total 0. A **genuine prompt** is a `type: "user"` entry that is not `isSidechain: true`, not `isMeta: true`, and whose `message.content` is a string, or an array containing no element with `type == "tool_result"`. Tool results are `type: "user"` entries too — they do not end the search.
- If the window being interpreted holds no checklist before reaching a genuine prompt (or the start of the window), the checklist is `None`. The checklist does **not** take part in the window-widening decision (`read_within`'s early return on status/title/branch) — it is a fallback signal and must not cause larger reads.
- **Activity** (`last_activity`): the newest mtime among the transcript itself and every `*.jsonl` file under `<transcript's parent dir>/<session_id>/subagents/`, searched recursively to depth 3 (real layout on this machine: `<id>/subagents/agent-*.jsonl` and `<id>/subagents/workflows/wf_*/agent-*.jsonl`). Subagents write there, not to the main transcript, so without this a session busy in a subagent reads as quiet. Missing transcript → `None`. Unreadable subagent dirs are ignored.

TodoWrite shape (Claude Code): `{"type":"assistant","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"TodoWrite","input":{"todos":[{"content":"A","status":"completed","activeForm":"Doing A"},{"content":"B","status":"in_progress","activeForm":"Doing B"}]}}],"stop_reason":"tool_use"}}`. No transcript on this machine actually calls TodoWrite, so tests use synthetic lines built with the existing helpers (`line`, `user_line`, `assistant_line` at `:442-470`).

## Guides

None of the config's Skills & guides entries apply.

## Implementation details

1. `TranscriptFacts` gains `pub checklist: Option<Step>` (doc comment: the current turn's TodoWrite progress; `None` when there is none in reach). It stays `Default`/`Eq`.
2. In `interpret`, while walking lines newest-first, track `turn_boundary: bool`. For each parsed entry, before/alongside the existing logic:
   - if `!turn_boundary && facts.checklist.is_none()` and the entry is a non-sidechain assistant with a TodoWrite `tool_use`, set `facts.checklist` from its todos (if `Step::new` returns `None`, leave it `None` but stop looking — the newest TodoWrite wins even if empty);
   - if the entry is a genuine prompt, set `turn_boundary = true`.
     Keep helper functions small (`todo_write_step(entry) -> Option<Option<Step>>`, `is_genuine_prompt(entry) -> bool`), free-form JSON like `meaning`.
3. Do not change `read_within`'s widening condition.
4. Add `pub fn last_activity(transcript: &Path, session_id: &str) -> Option<SystemTime>`: `fs::metadata(transcript)?.modified()`, then walk `transcript.parent()?.join(session_id).join("subagents")` with a small recursive helper bounded at depth 3, taking the max mtime of files whose extension is `jsonl`. No caching (it is a handful of stats per scan, and the stamp cache watches only the main file).
5. Doc comments in the file's existing voice: explain _why_ subagent files count and why the checklist does not widen windows.

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
  - a checklist does not make a transcript whose status/title/branch are answered read a wider window (assert via a transcript where the TodoWrite sits only beyond the first window: result is `None`, not found by widening, while the status is still read);
  - `last_activity`: transcript only; a newer `subagents/agent-x.jsonl` wins; a newer `subagents/workflows/wf_1/agent-y.jsonl` wins; non-`.jsonl` files are ignored; missing transcript → `None`. Set mtimes explicitly with `File::set_modified` (pattern in `src-tauri/src/discovery/status_files.rs` tests, `write_status`).
