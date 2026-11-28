## Stopped

Waits for: answer

1. Which title layout? (shown for elapsed, and step + ETA)
   - a. Repo first, status after an em dash: `◐ okven · feat/x — 3m` / `◐ okven · feat/x — 4/7 ~6m`
   - b. Repo first, status in brackets: `◐ okven · feat/x (3m)` / `◐ okven · feat/x (4/7, ~6m)`
   - c. Keep status first, bracketed: `◐ [3m] okven · feat/x` / `◐ [4/7 ~6m] okven · feat/x`
   - d. Keep status first, bar divider: `◐ 3m | okven · feat/x` / `◐ 4/7 ~6m | okven · feat/x`

2. How should the status read?
   - a. Terse as today: `3m`, `~6m`, `2m over`, `quiet 9m`
   - b. Popover's words: `3m elapsed`, `~6m left`, `2m past ETA`, `Quiet 9m`
   - c. Terse, and drop plain elapsed time — show only step, ETA, over or quiet

Recommend: 1a,2a (reply `rec` to take all) — the repo name stays put as the window's identity, and the em dash alone separates status from the branch's `·`.
