//! The two sounds a session plays when it wants the user back.
//!
//! These are not an addition. Installing AYDY takes the user's `afplay` hooks
//! off `Stop` and `Notification`, so playing these sounds is the whole
//! justification for that removal — an install that took the sounds away and
//! gave nothing back would leave the user worse off than before they had the
//! app. They are deliberately the same two files those hooks played, so the
//! takeover is invisible rather than something to relearn.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use crate::clock::Clock;
use crate::listener::server::lock_recovering;
use crate::notify::{transition_alert, Alert};
use crate::session::event::SessionId;
use crate::session::registry::RegistrySnapshot;
use crate::session::status::SessionStatus;
use crate::settings::store::Settings;

/// The player the removed hooks used, by absolute path.
///
/// An app launched by `launchd` or by the Finder inherits a minimal `PATH`, so
/// the location is spelled out rather than looked up.
const PLAYER: &str = "/usr/bin/afplay";

/// The sound a session that has finished plays.
pub const FINISHED_SOUND: &str = "/System/Library/Sounds/Funk.aiff";

/// The sound a session that has begun waiting on the user plays.
pub const NEEDS_INPUT_SOUND: &str = "/System/Library/Sounds/Hero.aiff";

/// How long one kind of sound stays quiet after it plays.
///
/// Ten sessions finishing together is one thing that happened, not ten, and ten
/// overlapping copies of the same sound carry less than one does. Each kind
/// keeps its own window, so a session finishing and another starting to wait
/// still say two different things at the same moment.
pub const QUIET_WINDOW: Duration = Duration::from_secs(2);

/// One of the two sounds AYDY plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    /// A session finished.
    Finished,
    /// A session began needing input.
    NeedsInput,
}

impl Sound {
    /// The file this sound plays.
    pub fn file(self) -> &'static str {
        match self {
            Self::Finished => FINISHED_SOUND,
            Self::NeedsInput => NEEDS_INPUT_SOUND,
        }
    }

    /// Whether the user has this sound turned on. The two toggles are read
    /// separately, so one being off says nothing about the other.
    fn enabled_in(self, settings: &Settings) -> bool {
        match self {
            Self::Finished => settings.finished_sound,
            Self::NeedsInput => settings.needs_input_sound,
        }
    }
}

/// A sound that could not be played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundError(pub String);

impl fmt::Display for SoundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SoundError {}

/// Something that can play a sound.
///
/// Behind a trait so tests assert on what was asked for rather than on what
/// came out of the speakers.
pub trait SoundPlayer: Send + Sync {
    /// Starts `sound` playing. Returns as soon as it has started, never when it
    /// has finished.
    fn play(&self, sound: Sound) -> Result<(), SoundError>;
}

/// The invocation that plays `sound`.
///
/// Built here rather than inline on the playing thread so that a test can read
/// back the program and the argument. This is the one line in the module that
/// reaches the speakers, and the only one no other test can see: the wrong path
/// in it is silence, or the wrong sound, with everything else still green — and
/// silence is precisely what installing AYDY promised not to leave behind.
fn play_command(sound: Sound) -> Command {
    let mut command = Command::new(PLAYER);

    command
        .arg(sound.file())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command
}

/// The player the running app uses.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSoundPlayer;

impl SoundPlayer for SystemSoundPlayer {
    /// Runs the player on a thread of its own.
    ///
    /// The whole invocation goes on the thread, so the caller — the loop that
    /// also updates the menu bar icon and pushes the session list — pays for
    /// starting a thread and nothing else. Waiting for the child there is what
    /// reaps it: a child nobody waits for stays a zombie for as long as its
    /// parent runs, and AYDY runs for weeks.
    fn play(&self, sound: Sound) -> Result<(), SoundError> {
        std::thread::Builder::new()
            .name("aydy-sound".to_string())
            .spawn(move || match play_command(sound).status() {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    eprintln!("aydy: playing {} finished with {status}", sound.file());
                }
                Err(error) => eprintln!("aydy: could not play {}: {error}", sound.file()),
            })
            .map(drop)
            .map_err(|error| SoundError(format!("no thread to play {} on: {error}", sound.file())))
    }
}

impl From<Alert> for Sound {
    fn from(alert: Alert) -> Self {
        match alert {
            Alert::Finished => Self::Finished,
            Alert::NeedsInput => Self::NeedsInput,
        }
    }
}

/// The sound a session earns by moving from `previous` to `status`.
///
/// What counts as a transition is [`transition_alert`]'s. All this adds is
/// which sound says it.
fn transition_sound(previous: Option<SessionStatus>, status: SessionStatus) -> Option<Sound> {
    transition_alert(previous, status).map(Sound::from)
}

/// What one observation has to remember for the next one to mean anything.
#[derive(Default)]
struct NotifierState {
    /// The status each session was in at the previous observation. A session
    /// missing from here has never been observed.
    previous: HashMap<SessionId, SessionStatus>,
    /// When each sound last played.
    last_played: HashMap<Sound, Instant>,
}

impl NotifierState {
    /// Whether `sound` may play now, recording it when it may.
    ///
    /// Only an actual play starts a quiet window, so a sound the user has
    /// turned off cannot silence the next one they left on.
    fn may_play(&mut self, sound: Sound, now: Instant) -> bool {
        if let Some(last) = self.last_played.get(&sound) {
            if now.duration_since(*last) < QUIET_WINDOW {
                return false;
            }
        }

        self.last_played.insert(sound, now);
        true
    }
}

/// Plays the sound each observation of the session registry earns.
pub struct SoundNotifier {
    player: Arc<dyn SoundPlayer>,
    settings: watch::Receiver<Settings>,
    clock: Arc<dyn Clock>,
    state: Mutex<NotifierState>,
}

impl SoundNotifier {
    /// A notifier that plays through `player`, reading the settings as they are
    /// pushed rather than by asking for them.
    pub fn new(
        player: Arc<dyn SoundPlayer>,
        settings: watch::Receiver<Settings>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            player,
            settings,
            clock,
            state: Mutex::new(NotifierState::default()),
        }
    }

    /// Plays whatever the difference between this snapshot and the previous one
    /// earns.
    pub fn observe(&self, snapshot: &RegistrySnapshot) {
        let settings = *self.settings.borrow();
        let now = self.clock.now();
        let mut state = lock_recovering(&self.state);

        let mut previous = HashMap::with_capacity(snapshot.session_count);
        let mut earned: HashSet<Sound> = HashSet::new();

        for session in snapshot
            .groups
            .iter()
            .flat_map(|group| group.sessions.iter())
        {
            let was = state.previous.get(&session.id).copied();
            previous.insert(session.id.clone(), session.status);

            let Some(sound) = transition_sound(was, session.status) else {
                continue;
            };

            // The registry owns the whole of this rule, and it is asked rather
            // than re-derived here: a session is the current one only when it
            // owns the active tab **and** that tab's window has focus. A tab
            // left active in a window the user has walked away from still
            // sounds, which is the case the app exists for.
            if session.is_current {
                continue;
            }

            earned.insert(sound);
        }

        // Rebuilt from the snapshot, and from every session in it whatever that
        // session earned. A transition the user was watching, or one their
        // toggle silenced, has still happened: holding the status from before it
        // would sound the moment they looked away, and dropping the session
        // outright would make the transition after it look like a first
        // sighting and swallow that one instead. Rebuilding is also what
        // forgets a session that has ended, so one that comes back is new.
        state.previous = previous;

        for sound in [Sound::NeedsInput, Sound::Finished] {
            if !earned.contains(&sound) || !sound.enabled_in(&settings) {
                continue;
            }

            if state.may_play(sound, now) {
                self.play(sound);
            }
        }
    }

    /// Plays a sound, containing anything that goes wrong.
    ///
    /// A missing file, a busy audio device or a player that comes apart is a
    /// sound the user does not hear. It is never a reason for the app to stop
    /// watching their sessions, which is the thing they installed it for.
    fn play(&self, sound: Sound) {
        let player = Arc::clone(&self.player);

        match catch_unwind(AssertUnwindSafe(move || player.play(sound))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("aydy: {error}"),
            Err(_) => eprintln!("aydy: playing a sound panicked; carrying on without it"),
        }
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// How a test player answers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Answer {
        /// The sound played.
        Played,
        /// The sound could not be played.
        Failed,
        /// The player came apart.
        Panicked,
    }

    /// A player that records what it was asked for and answers as told.
    #[derive(Debug)]
    pub struct RecordingPlayer {
        answer: Answer,
        asked: Mutex<Vec<Sound>>,
    }

    impl RecordingPlayer {
        /// A player that plays everything it is asked for.
        pub fn new() -> Self {
            Self::answering(Answer::Played)
        }

        /// A player that answers `answer` to everything it is asked for.
        pub fn answering(answer: Answer) -> Self {
            Self {
                answer,
                asked: Mutex::new(Vec::new()),
            }
        }

        /// Everything the player was asked for, in order.
        pub fn asked(&self) -> Vec<Sound> {
            lock_recovering(&self.asked).clone()
        }
    }

    impl Default for RecordingPlayer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SoundPlayer for RecordingPlayer {
        fn play(&self, sound: Sound) -> Result<(), SoundError> {
            lock_recovering(&self.asked).push(sound);

            match self.answer {
                Answer::Played => Ok(()),
                Answer::Failed => Err(SoundError(format!(
                    "the audio device is busy, so {} was not played",
                    sound.file()
                ))),
                Answer::Panicked => panic!("the sound player came apart"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::test_support::{Answer, RecordingPlayer};
    use super::*;
    use crate::clock::TestClock;
    use crate::session::event::{FocusReport, HookEvent, HookEventKind, WindowId};
    use crate::session::registry::SessionRegistry;

    /// A VS Code window, named by the process id of its extension host.
    const WINDOW: WindowId = 65_921;

    /// A real registry with a notifier watching it, driven exactly as the
    /// publishing loop drives them: apply events, take a snapshot, hand it over.
    ///
    /// The registry is real rather than a hand-built snapshot because the
    /// suppression rule this task must honour lives there — asking it is the
    /// whole point, and a hand-built `is_current` would only assert that this
    /// module can read a boolean it set itself.
    struct Harness {
        registry: SessionRegistry,
        notifier: SoundNotifier,
        player: Arc<RecordingPlayer>,
        settings: watch::Sender<Settings>,
        clock: Arc<TestClock>,
    }

    impl Harness {
        fn new() -> Self {
            Self::playing(Answer::Played)
        }

        fn playing(answer: Answer) -> Self {
            let clock = Arc::new(TestClock::new());
            let player = Arc::new(RecordingPlayer::answering(answer));
            let (settings, receiver) = watch::channel(Settings::default());

            Self {
                registry: SessionRegistry::new(Arc::clone(&clock) as Arc<dyn Clock>),
                notifier: SoundNotifier::new(
                    Arc::clone(&player) as Arc<dyn SoundPlayer>,
                    receiver,
                    Arc::clone(&clock) as Arc<dyn Clock>,
                ),
                player,
                settings,
                clock,
            }
        }

        fn hook(&mut self, session_id: &str, kind: HookEventKind) {
            self.registry
                .apply_hook_event(HookEvent::new(session_id, kind));
        }

        fn focus(&mut self, window_id: WindowId, focused: bool, active: Option<&str>) {
            self.registry.apply_focus_report(FocusReport::new(
                window_id,
                focused,
                active.map(str::to_string),
            ));
        }

        /// One observation, far enough after the last one that the quiet window
        /// can never be the reason a sound did not play.
        fn observe(&mut self) {
            self.clock.advance(QUIET_WINDOW);
            self.observe_at_once();
        }

        /// One observation right now, for the tests that are about the quiet
        /// window itself.
        fn observe_at_once(&mut self) {
            let snapshot = self.registry.snapshot();

            self.notifier.observe(&snapshot);
        }

        fn asked(&self) -> Vec<Sound> {
            self.player.asked()
        }

        /// A session that has started and is part-way through a turn, already
        /// observed once so the next observation is a transition.
        fn working(&mut self, session_id: &str) {
            self.hook(session_id, HookEventKind::SessionStart);
            self.hook(session_id, HookEventKind::UserPromptSubmit);
            self.observe();
        }
    }

    /// A permission prompt, as the listener delivers one.
    fn needs_input() -> HookEventKind {
        HookEventKind::Notification {
            message: Some("Needs permission to run npm test".to_string()),
        }
    }

    #[test]
    fn a_session_that_finishes_plays_the_finished_sound() {
        let mut harness = Harness::new();
        harness.working("session-1");

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn a_session_that_begins_needing_input_plays_the_needs_input_sound() {
        let mut harness = Harness::new();
        harness.working("session-1");

        harness.hook("session-1", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput]);
    }

    /// The two events have to be told apart from across a room, which is the
    /// only place they are ever heard from.
    #[test]
    fn the_two_sounds_are_the_two_the_replaced_hooks_played() {
        assert_eq!(Sound::Finished.file(), "/System/Library/Sounds/Funk.aiff");
        assert_eq!(Sound::NeedsInput.file(), "/System/Library/Sounds/Hero.aiff");
        assert_ne!(Sound::Finished.file(), Sound::NeedsInput.file());
    }

    /// What the real player actually runs. Playing cannot be heard from a test,
    /// but the command that would be played can be read back, and it is the one
    /// thing here whose being wrong is inaudible in every other test: a player
    /// that is not `afplay`, or a file that is not this sound's, leaves the user
    /// whose hooks were removed hearing the wrong thing or nothing at all.
    ///
    /// Both are spelled out rather than compared against the constants, since a
    /// constant compared against itself agrees with any value it is given.
    #[test]
    fn playing_a_sound_runs_afplay_on_that_sound_and_nothing_else() {
        for (sound, file) in [
            (Sound::Finished, "/System/Library/Sounds/Funk.aiff"),
            (Sound::NeedsInput, "/System/Library/Sounds/Hero.aiff"),
        ] {
            let command = play_command(sound);

            assert_eq!(command.get_program(), "/usr/bin/afplay");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                vec![OsStr::new(file)]
            );
        }
    }

    /// Nothing about the real player can be asserted from a test without making
    /// a noise, so what is asserted is that everything it names is there. An app
    /// launched at login has almost no `PATH`, which is why the player is an
    /// absolute path and why this checks that path rather than the name.
    #[test]
    fn the_player_and_both_sounds_are_files_this_machine_actually_has() {
        assert!(
            Path::new(PLAYER).exists(),
            "{PLAYER} is not on this machine"
        );

        for sound in [Sound::Finished, Sound::NeedsInput] {
            assert!(
                Path::new(sound.file()).exists(),
                "{} is not on this machine",
                sound.file()
            );
        }
    }

    /// The trap this whole module is shaped around: a status that is read
    /// rather than a transition that is detected sounds ten times a second.
    #[test]
    fn a_session_that_stays_finished_does_not_sound_again() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.hook("session-1", HookEventKind::Stop);

        harness.observe();
        harness.observe();
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn a_session_that_stays_needing_input_does_not_sound_again() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.hook("session-1", needs_input());

        harness.observe();
        harness.observe();
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput]);
    }

    /// Launching AYDY beside a day's worth of finished sessions must be silent.
    #[test]
    fn a_session_first_seen_already_finished_does_not_sound() {
        let mut harness = Harness::new();
        harness.hook("session-1", HookEventKind::SessionStart);

        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    #[test]
    fn a_session_first_seen_already_needing_input_does_not_sound() {
        let mut harness = Harness::new();
        harness.hook("session-1", needs_input());

        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    #[test]
    fn a_session_that_answers_a_prompt_and_then_finishes_plays_the_finished_sound() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.hook("session-1", needs_input());
        harness.observe();

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput, Sound::Finished]);
    }

    #[test]
    fn turning_the_finished_sound_off_leaves_the_needs_input_sound_on() {
        let mut harness = Harness::new();
        harness.settings.send_replace(Settings {
            finished_sound: false,
            ..Settings::default()
        });
        harness.working("session-1");
        harness.working("session-2");

        harness.hook("session-1", HookEventKind::Stop);
        harness.hook("session-2", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput]);
    }

    #[test]
    fn turning_the_needs_input_sound_off_leaves_the_finished_sound_on() {
        let mut harness = Harness::new();
        harness.settings.send_replace(Settings {
            needs_input_sound: false,
            ..Settings::default()
        });
        harness.working("session-1");
        harness.working("session-2");

        harness.hook("session-1", HookEventKind::Stop);
        harness.hook("session-2", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    /// The toggle is read from the channel it is pushed on, so a change made in
    /// the pane is in force for the very next event.
    #[test]
    fn a_toggle_moved_while_the_app_runs_takes_effect_without_a_restart() {
        let mut harness = Harness::new();
        harness.working("session-1");

        harness.settings.send_replace(Settings {
            finished_sound: false,
            ..Settings::default()
        });
        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    #[test]
    fn a_session_whose_tab_is_active_in_a_focused_window_stays_silent() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, true, Some("session-1"));

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    #[test]
    fn a_session_whose_tab_is_active_in_a_focused_window_stays_silent_when_it_needs_input() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, true, Some("session-1"));

        harness.hook("session-1", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    /// The half of the suppression rule that must not be collapsed. A tab left
    /// active in a window the user has walked away from is exactly the case
    /// AYDY exists for, and silencing it would be worse than not playing at all.
    #[test]
    fn a_session_whose_tab_is_active_in_an_unfocused_window_still_sounds() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, false, Some("session-1"));

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn a_session_whose_tab_is_active_in_an_unfocused_window_still_sounds_when_it_needs_input() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, false, Some("session-1"));

        harness.hook("session-1", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput]);
    }

    /// The other half: focus alone is not enough either. The user is looking at
    /// one tab, not at every session open in that window.
    #[test]
    fn a_session_in_a_background_tab_of_a_focused_window_still_sounds() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, true, Some("session-2"));

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    /// Without the companion extension no tab is ever reported active, so every
    /// transition sounds. That is the expected state, not a degraded one.
    #[test]
    fn a_session_with_no_focus_reported_at_all_sounds() {
        let mut harness = Harness::new();
        harness.working("session-1");

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    /// A transition the user watched has still happened, so the status it
    /// arrived at is what the next observation compares against. Holding the
    /// status from before it would sound the moment they looked away.
    #[test]
    fn a_suppressed_transition_does_not_sound_when_the_window_later_loses_focus() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, true, Some("session-1"));
        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        harness.focus(WINDOW, false, Some("session-1"));
        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    /// The other direction of the same rule. Forgetting a session because the
    /// user was watching it would make its next transition look like a first
    /// sighting, and a first sighting earns nothing — so the sound after the
    /// one that was rightly suppressed would be swallowed too.
    #[test]
    fn a_suppressed_transition_leaves_the_next_one_audible() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.focus(WINDOW, true, Some("session-1"));
        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        harness.focus(WINDOW, false, None);
        harness.hook("session-1", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput]);
    }

    #[test]
    fn ten_sessions_finishing_together_play_one_sound() {
        let mut harness = Harness::new();

        for index in 0..10 {
            harness.working(&format!("session-{index}"));
        }
        for index in 0..10 {
            harness.hook(&format!("session-{index}"), HookEventKind::Stop);
        }
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn a_second_session_finishing_inside_the_quiet_window_is_coalesced() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.working("session-2");
        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        harness.clock.advance(QUIET_WINDOW / 2);
        harness.hook("session-2", HookEventKind::Stop);
        harness.observe_at_once();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn a_session_finishing_after_the_quiet_window_sounds_again() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.working("session-2");
        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        harness.clock.advance(QUIET_WINDOW);
        harness.hook("session-2", HookEventKind::Stop);
        harness.observe_at_once();

        assert_eq!(harness.asked(), vec![Sound::Finished, Sound::Finished]);
    }

    /// The two kinds are rate-limited apart, because they are two different
    /// things to say.
    #[test]
    fn a_finish_and_a_wait_at_the_same_moment_both_sound() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.working("session-2");

        harness.hook("session-1", HookEventKind::Stop);
        harness.hook("session-2", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::NeedsInput, Sound::Finished]);
    }

    /// A sound the user turned off must not start a quiet window, or turning it
    /// back on would be silent for the next two seconds.
    #[test]
    fn a_sound_the_toggle_silenced_does_not_start_a_quiet_window() {
        let mut harness = Harness::new();
        harness.settings.send_replace(Settings {
            finished_sound: false,
            ..Settings::default()
        });
        harness.working("session-1");
        harness.working("session-2");
        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();

        harness.settings.send_replace(Settings::default());
        harness.hook("session-2", HookEventKind::Stop);
        harness.observe_at_once();

        assert_eq!(harness.asked(), vec![Sound::Finished]);
    }

    #[test]
    fn a_player_that_fails_does_not_stop_the_next_sound() {
        let mut harness = Harness::playing(Answer::Failed);
        harness.working("session-1");
        harness.working("session-2");

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();
        harness.hook("session-2", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished, Sound::NeedsInput]);
    }

    #[test]
    fn a_player_that_fails_does_not_stop_session_tracking() {
        let mut harness = Harness::playing(Answer::Failed);
        harness.working("session-1");
        harness.hook("session-1", HookEventKind::Stop);

        harness.observe();
        harness.hook("session-1", HookEventKind::UserPromptSubmit);

        let snapshot = harness.registry.snapshot();
        assert_eq!(snapshot.session_count, 1);
        assert_eq!(
            snapshot.groups[0].sessions[0].status,
            SessionStatus::Working
        );
    }

    #[test]
    fn a_player_that_panics_does_not_stop_the_next_sound() {
        let mut harness = Harness::playing(Answer::Panicked);
        harness.working("session-1");
        harness.working("session-2");

        harness.hook("session-1", HookEventKind::Stop);
        harness.observe();
        harness.hook("session-2", needs_input());
        harness.observe();

        assert_eq!(harness.asked(), vec![Sound::Finished, Sound::NeedsInput]);
    }

    #[test]
    fn a_session_that_is_removed_and_seen_again_is_a_new_session() {
        let mut harness = Harness::new();
        harness.working("session-1");
        harness.hook("session-1", HookEventKind::SessionEnd);
        harness.observe();

        harness.hook("session-1", HookEventKind::SessionStart);
        harness.observe();

        assert_eq!(harness.asked(), Vec::new());
    }

    #[test]
    fn a_status_observed_for_the_first_time_earns_nothing() {
        assert_eq!(transition_sound(None, SessionStatus::Done), None);
        assert_eq!(transition_sound(None, SessionStatus::NeedsInput), None);
    }

    #[test]
    fn a_status_that_has_not_changed_earns_nothing() {
        assert_eq!(
            transition_sound(Some(SessionStatus::Done), SessionStatus::Done),
            None
        );
        assert_eq!(
            transition_sound(Some(SessionStatus::NeedsInput), SessionStatus::NeedsInput),
            None
        );
    }

    #[test]
    fn working_and_unknown_earn_nothing() {
        assert_eq!(
            transition_sound(Some(SessionStatus::Done), SessionStatus::Working),
            None
        );
        assert_eq!(
            transition_sound(Some(SessionStatus::Working), SessionStatus::Unknown),
            None
        );
    }
}
