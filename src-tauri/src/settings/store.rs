//! The settings themselves: their defaults, their rules, and their file.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::atomic_file;
use crate::settings::SettingsError;

/// The file AYDY's settings live in, inside the app config directory.
pub const SETTINGS_FILE: &str = "settings.json";

/// Everything the user can change.
///
/// Fields this build does not know — left by an older or newer one — are
/// ignored on read rather than failing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The sound a finished session plays (REQ-SOUND-3).
    pub finished_sound: bool,
    /// The sound a session beginning to need input plays (REQ-SOUND-3).
    pub needs_input_sound: bool,
    /// Whether AYDY starts with the user's session (REQ-APP-2).
    pub launch_at_login: bool,
}

impl Default for Settings {
    /// Both sounds on, and launch at login on.
    ///
    /// Launch at login defaults on because a session monitor that is not
    /// running provides nothing while its hooks stay installed — the sessions
    /// carry on, and the only thing missing is the app that would have told the
    /// user about them.
    fn default() -> Self {
        Self {
            finished_sound: true,
            needs_input_sound: true,
            launch_at_login: true,
        }
    }
}

/// A change to some of the settings, as the pane sends it.
///
/// Every field is optional so one toggle can be moved without the interface
/// having to send back the values it did not touch — which is how a stale copy
/// of the settings would otherwise overwrite a change made somewhere else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsChange {
    pub finished_sound: Option<bool>,
    pub needs_input_sound: Option<bool>,
    pub launch_at_login: Option<bool>,
}

/// A login item that could not be registered or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginItemError(pub String);

impl fmt::Display for LoginItemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LoginItemError {}

/// What is registered to start AYDY when the user logs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// Nothing is.
    Absent,
    /// The copy of AYDY that is running.
    Ours,
    /// Some other copy — an AYDY that has been moved or reinstalled since, or a
    /// build that is no longer where the item says it is.
    Elsewhere,
}

/// Whether the app starts with the user's session.
///
/// Behind a trait because registering one writes a plist into the user's
/// `~/Library/LaunchAgents/`, which no test may do.
pub trait LoginItem: Send + Sync + fmt::Debug {
    /// What is registered to launch at login, if anything.
    fn registration(&self) -> Result<Registration, LoginItemError>;

    /// Registers or removes the login item.
    fn set_enabled(&self, enabled: bool) -> Result<(), LoginItemError>;
}

/// The user's settings, live.
///
/// Changes are published to whoever is listening rather than left for them to
/// notice (REQ-SET-5): the sound player holds a receiver, so a toggle moved in
/// the pane is in force for the very next event without a restart and without
/// it re-reading a file.
#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    login_item: Arc<dyn LoginItem>,
    state: watch::Sender<Settings>,
}

impl SettingsStore {
    /// Reads the settings at `path`, falling back to the defaults.
    ///
    /// A file that is missing or that cannot be parsed is not a reason to fail
    /// to start: AYDY is a menu bar app whose whole value is being running, and
    /// the defaults are a working configuration. A corrupt file is replaced by
    /// one holding them, because defaults that are only in force for this run
    /// would be fallen back to again on every launch after it.
    pub fn load(path: impl Into<PathBuf>, login_item: Arc<dyn LoginItem>) -> Self {
        let path = path.into();
        let stored = read(&path);
        let mut settings = stored.settings().unwrap_or_default();

        let launch_at_login = adopt_login_item(
            login_item.as_ref(),
            settings.launch_at_login,
            stored.had_a_file(),
        );
        settings.launch_at_login = launch_at_login.enabled;

        let store = Self {
            path,
            login_item,
            state: watch::Sender::new(settings),
        };

        // A first run writes the defaults out, so the next launch can tell that
        // launch at login has already been offered and stop re-registering an
        // item that is no longer there. A file that could not be read is
        // replaced for the same reason.
        //
        // Not, though, when the login item this run was supposed to register
        // refused: writing the file is what makes the next launch read the
        // system as the truth, and recording "off" over an item that failed to
        // appear would turn one failed write into a permanent opt-out of a
        // setting that defaults on. Leaving the file unwritten leaves the next
        // launch a first run, which tries again.
        if stored.needs_replacing() && launch_at_login.settled {
            if let Err(error) = store.persist(settings) {
                eprintln!("aydy: {error}");
            }
        }

        store
    }

    /// Where the settings file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The settings as they stand.
    pub fn current(&self) -> Settings {
        *self.state.borrow()
    }

    /// A receiver that sees every change as it is made (REQ-SET-5).
    pub fn subscribe(&self) -> watch::Receiver<Settings> {
        self.state.subscribe()
    }

    /// Applies `change`, leaving the settings it says nothing about alone.
    pub fn apply(&self, change: SettingsChange) -> Result<Settings, SettingsError> {
        self.change(|settings| {
            if let Some(enabled) = change.finished_sound {
                settings.finished_sound = enabled;
            }

            if let Some(enabled) = change.needs_input_sound {
                settings.needs_input_sound = enabled;
            }

            if let Some(enabled) = change.launch_at_login {
                settings.launch_at_login = enabled;
            }
        })
    }

    /// Makes `edit` to the settings, then puts the result into force.
    ///
    /// The edit runs under the lock the settings themselves are held by, so it
    /// is one read-modify-write rather than a read, an edit, and a write that
    /// something else can slip between. The pane sends one call per control and
    /// each arrives as a task of its own, so two changes really can be in
    /// flight at once — and a change is deliberately partial, so a copy taken
    /// before another change and written back after it would silently undo it.
    ///
    /// The login item is changed first, because it is the only step that can
    /// fail with nothing else having happened yet; a refusal leaves the
    /// settings exactly as they were. The write to disk comes last but still
    /// under the lock, so the file cannot end up holding an older change than
    /// the one the listeners were given. It comes after the value is in place,
    /// so a settings file that cannot be written — a full disk, a directory the
    /// user has made read-only — still leaves the change in force for this run
    /// rather than refusing it outright.
    fn change(&self, edit: impl FnOnce(&mut Settings)) -> Result<Settings, SettingsError> {
        let mut outcome = None;

        self.state.send_modify(|settings| {
            let previous = *settings;
            edit(settings);

            if settings.launch_at_login != previous.launch_at_login {
                if let Err(error) = self.login_item.set_enabled(settings.launch_at_login) {
                    *settings = previous;
                    outcome = Some(Err(SettingsError::LaunchAtLogin(error.0)));

                    return;
                }
            }

            outcome = Some(self.persist(*settings).map(|()| *settings));
        });

        outcome.expect("the settings to have been edited")
    }

    /// Writes `settings` out, atomically.
    fn persist(&self, settings: Settings) -> Result<(), SettingsError> {
        let mut text = serde_json::to_string_pretty(&settings)
            .expect("a settings value of plain fields to serialize");
        text.push('\n');

        atomic_file::write_atomically(&self.path, &text).map_err(|source| SettingsError::Write {
            path: self.path.clone(),
            source,
        })
    }
}

/// What launch at login came out as, and whether that answer is settled enough
/// to write down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LaunchAtLogin {
    /// What the setting is now, with the system and the stored value agreed.
    enabled: bool,
    /// False when the item this run was asked to register could not be, so the
    /// answer is a failure's and not a decision the user or the system made.
    settled: bool,
}

/// Whether the app launches at login, once the setting and the system agree.
///
/// The login item itself is the truth once there is one to read. An absent one
/// honestly reads as off: it was deleted, or macOS dropped it when the app
/// moved, and a settings pane reporting "on" over an item that is not there
/// would be stating something false. What the rule buys is that AYDY does not
/// put a plist that has gone back on every launch.
///
/// An item naming *another* executable is the one case that is neither. It is
/// not off — the user asked for launch at login and never took it back — but
/// leaving it alone means every login starts a copy of AYDY that may have been
/// moved, replaced or deleted since, which is a login item that silently does
/// nothing. So it stays on and is repointed at the copy that is running.
///
/// It does *not* catch a login item switched off in System Settings. On macOS
/// 13 and later that records an override in the background task database and
/// leaves the plist exactly where it was, so it is invisible from here.
///
/// The stored value is only what the system is set *from*, on the first run —
/// which is where the default of on takes effect — and what it falls back to
/// when the item cannot be read.
fn adopt_login_item(
    login_item: &dyn LoginItem,
    stored: bool,
    had_settings_file: bool,
) -> LaunchAtLogin {
    let found = login_item.registration();
    let registered = found
        .as_ref()
        .ok()
        .map(|found| *found != Registration::Absent);

    let wanted = match found {
        Ok(Registration::Elsewhere) => true,
        Ok(found) if had_settings_file => {
            return LaunchAtLogin {
                enabled: found == Registration::Ours,
                settled: true,
            }
        }
        _ => stored,
    };

    match login_item.set_enabled(wanted) {
        Ok(()) => LaunchAtLogin {
            enabled: wanted,
            settled: true,
        },
        Err(error) => {
            eprintln!("aydy: could not set launch at login: {error}");

            LaunchAtLogin {
                enabled: registered.unwrap_or(wanted),
                settled: false,
            }
        }
    }
}

/// What was found where the settings file goes.
///
/// A file that cannot be parsed is not the same thing as no file at all, though
/// both fall back to the defaults: a file that is there, in any state, is proof
/// that AYDY has run before, and so that the login item is the truth rather
/// than something still to be set for the first time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredSettings {
    /// Nothing has been written yet — a first run.
    FirstRun,
    /// A file that is not settings AYDY can read.
    Unreadable,
    /// What the file held.
    Read(Settings),
}

impl StoredSettings {
    /// The settings the file held, if it held any.
    fn settings(self) -> Option<Settings> {
        match self {
            Self::Read(settings) => Some(settings),
            Self::FirstRun | Self::Unreadable => None,
        }
    }

    /// Whether AYDY has been run before, whatever it left behind.
    fn had_a_file(self) -> bool {
        !matches!(self, Self::FirstRun)
    }

    /// Whether the settings now in force have to be written out.
    fn needs_replacing(self) -> bool {
        !matches!(self, Self::Read(_))
    }
}

/// The settings on disk, or why there are none to be had.
fn read(path: &Path) -> StoredSettings {
    let Ok(text) = fs::read_to_string(path) else {
        return StoredSettings::FirstRun;
    };

    match serde_json::from_str(&text) {
        Ok(settings) => StoredSettings::Read(settings),
        Err(error) => {
            eprintln!(
                "aydy: {} is not settings AYDY can read, so its defaults are used: {error}",
                path.display()
            );

            StoredSettings::Unreadable
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::test_support::scratch_dir;

    /// A login item that records what it was asked to do instead of writing a
    /// plist into `~/Library/LaunchAgents/`.
    #[derive(Debug)]
    struct FakeLoginItem {
        registration: Mutex<Registration>,
        changes: Mutex<Vec<bool>>,
        unreadable: bool,
        refuses: bool,
    }

    impl Default for FakeLoginItem {
        fn default() -> Self {
            Self {
                registration: Mutex::new(Registration::Absent),
                changes: Mutex::new(Vec::new()),
                unreadable: false,
                refuses: false,
            }
        }
    }

    impl FakeLoginItem {
        fn registered(enabled: bool) -> Self {
            Self {
                registration: Mutex::new(if enabled {
                    Registration::Ours
                } else {
                    Registration::Absent
                }),
                ..Self::default()
            }
        }

        /// An item that starts a copy of AYDY somewhere else — where installing
        /// the app leaves a user who had been running a build out of a
        /// directory.
        fn naming_another_copy() -> Self {
            Self {
                registration: Mutex::new(Registration::Elsewhere),
                ..Self::default()
            }
        }

        fn unreadable() -> Self {
            Self {
                unreadable: true,
                ..Self::default()
            }
        }

        fn refusing() -> Self {
            Self::default().that_refuses()
        }

        /// The same item, by a copy of AYDY that is in no position to register
        /// itself.
        fn that_refuses(mut self) -> Self {
            self.refuses = true;
            self
        }

        fn changes(&self) -> Vec<bool> {
            self.changes.lock().expect("the fake login item").clone()
        }

        fn registration_now(&self) -> Registration {
            *self.registration.lock().expect("the fake login item")
        }

        fn enabled(&self) -> bool {
            self.registration_now() != Registration::Absent
        }
    }

    impl LoginItem for FakeLoginItem {
        fn registration(&self) -> Result<Registration, LoginItemError> {
            if self.unreadable {
                return Err(LoginItemError("no launch agents directory".to_string()));
            }

            Ok(self.registration_now())
        }

        fn set_enabled(&self, enabled: bool) -> Result<(), LoginItemError> {
            if self.refuses {
                return Err(LoginItemError("the plist could not be written".to_string()));
            }

            self.changes
                .lock()
                .expect("the fake login item")
                .push(enabled);
            *self.registration.lock().expect("the fake login item") = if enabled {
                Registration::Ours
            } else {
                Registration::Absent
            };

            Ok(())
        }
    }

    /// A store over a scratch file, with both side effects faked.
    fn store_with(
        name: &str,
        contents: Option<&str>,
        login_item: Arc<FakeLoginItem>,
    ) -> SettingsStore {
        let path = scratch_dir(name).join(SETTINGS_FILE);

        if let Some(contents) = contents {
            fs::write(&path, contents).expect("the fixture to be written");
        }

        SettingsStore::load(path, login_item)
    }

    /// A store with a login item that starts registered, so the load leaves the
    /// default in place.
    fn store(name: &str, contents: Option<&str>) -> SettingsStore {
        store_with(name, contents, Arc::new(FakeLoginItem::registered(true)))
    }

    fn file_contents(store: &SettingsStore) -> String {
        fs::read_to_string(store.path()).expect("the settings file to have been written")
    }

    #[test]
    fn both_sounds_are_on_and_it_launches_at_login() {
        let settings = Settings::default();

        assert!(settings.finished_sound);
        assert!(settings.needs_input_sound);
        assert!(settings.launch_at_login);
    }

    #[test]
    fn a_settings_file_that_does_not_exist_yet_yields_the_defaults() {
        let store = store("missing", None);

        assert_eq!(store.current(), Settings::default());
    }

    /// A file AYDY cannot read is not a reason to fail to start, and the
    /// defaults are a working configuration.
    #[test]
    fn a_settings_file_that_cannot_be_parsed_yields_the_defaults() {
        let store = store("corrupt", Some("{ not json at all"));

        assert_eq!(store.current(), Settings::default());
    }

    /// Defaults that are only in force for this run would be fallen back to
    /// again on the next launch, and on every launch after that — so the file
    /// AYDY could not read is replaced by one it can.
    #[test]
    fn a_settings_file_that_cannot_be_parsed_is_replaced_by_one_that_can_be() {
        let store = store("corrupt-replaced", Some("{ not json at all"));

        assert_eq!(
            serde_json::from_str::<Settings>(&file_contents(&store)).expect("readable settings"),
            Settings::default()
        );
    }

    /// A file that cannot be parsed still proves AYDY has run before, so the
    /// login item is the truth for it exactly as it is for a file that reads.
    /// Damaged settings are not a reason to register a login item that has been
    /// removed.
    #[test]
    fn a_settings_file_that_cannot_be_parsed_still_takes_the_login_item_as_the_truth() {
        let login_item = Arc::new(FakeLoginItem::registered(false));
        let store = store_with(
            "corrupt-login-item",
            Some("{ not json at all"),
            Arc::clone(&login_item),
        );

        assert!(!store.current().launch_at_login);
        assert_eq!(login_item.changes(), Vec::<bool>::new());
    }

    /// A file holding some of the settings is not a corrupt one — an older
    /// version wrote it, or a newer one did.
    #[test]
    fn a_settings_file_holding_only_some_of_the_settings_defaults_the_rest() {
        let store = store(
            "partial",
            Some("{ \"finishedSound\": false, \"somethingElse\": 3 }"),
        );

        assert!(!store.current().finished_sound);
        assert!(store.current().needs_input_sound);
    }

    #[test]
    fn settings_saved_earlier_are_read_back() {
        let store = store(
            "saved",
            Some("{ \"finishedSound\": false, \"needsInputSound\": false }"),
        );

        assert!(!store.current().finished_sound);
        assert!(!store.current().needs_input_sound);
    }

    /// A first run writes the file, so the next launch can tell it is not one.
    #[test]
    fn a_first_run_writes_the_defaults_out() {
        let store = store("first-run", None);

        assert_eq!(
            serde_json::from_str::<Settings>(&file_contents(&store)).expect("readable settings"),
            Settings::default()
        );
    }

    #[test]
    fn a_change_is_written_to_the_settings_file() {
        let store = store("written", None);

        store
            .apply(SettingsChange {
                finished_sound: Some(false),
                needs_input_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the change to be applied");

        let written: Settings =
            serde_json::from_str(&file_contents(&store)).expect("readable settings");
        assert!(!written.finished_sound);
        assert!(!written.needs_input_sound);
    }

    /// A rename leaves nothing beside the file it replaced, so the app config
    /// directory does not accumulate one temporary file per change.
    #[test]
    fn saving_leaves_nothing_beside_the_settings_file() {
        let store = store("atomic", None);

        for enabled in [false, true, false, true] {
            store
                .apply(SettingsChange {
                    finished_sound: Some(enabled),
                    ..SettingsChange::default()
                })
                .expect("the change to be applied");
        }

        let directory = store.path().parent().expect("a parent directory");
        let left: Vec<String> = fs::read_dir(directory)
            .expect("the directory to be readable")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();

        assert_eq!(left, vec![SETTINGS_FILE.to_string()]);
    }

    /// A settings file half written by a crash would come back as a corrupt one
    /// and take every setting with it, so the save has to replace the file
    /// rather than fill it. A hard link keeps the original inode, so it holds
    /// what the file held before a rename and what it holds after a write in
    /// place.
    #[test]
    fn saving_replaces_the_settings_file_rather_than_writing_over_it() {
        let store = store("atomic-inode", None);
        let link = store.path().with_extension("json.link");
        fs::hard_link(store.path(), &link).expect("a second name for the original");
        let before = file_contents(&store);

        store
            .apply(SettingsChange {
                finished_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the change to be applied");

        assert!(file_contents(&store).contains("\"finishedSound\": false"));
        assert_eq!(
            fs::read_to_string(&link).unwrap(),
            before,
            "the new settings went into the file that was already there"
        );
    }

    /// REQ-SET-5. The sound player holds a receiver rather than re-reading the
    /// file, so a change has to arrive on it.
    #[test]
    fn a_change_reaches_a_listener_without_anything_being_read_back() {
        let store = store("published", None);
        let mut listener = store.subscribe();
        fs::remove_file(store.path()).expect("the settings file to go");

        store
            .apply(SettingsChange {
                needs_input_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the change to be applied");

        assert!(listener.has_changed().expect("the sender to be alive"));
        assert!(!listener.borrow_and_update().needs_input_sound);
    }

    #[test]
    fn a_listener_that_subscribes_late_sees_what_the_settings_already_are() {
        let store = store("published-late", None);
        store
            .apply(SettingsChange {
                finished_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the change to be applied");

        assert!(!store.subscribe().borrow().finished_sound);
    }

    /// A settings file that cannot be written must not stop the change taking
    /// effect for this run — it is only the copy that outlives the process.
    #[test]
    fn a_change_that_cannot_be_written_is_reported_and_still_takes_effect() {
        let store = store("unwritable", None);
        let directory = store.path().parent().expect("a parent directory");
        fs::remove_file(store.path()).expect("the settings file to go");
        fs::create_dir(store.path()).expect("something else to take its name");

        let failed = store.apply(SettingsChange {
            finished_sound: Some(false),
            ..SettingsChange::default()
        });

        fs::remove_dir_all(directory).expect("the scratch directory to be removable");

        assert!(matches!(failed, Err(SettingsError::Write { .. })));
        assert!(!store.current().finished_sound);
    }

    /// REQ-APP-2. Nothing is registered yet on a first run, so the default has
    /// to be applied rather than only recorded.
    #[test]
    fn a_first_run_registers_the_login_item_the_default_asks_for() {
        let login_item = Arc::new(FakeLoginItem::registered(false));
        let store = store_with("login-first-run", None, Arc::clone(&login_item));

        assert!(store.current().launch_at_login);
        assert_eq!(login_item.changes(), vec![true]);
        assert!(login_item.enabled());
    }

    /// The login item is the truth once there is one to read. An item that has
    /// gone — deleted, or dropped by macOS when the app moved — must not be put
    /// back on every launch, and the pane must not report on over an item that
    /// is not there.
    #[test]
    fn a_login_item_that_is_no_longer_registered_is_not_put_back() {
        let login_item = Arc::new(FakeLoginItem::registered(false));
        let store = store_with(
            "login-no-longer-registered",
            Some("{ \"launchAtLogin\": true }"),
            Arc::clone(&login_item),
        );

        assert!(!store.current().launch_at_login);
        assert_eq!(login_item.changes(), Vec::<bool>::new());
    }

    /// Launch at login defaults on, and the pane must not report it on over an
    /// item the system refused to register.
    #[test]
    fn a_first_run_that_cannot_register_the_login_item_reports_it_as_off() {
        let store = store_with(
            "login-first-run-refused",
            None,
            Arc::new(FakeLoginItem::refusing()),
        );

        assert!(!store.current().launch_at_login);
    }

    /// ...and the next launch tries again. The settings file is what makes a
    /// launch take the login item as the truth, so writing one that records
    /// "off" over an item that failed to appear would turn a single failed
    /// write into a permanent opt-out of a default that is on.
    #[test]
    fn a_first_run_that_cannot_register_the_login_item_stays_a_first_run() {
        let path = scratch_dir("login-first-run-retried").join(SETTINGS_FILE);
        let refused = SettingsStore::load(&path, Arc::new(FakeLoginItem::refusing()));

        assert!(!refused.current().launch_at_login);
        assert!(
            !path.exists(),
            "the failed first run wrote its settings out"
        );

        let login_item = Arc::new(FakeLoginItem::registered(false));
        let retried = SettingsStore::load(&path, Arc::clone(&login_item) as Arc<dyn LoginItem>);

        assert!(retried.current().launch_at_login);
        assert_eq!(login_item.changes(), vec![true]);
    }

    /// An item that starts another copy of AYDY is not a choice to switch
    /// launch at login off, and it is not a working login item either: the
    /// binary it names may have been moved, replaced or deleted since. It is
    /// repointed at the copy that is running.
    #[test]
    fn a_login_item_naming_another_copy_is_repointed_at_this_one() {
        let login_item = Arc::new(FakeLoginItem::naming_another_copy());
        let store = store_with(
            "login-elsewhere",
            Some("{ \"launchAtLogin\": true }"),
            Arc::clone(&login_item),
        );

        assert!(store.current().launch_at_login);
        assert_eq!(login_item.changes(), vec![true]);
        assert_eq!(login_item.registration_now(), Registration::Ours);
    }

    /// A copy that cannot register itself — one running out of a build
    /// directory, whose path the next rebuild empties — must leave the item
    /// naming whatever it named, rather than claiming it or reporting launch at
    /// login off.
    #[test]
    fn a_copy_that_cannot_register_leaves_the_login_item_where_it_was() {
        let login_item = Arc::new(FakeLoginItem::naming_another_copy().that_refuses());
        let store = store_with(
            "login-elsewhere-refused",
            Some("{ \"launchAtLogin\": true }"),
            Arc::clone(&login_item),
        );

        assert!(store.current().launch_at_login);
        assert_eq!(login_item.changes(), Vec::<bool>::new());
        assert_eq!(login_item.registration_now(), Registration::Elsewhere);
    }

    #[test]
    fn a_login_item_that_cannot_be_read_falls_back_to_what_was_stored() {
        let store = store_with(
            "login-unreadable",
            Some("{ \"launchAtLogin\": false }"),
            Arc::new(FakeLoginItem::unreadable()),
        );

        assert!(!store.current().launch_at_login);
    }

    #[test]
    fn turning_launch_at_login_off_removes_the_login_item() {
        let login_item = Arc::new(FakeLoginItem::registered(true));
        let store = store_with(
            "login-off",
            Some("{ \"launchAtLogin\": true }"),
            Arc::clone(&login_item),
        );

        let settings = store
            .apply(SettingsChange {
                launch_at_login: Some(false),
                ..SettingsChange::default()
            })
            .expect("the change to be applied");

        assert!(!settings.launch_at_login);
        assert_eq!(login_item.changes(), vec![false]);
        assert!(!login_item.enabled());
    }

    /// Registering the same login item again on every unrelated change would
    /// rewrite the plist and, on macOS 13 and later, re-notify the user that an
    /// item that can run in the background was added.
    #[test]
    fn a_change_that_leaves_launch_at_login_alone_does_not_touch_the_login_item() {
        let login_item = Arc::new(FakeLoginItem::registered(true));
        let store = store_with(
            "login-untouched",
            Some("{ \"launchAtLogin\": true }"),
            Arc::clone(&login_item),
        );

        store
            .apply(SettingsChange {
                launch_at_login: Some(true),
                finished_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the change to be applied");

        assert_eq!(login_item.changes(), Vec::<bool>::new());
    }

    /// A login item that refuses leaves the setting where it was, rather than
    /// reporting a state the system is not in.
    #[test]
    fn a_login_item_that_refuses_leaves_the_setting_as_it_was() {
        let store = store_with(
            "login-refused",
            Some("{ \"launchAtLogin\": false }"),
            Arc::new(FakeLoginItem::refusing()),
        );

        let failed = store.apply(SettingsChange {
            launch_at_login: Some(true),
            finished_sound: Some(false),
            ..SettingsChange::default()
        });

        assert!(matches!(failed, Err(SettingsError::LaunchAtLogin(_))));
        assert!(!store.current().launch_at_login);
        assert!(store.current().finished_sound);
    }

    /// A change carries only what moved, so the values it says nothing about
    /// have to survive it.
    #[test]
    fn a_change_leaves_the_settings_it_says_nothing_about_alone() {
        let store = store("untouched", None);
        store
            .apply(SettingsChange {
                finished_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the first change");

        let settings = store
            .apply(SettingsChange {
                needs_input_sound: Some(false),
                ..SettingsChange::default()
            })
            .expect("the second change");

        assert!(!settings.finished_sound);
        assert!(!settings.needs_input_sound);
    }

    /// A change is partial precisely so a stale copy cannot clobber a change
    /// made somewhere else — but only if it is applied to the settings as they
    /// are at that moment. The pane sends one call per control and each arrives
    /// as its own task, so two of them really do overlap: the user moves two
    /// toggles, and one of them bounces back.
    #[test]
    fn a_change_made_while_another_is_in_flight_loses_neither() {
        use std::sync::Barrier;

        for round in 0..200 {
            let store = Arc::new(store(&format!("concurrent-{round}"), None));
            let start = Arc::new(Barrier::new(2));
            let writers: Vec<_> = [
                SettingsChange {
                    finished_sound: Some(false),
                    ..SettingsChange::default()
                },
                SettingsChange {
                    needs_input_sound: Some(false),
                    ..SettingsChange::default()
                },
            ]
            .map(|change| {
                let store = Arc::clone(&store);
                let start = Arc::clone(&start);

                std::thread::spawn(move || {
                    start.wait();
                    store.apply(change).expect("the change to be applied");
                })
            })
            .into_iter()
            .collect();

            for writer in writers {
                writer.join().expect("the writer to finish");
            }

            let settings = store.current();
            let written: Settings =
                serde_json::from_str(&file_contents(&store)).expect("readable settings");

            assert!(!settings.finished_sound, "round {round}");
            assert!(!settings.needs_input_sound, "round {round}");
            assert_eq!(written, settings, "round {round}");
        }
    }

    #[test]
    fn an_empty_change_changes_nothing() {
        let store = store("empty-change", None);

        assert_eq!(
            store
                .apply(SettingsChange::default())
                .expect("the change to be applied"),
            Settings::default()
        );
    }
}
