//! Whether the companion extension's session tracking is installed: its hook
//! script on disk, and a command hook running that script for every event in
//! the user's Claude Code settings.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::hooks::settings_file::SettingsFile;

/// The path fragment every hook command the extension registers contains.
pub const HOOK_SCRIPT_MARKER: &str = "app.aydy/bin/aydy-hook.sh";

/// The events the extension registers a hook for.
pub const TRACKED_EVENTS: [&str; 5] = [
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    "Stop",
    "SessionEnd",
];

/// How much of session tracking is in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackingState {
    Installed,
    /// The hook script is missing.
    NotInstalled,
    /// The hook script is present but at least one event's hook is not.
    HooksMissing,
}

/// Whether every tracked event has a command hook running the extension's
/// script.
pub fn hooks_present(settings: &Value) -> bool {
    TRACKED_EVENTS.iter().all(|event| {
        settings
            .get("hooks")
            .and_then(|hooks| hooks.get(event))
            .and_then(Value::as_array)
            .is_some_and(|groups| groups.iter().any(group_runs_script))
    })
}

fn group_runs_script(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(runs_script))
}

fn runs_script(hook: &Value) -> bool {
    hook.get("type").and_then(Value::as_str) == Some("command")
        && hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.contains(HOOK_SCRIPT_MARKER))
}

/// The tracking state given the hook script's location and the settings file.
pub fn tracking_state(script: &Path, settings: &SettingsFile) -> TrackingState {
    if !script.is_file() {
        return TrackingState::NotInstalled;
    }

    match settings.read() {
        Ok(snapshot) if hooks_present(snapshot.value()) => TrackingState::Installed,
        _ => TrackingState::HooksMissing,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::test_support::scratch_dir;

    fn script_in(dir: &Path) -> PathBuf {
        let script = dir.join("bin/aydy-hook.sh");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "#!/bin/sh\n").unwrap();

        script
    }

    fn tracking_hook() -> Value {
        json!({
            "hooks": [{
                "type": "command",
                "command": "\"$HOME/Library/Application Support/app.aydy/bin/aydy-hook.sh\""
            }]
        })
    }

    fn settings_with(dir: &Path, events: &[&str], group: Value) -> SettingsFile {
        let hooks: serde_json::Map<String, Value> = events
            .iter()
            .map(|event| (event.to_string(), json!([group.clone()])))
            .collect();
        let path = dir.join("settings.json");
        fs::write(&path, json!({ "hooks": hooks }).to_string()).unwrap();

        SettingsFile::new(path)
    }

    #[test]
    fn no_script_is_not_installed() {
        let dir = scratch_dir("tracking-no-script");
        let settings = settings_with(&dir, &TRACKED_EVENTS, tracking_hook());

        assert_eq!(
            tracking_state(&dir.join("bin/aydy-hook.sh"), &settings),
            TrackingState::NotInstalled
        );
    }

    #[test]
    fn script_without_settings_file_is_hooks_missing() {
        let dir = scratch_dir("tracking-no-settings");
        let script = script_in(&dir);

        assert_eq!(
            tracking_state(&script, &SettingsFile::new(dir.join("settings.json"))),
            TrackingState::HooksMissing
        );
    }

    #[test]
    fn four_of_five_events_is_hooks_missing() {
        let dir = scratch_dir("tracking-four-events");
        let script = script_in(&dir);
        let settings = settings_with(&dir, &TRACKED_EVENTS[..4], tracking_hook());

        assert_eq!(
            tracking_state(&script, &settings),
            TrackingState::HooksMissing
        );
    }

    #[test]
    fn all_five_events_is_installed() {
        let dir = scratch_dir("tracking-installed");
        let script = script_in(&dir);
        let settings = settings_with(&dir, &TRACKED_EVENTS, tracking_hook());

        assert_eq!(tracking_state(&script, &settings), TrackingState::Installed);
    }

    #[test]
    fn unrelated_and_legacy_hooks_alone_are_hooks_missing() {
        let dir = scratch_dir("tracking-legacy");
        let script = script_in(&dir);
        let group = json!({
            "hooks": [
                { "type": "command", "command": "afplay /System/Library/Sounds/Glass.aiff" },
                {
                    "type": "http",
                    "url": "http://127.0.0.1:47823/hook",
                    "headers": { "X-Aydy-Hook": "1" }
                },
                {
                    "type": "command",
                    "command": "curl -s -H 'X-Aydy-Hook: 1' http://127.0.0.1:47823/hook"
                }
            ]
        });
        let settings = settings_with(&dir, &TRACKED_EVENTS, group);

        assert_eq!(
            tracking_state(&script, &settings),
            TrackingState::HooksMissing
        );
    }

    #[test]
    fn invalid_json_is_hooks_missing() {
        let dir = scratch_dir("tracking-invalid-json");
        let script = script_in(&dir);
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();

        assert_eq!(
            tracking_state(&script, &SettingsFile::new(path)),
            TrackingState::HooksMissing
        );
    }

    #[test]
    fn states_serialize_in_camel_case() {
        assert_eq!(
            serde_json::to_value(TrackingState::HooksMissing).unwrap(),
            json!("hooksMissing")
        );
    }
}
