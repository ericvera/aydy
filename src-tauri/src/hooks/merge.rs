//! Recognising and removing the hook entries older AYDY versions installed in
//! the `hooks` block.
//!
//! Everything here works on the `hooks` object as a [`serde_json::Map`], and
//! every entry it does not positively recognise is left exactly as it was.

use serde_json::{Map, Value};

/// The header older AYDY versions marked their entries with.
///
/// The marker cannot be the URL: the port moves, and an entry that could only
/// be found by the port it used to name would be orphaned by the first move.
/// A header is also the one field on a hook that is free-form by design, so
/// marking there adds no key to a shape Claude Code validates.
pub const MARKER_HEADER: &str = "X-Aydy-Hook";

/// The marker header's value.
pub const MARKER_VALUE: &str = "1";

/// Removes every entry an older AYDY registered, and reports how many went.
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

/// Whether `hook` is one an older AYDY registered.
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

/// Drops events left with no entries at all, so a removal leaves no empty
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hooks_of(json: Value) -> Map<String, Value> {
        json.as_object().expect("an object").clone()
    }

    fn legacy_http_hook() -> Value {
        serde_json::json!({
            "type": "http",
            "url": "http://127.0.0.1:51234/hook",
            "headers": { "X-Aydy-Hook": "1" },
            "timeout": 2
        })
    }

    fn legacy_curl_hook() -> Value {
        serde_json::json!({
            "type": "command",
            "command": "curl -s -o /dev/null --max-time 1 -H 'Content-Type: application/json' -H 'X-Aydy-Hook: 1' --data-binary @- http://127.0.0.1:51234/hook || true",
            "timeout": 2
        })
    }

    fn sound_hook(sound: &str) -> Value {
        serde_json::json!({
            "type": "command",
            "command": format!("afplay /System/Library/Sounds/{sound}.aiff"),
        })
    }

    fn extension_hook() -> Value {
        serde_json::json!({
            "type": "command",
            "command": "f=\"$HOME/Library/Application Support/app.aydy/bin/aydy-hook.sh\"; [ -f \"$f\" ] && /bin/sh \"$f\"; exit 0"
        })
    }

    #[test]
    fn legacy_http_and_curl_entries_are_recognised() {
        assert!(is_aydy_hook(&legacy_http_hook()));
        assert!(is_aydy_hook(&legacy_curl_hook()));
    }

    #[test]
    fn a_sound_hook_is_not_mistaken_for_an_aydy_entry() {
        assert!(!is_aydy_hook(&sound_hook("Funk")));
    }

    #[test]
    fn the_extensions_hook_is_not_mistaken_for_a_legacy_entry() {
        assert!(!is_aydy_hook(&extension_hook()));
    }

    #[test]
    fn an_http_hook_without_the_marker_is_left_alone() {
        let hook = serde_json::json!({ "type": "http", "url": "http://127.0.0.1:51234/hook" });

        assert!(!is_aydy_hook(&hook));
    }

    #[test]
    fn removing_aydys_entries_leaves_no_hooks_behind() {
        let mut hooks = hooks_of(serde_json::json!({
            "SessionStart": [{ "hooks": [legacy_curl_hook()] }],
            "Stop": [{ "hooks": [legacy_http_hook()] }],
            "SessionEnd": [{ "hooks": [legacy_http_hook()] }]
        }));

        let removed = remove_aydy_hooks(&mut hooks);

        assert_eq!(removed, 3);
        assert!(hooks.is_empty(), "{hooks:?}");
    }

    #[test]
    fn removing_aydys_entries_keeps_every_other_entry_in_its_group() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "hooks": [sound_hook("Funk"), legacy_http_hook(), extension_hook()] }]
        }));

        let removed = remove_aydy_hooks(&mut hooks);

        assert_eq!(removed, 1);
        assert_eq!(
            Value::Object(hooks),
            serde_json::json!({
                "Stop": [{ "hooks": [sound_hook("Funk"), extension_hook()] }]
            })
        );
    }

    #[test]
    fn an_event_left_empty_by_a_removal_loses_its_key() {
        let mut hooks = hooks_of(serde_json::json!({
            "Stop": [{ "hooks": [legacy_http_hook()] }],
            "PreToolUse": [{ "hooks": [{ "type": "command", "command": "true" }] }]
        }));

        remove_aydy_hooks(&mut hooks);

        assert_eq!(hooks.keys().collect::<Vec<&String>>(), ["PreToolUse"]);
    }
}
