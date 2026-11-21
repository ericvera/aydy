//! Inserting, finding and removing AYDY's hook entries inside the `hooks`
//! block, and identifying the sound hooks it takes over.
//!
//! Everything here works on the `hooks` object as a [`serde_json::Map`], and
//! every entry it does not positively recognise is left exactly as it was.

use serde_json::{Map, Value};

use crate::hooks::stash::RemovedEntry;
use crate::listener::routes::HOOK_PATH;
use crate::listener::server::RegisteredHooks;

/// The header AYDY marks its own entries with.
///
/// The marker cannot be the URL: the port moves, and an entry that could only
/// be found by the port it used to name would be orphaned by the first move.
/// A header is also the one field on a hook that is free-form by design, so
/// marking there adds no key to a shape Claude Code validates.
pub const MARKER_HEADER: &str = "X-Aydy-Hook";

/// The marker header's value.
pub const MARKER_VALUE: &str = "1";

/// The loopback host the listener binds, and the only host AYDY's hooks name.
pub const LOOPBACK_HOST: &str = "127.0.0.1";

/// The event that must be a `command` hook.
///
/// Claude Code's `getMatchingHooks` filters `http` hooks out of `SessionStart`
/// entirely — the entry is accepted, stored, and silently never invoked.
pub const SESSION_START: &str = "SessionStart";

/// The events registered as `http` hooks.
pub const HTTP_EVENTS: [&str; 4] = ["UserPromptSubmit", "Notification", "Stop", "SessionEnd"];

/// The event whose hooks share a 1.5-second budget across the whole machine.
pub const SESSION_END: &str = "SessionEnd";

/// The timeout, in seconds, on every hook but `SessionEnd`.
///
/// `http` hooks block the user's Claude session inside their timeout, and the
/// default is 600 seconds. Without an explicit short timeout, a stopped AYDY
/// with live hooks stalls every Claude session on the machine.
pub const HOOK_TIMEOUT_SECONDS: f64 = 2.0;

/// The timeout, in seconds, on `SessionEnd`. Its hooks share a 1.5-second
/// budget with hooks AYDY does not control, and a longer declared timeout
/// raises that budget for everyone.
pub const SESSION_END_TIMEOUT_SECONDS: f64 = 0.5;

/// How long the `SessionStart` command gives itself, inside its declared
/// timeout.
const CURL_MAX_TIME_SECONDS: &str = "1";

/// The events AYDY takes sound hooks over from.
const SOUND_EVENTS: [&str; 2] = ["Stop", "Notification"];

/// The macOS sound player, and the only command AYDY will identify as playing a
/// sound. Anything else — a script, a command AYDY cannot classify — is left
/// alone, because a false positive deletes the user's work.
const SOUND_PLAYER: &str = "afplay";

/// Which native hook type an entry is registered as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HookKind {
    /// A shell command, receiving the payload on stdin.
    Command,
    /// A blocking POST to the listener.
    Http,
}

/// One hook AYDY registers.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedHook {
    /// The event it is registered for.
    pub event: &'static str,
    /// How it reaches the listener.
    pub kind: HookKind,
    /// Its declared timeout, in seconds.
    pub timeout: f64,
    /// The entry as it is written into the settings file.
    pub hook: Value,
}

/// The URL the listener answers on.
pub fn hook_url(port: u16) -> String {
    format!("http://{LOOPBACK_HOST}:{port}{HOOK_PATH}")
}

/// The `SessionStart` command.
///
/// Three properties of this string are safety requirements, not style:
///
/// - It **always exits 0**. AYDY not running is a supported state, and a bare
///   `curl` exiting non-zero on connection refused puts a hook error in every
///   session start on the machine.
/// - It **discards its output**. A `SessionStart` command hook's stdout is
///   injected into the session as context.
/// - It **bounds its own time** inside the declared timeout.
pub fn session_start_command(port: u16) -> String {
    format!(
        "curl -s -o /dev/null --max-time {CURL_MAX_TIME_SECONDS} \
         -H 'Content-Type: application/json' -H '{MARKER_HEADER}: {MARKER_VALUE}' \
         --data-binary @- {} || true",
        hook_url(port)
    )
}

/// Every hook AYDY registers, in the order it registers them.
pub fn planned_hooks(port: u16) -> Vec<PlannedHook> {
    let mut planned = vec![PlannedHook {
        event: SESSION_START,
        kind: HookKind::Command,
        timeout: HOOK_TIMEOUT_SECONDS,
        hook: command_hook(port, HOOK_TIMEOUT_SECONDS),
    }];

    for event in HTTP_EVENTS {
        let timeout = timeout_for(event);

        planned.push(PlannedHook {
            event,
            kind: HookKind::Http,
            timeout,
            hook: http_hook(port, timeout),
        });
    }

    planned
}

/// The timeout an event's hook is registered with.
pub fn timeout_for(event: &str) -> f64 {
    if event == SESSION_END {
        SESSION_END_TIMEOUT_SECONDS
    } else {
        HOOK_TIMEOUT_SECONDS
    }
}

/// Inserts AYDY's entries, and reports how many landed.
///
/// Each goes into a matcher group of its own with no `matcher` key, which
/// matches every invocation of the event.
pub fn insert_aydy_hooks(hooks: &mut Map<String, Value>, port: u16) -> usize {
    let mut inserted = 0;

    for planned in planned_hooks(port) {
        let groups = hooks
            .entry(planned.event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));

        let Some(groups) = groups.as_array_mut() else {
            continue;
        };

        let mut group = Map::new();
        group.insert("hooks".to_string(), Value::Array(vec![planned.hook]));
        groups.push(Value::Object(group));
        inserted += 1;
    }

    inserted
}

/// Removes every entry AYDY registered, and reports how many went.
///
/// This is the first half of an idempotent install — remove all of AYDY's
/// entries, then insert the current set — and the first half of uninstall.
pub fn remove_aydy_hooks(hooks: &mut Map<String, Value>) -> usize {
    let mut removed = 0;

    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };

        groups.retain_mut(|group| {
            let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };

            let before = entries.len();
            entries.retain(|hook| !is_aydy_hook(hook));
            removed += before - entries.len();

            !entries.is_empty()
        });
    }

    prune_empty_events(hooks);

    removed
}

/// Removes the `Stop` and `Notification` entries that play a sound, and reports
/// what was removed so it can be stashed and restored.
///
/// AYDY plays the same two sounds for the same two events, so leaving these in
/// place means the user hears each one twice. Only entries that positively
/// invoke the macOS sound player are touched.
pub fn take_over_sound_hooks(hooks: &mut Map<String, Value>) -> Vec<RemovedEntry> {
    let mut removed = Vec::new();

    // In the order the file lists them, not in the order this module happens to
    // name them: restoring rebuilds the event keys in the order they were
    // removed, and an uninstall has to give the file back as it was found.
    let events: Vec<String> = hooks
        .keys()
        .filter(|event| SOUND_EVENTS.contains(&event.as_str()))
        .cloned()
        .collect();

    for event in events {
        let Some(groups) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };

        let mut kept = Vec::with_capacity(groups.len());

        for (group_index, group) in groups.iter().enumerate() {
            let original = group.clone();
            let mut remaining = group.clone();

            let Some(entries) = remaining.get_mut("hooks").and_then(Value::as_array_mut) else {
                kept.push(original);
                continue;
            };

            let mut sounds = Vec::new();
            let mut survivors = Vec::with_capacity(entries.len());

            for (hook_index, hook) in entries.iter().enumerate() {
                if is_sound_hook(hook) {
                    sounds.push((hook_index, hook.clone()));
                } else {
                    survivors.push(hook.clone());
                }
            }

            if sounds.is_empty() {
                kept.push(original);
                continue;
            }

            let group_removed = survivors.is_empty();
            *entries = survivors;

            for (hook_index, hook) in sounds {
                removed.push(RemovedEntry {
                    event: event.clone(),
                    group_index,
                    hook_index,
                    group_removed,
                    hook,
                    group: original.clone(),
                });
            }

            if !group_removed {
                kept.push(remaining);
            }
        }

        *groups = kept;
    }

    prune_empty_events(hooks);

    removed
}

/// Puts stashed entries back where they were, and reports how many landed.
///
/// Entries are restored in the order they were removed, so that indices
/// recorded against the untouched file still line up and an event key that had
/// to be recreated is recreated where it was.
///
/// Restoring twice must not duplicate anything, so an entry already back in the
/// file is skipped — but the test is **how many copies** the event lists, not
/// whether it lists one. Two structurally identical hooks under one event are a
/// plausible configuration (a different sound per notification kind is one
/// `afplay` per matcher group, and the two commands read the same), and
/// skipping on presence alone would hand back one of them and silently drop the
/// other.
pub fn restore_entries(hooks: &mut Map<String, Value>, entries: &[RemovedEntry]) -> usize {
    let mut events: Vec<&str> = Vec::new();

    for entry in entries {
        if !events.contains(&entry.event.as_str()) {
            events.push(&entry.event);
        }
    }

    let mut ordered: Vec<&RemovedEntry> = entries.iter().collect();
    ordered.sort_by_key(|entry| {
        (
            events
                .iter()
                .position(|event| *event == entry.event)
                .unwrap_or(0),
            entry.group_index,
            entry.hook_index,
        )
    });

    let mut restored = 0;
    let mut asked: Vec<(&str, &Value, usize)> = Vec::new();

    for entry in ordered {
        let wanted = match asked
            .iter_mut()
            .find(|(event, hook, _)| *event == entry.event && *hook == &entry.hook)
        {
            Some((_, _, count)) => {
                *count += 1;
                *count
            }
            None => {
                asked.push((&entry.event, &entry.hook, 1));
                1
            }
        };

        if count_hook(hooks, &entry.event, &entry.hook) >= wanted {
            continue;
        }

        let groups = hooks
            .entry(entry.event.clone())
            .or_insert_with(|| Value::Array(Vec::new()));

        let Some(groups) = groups.as_array_mut() else {
            continue;
        };

        if entry.group_removed {
            groups.insert(entry.group_index.min(groups.len()), entry.group.clone());
            restored += 1;
            continue;
        }

        let at = entry.group_index.min(groups.len().saturating_sub(1));
        let Some(existing) = groups
            .get_mut(at)
            .and_then(|group| group.get_mut("hooks"))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };

        existing.insert(entry.hook_index.min(existing.len()), entry.hook.clone());
        restored += 1;
    }

    restored
}

/// Points every AYDY entry at `port`, and reports how many were rewritten.
///
/// Two shapes carry the port: the `url` on the four `http` entries, and the URL
/// **inside the `SessionStart` command string**, which has no `url` field at
/// all. Rewriting only `url` fields leaves `SessionStart` posting to a dead port
/// forever, and silently, since the command still exits 0.
pub fn rewrite_port(hooks: &mut Map<String, Value>, port: u16) -> usize {
    let mut rewritten = 0;

    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };

        for group in groups.iter_mut() {
            let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };

            for hook in entries.iter_mut() {
                if !is_aydy_hook(hook) {
                    continue;
                }

                match hook.get("type").and_then(Value::as_str) {
                    Some("http") => {
                        hook["url"] = Value::String(hook_url(port));
                        rewritten += 1;
                    }
                    Some("command") => {
                        hook["command"] = Value::String(session_start_command(port));
                        rewritten += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    rewritten
}

/// Every entry AYDY registered, paired with the event it is registered for.
pub fn aydy_entries(hooks: &Map<String, Value>) -> Vec<(String, Value)> {
    let mut found = Vec::new();

    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };

        for group in groups {
            let Some(entries) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };

            for hook in entries {
                if is_aydy_hook(hook) {
                    found.push((event.clone(), hook.clone()));
                }
            }
        }
    }

    found
}

/// What the listener needs to know about the registered hooks.
///
/// The port is reported only when every AYDY entry names the same one. Entries
/// disagreeing means some of them are posting somewhere the listener is not,
/// which is exactly the state the health check exists to catch.
pub fn registered_hooks(hooks: Option<&Map<String, Value>>) -> RegisteredHooks {
    let Some(hooks) = hooks else {
        return RegisteredHooks::default();
    };

    let entries = aydy_entries(hooks);

    if entries.is_empty() {
        return RegisteredHooks::default();
    }

    let mut ports = entries.iter().map(|(_, hook)| port_of(hook));
    let first = ports.next().flatten();
    let agreed = first.filter(|port| ports.all(|other| other == Some(*port)));

    RegisteredHooks {
        installed: true,
        port: agreed,
    }
}

/// The port an AYDY entry posts to.
///
/// Looks in the command string as well as the `url` field: the `SessionStart`
/// entry has no `url`, and code that finds AYDY's URLs by looking for `url`
/// keys misses it.
pub fn port_of(hook: &Value) -> Option<u16> {
    let carrier = hook
        .get("url")
        .or_else(|| hook.get("command"))
        .and_then(Value::as_str)?;

    port_in(carrier)
}

/// Whether `hook` is one AYDY registered.
pub fn is_aydy_hook(hook: &Value) -> bool {
    match hook.get("type").and_then(Value::as_str) {
        Some("http") => {
            hook.get("headers")
                .and_then(Value::as_object)
                .and_then(|headers| headers.get(MARKER_HEADER))
                .and_then(Value::as_str)
                == Some(MARKER_VALUE)
        }
        Some("command") => hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.contains(MARKER_HEADER)),
        _ => false,
    }
}

/// Whether `hook` is a `command` entry that plays a sound.
pub fn is_sound_hook(hook: &Value) -> bool {
    if hook.get("type").and_then(Value::as_str) != Some("command") {
        return false;
    }

    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(is_sound_command)
}

/// Whether `command` does nothing but play a sound.
///
/// Deliberately narrow. A command that chains, pipes, substitutes or expands
/// anything is not identified, however likely it is to make a noise, because
/// removing an entry AYDY has misread destroys work the user did.
fn is_sound_command(command: &str) -> bool {
    let command = command.trim();

    if command.contains(['&', ';', '|', '`', '$', '\n', '\r', '>', '<']) {
        return false;
    }

    let mut words = command.split_whitespace();
    let Some(program) = words.next() else {
        return false;
    };

    program.rsplit('/').next().unwrap_or(program) == SOUND_PLAYER
}

/// A `timeout` value, written as a whole number where it is one.
fn timeout_value(seconds: f64) -> Value {
    if seconds.fract() == 0.0 && seconds >= 0.0 {
        return Value::from(seconds as u64);
    }

    Value::from(seconds)
}

fn http_hook(port: u16, timeout: f64) -> Value {
    let mut headers = Map::new();
    headers.insert(MARKER_HEADER.to_string(), Value::from(MARKER_VALUE));

    let mut hook = Map::new();
    hook.insert("type".to_string(), Value::from("http"));
    hook.insert("url".to_string(), Value::from(hook_url(port)));
    hook.insert("headers".to_string(), Value::Object(headers));
    hook.insert("timeout".to_string(), timeout_value(timeout));

    Value::Object(hook)
}

fn command_hook(port: u16, timeout: f64) -> Value {
    let mut hook = Map::new();
    hook.insert("type".to_string(), Value::from("command"));
    hook.insert(
        "command".to_string(),
        Value::from(session_start_command(port)),
    );
    hook.insert("timeout".to_string(), timeout_value(timeout));

    Value::Object(hook)
}

/// How many copies of `hook` `event` already lists.
///
/// A count rather than a yes-or-no: an event can legitimately list the same
/// hook body more than once, and a restore that stopped at the first copy would
/// drop every other one.
fn count_hook(hooks: &Map<String, Value>, event: &str, hook: &Value) -> usize {
    hooks
        .get(event)
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                .flatten()
                .filter(|entry| *entry == hook)
                .count()
        })
        .unwrap_or(0)
}

/// Drops events left with no entries at all, so uninstalling leaves no empty
/// list behind where there was no key before.
fn prune_empty_events(hooks: &mut Map<String, Value>) {
    let empty: Vec<String> = hooks
        .iter()
        .filter(|(_, groups)| groups.as_array().is_some_and(|groups| groups.is_empty()))
        .map(|(event, _)| event.clone())
        .collect();

    for event in empty {
        hooks.shift_remove(&event);
    }
}

/// The port a URL or command string names.
fn port_in(text: &str) -> Option<u16> {
    let needle = format!("{LOOPBACK_HOST}:");
    let start = text.find(&needle)? + needle.len();

    text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 51_234;

    fn hooks_of(json: Value) -> Map<String, Value> {
        json.as_object().expect("an object").clone()
    }

    fn installed(port: u16) -> Map<String, Value> {
        let mut hooks = Map::new();
        insert_aydy_hooks(&mut hooks, port);

        hooks
    }

    fn sound_hook(sound: &str) -> Value {
        serde_json::json!({
            "type": "command",
            "command": format!("afplay /System/Library/Sounds/{sound}.aiff"),
        })
    }

    #[test]
    fn all_five_events_are_registered() {
        let hooks = installed(PORT);

        let mut events: Vec<&String> = hooks.keys().collect();
        events.sort();

        assert_eq!(
            events,
            [
                "Notification",
                "SessionEnd",
                "SessionStart",
                "Stop",
                "UserPromptSubmit"
            ]
        );
    }

    #[test]
    fn session_start_is_a_command_hook_and_never_an_http_one() {
        let planned = planned_hooks(PORT);
        let session_start = planned
            .iter()
            .find(|hook| hook.event == SESSION_START)
            .expect("a SessionStart hook");

        assert_eq!(session_start.kind, HookKind::Command);
        assert_eq!(
            session_start.hook.get("type").and_then(Value::as_str),
            Some("command")
        );
        assert_eq!(session_start.hook.get("url"), None);
    }

    #[test]
    fn every_registered_hook_declares_a_timeout_of_at_most_two_seconds() {
        for (event, hook) in aydy_entries(&installed(PORT)) {
            let timeout = hook
                .get("timeout")
                .and_then(Value::as_f64)
                .unwrap_or_else(|| panic!("{event} to declare a timeout"));

            assert!(
                timeout > 0.0 && timeout <= 2.0,
                "{event} declares a timeout of {timeout}s"
            );
        }
    }

    #[test]
    fn the_session_end_hook_declares_a_timeout_of_five_hundred_milliseconds() {
        let hooks = installed(PORT);
        let (_, hook) = aydy_entries(&hooks)
            .into_iter()
            .find(|(event, _)| event == SESSION_END)
            .expect("a SessionEnd hook");

        assert_eq!(hook.get("timeout").and_then(Value::as_f64), Some(0.5));
        assert_eq!(
            serde_json::to_string(hook.get("timeout").expect("a timeout")).unwrap(),
            "0.5"
        );
    }

    #[test]
    fn the_session_start_command_is_silent_bounded_and_forgiving() {
        let command = session_start_command(PORT);

        assert!(command.contains("-s"), "{command}");
        assert!(command.contains("-o /dev/null"), "{command}");
        assert!(command.contains("--max-time "), "{command}");
        assert!(command.trim_end().ends_with("|| true"), "{command}");
    }

    #[test]
    fn the_session_start_command_carries_the_port_that_no_url_field_holds() {
        let hooks = installed(PORT);
        let (_, hook) = aydy_entries(&hooks)
            .into_iter()
            .find(|(event, _)| event == SESSION_START)
            .expect("a SessionStart hook");

        assert_eq!(hook.get("url"), None);
        assert_eq!(port_of(&hook), Some(PORT));
    }

    #[test]
    fn aydy_entries_are_found_by_their_marker_and_not_by_their_url() {
        let mut hooks = installed(PORT);
        rewrite_port(&mut hooks, 60_000);

        assert_eq!(aydy_entries(&hooks).len(), 5);
    }

    #[test]
    fn a_port_move_rewrites_every_registered_entry() {
        let mut hooks = installed(PORT);

        let rewritten = rewrite_port(&mut hooks, 60_000);

        assert_eq!(rewritten, 5);
        for (event, hook) in aydy_entries(&hooks) {
            assert_eq!(
                port_of(&hook),
                Some(60_000),
                "{event} still names an old port"
            );
        }
    }

    #[test]
    fn a_port_move_rewrites_the_url_inside_the_session_start_command() {
        let mut hooks = installed(PORT);

        rewrite_port(&mut hooks, 60_000);

        let (_, hook) = aydy_entries(&hooks)
            .into_iter()
            .find(|(event, _)| event == SESSION_START)
            .expect("a SessionStart hook");
        let command = hook
            .get("command")
            .and_then(Value::as_str)
            .expect("a command");

        assert!(command.contains("http://127.0.0.1:60000/hook"), "{command}");
        assert!(!command.contains("51234"), "{command}");
    }

    #[test]
    fn a_port_move_leaves_entries_that_are_not_aydys_alone() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "hooks": [
                { "type": "command", "command": "afplay /System/Library/Sounds/Funk.aiff" },
                { "type": "http", "url": "http://127.0.0.1:51234/elsewhere" }
            ] }]
        }));

        rewrite_port(&mut hooks, 60_000);

        assert_eq!(
            hooks["Stop"][0]["hooks"],
            serde_json::json!([
                { "type": "command", "command": "afplay /System/Library/Sounds/Funk.aiff" },
                { "type": "http", "url": "http://127.0.0.1:51234/elsewhere" }
            ])
        );
    }

    #[test]
    fn installing_twice_leaves_exactly_one_set_of_entries() {
        let mut hooks = installed(PORT);

        remove_aydy_hooks(&mut hooks);
        insert_aydy_hooks(&mut hooks, PORT);

        assert_eq!(aydy_entries(&hooks).len(), 5);
    }

    #[test]
    fn removing_aydys_entries_leaves_no_hooks_behind() {
        let mut hooks = installed(PORT);

        let removed = remove_aydy_hooks(&mut hooks);

        assert_eq!(removed, 5);
        assert!(hooks.is_empty(), "{hooks:?}");
    }

    #[test]
    fn both_sound_hooks_are_identified_and_removed() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Funk")] }],
            "Notification": [{ "matcher": "*", "hooks": [sound_hook("Hero")] }]
        }));

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed.len(), 2);
        assert!(hooks.is_empty(), "{hooks:?}");
    }

    #[test]
    fn a_removed_sound_hook_records_the_group_it_came_from_including_its_matcher() {
        let mut hooks = hooks_of(serde_json::json!({
            "Notification": [{ "matcher": "*", "hooks": [sound_hook("Hero")] }]
        }));

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(
            removed[0].group,
            serde_json::json!({ "matcher": "*", "hooks": [sound_hook("Hero")] })
        );
        assert!(removed[0].group_removed);
    }

    #[test]
    fn a_stop_hook_that_is_not_a_sound_player_is_left_alone() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [
                { "type": "command", "command": "/Users/eric/bin/notify-me.sh" }
            ] }]
        });
        let mut hooks = hooks_of(original.clone());

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed, Vec::new());
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn a_command_that_only_mentions_the_sound_player_is_left_alone() {
        for command in [
            "afplay /System/Library/Sounds/Funk.aiff && rsync -a ~/notes /backup",
            "echo done | afplay -",
            "my-wrapper afplay /System/Library/Sounds/Funk.aiff",
            "$SOUND /System/Library/Sounds/Funk.aiff",
            "afplay-all /System/Library/Sounds/Funk.aiff",
        ] {
            let mut hooks = hooks_of(serde_json::json!({
                "Stop": [{ "hooks": [{ "type": "command", "command": command }] }]
            }));

            assert_eq!(
                take_over_sound_hooks(&mut hooks),
                Vec::new(),
                "{command} was identified as a sound hook"
            );
        }
    }

    #[test]
    fn a_sound_player_named_by_its_full_path_is_identified() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "hooks": [
                { "type": "command", "command": "/usr/bin/afplay /System/Library/Sounds/Funk.aiff" }
            ] }]
        }));

        assert_eq!(take_over_sound_hooks(&mut hooks).len(), 1);
    }

    #[test]
    fn a_sound_hook_registered_for_another_event_is_left_alone() {
        let original = serde_json::json!({
            "SessionEnd": [{ "hooks": [sound_hook("Funk")] }]
        });
        let mut hooks = hooks_of(original.clone());

        assert_eq!(take_over_sound_hooks(&mut hooks), Vec::new());
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn a_sound_hook_sharing_a_group_leaves_the_rest_of_the_group_in_place() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "matcher": "*", "hooks": [
                sound_hook("Funk"),
                { "type": "command", "command": "/Users/eric/bin/notify-me.sh" }
            ] }]
        }));

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed.len(), 1);
        assert!(!removed[0].group_removed);
        assert_eq!(
            hooks["Stop"],
            serde_json::json!([{ "matcher": "*", "hooks": [
                { "type": "command", "command": "/Users/eric/bin/notify-me.sh" }
            ] }])
        );
    }

    #[test]
    fn restoring_puts_both_sound_hooks_back_exactly_as_they_were() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Funk")] }],
            "Notification": [{ "matcher": "*", "hooks": [sound_hook("Hero")] }]
        });
        let mut hooks = hooks_of(original.clone());
        let removed = take_over_sound_hooks(&mut hooks);

        restore_entries(&mut hooks, &removed);

        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn restoring_puts_a_hook_back_beside_the_group_members_that_stayed() {
        let original = serde_json::json!({
            "Stop": [{ "matcher": "*", "hooks": [
                sound_hook("Funk"),
                { "type": "command", "command": "/Users/eric/bin/notify-me.sh" }
            ] }]
        });
        let mut hooks = hooks_of(original.clone());
        let removed = take_over_sound_hooks(&mut hooks);

        restore_entries(&mut hooks, &removed);

        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn restoring_twice_does_not_duplicate_an_entry() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Funk")] }]
        });
        let mut hooks = hooks_of(original.clone());
        let removed = take_over_sound_hooks(&mut hooks);

        restore_entries(&mut hooks, &removed);
        restore_entries(&mut hooks, &removed);

        assert_eq!(Value::Object(hooks), original);
    }

    /// A different sound per notification kind: two matcher groups, one
    /// `afplay` each, and the two command strings identical.
    fn twin_notification_sounds() -> Value {
        serde_json::json!({
            "Notification": [
                { "matcher": "permission_prompt", "hooks": [sound_hook("Hero")] },
                { "matcher": "idle_prompt", "hooks": [sound_hook("Hero")] }
            ]
        })
    }

    #[test]
    fn two_identical_sound_hooks_under_one_event_are_both_removed_and_restored() {
        let original = twin_notification_sounds();
        let mut hooks = hooks_of(original.clone());

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed.len(), 2);
        assert!(hooks.is_empty(), "{hooks:?}");

        let restored = restore_entries(&mut hooks, &removed);

        assert_eq!(restored, 2);
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn two_identical_sound_hooks_in_one_group_are_both_removed_and_restored() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Funk"), sound_hook("Funk")] }]
        });
        let mut hooks = hooks_of(original.clone());

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed.len(), 2);

        let restored = restore_entries(&mut hooks, &removed);

        assert_eq!(restored, 1);
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn two_identical_sound_hooks_beside_a_survivor_are_both_restored() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [
                sound_hook("Funk"),
                sound_hook("Funk"),
                { "type": "command", "command": "/Users/eric/bin/notify-me.sh" }
            ] }]
        });
        let mut hooks = hooks_of(original.clone());

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed.len(), 2);

        restore_entries(&mut hooks, &removed);

        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn identical_sound_hooks_under_two_events_are_both_restored() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Hero")] }],
            "Notification": [{ "matcher": "*", "hooks": [sound_hook("Hero")] }]
        });
        let mut hooks = hooks_of(original.clone());

        let removed = take_over_sound_hooks(&mut hooks);

        assert_eq!(removed.len(), 2);

        let restored = restore_entries(&mut hooks, &removed);

        assert_eq!(restored, 2);
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn restoring_twice_does_not_duplicate_identical_entries() {
        let original = twin_notification_sounds();
        let mut hooks = hooks_of(original.clone());
        let removed = take_over_sound_hooks(&mut hooks);

        restore_entries(&mut hooks, &removed);
        let again = restore_entries(&mut hooks, &removed);

        assert_eq!(again, 0);
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn nothing_is_registered_before_an_install() {
        assert_eq!(
            registered_hooks(Some(&Map::new())),
            RegisteredHooks::default()
        );
        assert_eq!(registered_hooks(None), RegisteredHooks::default());
    }

    #[test]
    fn installed_hooks_report_the_port_they_post_to() {
        assert_eq!(
            registered_hooks(Some(&installed(PORT))),
            RegisteredHooks {
                installed: true,
                port: Some(PORT)
            }
        );
    }

    #[test]
    fn the_session_start_command_alone_reports_the_registered_port() {
        let mut hooks = installed(PORT);
        for event in HTTP_EVENTS {
            hooks.shift_remove(event);
        }

        assert_eq!(
            registered_hooks(Some(&hooks)),
            RegisteredHooks {
                installed: true,
                port: Some(PORT)
            }
        );
    }

    #[test]
    fn entries_that_disagree_about_the_port_report_none() {
        let mut hooks = installed(PORT);
        hooks["Stop"][0]["hooks"][0]["url"] = Value::from(hook_url(60_000));

        assert_eq!(
            registered_hooks(Some(&hooks)),
            RegisteredHooks {
                installed: true,
                port: None
            }
        );
    }

    #[test]
    fn a_sound_hook_is_not_mistaken_for_an_aydy_entry() {
        assert!(!is_aydy_hook(&sound_hook("Funk")));
    }

    #[test]
    fn an_event_left_empty_by_a_removal_loses_its_key() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Funk")] }],
            "PreToolUse": [{ "hooks": [{ "type": "command", "command": "true" }] }]
        }));

        take_over_sound_hooks(&mut hooks);

        assert_eq!(hooks.keys().collect::<Vec<&String>>(), ["PreToolUse"]);
    }

    #[test]
    fn a_port_is_read_out_of_a_url_and_out_of_a_command() {
        assert_eq!(port_in("http://127.0.0.1:51234/hook"), Some(51_234));
        assert_eq!(
            port_in("curl -s --data-binary @- http://127.0.0.1:60000/hook || true"),
            Some(60_000)
        );
        assert_eq!(port_in("afplay /System/Library/Sounds/Funk.aiff"), None);
    }
}
