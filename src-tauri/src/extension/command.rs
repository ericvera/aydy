//! The channel from AYDY back to one particular VS Code window.
//!
//! The extension binds a loopback port of its own and names it at
//! registration, and AYDY posts commands to that port. The alternative — the
//! extension holding an open request to AYDY and receiving commands on it —
//! needs a correlation id and a second request to carry each answer back,
//! because a command's result is the point: revealing a tab either worked or it
//! did not, and AYDY has to say so rather than assume. A port per window makes
//! the answer the response to the call that asked, and makes addressing one
//! window a different socket rather than a routed stream.
//!
//! Every reply names the window that sent it. A port is only an address, and a
//! closed window's port can be taken by something else; a reply from a PID
//! other than the one addressed is treated as no reply at all.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::extension::registry::{ExtensionPid, ExtensionRegistry};

/// The path the extension serves commands on.
pub const COMMAND_PATH: &str = "/command";

/// How long a command waits for the window to answer.
///
/// The extension host is a Node process on the same machine doing almost
/// nothing on this path, so anything slower than this is a window that has
/// stopped answering rather than one that is thinking.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

/// What AYDY asks one window to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "command", rename_all = "camelCase")]
pub enum Command {
    /// A no-op, answered by the window that received it.
    ///
    /// It exists so the channel is proven before anything depends on it: a
    /// window whose callback port cannot be reached is not registered at all,
    /// which is decided at registration rather than the first time a jump is
    /// attempted.
    Ping,
    /// Bring the tab showing `session_id` to the front of this window.
    ///
    /// `tab_label` is what the tab is expected to read, and it is sent so the
    /// window can check for itself that the tab is there before it calls
    /// Claude Code — the call opens a second live copy of the session when the
    /// session is not open in the window that makes it.
    #[serde(rename_all = "camelCase")]
    Reveal {
        session_id: String,
        tab_label: String,
    },
}

/// One command, addressed to the window meant to run it.
///
/// The command's own fields ride alongside `pid` rather than under it, so a
/// reveal reads as one flat object on the wire and `ping` is written exactly as
/// it always was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    pub pid: ExtensionPid,
    #[serde(flatten)]
    pub command: Command,
}

/// What a window did about a reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealOutcome {
    /// The session's tab was in this window and has been brought forward.
    Revealed,
    /// No tab in this window carries that session, so nothing was called.
    /// This is the guard, not a failure: calling anyway is what duplicates a
    /// session.
    NotPresent,
    /// The Claude Code extension in that window is a version this reveal path
    /// has not been checked against, so the tab was left alone.
    UnexpectedVersion,
    /// A tab opened when the call was made, which means the session was not
    /// there after all and a second live copy of it may now exist.
    Duplicated,
    /// The call itself raised, so nothing was revealed.
    Failed,
    /// A word this build of AYDY does not know.
    Unknown,
}

impl<'de> Deserialize<'de> for RevealOutcome {
    /// Reads whatever the window said, including something this build has
    /// never heard of.
    ///
    /// The extension is installed from the Marketplace and updated on a
    /// cadence of its own, so a newer one naming an outcome added since is an
    /// ordinary state of the world. Refusing to read the reply would make AYDY
    /// forget the window, and forgetting a window it can plainly reach is a
    /// worse answer than not knowing what it said.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let said = String::deserialize(deserializer)?;

        Ok(match said.as_str() {
            "revealed" => Self::Revealed,
            "notPresent" => Self::NotPresent,
            "unexpectedVersion" => Self::UnexpectedVersion,
            "duplicated" => Self::Duplicated,
            "failed" => Self::Failed,
            _ => Self::Unknown,
        })
    }
}

/// What a window answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReply {
    /// The answering extension host's own process id.
    pub pid: ExtensionPid,
    /// Whether it did what it was asked.
    pub ok: bool,
    /// What it did about a reveal, which `ok` alone cannot carry: a reveal
    /// that was refused and one that opened a tab are both `false` and are not
    /// the same thing to do next.
    #[serde(default)]
    pub reveal: Option<RevealOutcome>,
}

/// Why a command did not get where it was going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// No window is registered under that process id.
    NotRegistered,
    /// The window's callback port could not be posted to.
    Undeliverable(String),
    /// Something answered, but not with a reply AYDY can read.
    Unanswered,
    /// A different window answered, so the port has been taken by someone else.
    WrongWindow { answered: ExtensionPid },
    /// The window answered that it could not do it.
    Refused,
}

/// How a command is carried to a window.
///
/// A trait so the dispatcher's addressing and liveness rules can be driven
/// without binding sockets, and so the one place that speaks HTTP is the one
/// place that has to be right about it.
pub trait ExtensionChannel: Send + Sync {
    /// Posts `body` to the command endpoint of the extension on `port`,
    /// answering with the body it replied with.
    fn post(&self, port: u16, body: &str) -> Result<String, String>;
}

/// The real channel, over loopback HTTP.
#[derive(Debug)]
pub struct LoopbackChannel {
    agent: ureq::Agent,
}

impl LoopbackChannel {
    /// A channel that posts to whatever loopback port it is handed.
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(COMMAND_TIMEOUT))
                .user_agent(concat!("AYDY/", env!("CARGO_PKG_VERSION")))
                // A status is the window's answer about itself, which is read
                // here rather than collapsed into a transport failure.
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }
}

impl Default for LoopbackChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionChannel for LoopbackChannel {
    fn post(&self, port: u16, body: &str) -> Result<String, String> {
        let url = format!("http://127.0.0.1:{port}{COMMAND_PATH}");

        match self
            .agent
            .post(&url)
            .content_type("application/json")
            .send(body)
        {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let answer = response.body_mut().read_to_string().unwrap_or_default();

                if status == 200 {
                    Ok(answer)
                } else {
                    Err(format!("it answered {status}"))
                }
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Sends commands to registered windows.
pub struct ExtensionCommands {
    registry: Arc<ExtensionRegistry>,
    channel: Arc<dyn ExtensionChannel>,
}

impl ExtensionCommands {
    /// A dispatcher that addresses the windows in `registry` over `channel`.
    pub fn new(registry: Arc<ExtensionRegistry>, channel: Arc<dyn ExtensionChannel>) -> Self {
        Self { registry, channel }
    }

    /// The windows this dispatcher can reach.
    pub fn registry(&self) -> &Arc<ExtensionRegistry> {
        &self.registry
    }

    /// Sends `command` to the window behind `pid`, and to no other.
    ///
    /// A window that cannot be reached, or that turns out to be a different
    /// window on a recycled port, is forgotten on the spot. Leaving it
    /// registered would keep AYDY offering to jump to a window that is not
    /// there, and — once Task 5.4 sends a reveal — would aim that reveal at
    /// whichever window did answer, which is how a second live copy of a
    /// session gets opened.
    pub fn send(&self, pid: ExtensionPid, command: Command) -> Result<(), CommandError> {
        if self.request(pid, command)?.ok {
            Ok(())
        } else {
            Err(CommandError::Refused)
        }
    }

    /// Sends `command` to the window behind `pid` and hands back its whole
    /// reply.
    ///
    /// A reveal answers more than yes or no — a refusal and an accidentally
    /// opened tab are both a `false` — so the caller that has to tell them
    /// apart reads the reply rather than a mapped error.
    pub fn request(
        &self,
        pid: ExtensionPid,
        command: Command,
    ) -> Result<CommandReply, CommandError> {
        let Some(registration) = self.registry.registration(pid) else {
            return Err(CommandError::NotRegistered);
        };

        let request = CommandRequest { pid, command };
        let Ok(body) = serde_json::to_string(&request) else {
            return Err(CommandError::Undeliverable(
                "the command could not be written".to_string(),
            ));
        };

        let answer = match self.channel.post(registration.callback_port, &body) {
            Ok(answer) => answer,
            Err(reason) => {
                self.registry.forget(pid);

                return Err(CommandError::Undeliverable(reason));
            }
        };

        let Ok(reply) = serde_json::from_str::<CommandReply>(&answer) else {
            self.registry.forget(pid);

            return Err(CommandError::Unanswered);
        };

        if reply.pid != pid {
            self.registry.forget(pid);

            return Err(CommandError::WrongWindow {
                answered: reply.pid,
            });
        }

        Ok(reply)
    }

    /// Confirms the channel to the window behind `pid` is open.
    pub fn ping(&self, pid: ExtensionPid) -> Result<(), CommandError> {
        self.send(pid, Command::Ping)
    }

    /// Asks the window behind `pid` to bring the tab showing `session_id`
    /// forward, and answers what it did.
    ///
    /// The window is asked rather than told: it checks its own tabs before it
    /// calls Claude Code, because this is the one command in the app whose
    /// misdelivery starts a second live copy of the user's session. AYDY has
    /// already proved through the process tree that this window runs the
    /// session; the window proves the tab is in front of it.
    pub fn reveal(
        &self,
        pid: ExtensionPid,
        session_id: &str,
        tab_label: &str,
    ) -> Result<RevealOutcome, CommandError> {
        let reply = self.request(
            pid,
            Command::Reveal {
                session_id: session_id.to_string(),
                tab_label: tab_label.to_string(),
            },
        )?;

        // A window that said nothing about the reveal is not a window that
        // revealed anything, whatever `ok` says.
        Ok(reply.reveal.unwrap_or(RevealOutcome::Unknown))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use crate::clock::TestClock;
    use crate::extension::registry::Registration;
    use crate::listener::server::lock_recovering;

    /// One post a channel was asked to make.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Posted {
        port: u16,
        body: String,
    }

    /// A channel that records what it was asked to post and answers with
    /// whatever it was told to.
    struct FakeChannel {
        posted: Mutex<Vec<Posted>>,
        answer: Mutex<Result<String, String>>,
    }

    impl FakeChannel {
        fn answering(answer: Result<String, String>) -> Arc<Self> {
            Arc::new(Self {
                posted: Mutex::new(Vec::new()),
                answer: Mutex::new(answer),
            })
        }

        fn replying(pid: ExtensionPid, ok: bool) -> Arc<Self> {
            Self::answering(Ok(format!(r#"{{"pid":{pid},"ok":{ok}}}"#)))
        }

        /// A channel whose windows each answer as themselves, which is what
        /// makes a command posted to the wrong port visible.
        fn echoing() -> Arc<Self> {
            Self::answering(Ok(String::new()))
        }

        fn posted(&self) -> Vec<Posted> {
            lock_recovering(&self.posted).clone()
        }
    }

    impl ExtensionChannel for FakeChannel {
        fn post(&self, port: u16, body: &str) -> Result<String, String> {
            lock_recovering(&self.posted).push(Posted {
                port,
                body: body.to_string(),
            });

            match lock_recovering(&self.answer).clone() {
                Ok(answer) if answer.is_empty() => {
                    let addressed: serde_json::Value =
                        serde_json::from_str(body).expect("a command request");

                    Ok(format!(r#"{{"pid":{},"ok":true}}"#, addressed["pid"]))
                }
                answer => answer,
            }
        }
    }

    fn window(pid: ExtensionPid, callback_port: u16) -> Registration {
        Registration {
            pid,
            callback_port,
            workspace_folders: vec![PathBuf::from("/Users/eric/Code/aydy")],
            claude_code_version: Some("2.1.238".to_string()),
        }
    }

    fn commands(channel: Arc<dyn ExtensionChannel>) -> (Arc<ExtensionRegistry>, ExtensionCommands) {
        let registry = Arc::new(ExtensionRegistry::new(Arc::new(TestClock::new())));

        (
            Arc::clone(&registry),
            ExtensionCommands::new(registry, channel),
        )
    }

    #[test]
    fn a_command_reaches_the_window_it_is_addressed_to() {
        let channel = FakeChannel::replying(65_921, true);
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(commands.ping(65_921), Ok(()));

        assert_eq!(
            channel.posted(),
            vec![Posted {
                port: 55_123,
                body: r#"{"pid":65921,"command":"ping"}"#.to_string()
            }]
        );
    }

    /// The whole reason a command is addressed at all: a reveal sent to a
    /// window that does not host the session opens a second live copy of it.
    /// Each window is asked in turn and each has to be the only one posted to,
    /// whichever order they registered in.
    #[test]
    fn a_command_for_one_window_is_never_posted_to_another() {
        let channel = FakeChannel::echoing();
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(17_804, 55_124));
        registry.register(window(65_921, 55_123));

        assert_eq!(commands.ping(65_921), Ok(()));
        assert_eq!(commands.ping(17_804), Ok(()));

        assert_eq!(
            channel.posted(),
            vec![
                Posted {
                    port: 55_123,
                    body: r#"{"pid":65921,"command":"ping"}"#.to_string()
                },
                Posted {
                    port: 55_124,
                    body: r#"{"pid":17804,"command":"ping"}"#.to_string()
                }
            ]
        );
    }

    #[test]
    fn a_command_for_a_window_that_never_registered_is_not_sent_anywhere() {
        let channel = FakeChannel::replying(65_921, true);
        let (_registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);

        assert_eq!(commands.ping(65_921), Err(CommandError::NotRegistered));

        assert_eq!(channel.posted(), Vec::new());
    }

    /// The window closed between registering and being asked, which is the
    /// liveness signal that does not have to wait for the timeout.
    #[test]
    fn a_window_that_cannot_be_reached_is_dropped() {
        let channel = FakeChannel::answering(Err("connection refused".to_string()));
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.ping(65_921),
            Err(CommandError::Undeliverable(
                "connection refused".to_string()
            ))
        );

        assert_eq!(registry.registration(65_921), None);
    }

    /// The window closed and something else took its port. Trusting that answer
    /// would leave AYDY addressing a stranger as though it were a VS Code
    /// window.
    #[test]
    fn a_reply_from_a_different_window_is_not_an_answer_and_drops_the_registration() {
        let channel = FakeChannel::replying(17_804, true);
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.ping(65_921),
            Err(CommandError::WrongWindow { answered: 17_804 })
        );

        assert_eq!(registry.registration(65_921), None);
    }

    #[test]
    fn a_reply_that_cannot_be_read_drops_the_registration() {
        let channel = FakeChannel::answering(Ok("not json at all".to_string()));
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(commands.ping(65_921), Err(CommandError::Unanswered));

        assert_eq!(registry.registration(65_921), None);
    }

    /// The window is there and answering; it just could not do this. That is
    /// not a liveness failure, so the registration stays.
    #[test]
    fn a_window_that_refuses_a_command_stays_registered() {
        let channel = FakeChannel::replying(65_921, false);
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(commands.ping(65_921), Err(CommandError::Refused));

        assert_eq!(registry.registration(65_921), Some(window(65_921, 55_123)));
    }

    #[test]
    fn a_reveal_names_the_session_and_the_tab_it_expects_to_find() {
        let channel = FakeChannel::answering(Ok(
            r#"{"pid":65921,"ok":true,"reveal":"revealed"}"#.to_string()
        ));
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.reveal(65_921, "s1", "Models section configura\u{2026}"),
            Ok(RevealOutcome::Revealed)
        );

        assert_eq!(
            channel.posted(),
            vec![Posted {
                port: 55_123,
                body: r#"{"pid":65921,"command":"reveal","sessionId":"s1","tabLabel":"Models section configura…"}"#
                    .to_string()
            }]
        );
    }

    /// The guard on the far side doing its job. It is not a failure of the
    /// channel, so the window stays registered and stays jumpable.
    #[test]
    fn a_window_that_refuses_a_reveal_says_why_and_stays_registered() {
        let channel = FakeChannel::answering(Ok(
            r#"{"pid":65921,"ok":false,"reveal":"notPresent"}"#.to_string(),
        ));
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.reveal(65_921, "s1", "Fix the auth flow"),
            Ok(RevealOutcome::NotPresent)
        );

        assert_eq!(registry.registration(65_921), Some(window(65_921, 55_123)));
    }

    /// The single most damaging outcome this app has, and it has to survive the
    /// trip back rather than arriving as a plain `false`.
    #[test]
    fn a_reveal_that_opened_a_tab_comes_back_as_a_duplicate() {
        let channel = FakeChannel::answering(Ok(
            r#"{"pid":65921,"ok":false,"reveal":"duplicated"}"#.to_string(),
        ));
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.reveal(65_921, "s1", "Fix the auth flow"),
            Ok(RevealOutcome::Duplicated)
        );
    }

    /// A newer extension naming an outcome added since this build shipped. The
    /// window is plainly reachable, so it keeps its registration; AYDY simply
    /// does not claim the tab was revealed.
    #[test]
    fn an_outcome_this_build_does_not_know_is_read_as_unknown() {
        let channel = FakeChannel::answering(Ok(
            r#"{"pid":65921,"ok":false,"reveal":"sidebarInstead"}"#.to_string(),
        ));
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.reveal(65_921, "s1", "Fix the auth flow"),
            Ok(RevealOutcome::Unknown)
        );

        assert_eq!(registry.registration(65_921), Some(window(65_921, 55_123)));
    }

    /// An extension that predates reveals answers the shape it always did.
    /// Reading that as a reveal would have AYDY wait for a tab that was never
    /// brought forward.
    #[test]
    fn a_reply_that_says_nothing_about_the_reveal_is_not_a_reveal() {
        let channel = FakeChannel::replying(65_921, true);
        let (registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);
        registry.register(window(65_921, 55_123));

        assert_eq!(
            commands.reveal(65_921, "s1", "Fix the auth flow"),
            Ok(RevealOutcome::Unknown)
        );
    }

    #[test]
    fn a_reveal_for_a_window_that_never_registered_is_not_sent_anywhere() {
        let channel = FakeChannel::echoing();
        let (_registry, commands) = commands(Arc::clone(&channel) as Arc<dyn ExtensionChannel>);

        assert_eq!(
            commands.reveal(65_921, "s1", "Fix the auth flow"),
            Err(CommandError::NotRegistered)
        );

        assert_eq!(channel.posted(), Vec::new());
    }

    /// The real client against a real socket, so the transport is proven rather
    /// than only the addressing above it.
    #[tokio::test]
    async fn the_real_channel_completes_a_round_trip_over_loopback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let port = listener.local_addr().expect("a local address").port();
        let router = axum::Router::new().route(
            COMMAND_PATH,
            axum::routing::post(|body: String| async move {
                let request: serde_json::Value =
                    serde_json::from_str(&body).expect("a command request");

                format!(r#"{{"pid":{},"ok":true}}"#, request["pid"])
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let registry = Arc::new(ExtensionRegistry::new(Arc::new(TestClock::new())));
        registry.register(window(65_921, port));
        let commands = ExtensionCommands::new(registry, Arc::new(LoopbackChannel::new()));

        let answered = tokio::task::spawn_blocking(move || commands.ping(65_921))
            .await
            .expect("the ping to run");

        assert_eq!(answered, Ok(()));
    }

    #[tokio::test]
    async fn the_real_channel_reports_a_port_nothing_is_listening_on() {
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let port = closed.local_addr().expect("a local address").port();
        drop(closed);

        let registry = Arc::new(ExtensionRegistry::new(Arc::new(TestClock::new())));
        registry.register(window(65_921, port));
        let watching = Arc::clone(&registry);
        let commands = ExtensionCommands::new(registry, Arc::new(LoopbackChannel::new()));

        let answered = tokio::task::spawn_blocking(move || commands.ping(65_921))
            .await
            .expect("the ping to run");

        assert!(matches!(answered, Err(CommandError::Undeliverable(_))));
        assert_eq!(watching.registration(65_921), None);
    }
}
