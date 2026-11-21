//! The hook endpoint and the payload Claude Code posts to it, and the
//! endpoints the companion VS Code extension registers itself on and reports
//! its window's focus from.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::extension::registry::{ExtensionPid, Registration, HEARTBEAT_INTERVAL};
use crate::listener::server::{ListenerState, RegistryInput};
use crate::session::hook_payload::{present, HookPayload};
use crate::session::tab_match::WindowFocus;

/// The path the registered hooks post to.
pub const HOOK_PATH: &str = "/hook";

/// The path a VS Code window registers itself on.
pub const EXTENSION_REGISTER_PATH: &str = "/extension/register";

/// The path a registered window says it is still there on.
pub const EXTENSION_HEARTBEAT_PATH: &str = "/extension/heartbeat";

/// The path a window closing cleanly says so on.
pub const EXTENSION_UNREGISTER_PATH: &str = "/extension/unregister";

/// The path a window reports its focus and its active tab on.
pub const EXTENSION_FOCUS_PATH: &str = "/extension/focus";

/// Receives one hook event.
///
/// Answers 200 with an empty body before the event is applied, whatever the
/// payload turns out to be. Claude Code blocks the user's session on this
/// response and reports anything else — a non-2xx status, or a body that is
/// neither empty nor a JSON object — as a hook error in their transcript. A
/// monitoring tool has no business degrading the session it is monitoring, so
/// an unreadable body is dropped silently rather than reported.
pub async fn receive_hook(
    State(state): State<ListenerState>,
    body: Result<Bytes, BytesRejection>,
) -> StatusCode {
    if let Some(event) = body
        .ok()
        .and_then(|body| serde_json::from_slice::<HookPayload>(&body).ok())
        .and_then(HookPayload::into_event)
    {
        state.dispatch(RegistryInput::Hook(event));
    }

    StatusCode::OK
}

/// What a VS Code window reports about itself.
///
/// Only the two identifying fields are required. A window that reports no
/// folders is a window with no folder open, which is a real thing to be and is
/// not a reason to refuse it.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RegistrationPayload {
    /// The extension host's own process id, which is what names the window.
    pub pid: Option<ExtensionPid>,
    /// The loopback port this window will take commands on.
    pub callback_port: Option<u16>,
    pub workspace_folders: Vec<PathBuf>,
    pub claude_code_version: Option<String>,
}

impl RegistrationPayload {
    /// The registration this payload describes, or `None` when it does not
    /// name a window AYDY could address.
    ///
    /// Zero is not a process and not a port, and either would register a window
    /// that no session could ever be matched to and no command could ever
    /// reach.
    pub fn into_registration(self) -> Option<Registration> {
        Some(Registration {
            pid: self.pid.filter(|pid| *pid != 0)?,
            callback_port: self.callback_port.filter(|port| *port != 0)?,
            workspace_folders: self.workspace_folders,
            claude_code_version: present(self.claude_code_version),
        })
    }
}

/// A window naming itself and nothing else.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowPayload {
    pub pid: Option<ExtensionPid>,
}

/// What a window reports about its focus and its active tab.
///
/// The two are separate fields and are never combined into one. A tab active in
/// a window sitting behind a browser is the walked-away case the app exists to
/// notify about, so "this tab is active" says nothing on its own about whether
/// the user is looking at it.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FocusPayload {
    pub pid: Option<ExtensionPid>,
    /// Whether the reporting window currently has focus.
    pub focused: bool,
    /// The label on the window's active Claude Code tab. Null — and an absent
    /// field — mean no Claude Code tab is active in this window, which is a
    /// different fact from a tab whose session cannot be told, and the label is
    /// carried raw because only AYDY holds the titles to match it against.
    pub active_tab_label: Option<String>,
}

impl FocusPayload {
    /// The report this payload describes, or `None` when it names no window.
    pub fn into_focus(self) -> Option<WindowFocus> {
        Some(WindowFocus {
            window_id: self.pid.filter(|pid| *pid != 0)?,
            focused: self.focused,
            active_tab_label: self.active_tab_label,
        })
    }
}

/// AYDY's answer to a window that reported itself.
///
/// The body is what tells the extension it reached an AYDY that understands
/// these endpoints at all: the listener answers anything it does not route with
/// 200 and an empty body, so a status alone would have an older AYDY look like
/// a successful registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredPayload {
    pub registered: bool,
    /// How often AYDY expects to hear from a registered window. Sent rather
    /// than assumed, because the extension ships and updates on its own
    /// cadence and a constant compiled into an older build would drift.
    pub heartbeat_seconds: u64,
}

impl RegisteredPayload {
    /// The answer for a window AYDY now holds.
    pub fn registered() -> Self {
        Self {
            registered: true,
            heartbeat_seconds: HEARTBEAT_INTERVAL.as_secs(),
        }
    }

    /// The answer for a window AYDY does not hold, whatever the reason.
    pub fn unregistered() -> Self {
        Self {
            registered: false,
            heartbeat_seconds: HEARTBEAT_INTERVAL.as_secs(),
        }
    }
}

/// Registers one VS Code window, once its callback port has answered.
///
/// The confirming ping is what makes `registered` mean something: a window is
/// only useful if AYDY can reach it, and finding that out now costs one
/// loopback round trip, whereas finding it out later means a jump that silently
/// does nothing. A window that does not answer is dropped and will register
/// again on its next attempt.
pub async fn register_extension(
    State(state): State<ListenerState>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Some(registration) =
        read::<RegistrationPayload>(body).and_then(|payload| payload.into_registration())
    else {
        return answered(StatusCode::BAD_REQUEST, RegisteredPayload::unregistered());
    };

    let pid = registration.pid;
    let extensions = Arc::clone(state.extensions());
    extensions.registry().register(registration);

    let confirmed = tokio::task::spawn_blocking(move || {
        if extensions.ping(pid).is_err() {
            extensions.registry().forget(pid);

            return false;
        }

        true
    })
    .await
    .unwrap_or(false);

    if confirmed {
        answered(StatusCode::OK, RegisteredPayload::registered())
    } else {
        answered(StatusCode::OK, RegisteredPayload::unregistered())
    }
}

/// Records that a registered window is still open.
///
/// Answering `registered: false` is how a window learns to register again
/// without anything having to reach it — after AYDY restarted, or after its
/// registration expired while the machine was asleep.
pub async fn extension_heartbeat(
    State(state): State<ListenerState>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Some(pid) = read::<WindowPayload>(body).and_then(|payload| payload.pid) else {
        return answered(StatusCode::BAD_REQUEST, RegisteredPayload::unregistered());
    };

    if state.extensions().registry().heartbeat(pid) {
        answered(StatusCode::OK, RegisteredPayload::registered())
    } else {
        answered(StatusCode::OK, RegisteredPayload::unregistered())
    }
}

/// Drops a window that is closing, rather than waiting out its timeout.
///
/// The registry is told as well as the extension registry: a window that has
/// gone must stop being the answer to where the user is, or the session its
/// last report left in front goes on suppressing its own notifications with
/// nothing left to correct it.
pub async fn unregister_extension(
    State(state): State<ListenerState>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Some(pid) = read::<WindowPayload>(body).and_then(|payload| payload.pid) else {
        return answered(StatusCode::BAD_REQUEST, RegisteredPayload::unregistered());
    };

    state.extensions().registry().forget(pid);
    state.dispatch(RegistryInput::WindowClosed(pid));

    answered(StatusCode::OK, RegisteredPayload::unregistered())
}

/// Receives one window's focus and active tab.
///
/// Only a window AYDY currently holds is listened to. Focus is a fact about
/// every window at once — one reporting it takes it from whichever had it — and
/// a registration is the app's only proof that a process is the window it says
/// it is, confirmed over that window's own callback port when it registered.
/// An unheld window is answered `registered: false`, which is how it learns to
/// register again rather than reporting into nothing.
pub async fn extension_focus(
    State(state): State<ListenerState>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Some(focus) = read::<FocusPayload>(body).and_then(FocusPayload::into_focus) else {
        return answered(StatusCode::BAD_REQUEST, RegisteredPayload::unregistered());
    };

    if state
        .extensions()
        .registry()
        .registration(focus.window_id)
        .is_none()
    {
        return answered(StatusCode::OK, RegisteredPayload::unregistered());
    }

    // The same report is the only confirmation a jump ever gets. Nothing else
    // can say the window came forward — `code` answers as soon as it has handed
    // the folder over, and macOS tells an app nothing about another app's
    // windows — so a jump waiting on this window is answered here, before the
    // report is queued for the registry, rather than made to wait behind it.
    state.jumps().confirm(&focus);
    state.dispatch(RegistryInput::Focus(focus));

    answered(StatusCode::OK, RegisteredPayload::registered())
}

fn read<T: for<'a> Deserialize<'a>>(body: Result<Bytes, BytesRejection>) -> Option<T> {
    serde_json::from_slice(&body.ok()?).ok()
}

fn answered(status: StatusCode, payload: RegisteredPayload) -> Response {
    let body = serde_json::to_string(&payload).unwrap_or_default();

    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration(body: &str) -> Option<Registration> {
        serde_json::from_str::<RegistrationPayload>(body)
            .ok()
            .and_then(RegistrationPayload::into_registration)
    }

    #[test]
    fn a_window_reporting_its_process_id_and_callback_port_registers() {
        let registered = registration(
            r#"{"pid":65921,"callbackPort":55123,"workspaceFolders":["/Users/eric/Code/aydy"],"claudeCodeVersion":"2.1.238"}"#,
        )
        .expect("a registration");

        assert_eq!(registered.pid, 65_921);
        assert_eq!(registered.callback_port, 55_123);
        assert_eq!(
            registered.workspace_folders,
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
        assert_eq!(registered.claude_code_version, Some("2.1.238".to_string()));
    }

    /// A window with no folder open, or with the Claude Code extension not
    /// installed at all. Neither is a reason to refuse the window: what it is
    /// for is naming which sessions belong to it.
    #[test]
    fn a_window_with_no_folders_and_no_claude_code_still_registers() {
        let registered =
            registration(r#"{"pid":65921,"callbackPort":55123}"#).expect("a registration");

        assert_eq!(registered.workspace_folders, Vec::<PathBuf>::new());
        assert_eq!(registered.claude_code_version, None);
    }

    #[test]
    fn a_window_that_names_no_process_id_or_no_port_cannot_be_registered() {
        assert_eq!(registration(r#"{"callbackPort":55123}"#), None);
        assert_eq!(registration(r#"{"pid":65921}"#), None);
        assert_eq!(registration(r#"{"pid":65921,"callbackPort":0}"#), None);
        assert_eq!(registration(r#"{"pid":0,"callbackPort":55123}"#), None);
        assert_eq!(registration("not json at all"), None);
    }

    /// The window is what the extension reports, and the extension is a
    /// separate program on its own release cadence. A field it stops sending
    /// must not take the rest of the registration with it.
    #[test]
    fn a_registration_carrying_fields_aydy_does_not_know_still_registers() {
        assert!(
            registration(r#"{"pid":65921,"callbackPort":55123,"somethingNew":{"deep":true}}"#)
                .is_some()
        );
    }

    fn window_focus(body: &str) -> Option<WindowFocus> {
        serde_json::from_str::<FocusPayload>(body)
            .ok()
            .and_then(FocusPayload::into_focus)
    }

    /// The split the whole feature turns on: what arrives is two fields, and
    /// neither one implies the other.
    #[test]
    fn a_focus_report_carries_focus_and_the_active_tab_as_two_separate_facts() {
        let focus =
            window_focus(r#"{"pid":65921,"focused":true,"activeTabLabel":"Fix the auth flow"}"#)
                .expect("a report");

        assert_eq!(focus.window_id, 65_921);
        assert!(focus.focused);
        assert_eq!(
            focus.active_tab_label,
            Some("Fix the auth flow".to_string())
        );
    }

    #[test]
    fn a_window_reporting_a_tab_it_is_not_looking_at_reports_it_unfocused() {
        let focus =
            window_focus(r#"{"pid":65921,"focused":false,"activeTabLabel":"Fix the auth flow"}"#)
                .expect("a report");

        assert!(!focus.focused);
        assert_eq!(
            focus.active_tab_label,
            Some("Fix the auth flow".to_string())
        );
    }

    /// No Claude tab active is a different fact from one whose session cannot
    /// be told, so the absent label is carried as absent rather than as a name
    /// nothing matches.
    #[test]
    fn a_window_with_no_claude_tab_active_reports_no_label() {
        assert_eq!(
            window_focus(r#"{"pid":65921,"focused":true,"activeTabLabel":null}"#)
                .expect("a report")
                .active_tab_label,
            None
        );
        assert_eq!(
            window_focus(r#"{"pid":65921,"focused":true}"#)
                .expect("a report")
                .active_tab_label,
            None
        );
    }

    /// The label is carried exactly as the editor shows it, because only AYDY
    /// holds the titles it has to be compared against.
    #[test]
    fn the_placeholder_label_is_carried_through_rather_than_read_as_a_name() {
        assert_eq!(
            window_focus(r#"{"pid":65921,"focused":true,"activeTabLabel":"Claude Code"}"#)
                .expect("a report")
                .active_tab_label,
            Some("Claude Code".to_string())
        );
    }

    #[test]
    fn a_focus_report_naming_no_window_is_not_a_report() {
        assert_eq!(window_focus(r#"{"focused":true}"#), None);
        assert_eq!(window_focus(r#"{"pid":0,"focused":true}"#), None);
        assert_eq!(window_focus("not json at all"), None);
    }

    #[test]
    fn the_answer_names_the_cadence_aydy_expects_to_hear_from_the_window_on() {
        assert_eq!(
            serde_json::to_string(&RegisteredPayload::registered()).expect("a body"),
            r#"{"registered":true,"heartbeatSeconds":10}"#
        );
        assert_eq!(
            serde_json::to_string(&RegisteredPayload::unregistered()).expect("a body"),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
    }
}
