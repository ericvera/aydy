//! Install, reinstall, uninstall, status, and the preview of pending changes.
//!
//! Installing is not "write my five hooks". It is: remove AYDY's own entries,
//! remove and stash the sound hooks AYDY takes over, then write the current
//! set — and every step has its inverse in uninstall.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::hooks::merge::{
    aydy_entries, insert_aydy_hooks, planned_hooks, registered_hooks, remove_aydy_hooks,
    restore_entries, rewrite_port, take_over_sound_hooks, HookKind, PlannedHook,
};
use crate::hooks::settings_file::{render, SettingsFile, SettingsSnapshot};
use crate::hooks::stash::{merge_stashed, HookStash, RemovedEntry};
use crate::hooks::HookError;
use crate::listener::port::PortState;
use crate::listener::server::{lock_recovering, HookRegistration, RegisteredHooks};

/// The key AYDY's entries live under in the settings file.
const HOOKS_KEY: &str = "hooks";

/// One hook the install will add.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddedHook {
    /// The event it will be registered for.
    pub event: String,
    /// How it will reach the listener.
    pub kind: HookKind,
    /// Its declared timeout, in seconds.
    pub timeout: f64,
}

/// One entry the install will remove.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemovedHook {
    /// The event it is registered for.
    pub event: String,
    /// The command it runs, which is what names it to the user.
    pub command: Option<String>,
}

/// The exact change an install will make, for the first-run screen to render
/// before the user confirms.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangePreview {
    /// The file that will be edited.
    pub settings_path: String,
    /// The hooks that will be added.
    pub added: Vec<AddedHook>,
    /// The entries that will be removed. Every one of these is named to the
    /// user before they confirm.
    pub removed: Vec<RemovedHook>,
    /// The `hooks` block as it will be written.
    pub hooks_json: String,
}

impl ChangePreview {
    /// How many hooks will be registered.
    ///
    /// The first-run screen states this number, and it comes from what is
    /// actually registered rather than from a constant, so registering a sixth
    /// event keeps the copy honest.
    pub fn hook_count(&self) -> usize {
        self.added.len()
    }
}

/// Whether AYDY's hooks are in the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct HookStatus {
    /// Whether any AYDY entry is registered.
    pub installed: bool,
    /// How many AYDY entries are registered.
    pub hook_count: usize,
    /// The port those entries post to, when they agree on one.
    pub port: Option<u16>,
}

/// What a completed install or uninstall did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeOutcome {
    /// How many AYDY entries the settings file now holds.
    pub hooks_registered: usize,
    /// How many entries were removed from the user's configuration.
    pub entries_removed: usize,
    /// How many stashed entries were put back.
    pub entries_restored: usize,
}

/// What a plan will do to the stash once the settings file is written.
#[derive(Debug, Clone, PartialEq)]
enum StashUpdate {
    /// Leave it as it is.
    Keep,
    /// Record these entries.
    Replace(Vec<RemovedEntry>),
    /// Everything in it has been put back.
    Clear,
}

/// A change that has been worked out but not yet written.
///
/// Holding the snapshot the plan was built from is what lets the commit prove
/// the file has not changed since — and the first-run screen renders the same
/// plan that is then committed, so what the user is shown is what happens.
#[derive(Debug)]
pub struct ChangePlan {
    snapshot: SettingsSnapshot,
    next: Value,
    preview: ChangePreview,
    stash: StashUpdate,
    registered: RegisteredHooks,
    outcome: ChangeOutcome,
}

impl ChangePlan {
    /// The change this plan will make.
    pub fn preview(&self) -> &ChangePreview {
        &self.preview
    }
}

/// The registered-hook facts, as the listener's health check reads them.
///
/// The installer owns the settings file, so the listener is told the answer
/// rather than reading the file itself: two readers is how the two disagree.
#[derive(Debug, Default)]
pub struct HookRegistry {
    state: Mutex<RegisteredHooks>,
}

impl HookRegistry {
    /// A registry that reports nothing registered, which is also the truth
    /// before the first install.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records what the settings file holds.
    pub fn set(&self, hooks: RegisteredHooks) {
        *lock_recovering(&self.state) = hooks;
    }

    /// What the settings file holds.
    pub fn get(&self) -> RegisteredHooks {
        *lock_recovering(&self.state)
    }
}

impl HookRegistration for HookRegistry {
    fn registered(&self) -> RegisteredHooks {
        self.get()
    }
}

/// Registers, repairs and removes AYDY's hooks.
#[derive(Debug)]
pub struct HookInstaller {
    settings: SettingsFile,
    stash: HookStash,
    registry: Arc<HookRegistry>,
}

impl HookInstaller {
    /// An installer editing `settings`, stashing removals in `stash`, and
    /// keeping `registry` current.
    pub fn new(settings: SettingsFile, stash: HookStash, registry: Arc<HookRegistry>) -> Self {
        Self {
            settings,
            stash,
            registry,
        }
    }

    /// The settings file being edited.
    pub fn settings_path(&self) -> &std::path::Path {
        self.settings.path()
    }

    /// What the listener's health check reads.
    pub fn registry(&self) -> Arc<HookRegistry> {
        Arc::clone(&self.registry)
    }

    /// Whether AYDY's hooks are registered, read from the file.
    ///
    /// A settings file that cannot be read or parsed reports nothing installed,
    /// which is what puts the first-run screen in front of the user rather than
    /// an empty session list.
    pub fn status(&self) -> HookStatus {
        let Ok(snapshot) = self.settings.read() else {
            self.registry.set(RegisteredHooks::default());

            return HookStatus::default();
        };

        let hooks = hooks_object(snapshot.value());
        let registered = registered_hooks(hooks.as_ref());
        self.registry.set(registered);

        HookStatus {
            installed: registered.installed,
            hook_count: hooks.map(|hooks| aydy_entries(&hooks).len()).unwrap_or(0),
            port: registered.port,
        }
    }

    /// Works out the install without writing anything.
    ///
    /// This is what the first-run screen shows before asking for confirmation;
    /// nothing reaches the file until [`HookInstaller::commit`] is called with
    /// the plan.
    pub fn plan_install(&self, port: u16) -> Result<ChangePlan, HookError> {
        let snapshot = self.settings.read()?;
        let mut next = snapshot.value().clone();
        let planned = planned_hooks(port);
        let mut removed = Vec::new();

        let inserted = self.edit(&mut next, |hooks| {
            remove_aydy_hooks(hooks);
            removed = take_over_sound_hooks(hooks);

            Ok(insert_aydy_hooks(hooks, port))
        })?;

        if inserted != planned.len() {
            return Err(HookError::Unexpected {
                path: self.settings.path().to_path_buf(),
                at: HOOKS_KEY.to_string(),
            });
        }

        let stash = if removed.is_empty() {
            StashUpdate::Keep
        } else {
            StashUpdate::Replace(merge_stashed(self.stash.entries(), removed.clone()))
        };

        Ok(ChangePlan {
            preview: self.preview(&snapshot, &next, &planned, &removed),
            registered: RegisteredHooks {
                installed: true,
                port: Some(port),
            },
            outcome: ChangeOutcome {
                hooks_registered: inserted,
                entries_removed: removed.len(),
                entries_restored: 0,
            },
            snapshot,
            next,
            stash,
        })
    }

    /// Works out the uninstall without writing anything.
    pub fn plan_uninstall(&self) -> Result<ChangePlan, HookError> {
        let snapshot = self.settings.read()?;
        let mut next = snapshot.value().clone();
        let stashed = self.stash.entries();

        let (removed, restored) = self.edit(&mut next, |hooks| {
            let removed = remove_aydy_hooks(hooks);
            let restored = restore_entries(hooks, &stashed);

            Ok((removed, restored))
        })?;

        Ok(ChangePlan {
            preview: self.preview(&snapshot, &next, &[], &[]),
            registered: RegisteredHooks::default(),
            outcome: ChangeOutcome {
                hooks_registered: 0,
                entries_removed: removed,
                entries_restored: restored,
            },
            snapshot,
            next,
            stash: if stashed.is_empty() {
                StashUpdate::Keep
            } else {
                StashUpdate::Clear
            },
        })
    }

    /// Writes a plan.
    ///
    /// Aborts with [`HookError::ChangedOnDisk`], having modified nothing, if
    /// the settings file changed since the plan was made — which is why the
    /// check runs first, before either file is touched.
    ///
    /// **The record of a removal always outlives the removal itself.** An
    /// install writes the stash before the settings, so a crash or a failed
    /// settings write leaves the sound hooks still in the user's file and
    /// recorded rather than removed and forgotten. An uninstall clears the
    /// stash after, for the same reason in the other direction: the entries are
    /// back in the file before the only other copy of them is dropped. The
    /// overlap is harmless — [`merge_stashed`] will not re-record an entry it
    /// already holds, and [`restore_entries`] counts what the file already
    /// lists rather than restoring blind — so the only unrecoverable ordering
    /// is the one where the record goes first.
    pub fn commit(&self, plan: ChangePlan) -> Result<ChangeOutcome, HookError> {
        self.settings.verify_unchanged(&plan.snapshot)?;

        if let StashUpdate::Replace(entries) = &plan.stash {
            self.stash.replace(entries)?;
        }

        self.settings.write(&plan.snapshot, &plan.next)?;

        if plan.stash == StashUpdate::Clear {
            self.stash.clear()?;
        }

        self.registry.set(plan.registered);

        Ok(plan.outcome)
    }

    /// The change an install would make, for the screen that asks the user to
    /// confirm it.
    pub fn preview_install(&self, port: u16) -> Result<ChangePreview, HookError> {
        Ok(self.plan_install(port)?.preview)
    }

    /// Registers AYDY's hooks, taking over the sound hooks it replaces.
    ///
    /// Idempotent: AYDY's own entries are removed before the current set is
    /// written, so installing any number of times leaves exactly one set.
    pub fn install(&self, port: u16) -> Result<ChangeOutcome, HookError> {
        self.commit(self.plan_install(port)?)
    }

    /// Repairs AYDY's entries without needing an uninstall first. Identical to
    /// an install, which is what makes the repair total rather than partial.
    pub fn reinstall(&self, port: u16) -> Result<ChangeOutcome, HookError> {
        self.install(port)
    }

    /// Removes AYDY's entries and puts back everything it took over.
    pub fn uninstall(&self) -> Result<ChangeOutcome, HookError> {
        self.commit(self.plan_uninstall()?)
    }

    /// Points AYDY's registered entries at `port`, and reports how many were
    /// rewritten.
    ///
    /// Nothing is written when no AYDY entry is registered, or when they
    /// already name `port` — the app binds the port it remembered on most
    /// launches, and rewriting the user's settings file to the bytes it already
    /// holds is not a change worth making.
    pub fn apply_port_move(&self, port: u16) -> Result<usize, HookError> {
        let snapshot = self.settings.read()?;
        let mut next = snapshot.value().clone();

        let rewritten = self.edit(&mut next, |hooks| Ok(rewrite_port(hooks, port)))?;

        if rewritten == 0 {
            self.registry.set(RegisteredHooks::default());

            return Ok(0);
        }

        if &next != snapshot.value() {
            self.settings.write(&snapshot, &next)?;
        }

        self.registry.set(RegisteredHooks {
            installed: true,
            port: Some(port),
        });

        Ok(rewritten)
    }

    /// Runs `edit` over the document's `hooks` block.
    ///
    /// The rest of the document is never rebuilt — it is the value that was
    /// parsed, so every key AYDY has never been taught about is still there,
    /// in the order it was in. A `hooks` block emptied by the edit loses its
    /// key, so uninstalling from a file that had no `hooks` key leaves it with
    /// none.
    fn edit<T>(
        &self,
        document: &mut Value,
        edit: impl FnOnce(&mut Map<String, Value>) -> Result<T, HookError>,
    ) -> Result<T, HookError> {
        let unexpected = |at: &str| HookError::Unexpected {
            path: self.settings.path().to_path_buf(),
            at: at.to_string(),
        };

        let root = document
            .as_object_mut()
            .ok_or_else(|| unexpected("the document root"))?;

        let mut hooks = match root.get(HOOKS_KEY) {
            None => Map::new(),
            Some(Value::Object(existing)) => existing.clone(),
            Some(_) => return Err(unexpected(HOOKS_KEY)),
        };

        let result = edit(&mut hooks)?;

        if hooks.is_empty() {
            root.shift_remove(HOOKS_KEY);
        } else {
            root.insert(HOOKS_KEY.to_string(), Value::Object(hooks));
        }

        Ok(result)
    }

    fn preview(
        &self,
        snapshot: &SettingsSnapshot,
        next: &Value,
        planned: &[PlannedHook],
        removed: &[RemovedEntry],
    ) -> ChangePreview {
        let block = next
            .get(HOOKS_KEY)
            .cloned()
            .unwrap_or(Value::Object(Map::new()));

        ChangePreview {
            settings_path: self.settings.path().display().to_string(),
            added: planned
                .iter()
                .map(|hook| AddedHook {
                    event: hook.event.to_string(),
                    kind: hook.kind,
                    timeout: hook.timeout,
                })
                .collect(),
            removed: removed
                .iter()
                .map(|entry| RemovedHook {
                    event: entry.event.clone(),
                    command: entry.command().map(str::to_string),
                })
                .collect(),
            hooks_json: render(&block, snapshot.style()),
        }
    }
}

/// The `hooks` block, when the document has one.
fn hooks_object(document: &Value) -> Option<Map<String, Value>> {
    document.get(HOOKS_KEY).and_then(Value::as_object).cloned()
}

/// Keeps the registered hook URLs pointing at the listener for as long as the
/// app runs.
///
/// Subscribes to the listener's port and rewrites AYDY's entries whenever it
/// changes. The rewrite reads and writes a file, so it runs on the blocking
/// pool: blocking an async worker can starve the runtime's IO and timer driver,
/// which would stop the listener answering at all.
pub async fn follow_port_moves(installer: Arc<HookInstaller>, ports: Arc<PortState>) {
    let mut updates = ports.subscribe();

    // Read the file once before waiting on a port. Hooks installed while
    // nothing is listening is exactly the state the health check exists to
    // report, and a listener that never binds would otherwise leave the app
    // reporting no hooks at all — which reads as healthy.
    let reading = Arc::clone(&installer);
    let _ = tokio::task::spawn_blocking(move || reading.status()).await;

    loop {
        let port = updates.borrow_and_update().port;

        if let Some(port) = port {
            let installer = Arc::clone(&installer);
            let rewritten =
                tokio::task::spawn_blocking(move || installer.apply_port_move(port)).await;

            match rewritten {
                Ok(Err(error)) => {
                    eprintln!("aydy: could not point the registered hooks at port {port}: {error}");
                }
                Err(error) => {
                    eprintln!("aydy: rewriting the registered hook URLs failed: {error}");
                }
                Ok(Ok(_)) => {}
            }
        }

        if updates.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::hooks::merge::{
        is_aydy_hook, session_start_command, HTTP_EVENTS, SESSION_END, SESSION_START,
    };
    use crate::hooks::test_support::{
        scratch_dir, REAL_SETTINGS, SETTINGS_WITHOUT_HOOKS, SETTINGS_WITH_TWIN_SOUND_HOOKS,
    };
    use crate::listener::server::ListenerHealth;

    const PORT: u16 = 51_234;

    /// An installer over a scratch copy of a settings file. Nothing here ever
    /// touches the developer's own `~/.claude/settings.json`.
    struct Scratch {
        installer: HookInstaller,
        settings: PathBuf,
        stash: HookStash,
    }

    impl Scratch {
        fn new(name: &str, contents: &str) -> Self {
            let directory = scratch_dir(name);
            let settings = directory.join("settings.json");
            fs::write(&settings, contents).expect("the fixture to be written");
            let stash = HookStash::new(directory.join("removed-hooks.json"));

            Self {
                installer: HookInstaller::new(
                    SettingsFile::new(&settings),
                    stash.clone(),
                    Arc::new(HookRegistry::new()),
                ),
                settings,
                stash,
            }
        }

        fn text(&self) -> String {
            fs::read_to_string(&self.settings).expect("the settings file to be readable")
        }

        fn document(&self) -> Value {
            serde_json::from_str(&self.text()).expect("the settings file to parse")
        }

        fn hooks(&self) -> Map<String, Value> {
            hooks_object(&self.document()).unwrap_or_default()
        }
    }

    fn entries(hooks: &Map<String, Value>) -> Vec<(String, Value)> {
        aydy_entries(hooks)
    }

    #[test]
    fn installing_leaves_every_unrelated_key_byte_identical() {
        let scratch = Scratch::new("unrelated", REAL_SETTINGS);
        let before: Value = serde_json::from_str(REAL_SETTINGS).expect("the fixture to parse");

        scratch.installer.install(PORT).expect("the install to run");

        let mut after = scratch.document();
        after["hooks"] = before["hooks"].clone();

        assert_eq!(
            render(&after, &Default::default()),
            REAL_SETTINGS,
            "something outside the hooks block changed"
        );
    }

    #[test]
    fn installing_registers_every_hook_it_plans_to() {
        let scratch = Scratch::new("registers", REAL_SETTINGS);

        let outcome = scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(outcome.hooks_registered, planned_hooks(PORT).len());
        assert_eq!(entries(&scratch.hooks()).len(), outcome.hooks_registered);
    }

    #[test]
    fn the_reported_hook_count_matches_what_is_registered() {
        let scratch = Scratch::new("count", REAL_SETTINGS);
        let preview = scratch.installer.preview_install(PORT).expect("a preview");

        scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(preview.hook_count(), entries(&scratch.hooks()).len());
        assert_eq!(scratch.installer.status().hook_count, preview.hook_count());
    }

    #[test]
    fn session_start_is_registered_as_a_command_hook() {
        let scratch = Scratch::new("session-start", REAL_SETTINGS);

        scratch.installer.install(PORT).expect("the install to run");

        let hooks = scratch.hooks();
        let registered: Vec<&Value> = hooks[SESSION_START]
            .as_array()
            .expect("a list of groups")
            .iter()
            .flat_map(|group| group["hooks"].as_array().expect("a list of hooks"))
            .filter(|hook| is_aydy_hook(hook))
            .collect();

        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0]["type"], Value::from("command"));
        assert_ne!(registered[0]["type"], Value::from("http"));
    }

    #[test]
    fn every_registered_hook_declares_a_short_timeout() {
        let scratch = Scratch::new("timeouts", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");

        for (event, hook) in entries(&scratch.hooks()) {
            let timeout = hook["timeout"]
                .as_f64()
                .unwrap_or_else(|| panic!("{event} to declare a timeout"));
            let limit = if event == SESSION_END { 0.5 } else { 2.0 };

            assert!(
                timeout > 0.0 && timeout <= limit,
                "{event} declares a timeout of {timeout}s, over its {limit}s limit"
            );
        }
    }

    #[test]
    fn the_session_end_timeout_is_written_as_a_fraction_of_a_second() {
        let scratch = Scratch::new("fractional", REAL_SETTINGS);

        scratch.installer.install(PORT).expect("the install to run");

        assert!(
            scratch.text().contains("\"timeout\": 0.5"),
            "{}",
            scratch.text()
        );
    }

    #[test]
    fn both_sound_hooks_are_removed_and_stashed() {
        let scratch = Scratch::new("takeover", REAL_SETTINGS);

        let outcome = scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(outcome.entries_removed, 2);
        assert!(!scratch.text().contains("afplay"), "{}", scratch.text());

        let stashed: Vec<Option<String>> = scratch
            .stash
            .entries()
            .iter()
            .map(|entry| entry.command().map(str::to_string))
            .collect();

        assert_eq!(
            stashed,
            [
                Some("afplay /System/Library/Sounds/Funk.aiff".to_string()),
                Some("afplay /System/Library/Sounds/Hero.aiff".to_string())
            ]
        );
    }

    #[test]
    fn two_identical_sound_hooks_under_one_event_are_both_stashed_and_both_restored() {
        let scratch = Scratch::new("twins", SETTINGS_WITH_TWIN_SOUND_HOOKS);

        let outcome = scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(outcome.entries_removed, 2);
        assert_eq!(
            scratch.stash.entries().len(),
            outcome.entries_removed,
            "an entry was removed from the user's file without being recorded"
        );

        let restored = scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(restored.entries_restored, 2);
        assert_eq!(scratch.text(), SETTINGS_WITH_TWIN_SOUND_HOOKS);
    }

    #[test]
    fn identical_sound_hooks_under_two_events_are_both_stashed_and_both_restored() {
        let original = r#"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "afplay /System/Library/Sounds/Hero.aiff"
          }
        ]
      }
    ],
    "Notification": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "afplay /System/Library/Sounds/Hero.aiff"
          }
        ]
      }
    ]
  }
}
"#;
        let scratch = Scratch::new("twins-two-events", original);

        let outcome = scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(outcome.entries_removed, 2);
        assert_eq!(scratch.stash.entries().len(), 2);

        scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(scratch.text(), original);
    }

    #[test]
    fn a_second_install_does_not_stash_an_entry_it_already_holds() {
        let scratch = Scratch::new("twins-idempotent", SETTINGS_WITH_TWIN_SOUND_HOOKS);
        scratch.installer.install(PORT).expect("the first install");
        fs::write(&scratch.settings, SETTINGS_WITH_TWIN_SOUND_HOOKS)
            .expect("the sound hooks to come back");

        scratch.installer.install(PORT).expect("the second install");

        assert_eq!(
            scratch.stash.entries().len(),
            2,
            "the same two entries were recorded twice"
        );
    }

    #[test]
    fn uninstalling_restores_the_file_exactly_as_it_was() {
        let scratch = Scratch::new("roundtrip", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");

        scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(scratch.text(), REAL_SETTINGS);
    }

    #[test]
    fn uninstalling_restores_the_matcher_that_only_one_sound_hook_had() {
        let scratch = Scratch::new("matcher", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");

        scratch.installer.uninstall().expect("the uninstall to run");

        let hooks = scratch.hooks();

        assert_eq!(hooks["Notification"][0]["matcher"], Value::from("*"));
        assert_eq!(hooks["Stop"][0].get("matcher"), None);
    }

    #[test]
    fn uninstalling_empties_the_stash() {
        let scratch = Scratch::new("stash-cleared", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");

        scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(scratch.stash.entries(), Vec::new());
    }

    #[test]
    fn installing_twice_leaves_exactly_one_set_of_entries() {
        let scratch = Scratch::new("idempotent", REAL_SETTINGS);

        scratch.installer.install(PORT).expect("the first install");
        let after_first = scratch.text();
        scratch.installer.install(PORT).expect("the second install");

        assert_eq!(scratch.text(), after_first);
        assert_eq!(entries(&scratch.hooks()).len(), planned_hooks(PORT).len());
    }

    #[test]
    fn installing_twice_then_uninstalling_still_restores_the_file() {
        let scratch = Scratch::new("idempotent-uninstall", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the first install");
        scratch.installer.install(PORT).expect("the second install");

        scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(scratch.text(), REAL_SETTINGS);
    }

    #[test]
    fn reinstalling_repairs_entries_without_an_uninstall_first() {
        let scratch = Scratch::new("reinstall", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");
        let mut damaged = scratch.document();
        damaged["hooks"]["Stop"] = Value::Array(Vec::new());
        fs::write(&scratch.settings, render(&damaged, &Default::default()))
            .expect("the damage to land");

        scratch
            .installer
            .reinstall(PORT)
            .expect("the reinstall to run");

        assert_eq!(entries(&scratch.hooks()).len(), planned_hooks(PORT).len());
    }

    #[test]
    fn a_stop_hook_that_is_not_a_sound_player_survives_an_install() {
        let original = "{\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"/Users/eric/bin/notify-me.sh\"\n          }\n        ]\n      }\n    ]\n  }\n}\n";
        let scratch = Scratch::new("not-a-sound", original);

        scratch.installer.install(PORT).expect("the install to run");

        assert!(
            scratch.text().contains("/Users/eric/bin/notify-me.sh"),
            "{}",
            scratch.text()
        );
        assert_eq!(scratch.stash.entries(), Vec::new());
    }

    #[test]
    fn installing_into_a_file_with_no_hooks_key_adds_one() {
        let scratch = Scratch::new("no-hooks", SETTINGS_WITHOUT_HOOKS);

        scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(entries(&scratch.hooks()).len(), planned_hooks(PORT).len());
        assert_eq!(
            scratch.document()["permissions"]["allow"],
            serde_json::json!(["Bash(git status:*)"])
        );
    }

    #[test]
    fn uninstalling_from_a_file_that_had_no_hooks_key_leaves_it_with_none() {
        let scratch = Scratch::new("no-hooks-uninstall", SETTINGS_WITHOUT_HOOKS);
        scratch.installer.install(PORT).expect("the install to run");

        scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(scratch.text(), SETTINGS_WITHOUT_HOOKS);
    }

    #[test]
    fn a_settings_file_that_changed_since_the_plan_was_made_is_not_written() {
        let scratch = Scratch::new("concurrent", REAL_SETTINGS);
        let plan = scratch.installer.plan_install(PORT).expect("a plan");
        let meanwhile = "{\n  \"model\": \"sonnet\"\n}\n";
        fs::write(&scratch.settings, meanwhile).expect("the concurrent write to land");

        let error = scratch
            .installer
            .commit(plan)
            .expect_err("the commit to abort");

        assert!(matches!(error, HookError::ChangedOnDisk { .. }));
        assert_eq!(scratch.text(), meanwhile);
        assert_eq!(scratch.stash.entries(), Vec::new());
    }

    /// An installer whose settings file sits in a directory of its own, so a
    /// test can seal that directory and make the settings write fail without
    /// touching the stash.
    struct Sealable {
        installer: HookInstaller,
        directory: PathBuf,
        settings: PathBuf,
        stash: HookStash,
    }

    impl Sealable {
        fn new(name: &str, contents: &str) -> Self {
            let root = scratch_dir(name);
            let directory = root.join("claude");
            fs::create_dir_all(&directory).expect("the settings directory to be creatable");
            let settings = directory.join("settings.json");
            fs::write(&settings, contents).expect("the fixture to be written");
            let stash = HookStash::new(root.join("removed-hooks.json"));

            Self {
                installer: HookInstaller::new(
                    SettingsFile::new(&settings),
                    stash.clone(),
                    Arc::new(HookRegistry::new()),
                ),
                directory,
                settings,
                stash,
            }
        }

        fn set_mode(&self, mode: u32) {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&self.directory)
                .expect("the settings directory to exist")
                .permissions();
            permissions.set_mode(mode);
            fs::set_permissions(&self.directory, permissions)
                .expect("the settings directory permissions to be set");
        }

        /// Runs `body` with the settings file unwritable, and unseals the
        /// directory afterwards whatever happens.
        fn while_unwritable<T>(&self, body: impl FnOnce() -> T) -> T {
            self.set_mode(0o500);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
            self.set_mode(0o700);

            match outcome {
                Ok(value) => value,
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }

        fn text(&self) -> String {
            fs::read_to_string(&self.settings).expect("the settings file to be readable")
        }
    }

    #[test]
    fn a_settings_write_that_fails_after_the_stash_leaves_the_removals_recoverable() {
        let scratch = Sealable::new("stash-first", REAL_SETTINGS);
        let plan = scratch.installer.plan_install(PORT).expect("a plan");

        let error = scratch
            .while_unwritable(|| scratch.installer.commit(plan))
            .expect_err("the settings write to fail");

        assert!(matches!(error, HookError::Write { .. }), "{error:?}");
        assert_eq!(scratch.text(), REAL_SETTINGS);
        assert_eq!(
            scratch.stash.entries().len(),
            2,
            "the sound hooks are recorded nowhere and are about to be taken"
        );

        // The overlap is harmless: the file and the stash both hold the two
        // entries, and neither a retried install nor the uninstall duplicates
        // them.
        scratch
            .installer
            .install(PORT)
            .expect("the retried install");
        scratch.installer.uninstall().expect("the uninstall to run");

        assert_eq!(scratch.text(), REAL_SETTINGS);
    }

    #[test]
    fn a_settings_write_that_fails_during_an_uninstall_keeps_the_stash() {
        let scratch = Sealable::new("clear-last", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");
        let plan = scratch.installer.plan_uninstall().expect("a plan");

        let error = scratch
            .while_unwritable(|| scratch.installer.commit(plan))
            .expect_err("the settings write to fail");

        assert!(matches!(error, HookError::Write { .. }), "{error:?}");
        assert_eq!(
            scratch.stash.entries().len(),
            2,
            "the stash was emptied before its entries were back in the file"
        );

        scratch
            .installer
            .uninstall()
            .expect("the retried uninstall");

        assert_eq!(scratch.text(), REAL_SETTINGS);
    }

    #[test]
    fn the_preview_names_every_entry_that_will_be_removed() {
        let scratch = Scratch::new("preview-removals", REAL_SETTINGS);

        let preview = scratch.installer.preview_install(PORT).expect("a preview");

        assert_eq!(
            preview.removed,
            [
                RemovedHook {
                    event: "Stop".to_string(),
                    command: Some("afplay /System/Library/Sounds/Funk.aiff".to_string()),
                },
                RemovedHook {
                    event: "Notification".to_string(),
                    command: Some("afplay /System/Library/Sounds/Hero.aiff".to_string()),
                }
            ]
        );
    }

    #[test]
    fn the_preview_describes_every_hook_that_will_be_added() {
        let scratch = Scratch::new("preview-additions", REAL_SETTINGS);

        let preview = scratch.installer.preview_install(PORT).expect("a preview");

        assert_eq!(preview.hook_count(), 5);
        assert_eq!(
            preview.added[0],
            AddedHook {
                event: SESSION_START.to_string(),
                kind: HookKind::Command,
                timeout: 2.0,
            }
        );
        assert!(preview.hooks_json.contains("\"timeout\": 0.5"));
        assert_eq!(
            preview.settings_path,
            scratch.settings.display().to_string()
        );
    }

    #[test]
    fn previewing_writes_nothing() {
        let scratch = Scratch::new("preview-writes-nothing", REAL_SETTINGS);

        scratch.installer.preview_install(PORT).expect("a preview");

        assert_eq!(scratch.text(), REAL_SETTINGS);
        assert_eq!(scratch.stash.entries(), Vec::new());
    }

    #[test]
    fn status_reports_nothing_installed_before_the_first_install() {
        let scratch = Scratch::new("status-before", REAL_SETTINGS);

        assert_eq!(scratch.installer.status(), HookStatus::default());
    }

    #[test]
    fn status_reports_the_hooks_and_the_port_after_an_install() {
        let scratch = Scratch::new("status-after", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");

        assert_eq!(
            scratch.installer.status(),
            HookStatus {
                installed: true,
                hook_count: 5,
                port: Some(PORT),
            }
        );
    }

    #[test]
    fn status_reports_nothing_installed_for_a_settings_file_it_cannot_parse() {
        let scratch = Scratch::new("status-malformed", "{ not json at all");

        assert_eq!(scratch.installer.status(), HookStatus::default());
    }

    #[test]
    fn hooks_registered_with_the_listener_unbound_cannot_receive_events() {
        let scratch = Scratch::new("health-installed", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");
        scratch.installer.status();

        let health = ListenerHealth::new(Arc::new(PortState::new()), scratch.installer.registry());

        assert!(health.cannot_receive_events());
    }

    #[test]
    fn no_hooks_registered_means_the_app_is_not_treated_as_broken() {
        let scratch = Scratch::new("health-absent", REAL_SETTINGS);
        scratch.installer.status();

        let health = ListenerHealth::new(Arc::new(PortState::new()), scratch.installer.registry());

        assert!(!health.cannot_receive_events());
    }

    #[test]
    fn the_health_check_finds_the_port_in_the_session_start_command_alone() {
        let scratch = Scratch::new("health-command", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");
        let mut document = scratch.document();
        for event in HTTP_EVENTS {
            document["hooks"]
                .as_object_mut()
                .expect("a hooks block")
                .shift_remove(event);
        }
        fs::write(&scratch.settings, render(&document, &Default::default()))
            .expect("the trimmed file to land");

        let status = scratch.installer.status();

        assert_eq!(status.port, Some(PORT));

        let ports = Arc::new(PortState::new());
        ports.set_bound(PORT, None);
        let health = ListenerHealth::new(ports, scratch.installer.registry());

        assert!(!health.cannot_receive_events());
    }

    #[test]
    fn a_port_move_rewrites_every_registered_entry_including_the_command() {
        let scratch = Scratch::new("port-move", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");

        let rewritten = scratch
            .installer
            .apply_port_move(60_000)
            .expect("the rewrite to run");

        assert_eq!(rewritten, 5);
        assert_eq!(scratch.installer.status().port, Some(60_000));
        assert!(
            scratch.text().contains(&session_start_command(60_000)),
            "{}",
            scratch.text()
        );
        assert!(!scratch.text().contains("51234"), "{}", scratch.text());
    }

    #[test]
    fn a_port_move_before_an_install_writes_nothing() {
        let scratch = Scratch::new("port-move-uninstalled", REAL_SETTINGS);

        let rewritten = scratch
            .installer
            .apply_port_move(60_000)
            .expect("the rewrite to run");

        assert_eq!(rewritten, 0);
        assert_eq!(scratch.text(), REAL_SETTINGS);
    }

    #[tokio::test]
    async fn the_registered_urls_follow_the_listener_to_a_new_port() {
        let scratch = Scratch::new("follow", REAL_SETTINGS);
        scratch.installer.install(PORT).expect("the install to run");
        let installer = Arc::new(HookInstaller::new(
            SettingsFile::new(&scratch.settings),
            scratch.stash.clone(),
            Arc::new(HookRegistry::new()),
        ));
        let ports = Arc::new(PortState::new());
        let following = tokio::spawn(follow_port_moves(
            Arc::clone(&installer),
            Arc::clone(&ports),
        ));

        ports.set_bound(60_000, Some(PORT));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while installer.registry().get().port != Some(60_000) {
            assert!(
                std::time::Instant::now() < deadline,
                "the rewrite never ran"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        following.abort();

        assert!(fs::read_to_string(&scratch.settings)
            .unwrap()
            .contains(&session_start_command(60_000)));
    }

    /// The one property no JSON assertion can establish: what the shell
    /// actually does with the command when AYDY is not running, which is a
    /// supported state on every machine this ships to.
    #[test]
    fn the_session_start_command_stays_quiet_when_nothing_is_listening() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = closed.local_addr().expect("a local address").port();
        drop(closed);

        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(session_start_command(port))
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the command to run");

        assert!(
            output.status.success(),
            "the command exited {:?}, which is a hook error in every session start",
            output.status.code()
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    }
}
