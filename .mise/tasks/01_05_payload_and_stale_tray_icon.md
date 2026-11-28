# 01_05 Signals on the wire and the Stale tray icon

## Goal

Send each session's work signals to the popover, and give the menu bar icon a fifth state, Stale, drawn as the D1 shape from the mock.

## Files to modify/create

- `src-tauri/src/ipc/snapshot_channel.rs`
- `src-tauri/src/tray/icon.rs`
- `src-tauri/icons/tray/stale.png` (new asset)
- `docs/design/menubar-ux.md` (the icon section)

## Background

AYDY (Tauri v2 menu bar app) pushes a `SnapshotPayload` to the popover over a `tauri::ipc::Channel` every 100 ms when it changed (`snapshot_channel.rs:1-11`, `SessionPayload` `:80-112`, durations as whole ms via `milliseconds` `:180-183`). The same observation derives the tray icon (`tray/icon.rs`, `indication` `:53-84`). Icons are macOS template PNGs, 36×36 RGBA, black plus alpha, told apart by shape only (`docs/design/menubar-ux.md`, "Why shape and not colour").

Prior tasks produced `SessionSnapshot.signals: Option<WorkSignals>` (`src-tauri/src/session/registry.rs`) where `WorkSignals { stale, quiet_for: Option<Duration>, turn_elapsed: Option<Duration>, step: Option<(Step, StepSource)>, eta: Option<Eta { remaining_ms: i64, reported_ago: Duration }>, fraction_permille: Option<u16> }` (`src-tauri/src/session/signals.rs`).

Decisions reaching this task (owner-approved, `.mise/goals.md` decision 4 and mock `.mise/mock/d-tray-icon.html`):

- Precedence: Needs input (unread) > Something new (unread) > **Stale** > Working > All quiet.
- Stale count = number of stale sessions.
- Shape D1: the working icon's ring drawn **dashed**, with its left half still filled solid. In the mock's 18-unit viewBox: ring `r=7`, stroke 1.5, `stroke-dasharray="2.2 2.2"`, left half disc `M9 2 A7 7 0 0 0 9 16 Z`. At 36 px every unit is 2 px. Because the left half is solid, the dashes are visible only on the right half.

## Guides

- docs/design/menubar-ux.md (doc): popover layout, session states, notification copy — create it when the popover is first designed

## Implementation details

1. `snapshot_channel.rs`:
   - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "camelCase")] pub enum StepSourcePayload { Agent, Checklist }`.
   - `StepPayload { done: u32, total: u32, source: StepSourcePayload }`, `EtaPayload { remaining_ms: i64, reported_ago_ms: u64 }`, `SignalsPayload { stale: bool, quiet_ms: Option<u64>, turn_elapsed_ms: Option<u64>, step: Option<StepPayload>, eta: Option<EtaPayload>, fraction_permille: Option<u16> }`, all camelCase, `From<&WorkSignals>`.
   - `SessionPayload` gains `pub signals: Option<SignalsPayload>`. `None` serialises as `null` (no `skip_serializing_if`), matching the other optional fields.
2. `tray/icon.rs`:
   - `IconState::Stale` between `SomethingNew` and `Working`, doc comment: a working session has gone quiet; ranks below anything unread because a stuck session matters less than one already waiting on you.
   - `image()` → `include_bytes!("../../icons/tray/stale.png")`.
   - `indication`: after the unread branch, count sessions with `signals.is_some_and(|signals| signals.stale)`; if > 0 return `Stale` with that count; then Working as today.
   - Update the two image tests (`every_state_has_a_template_image_of_its_own`, `every_state_shows_the_icon_named_after_it`) to include `Stale` / `stale.png`; update the `session()` test helper with `signals: None` and add a helper for a stale working session.
3. `stale.png`: derive it from `src-tauri/icons/tray/working.png` so ring radius, stroke and anti-aliasing match exactly: decode it, and on the right half (x > centre) zero the alpha of ring pixels whose arc position falls in a dash gap (dash and gap each 2.2 units = 4.4 px of arc at r = 14 px; start the pattern at the top, 12 o'clock, so the dashes are symmetric about the horizontal axis). Do it with a throwaway Node script using only `node:zlib` (PNG: 8-bit RGBA, non-interlaced; undo the per-row filters on decode, write filter 0 on encode) kept in the session scratchpad, **not** committed. Look at the result (Read the PNG) at both 36 px and enlarged before committing.
4. `docs/design/menubar-ux.md`: the states table becomes five rows (Stale, "ring drawn dashed with its left half filled", count "stale sessions") at position 3; renumber; add a bullet explaining Stale (a Working session with nothing written to its transcript, including subagent transcripts, for 5 minutes; stale is a signal beside status, not a status) and that a stale session gives up the working count while it lasts — the popover still shows it; the Counts paragraph mentions the stale count; "four states" → "five" where it appears in that section. Add the dashed-ring line to the "empty ring → …" list.

## Gotchas

- Template images must stay pure black RGB with alpha only; do not introduce grey RGB values.
- `IconIndication` is compared for change in `Observation` (`snapshot_channel.rs:165-170`); nothing else to do, but a stale session's `quiet_ms` changes every observation, so the payload is re-sent every 100 ms while anything is Working — already true via `workingTimeMs`.
- Snapshot-channel tests that build `SessionSnapshot` literals already got `signals: None` in 01_03; add a payload test rather than editing those.
- The TypeScript `Session` type does not have `signals` yet (task 02_01); an extra JSON field is harmless.

## Verification

- Run `cargo test` and `cargo clippy --all-targets -- -D warnings` from `src-tauri/`.
- New tests:
  - `snapshot_channel.rs`: a working session with signals serialises to JSON with `signals.stale`, `quietMs`, `turnElapsedMs`, `step.source == "agent"`, `eta.remainingMs` (negative allowed) and `fractionPermille`; a Done session serialises `"signals": null`.
  - `tray/icon.rs`: a stale working session (read) shows Stale with count = stale sessions, not working sessions; any unread session outranks Stale; a working session that is not stale still shows Working; Stale's image is distinct from every other state's.
- Substitute check for the icon artwork: Read `src-tauri/icons/tray/stale.png` and confirm the dashed right half and solid left half, matching `.mise/mock/d-tray-icon.html` D1.
