//! The payload Claude Code's hooks carry, and the registry event it describes.

use std::path::PathBuf;

use serde::Deserialize;

use crate::session::event::{HookEvent, HookEventKind};

/// One hook payload.
///
/// Every field is optional and unknown fields are ignored, so a payload from a
/// Claude Code version that renamed or dropped something still parses instead
/// of failing the whole request.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct HookPayload {
    pub session_id: Option<String>,
    pub hook_event_name: Option<String>,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    /// `Notification` only: what the session is waiting for, in the words
    /// Claude Code shows the user. This is the only account of what is being
    /// awaited, so it is carried through to the registry.
    pub message: Option<String>,
    /// `Notification` only: the machine-readable kind of prompt —
    /// `permission_prompt`, `idle_prompt` and friends. Used only when the
    /// message is missing, so that something describes the wait rather than
    /// nothing.
    pub notification_type: Option<String>,
    /// `Stop` only: the background work still attached to the session, kept as
    /// raw JSON so a malformed value counts as none instead of failing the
    /// payload.
    pub background_tasks: Option<serde_json::Value>,
}

/// The `notification_type`s that tell the user something without asking
/// anything of them.
const INFORMATIONAL_NOTIFICATION_TYPES: [&str; 2] = ["agent_completed", "auth_success"];

impl HookPayload {
    /// The registry event this payload describes, or `None` when it names no
    /// session or an event the app does not track.
    pub fn into_event(self) -> Option<HookEvent> {
        let session_id = present(self.session_id)?;

        let kind = match self.hook_event_name.as_deref() {
            Some("SessionStart") => HookEventKind::SessionStart,
            Some("UserPromptSubmit") => HookEventKind::UserPromptSubmit,
            Some("Notification")
                if self.notification_type.as_deref().is_some_and(|kind| {
                    INFORMATIONAL_NOTIFICATION_TYPES.contains(&kind.trim())
                }) =>
            {
                HookEventKind::Informational
            }
            // When the message is missing the fallback is the raw enum token —
            // `permission_prompt`, `idle_prompt` and friends — not copy written
            // for a person, so anything showing this to the user has to
            // translate it first.
            Some("Notification") => HookEventKind::Notification {
                message: present(self.message).or_else(|| present(self.notification_type)),
            },
            Some("Stop") if subagents_running(self.background_tasks.as_ref()) => {
                HookEventKind::SubagentsRunning
            }
            Some("Stop") => HookEventKind::Stop,
            Some("SessionEnd") => HookEventKind::SessionEnd,
            _ => return None,
        };

        Some(HookEvent {
            session_id,
            cwd: self.cwd,
            transcript_path: self.transcript_path,
            kind,
        })
    }
}

fn subagents_running(tasks: Option<&serde_json::Value>) -> bool {
    tasks
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tasks| {
            tasks.iter().any(|task| {
                task.get("type").and_then(serde_json::Value::as_str) == Some("subagent")
                    && task.get("status").and_then(serde_json::Value::as_str) == Some("running")
            })
        })
}

pub fn present(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(body: &str) -> Option<HookEvent> {
        serde_json::from_str::<HookPayload>(body)
            .ok()
            .and_then(HookPayload::into_event)
    }

    #[test]
    fn a_session_start_payload_becomes_a_session_start_event() {
        let event =
            payload(r#"{"session_id":"s1","hook_event_name":"SessionStart","source":"startup"}"#)
                .expect("an event");

        assert_eq!(event.kind, HookEventKind::SessionStart);
        assert_eq!(event.session_id, "s1");
    }

    #[test]
    fn a_user_prompt_submit_payload_becomes_a_user_prompt_submit_event() {
        let event = payload(r#"{"session_id":"s1","hook_event_name":"UserPromptSubmit"}"#)
            .expect("an event");

        assert_eq!(event.kind, HookEventKind::UserPromptSubmit);
    }

    #[test]
    fn a_stop_payload_becomes_a_stop_event() {
        let event = payload(r#"{"session_id":"s1","hook_event_name":"Stop"}"#).expect("an event");

        assert_eq!(event.kind, HookEventKind::Stop);
    }

    #[test]
    fn a_stop_while_a_subagent_is_running_becomes_a_subagents_running_event() {
        let event = payload(
            r#"{"session_id":"s1","hook_event_name":"Stop","background_tasks":[{"id":"a4cf","type":"subagent","status":"running","agent_type":"mise:runner"}]}"#,
        )
        .expect("an event");

        assert_eq!(event.kind, HookEventKind::SubagentsRunning);
    }

    #[test]
    fn a_stop_with_no_running_subagent_is_a_stop() {
        for tasks in [
            r#""background_tasks":[],"#,
            r#""background_tasks":[{"type":"shell","status":"running"}],"#,
            r#""background_tasks":[{"type":"subagent","status":"completed"}],"#,
            "",
        ] {
            let body = format!(r#"{{{tasks}"session_id":"s1","hook_event_name":"Stop"}}"#);

            assert_eq!(
                payload(&body).expect("an event").kind,
                HookEventKind::Stop,
                "{body}"
            );
        }
    }

    #[test]
    fn a_stop_with_malformed_background_tasks_is_a_stop() {
        for tasks in [r#""oops""#, r#"[1, null, {"type":"subagent"}]"#] {
            let body = format!(
                r#"{{"session_id":"s1","hook_event_name":"Stop","background_tasks":{tasks}}}"#
            );

            assert_eq!(
                payload(&body).expect("an event").kind,
                HookEventKind::Stop,
                "{body}"
            );
        }
    }

    #[test]
    fn an_informational_notification_becomes_an_informational_event() {
        for body in [
            r#"{"session_id":"s1","hook_event_name":"Notification","notification_type":"agent_completed"}"#,
            r#"{"session_id":"s1","hook_event_name":"Notification","notification_type":"auth_success","message":"Signed in"}"#,
        ] {
            assert_eq!(
                payload(body).expect("an event").kind,
                HookEventKind::Informational,
                "{body}"
            );
        }
    }

    #[test]
    fn a_prompt_notification_still_asks_for_input() {
        for kind in ["permission_prompt", "idle_prompt"] {
            let body = format!(
                r#"{{"session_id":"s1","hook_event_name":"Notification","notification_type":"{kind}"}}"#
            );

            assert!(matches!(
                payload(&body).expect("an event").kind,
                HookEventKind::Notification { .. }
            ));
        }
    }

    #[test]
    fn a_session_end_payload_becomes_a_session_end_event() {
        let event =
            payload(r#"{"session_id":"s1","hook_event_name":"SessionEnd","reason":"clear"}"#)
                .expect("an event");

        assert_eq!(event.kind, HookEventKind::SessionEnd);
    }

    #[test]
    fn a_notification_payload_carries_the_message_describing_the_wait() {
        let event = payload(
            r#"{"session_id":"s1","hook_event_name":"Notification","message":"Claude needs your permission to use Bash","notification_type":"permission_prompt"}"#,
        )
        .expect("an event");

        assert_eq!(
            event.kind,
            HookEventKind::Notification {
                message: Some("Claude needs your permission to use Bash".to_string())
            }
        );
    }

    #[test]
    fn a_notification_without_a_message_falls_back_to_the_notification_type() {
        let event = payload(
            r#"{"session_id":"s1","hook_event_name":"Notification","notification_type":"idle_prompt"}"#,
        )
        .expect("an event");

        assert_eq!(
            event.kind,
            HookEventKind::Notification {
                message: Some("idle_prompt".to_string())
            }
        );
    }

    #[test]
    fn a_notification_describing_nothing_carries_no_message() {
        let event =
            payload(r#"{"session_id":"s1","hook_event_name":"Notification"}"#).expect("an event");

        assert_eq!(event.kind, HookEventKind::Notification { message: None });
    }

    #[test]
    fn the_working_directory_and_transcript_path_are_carried_through() {
        let event = payload(
            r#"{"session_id":"s1","hook_event_name":"Stop","cwd":"/Users/eric/Code/aydy","transcript_path":"/Users/eric/.claude/projects/aydy/s1.jsonl"}"#,
        )
        .expect("an event");

        assert_eq!(event.cwd, Some(PathBuf::from("/Users/eric/Code/aydy")));
        assert_eq!(
            event.transcript_path,
            Some(PathBuf::from("/Users/eric/.claude/projects/aydy/s1.jsonl"))
        );
    }

    #[test]
    fn an_event_name_the_app_does_not_track_produces_no_event() {
        assert_eq!(
            payload(r#"{"session_id":"s1","hook_event_name":"PreToolUse"}"#),
            None
        );
    }

    #[test]
    fn a_payload_naming_no_session_produces_no_event() {
        assert_eq!(payload(r#"{"hook_event_name":"Stop"}"#), None);
        assert_eq!(
            payload(r#"{"session_id":"  ","hook_event_name":"Stop"}"#),
            None
        );
    }

    #[test]
    fn a_body_that_is_not_a_hook_payload_produces_no_event() {
        assert_eq!(payload("not json at all"), None);
        assert_eq!(payload("[1, 2, 3]"), None);
        assert_eq!(payload(r#"{"session_id":42}"#), None);
    }
}
