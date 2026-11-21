# 02_04 Feed the registry from the status and seen files

## Goal

The app learns about sessions by reading the status and seen files every half second. It applies only sessions whose Claude process is alive, at the time each event was recorded. Sounds stay silent for everything loaded by the first pass. The HTTP listener stops feeding hook events into the registry.

## Files to modify/create

- src-tauri/src/discovery/file_feed.rs (new)
- src-tauri/src/discovery/mod.rs
- src-tauri/src/lib.rs
- src-tauri/src/listener/server.rs
- src-tauri/src/listener/routes.rs

## Background

- Task 02_02 produced `discovery/status_files.rs`: `read_statuses(dir) -> Vec<StatusRecord>`, `read_seen(dir) -> HashMap<String,u64>` and `DataDir`.
- Task 02_03 produced `SessionRegistry::apply_status(record: StatusRecord, now_ms: u64)`, which places the event at its recorded time (`written_at_ms`) on the registry clock, and `apply_seen(id, ms)`. `DataDir` is managed in `lib.rs`.
- Task 02_03 also made `SoundNotifier` (`notify/sound.rs`) start unarmed: it earns nothing until `arm()` is called, and the first observation after `arm()` is a silent baseline. `lib.rs` currently calls `sounds.arm()` right after constructing it.
- Why startup must be silent: on start the first pass replays every live status file. A session the scan first saw as `Unknown` becomes `Done`, which `transition_alert` (`notify/mod.rs:31-40`) treats as a transition. Sounds must play only for changes seen after the initial load.
- Liveness source, the same as the reconciler's:
  - `discovery/session_files.rs:71` `read_all(dir) -> io::Result<SessionFiles>`
  - `:140` `ProcessTable` with `LiveProcesses` (`:146`) and `FixedProcesses` for tests (`:175`)
  - `discovery/reconciler.rs:59` `user_sessions_dir()`
  - `reconciler.rs:196` for how it splits files into live and stale.
- The background loop pattern is `discovery/reconciler.rs:410` (`run`) with `ScanShutdown` (`:464-480`), spawned in `lib.rs:222-238` and triggered on exit at `lib.rs:254-263`.
- Hook events reach the registry through `listener/routes.rs:99-111` (`receive_hook`), then `RegistryInput::Hook` (`listener/server.rs:32-40`), then `apply_input` (`server.rs:145-152`). The legacy HTTP hook entries keep POSTing until stage 3, so the endpoint must keep answering 200.
- Rule from the goals: the app hides sessions whose process is no longer alive when it reads the files. A status file whose session has no live process is not shown.

## Guides

None match.

## Implementation details

- `file_feed.rs`:
  - `pub const FEED_INTERVAL: Duration = 500 ms` and `LIVENESS_INTERVAL = 5 s`.
  - `pub struct FileFeed { data: DataDir, sessions_dir: Option<PathBuf>, processes: Arc<dyn ProcessTable>, applied: HashMap<String, u64>, live: HashSet<String>, live_read_at: Option<Instant> }`.
  - `pub fn pass(&mut self, now: Instant) -> FeedChanges`, where `FeedChanges { apply: Vec<StatusRecord>, ended: Vec<String>, seen: HashMap<String,u64> }`:
    1. Refresh the live set when it is older than `LIVENESS_INTERVAL`. If the sessions dir is unreadable, keep the previous set.
    2. Read the statuses, then the seen times, in that order.
    3. `apply` holds the records for live sessions whose mtime differs from `applied`.
    4. `ended` holds the ids in `applied` whose file vanished. Remove them from `applied`.
    5. `seen` holds every seen time for the ids present.
  - `pub fn apply_changes(registry: &Mutex<SessionRegistry>, changes, now_ms: u64)` applies the records with `apply_status(record, now_ms)`, then a `SessionEnd` `HookEvent` for each ended id, then `apply_seen`. It uses `crate::sync::lock_recovering`. `now_ms` is `SystemTime::now()` as epoch ms, read once per pass by the caller; tests pass a fixed value.
  - `pub async fn run(feed, registry, on_loaded: Box<dyn FnOnce() + Send>, shutdown: oneshot::Receiver<()>)` mirrors `reconciler::run`, runs on the blocking pool, and comes with `FeedShutdown` like `ScanShutdown`. It calls `on_loaded` exactly once, right after the first pass's changes have been applied to the registry.
- In `discovery/mod.rs`, add `pub mod file_feed;`.
- `lib.rs`:
  - Create the feed with `DataDir::new(app_data_dir)`, `user_sessions_dir()` and `Arc::new(LiveProcesses)`.
  - Spawn it, manage `FeedShutdown`, and trigger it on exit.
  - Remove the `sounds.arm()` call placed after `SoundNotifier::new` in 02_03, and pass `on_loaded` as a closure holding an `Arc` of the notifier that calls `arm()`. The snapshot publisher's next observation after that is the silent baseline, and it already reflects the initial load.
  - When `user_sessions_dir()` is `None`, run with `sessions_dir: None` and treat every status as live.
- `routes.rs`: `receive_hook` still parses for validation but no longer dispatches. It always returns `StatusCode::OK`. Update its doc comment: the legacy entries still post here until the app removes them.
- `server.rs`: remove the `RegistryInput::Hook` variant and its arm. Switch any `lock_recovering` use to the re-export as needed.

## Gotchas

- A session whose process is not live yet but will be in a moment (the hook fired before Claude wrote its sessions file) gets picked up on the next liveness refresh, because `applied` is not updated for skipped records. Do not mark skipped records as applied.
- Do not apply `SessionEnd` for ids that were never applied.
- The reconciler still runs and still marks sessions gone. Do not duplicate its removal logic here.
- Call `on_loaded` after the first pass even when it applied nothing, or sounds would never arm on a machine with no sessions.

## Verification

- Unit tests in `file_feed.rs` with scratch dirs and `FixedProcesses` cover:
  - a live session's record applies once, and again only after the mtime changes
  - a dead session's record is not applied, then is applied once its pid becomes live
  - a deleted status file yields `ended` once
  - seen times are passed through
  - an unreadable sessions dir keeps the previous live set
  - `apply_changes` produces an unread session in the registry snapshot
  - `apply_changes` passes `now_ms` through: a Stop recorded an hour before `now_ms` sorts below one recorded a minute before it
  - `run` calls `on_loaded` once, after the first pass is applied (drive it with a short interval and a flag or channel), and not again on later passes
- Update the `routes.rs` and `server.rs` tests: a hook POST still answers 200 and no longer changes the registry.
- Run `cargo test` and `cargo clippy` from `src-tauri/`.
