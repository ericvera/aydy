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

| #   | State         | Shape                                       | Count            |
| --- | ------------- | ------------------------------------------- | ---------------- |
| 1   | Needs input   | disc with an exclamation mark               | unread sessions  |
| 2   | Something new | solid disc                                  | unread sessions  |
| 3   | Stale         | ring drawn dashed with its left half filled | stale sessions   |
| 4   | Working       | ring with its left half filled              | working sessions |
| 5   | All quiet     | thin ring                                   | none             |

1. **Needs input** — at least one **unread** session is waiting on the user.
2. **Something new** — at least one **unread** session in any other status. This
   is the state the app is named after: work finished while you were away and
   you have not looked yet. It outranks working, because working is the state
   you already expect.
3. **Stale** — a Working session has written nothing to its transcript,
   subagent transcripts included, for 5 minutes. Stale is a signal beside the
   status, not a status: the session is still Working. It ranks below anything
   unread, because a stuck session matters less than one already waiting on
   you. While a session is stale the icon gives up the working count; the
   popover still shows it.
4. **Working** — something is running and nothing is waiting to be looked at.
5. **All quiet** — nothing needs attention.

**A session that has been read never raises the icon above working**, even while
its status is still Needs input. Reading is what makes the amber state drop;
nothing is dismissed by hand. Mock scenario 9 is this rule in motion — the
moment a window regains focus, its row clears and the icon falls from state 1 to
state 2.

### Counts

States 1 and 2 count **unread** sessions — every unread session, not only the
waiting ones, so the number matches the popover header's `· N new`. State 3
counts **stale** sessions, and state 4 counts **working** sessions. State 5
carries no count at all.

The count is the tray item's title, drawn beside the icon in the menu bar font.

### Why shape and not colour

The mock draws the states in status colours. The shipped icons cannot:
macOS menu bar icons are **template images**, black plus alpha, tinted by the
system to suit the menu bar they land in — which is what makes an icon legible
in both a light and a dark menu bar, and in front of any wallpaper. Colour is
therefore not available to carry meaning, and the five states have to be told
apart by silhouette. The circle family of the mock is kept; the fill is what
varies:

- empty ring → nothing to see
- half-filled ring → part way through
- half-filled ring drawn dashed → working, but the line has broken up
- solid disc → finished
- disc with an exclamation → finished **and** blocked on you

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

Any later screen — setup, settings — belongs **inside** this
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
   same unread count the icon shows. The settings control sits on the right — a
   gear, and the only way into settings. It is on the list and nowhere else.
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
- **No row says you are here.** The app no longer hears which window has
  focus, so it cannot know which session the user is in, and no row carries
  anything beyond its dot and title.
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

- Every row is interactive and every click marks its session read. A session no
  VS Code window owns is cleared by that click alone, so a row that cannot be
  jumped to stays live even though the mock draws it inert.
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

A session is **unread** when its last event is a `Stop` or a `Notification` and
that event is newer than the session's last seen time. A `Stop` sent while
subagents the turn started are still running is not a finished turn: the session
stays Working and is not unread until a `Stop` with no running subagents
arrives. Informational notifications — a background agent completing, a sign-in
succeeding — neither raise nor clear an unread and play no sound. Any later
event — a new prompt, a fresh start — makes it read again whatever the seen time
says. There is no dismiss control. Two things write a seen time, and nothing
else:

- **The VS Code window that owns the session, while it has focus.** It marks
  every one of its sessions seen when it gains focus, and again whenever one of
  them changes while it still has it. So arriving at a window clears all of its
  sessions, and a session that finishes in the window the user is sitting in is
  never unread at all.
- **A click on the row in the popover**, jumpable or not. For a session no
  window owns, this is the only way it clears.

The VS Code tab count — `● N repo · branch` in each window's title — counts
that window's unread sessions by the same rule, read from the same files, so the
window and the popover cannot disagree about what is new. With nothing unread,
the title shows `◐` while one of the window's prompts is running, or while a
session's subagents are still running, matching the menubar's working ring, and
`○` otherwise.

### What a jump does, and what it says when it cannot

A click writes a request file naming the session. The one VS Code window that
owns the session picks it up, brings the session's own tab forward and answers
with what to raise; only then does the app raise it. The tab moves first, so
the user never watches it switch after the window is already in front of them.

**It closes the popover on the answer, not on the click.** A jump that found no
window did not navigate, and that is a click that leaves the popover open — so
the popover is taken down once the app has said which of the two happened. The
wait is invisible in the ordinary case, because the window coming forward takes
focus from the popover and the popover hides on focus loss before the answer
arrives.

**Nothing is reported as a success until a window answers.** If no window picks
the request up within a few seconds, the app stops waiting and says so. There
are two ways for a jump to be said out loud, and they are not the same weight:

- **No VS Code window took the jump.** The window that had this session may be
  closed — a `warning`. Nothing moved, and nothing needs undoing.
- **A new Claude tab opened** — an `error`, and the only banner in the app that
  asks the user to do something to an editor: close the tab without typing in
  it. It means a guard was wrong and a second live copy of that session may now
  be running.

Both are dismissible and both go away by themselves the moment another row is
clicked. A jump that simply found no window says **nothing at all** — that is
the ordinary state of a session AYDY cannot place, and drawing it as a failure
would make an app that is working look broken.

## Setup: the screen before there is a list

AYDY cannot see a single session until the VS Code extension has put its hook
script in place and registered its hooks in `~/.claude/settings.json`. **The app
installs nothing itself** — the extension writes both, and records sessions even
while the app is closed. The screen appears **whenever tracking is not
installed**, not only on the first launch: an uninstalled extension, or hooks
removed by hand, put it back rather than leaving an empty list that would look
like a bug.

It has three states, all under a header titled **Setup**:

- **Never installed** (no hook script). It says **Install the AYDY extension in
  VS Code**, explains that the extension tracks sessions even while this app is
  closed and AYDY shows what it records, and offers **Show me how**. A note
  under it says that an extension already installed only needs any VS Code
  window opened.
- **Show me how** reveals the steps inline, on the same screen: open the
  Command Palette, run **Extensions: Install from VSIX…**, choose
  **aydy-companion.vsix**, open any project window.
- **Hooks missing** (script present, hook entries gone). It says **AYDY's hooks
  are missing**, that sessions are not being recorded, and that opening any VS
  Code window has the extension put them back. There is nothing to press.

- **It clears by itself.** While tracking is not installed the app checks
  every two seconds, and the list replaces the screen as soon as the files
  appear. Once installed the checking stops, and it is asked again each time
  the popover opens and each time settings opens — the popover is never torn
  down, so an extension uninstalled later is noticed the next time anyone
  looks.
- **Nothing is shown until the first answer.** Guessing would flash the wrong
  screen for a frame.
- **There is no way to settings, and no footer.** Installing the extension is
  the only thing that can be done until tracking exists, so the screen carries
  no gear and no quit. Quit stays on the session list, which is the only screen
  that has a footer.

## Settings

One screen inside the popover — not a window, not a sheet. It is reached from
the gear in the list header and left by the ✕ in its own, and those are the only
two ways in and out. The install screen carries a header of its own — the same
shared one — but **no gear in it**, so there is nothing to reach settings from
until tracking is installed: installing the extension is still the only thing
that can be done. The gear is passed into the shared header by the list, never
built into it, or hoisting the header would carry the gear onto that screen
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
- **VS Code extension** — one **Session tracking** row reading **Installed**,
  with a note that the extension records every Claude Code session whether or
  not AYDY is running, and that AYDY only reads what it records. Settings is
  reachable only once tracking is installed, so the row never says anything
  else; when tracking goes missing the Setup screen replaces the list instead.

### A departure from mock scenario 5

It is the status-colour reservation in `ui-conventions.md`: colour that means
status may only be used by status.

- **A switch that is on is `brand-primary`, not the mock's green.** Green here
  is `status-done` doing duty as an accent.

## Notices

Notices are banners across the top of the popover rather than system
notifications. The jump outcomes above are the ones the list shows.

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

Every number on screen comes from two sets of files under AYDY's data
directory: the **status files** the hook script writes, one per session with its
latest hook event, and the **seen files** the extension and a popover click
write, one per session with when it was last looked at. Rust reads both every
half second and derives grouping, ordering, unread, both counts and the icon
state from a single registry snapshot, then pushes it to the popover over a
`tauri::ipc::Channel` at most ten times a second, and only when it has changed.
The interface renders what it is given and derives nothing about state on its
own — a second derivation in the frontend is how an icon and a list start to
disagree.
