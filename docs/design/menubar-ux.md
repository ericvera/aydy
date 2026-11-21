# Menu bar UX

How AYDY presents itself in the menu bar: the icon, what it means, and the
popover that hangs under it. Written from `.mise/mocks.html` scenarios 1 and 3,
which are the approved design.

Read [ui-conventions.md](./ui-conventions.md) first — it owns tokens, spacing
and component layering. This document owns layout and behaviour.

## The icon

The tray icon is the whole product for most of the day: it has to answer
"is anything waiting for me?" without being clicked.

### States, in precedence order

The icon shows the **most urgent state present**, so one glance answers the
question.

| #   | State         | Shape                          | Count            |
| --- | ------------- | ------------------------------ | ---------------- |
| 1   | Not receiving | ring with a slash              | none             |
| 2   | Needs input   | disc with an exclamation mark  | unread sessions  |
| 3   | Something new | solid disc                     | unread sessions  |
| 4   | Working       | ring with its left half filled | working sessions |
| 5   | All quiet     | thin ring                      | none             |

1. **Not receiving** — hooks are registered but their events are going nowhere,
   so the list cannot be trusted. It outranks everything because every state
   below it is a claim about sessions the app may not be seeing. A fresh install
   with no hooks yet is **not** this state: nothing is being sent, so nothing is
   being lost.
2. **Needs input** — at least one **unread** session is waiting on the user.
3. **Something new** — at least one **unread** session in any other status. This
   is the state the app is named after: work finished while you were away and
   you have not looked yet. It outranks working, because working is the state
   you already expect.
4. **Working** — something is running and nothing is waiting to be looked at.
5. **All quiet** — nothing needs attention.

**A session that has been read never raises the icon above working**, even while
its status is still Needs input. Reading is what makes the amber state drop;
nothing is dismissed by hand. Mock scenario 9 is this rule in motion — the
moment a window regains focus, its row clears and the icon falls from state 2 to
state 3.

### Counts

States 2 and 3 count **unread** sessions — every unread session, not only the
waiting ones, so the number matches the popover header's `· N new`. State 4
counts **working** sessions. States 1 and 5 carry no count at all.

The count is the tray item's title, drawn beside the icon in the menu bar font.

### Why shape and not colour

The mock draws the five states in status colours. The shipped icons cannot:
macOS menu bar icons are **template images**, black plus alpha, tinted by the
system to suit the menu bar they land in — which is what makes an icon legible
in both a light and a dark menu bar, and in front of any wallpaper. Colour is
therefore not available to carry meaning, and states 2, 3 and 4 have to be told
apart by silhouette. The circle family of the mock is kept; the fill is what
varies:

- empty ring → nothing to see
- half-filled ring → part way through
- solid disc → finished
- disc with an exclamation → finished **and** blocked on you
- ring with a slash → the app itself is broken

This is also why status colour never leaks into the icon: in the popover,
colour is reserved for the status dot (see ui-conventions.md).

Source: `src-tauri/icons/tray/*.png`, 36×36 RGBA with black pixels and an alpha
mask, drawn at 18pt in the menu bar.

## The popover

### Dimensions and anchoring

- **320px wide**, fixed. The mock's `:root` says 380px; that value is stale, and
  scenarios 1, 1A and 9 all override it inline to 320. `w-popover` is the token.
- Height is fixed at 420px for now. The list has to size the window to its
  content, which ships with the list.
- Anchored **centred under the tray icon**, clamped so an icon near the edge of
  the screen cannot push the popover off it.
- No decorations, not resizable, always on top, not in the app switcher, and no
  Dock icon — AYDY is an `Accessory` app.

### The shell

The mock's `.popover` is the whole visible frame, and every screen inside the
window sits in it:

- **`rounded-large`** — a 10px corner, the mock's `border-radius: 10px`.
- **`border border-neutral-4`** — the 1px hairline that separates the popover
  from whatever is behind it. Without it the shell dissolves into a dark
  wallpaper.
- **`bg-neutral-1`** for the background and **`overflow-hidden`** so nothing —
  a hovered row, a scrolling list — paints over the corner.
- **`h-screen w-popover`** so the shell fills the window rather than the window
  showing through beside it.

The corner is only a corner because **the window is transparent**
(`transparent: true`, with `app.macOSPrivateApi: true`, which macOS requires for
it). An opaque borderless window is a square with a square shadow, and a rounded
`div` inside it just paints a rounded rectangle on a square card. For the same
reason `body` must not paint a background — it would fill the corners back in.
The drop shadow is the window's own (`shadow: true`); macOS derives it from the
opaque pixels, so it follows the rounded edge.

Any later screen — first run, settings, reconnecting — belongs **inside** this
shell. It is one window, and the frame does not change with the content.

### Opening and closing

- **Left click on the tray icon toggles it.** There is no menu; left click never
  opens one.
- **It hides as soon as it loses focus**, which is how clicking anywhere else
  dismisses it. This rides on `WindowEvent::Focused(false)`, so Tauri's
  `unstable` feature must stay off — it stops that event firing.
- A consequence worth knowing before it surprises someone: **opening a native
  dialog also fires focus loss** and takes the popover down underneath it. Any
  action that opens one needs to suppress the hide for its duration.

### Layout

Top to bottom, at 320px:

1. **Header** — `px-5 pt-4 pb-3`. An uppercase `text-1 font-3 neutral-5` count
   line on the left: `No sessions`, `4 sessions`, or `6 sessions · 2 new`. The
   `· N new` half appears only when something is unread, and its number is the
   same unread count the icon shows. While the list is stale the header says
   **Reconnecting** instead of a count — see the stale notice below. The
   settings control sits on the right — a gear, and the only way into settings.
   It is on the list and nowhere else, and it survives the stale state, which is
   a state the user may well want to open settings from.
2. **Body** — the grouped session list, one line per session.
3. **Footer** — quit, and nothing else.

The header count is written here because it is the popover's half of the icon's
count: both come from one snapshot computed in Rust, so they cannot disagree.

The three are a column inside the shell: header and footer are `shrink-0`, the
body is `flex-1 min-h-0 overflow-y-auto`. The window height is fixed, so a list
longer than the popover scrolls between them rather than growing the window.

**The footer carries no working-time total.** Mock scenario 2 draws one; it does
not ship, because aggregate durations across sessions are out of scope and the
total would need sessions to be retained past their end. Quit appears on the
session list and nowhere else — no other screen in the mock has a footer.

### The list

- **A row is a status dot and a title, on one line.** Nothing else: no working
  time, no longest turn, no status text, no tab identifier. Four rounds of mock
  review took each of those off the row, and they are on hover instead.
- **Unread is weight and brightness** — `title-unread` against `title-read`, the
  two shortcuts in `uno.config.ts`. Never colour: an accent bar and a trailing
  coloured dot were both considered and rejected, because either makes unread
  and status impossible to tell apart. When everything is unread the list is
  uniformly bold and has no internal contrast; that is the accepted cost, and
  mock scenario 1A is what it looks like.
- **The dot owns colour**, one hue per status, `bg-status-*`. Unknown takes
  `status-idle`.
- **The one exception to a row's dot and title** is the current-session mark:
  when AYDY knows which session the user is in, that row says **you are here**.
  At most one row ever carries it, which is structural — the registry holds one
  focused window.
- **Group headers** are `<repo> · <branch>`, or the repository alone while the
  branch is unknown, plus one **Not yet identified** group and one **Not
  jumpable** group. They do **not** indent their rows; indentation was tried
  during mock review and rejected. Groups are separated by `group-divider`, the
  1px `neutral-3` line above every group but the first.
- **Hover detail** is a passive reveal on the row itself — not a popup, not a
  click target. It is a second line under the title carrying the status in
  words and both durations, each labelled, so working time is never read as
  elapsed time. It is available on every row, including rows that cannot be
  jumped to.
- **The reveal is CSS**, and never component state. The popover hides without
  unmounting its DOM and the list reorders under a still pointer, and neither
  produces the pointer event a stored flag would need to close again — a row
  would keep showing its working time, longest turn and status with the pointer
  nowhere near it. The engine re-resolves `:hover` after both.
- **The reveal moves nothing**, and it takes two nested groups to do that:
  `group/list` on the list and `group/row` on the row, against
  `hidden invisible group-hover/list:flex group-hover/row:visible` on the
  detail. The space for the line is taken as soon as the pointer is anywhere on
  the list, and hovering a row only paints a line that is already standing
  there. The list's own box is `flex-1` and never resizes with its contents, so
  the pointer being inside it is not something the reveal can change.

  This replaced a per-row `hidden group-hover:flex`, which was the reported
  flicker: driven in a browser at the popover's 320px, a row was 27px tall and
  45px hovered, so every row below the pointer jumped 17px each time the pointer
  crossed a row boundary, and running the mouse down the list was one jump per
  row. There is no feedback loop underneath it — with the pointer held still the
  same row stays hovered — so what the user sees is the jumping alone, and
  standing still is the whole fix. Measured again after: a 125px sweep across
  three rows changes no row's top or height by a pixel.

  Reserving the line in every row all the time was the other way to stand still,
  and it is not the one taken: a blank line under every title turns the one-line
  row four rounds of mock review argued down into a two-line row for as long as
  the popover is open, and cuts a 420px popover from twelve rows to seven. With
  the pointer off the list, the list is exactly what was drawn. The cost that is
  accepted instead is that the list opens up while the pointer is on it.

- **A row's label is rendered exactly as Rust resolves it.** The fallback chain
  and its **Untitled session** placeholder are `DEFAULT_SESSION_LABEL`'s; a
  second copy in the interface would go on showing the old wording after the
  Rust one changed.
- **The empty state** says **Nothing running** and explains that starting Claude
  Code in any project makes it appear.

### Clicking a row

- Every row is interactive and every click marks its session read. Without the
  companion extension that click is the only way to clear unread, so a row that
  cannot be jumped to stays live even though the mock draws it inert.
- Only a jumpable row navigates, and navigating is what closes the popover. A
  click that only marks a session read leaves the popover open, since the user
  is still reading the list.
- A row that cannot be jumped to is drawn so the user does not expect otherwise:
  no hover background and the default cursor, inside a group headed **Not
  jumpable**.
- The popover is closed by a Rust command rather than the window API. Tauri's
  `removeUnusedCommands` strips `core:window:hide` from the build, and routing
  every hide through `tray::popover::hide` is what keeps the tray icon's
  click-to-close bookkeeping seeing all of them.

### What clears an unread

Two things, and nothing else — there is no dismiss control:

- **A click on the row**, jumpable or not.
- **A window reporting that it has focus**, which clears every session that
  window is running.

**The second one departs from the written requirement, on the user's word at the
acceptance gate.** As specified, focus cleared only the session owning the
window's _active tab_, so that focusing a window holding four sessions would not
clear three the user had not looked at. Against a real editor that rule almost
never fires. A window reports no Claude tab in front whenever the thing in front
is a source file, a diff, a terminal or the settings pane — which is nearly
always, for anyone actually working — and even with the panel in front the tab is
matched by its visible label, which fails for a session with no generated title
yet, one whose tab still shows its first prompt, and one the user renamed by
hand. So the user came back to a window, sat in it, and the mark stayed up. Their
words: _when I open or interact with a window, it does not reset the notification
that there are new things to look at._

Window membership comes from the process tree instead of from a tab label, so it
holds for every session the join placed, titled or not. The scoping the original
rule was protecting is kept where it matters: a session belongs to exactly one
window, so focus in one window never touches another's, and a session nothing
placed in a window — the ordinary state with no companion extension installed —
is reachable only by a click, exactly as before.

**Only a report from the editor clears this way.** The registry re-matches every
window's reported tab label on each scan, because titles arrive up to a scan
after the tab that shows them; that re-match is not the user arriving anywhere.
Sweeping there would take an unread back within a scan of raising it, and a
session that finished behind the tab in front would never be announced at all.
The re-match still clears the one session it newly identifies as the focused
window's active tab, which is the case it exists for.

**What did not widen, deliberately:**

- **Whether an event raises an unread at all**, and with it whether a sound
  plays. Reading is the app being told the user has arrived; staying silent is
  the app deciding not to tell them in the first place, and the window someone is
  typing in is exactly where a Claude tab behind the one in front goes unnoticed.
  Widened, a session that finished in the focused window would get no sound, no
  mark and no icon — announced nowhere, which is the opposite of what was asked
  for. Suppression stays on the strict rule: the session owns the active tab
  **and** that tab's window has focus.
- **The you are here mark.** Its claim is that this is the session the user is
  in, and at most one row may carry it. On the window rule every row in the
  focused window would claim it, about sessions the user is not in.

The asymmetry that leaves is intended: a session that finishes in a window the
user is already sitting in does sound and does go unread, and clears on their
next move — switching tab, or leaving the window and coming back, both of which
the editor reports.

### What a jump does, and what it says when it cannot

The tab first, then the window: the extension is asked to bring the session's own
tab forward, and only then is the window raised. The other order has the user
watch the tab switch after the window is already in front of them.

**It closes the popover on the answer, not on the click.** A jump that found no
window to raise did not navigate, and that is a click that leaves the popover
open — so the popover is taken down once the app has said which of the two
happened. The wait is invisible in the ordinary case, because the window coming
forward takes focus from the popover and the popover hides on focus loss before
the answer arrives.

**Nothing is reported as a success until the window says so.** Every mechanism
behind a jump fails silently — the editor command answers about a tab rather
than about focus, and `code` answers as soon as it has handed the folder over —
so the app waits for the extension's own focus report, tries once more without
it, and then says it could not. There are two ways for a jump to be said out
loud, and they are not the same weight:

- **VS Code did not come forward** — a `warning`. It may well have worked;
  nothing confirmed it.
- **A new Claude tab opened** — an `error`, and the only banner in the app that
  asks the user to do something to an editor: close the tab without typing in
  it. It means a guard was wrong and a second live copy of that session may now
  be running.

Both are dismissible and both go away by themselves the moment another row is
clicked. A jump that simply found no window says **nothing at all** — that is
the ordinary state of a session AYDY cannot place, and drawing it as a failure
would make an app that is working look broken.

**Without the extension a click still raises a window**, because Claude Code
writes one lock file per editor window naming the folders it has open, and that
answers the only question raising needs: does a window have this folder open? A
session neither the extension nor those files can place goes nowhere, silently.

### Offering the companion extension

A line under the list, above the footer: what the extension adds, and that no
window is reporting yet. It is shown only while none is, it is dismissible, and
being dismissed is remembered — a companion nobody hears about does not get
installed, and one that asks every day is worse than that. It is a quiet line
and never a banner: everything it adds is an improvement on something that
already works, and status colour belongs to status.

It stays off the stale list, which is the app saying it cannot see what is
running — no moment to suggest an improvement to how well it works.

## Setup: the screen before there is a list

AYDY cannot see a single session until it has written hooks into
`~/.claude/settings.json`, so the first thing it ever shows is a request to edit
a file it does not own. The screen appears **whenever the hooks are absent**,
not only on the first launch: a user who removed them by hand gets it back
rather than an empty list that would look like a bug.

- **It explains, then asks.** It states how many hooks go in, which file they go
  in, and that everything else in that file is left untouched — then asks. A
  **Show me the change first** action renders the installer's own plan: the
  hooks to be added with how each runs and its timeout, the entries to be
  removed, and the `hooks` block exactly as it will be written.
- **The hook count is the installer's**, never copy. It is counted from what
  will actually be registered, so registering a sixth hook changes the sentence
  by itself. Five is the number today and it is nowhere in the interface.
- **What will be removed is named before the button, not after it.** Installing
  takes over the `Stop` and `Notification` hooks that play a sound, and deleting
  configuration the user wrote is the most consequential thing this app does.
  Each entry is listed by its event and the command it runs, with the reason
  (AYDY plays those sounds itself, so you do not hear both) and the way out (its
  sounds can be turned off afterwards). It is a warning banner above the
  install, never a footnote below it.
- **A failed write leads with the reassurance.** Every failure path leaves the
  settings file untouched — the write is atomic and aborts before it starts if
  the file changed underneath it — so the banner opens with **Nothing in
  ~/.claude/settings.json was modified**, then the reason, then **Try again**
  and **Reveal file**.
- **What failed decides what Try again does, and nothing installs from a change
  that was never planned.** Planning is its own read and it has its own failure
  — the app has to know the listener's port before it can plan, and a popover
  opened while the listener is still binding gets a plan that timed out. With
  no plan there is no hook count, no removal warning, and nothing has named the
  entries an install would delete, so **Try again** plans again rather than
  writing. The screen offers no action that installs until the plan exists.
- **A settings file that cannot be read at all gets a screen of its own.** Then
  AYDY cannot say whether its hooks are there, so neither the first-run screen
  nor the list is honest — and a failure that nothing renders would leave an
  empty popover that never reads the file again. It states that nothing was
  modified, gives the reason, and offers the same **Try again** and **Reveal
  file**.
- **There is no way to settings, and no footer.** Installing the hooks is the
  only thing that can be done until they exist, so the screen carries no gear
  and no quit. Quit stays on the session list, which is the only screen in the
  mocks that has a footer.
- **The path is written the way the user writes it**, `~/.claude/settings.json`
  rather than an absolute path with their name in it, and it comes from the
  installer — so a `CLAUDE_CONFIG_DIR` pointing somewhere else is shown as it
  is.

## Settings

One screen inside the popover — not a window, not a sheet. It is reached from
the gear in the list header and left by the ✕ in its own, and those are the only
two ways in and out. The screens that ask for the hooks carry a header of their
own — the same shared one — but **no gear in it**, so there is nothing to reach
settings from until the hooks exist: installing them is still the only thing
that can be done. The gear is passed into the shared header by the list, never
built into it, or hoisting the header would carry the gear onto those screens
with it.

**It renders and sends; it does not decide.** Every command answers with the
settings as they now are — so what is drawn is the state the app is in, never
the one that was asked for. The one thing the pane owes the user is the
**reason** a change did not take.

**A control sends only itself.** Each change carries the one field that moved,
so a pane rendered from a copy taken a moment ago cannot put back a value
something else has changed since.

- **Sounds** — one switch per event.
- **General** — launch at login.
- **Companion extension** — that it exists, what it adds, and whether it is
  connected. It is not in the mock: a companion the user never hears about does
  not get installed, and several behaviours degrade quietly without it.
- **Hooks** — see below.

**There is no listener port, and there is not going to be one.** Choosing a port
is the decision the app exists to make for itself; mock scenario 5's caption
still describes a field for it and is stale.

### Hook status: two facts, never one

Whether the hooks are **installed** and whether their events are **arriving**
are stated as separate lines. Hooks registered against a port nothing is
listening on are installed and silent — exactly the state the port-move and
stale notices exist to explain — and a pane that reported only that its hooks
exist would say nothing at all about it.

When it is that state, a **warning banner** below the two lines says what is
wrong and which action repairs it. It is the only state on the screen the user
can act on, and it is the same condition the stale notice raises as a warning
over the session list — the two are one fact and are given one weight.

- **Reinstall** rewrites the entries in place, against the port the listener
  currently holds.
- **Remove hooks** takes AYDY's entries out and **puts back the sound hooks the
  install replaced**. That restoration is what made removing them acceptable in
  the first place, so it is stated on the screen rather than assumed.
- **Removing does not quit.** The app goes back to the Setup screen, which is
  what it shows whenever the hooks are absent, and settings closes behind it —
  a pane managing hooks that are no longer there is not a screen to leave up.
- Either action can fail, and every failure leaves the settings file exactly as
  it was, so the banner leads with that and then gives the reason.

### Two departures from mock scenario 5

Both are the status-colour reservation in `ui-conventions.md`: colour that means
status may only be used by status.

- **A switch that is on is `brand-primary`, not the mock's green.** Green here
  is `status-done` doing duty as an accent.
- **Remove hooks is a neutral button, not a red one.** The consequence is
  carried by the sentence under it, which has to be read anyway — it names what
  comes back.

## Notices

Two things can be true about the app itself while the list is up, and both are
banners across the top of the popover rather than system notifications.

- **The port moved** — a `notice`, the neutral tint. The listener found its
  remembered port taken, took another, and rewrote its own registered hooks
  before this is ever shown. It names both ports and says the move is already
  resolved. The port appears **nowhere else in the interface**, and nothing here
  offers one to choose: choosing is the decision the app exists to make for
  itself. Mock scenario 6 draws this amber and its caption calls it an error;
  both are stale, and after a successful move nothing is wrong.
- **The list is stale** — a `warning`. Once the hooks exist, events that go
  nowhere mean the list cannot be trusted, so it is **replaced**, not annotated:
  the notice plus how many sessions were last seen and how long ago. A list left
  up would be presented as current, and an empty one would read as "nothing is
  running". This state cannot apply before the hooks exist — nothing is being
  sent then, so nothing is being lost, and a freshly installed app must not
  present itself as broken.
  - **The header goes with it.** The count in the header is the live one and
    the notice's count is the frozen one, so a header left counting contradicts
    the notice underneath it: session discovery reaps sessions on its own scan
    while events cannot reach the app, which drains the live count to zero
    during the spell and reads as **No sessions** above **Last seen 3
    sessions**. The header says **Reconnecting** instead, which is what mock
    scenario 6 draws.

Banners have three variants and one tint each: `notice` (neutral, something
that has already resolved itself), `warning` (the amber status tint, something
is wrong now) and `error` (the red status tint, an action failed). The tinted
backgrounds are what `statustransparent-*` exists for; the text stays
`neutral-7`, because status colour on the text would compete with the status
dots below it.

### Opening something in front of the popover

The popover hides on focus loss, so **any action that opens the Finder or a
native dialog has to arm the focus suppression first** — `suppress_next_focus_loss()`
in `tray::popover`, which lets exactly one focus loss pass without hiding.
**Reveal file** is the first such action; settings will have more.

It is armed for one event and expires after two seconds, rather than
suppressing everything for a while: once the popover is unfocused there is no
second focus loss to fall back on, so a suppression that outlived its action
would swallow the click the user dismisses the popover with. The escape hatch,
if a suppressed popover is left up with the pointer elsewhere, is the tray icon,
which still toggles it.

### Where the numbers come from

Rust derives grouping, ordering, both counts and the icon state from a single
registry snapshot, and pushes it to the popover over a `tauri::ipc::Channel` at
most ten times a second, and only when it has changed. The interface renders
what it is given and derives nothing about state on its own — a second
derivation in the frontend is how an icon and a list start to disagree.
