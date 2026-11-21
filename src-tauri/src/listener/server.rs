//! The axum listener: routing, binding, the registry pump and shutdown.

use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use crate::extension::command::ExtensionCommands;
use crate::listener::port::{bind_loopback, PortMemory, PortState};
use crate::listener::routes::{
    extension_focus, extension_heartbeat, receive_hook, register_extension, unregister_extension,
    EXTENSION_FOCUS_PATH, EXTENSION_HEARTBEAT_PATH, EXTENSION_REGISTER_PATH,
    EXTENSION_UNREGISTER_PATH, HOOK_PATH,
};
use crate::session::event::WindowId;
use crate::session::registry::SessionRegistry;
use crate::session::tab_match::WindowFocus;
pub use crate::sync::lock_recovering;

/// One thing the registry is told, queued off the request path.
///
/// Everything the listener receives arrives on one queue so that it is applied
/// in the order it was received.
#[derive(Debug)]
pub enum RegistryInput {
    /// What a VS Code window reports about its focus and its active tab.
    Focus(WindowFocus),
    /// A window that has closed, so its focus stops counting as the last thing
    /// known about where the user is.
    WindowClosed(WindowId),
}

/// The state every request handler shares.
#[derive(Clone)]
pub struct ListenerState {
    inputs: mpsc::UnboundedSender<RegistryInput>,
    extensions: Arc<ExtensionCommands>,
}

impl ListenerState {
    /// A state that queues registry inputs onto `inputs` and registers windows
    /// with `extensions`.
    pub fn new(
        inputs: mpsc::UnboundedSender<RegistryInput>,
        extensions: Arc<ExtensionCommands>,
    ) -> Self {
        Self { inputs, extensions }
    }

    /// The registered VS Code windows, and the way back to one of them.
    pub fn extensions(&self) -> &Arc<ExtensionCommands> {
        &self.extensions
    }

    /// Queues `input` for the registry and returns.
    ///
    /// The queue is unbounded and never applies anything itself, so the
    /// response is never waiting on the registry lock, on another event, or on
    /// anything the registry triggers. Claude Code blocks the user's session
    /// until the response arrives, which is why no work happens on that path.
    pub fn dispatch(&self, input: RegistryInput) {
        let _ = self.inputs.send(input);
    }
}

/// The router serving the hook endpoint and the companion extension's.
///
/// Anything that is not one of those endpoints is answered the same way the
/// hook endpoint answers a payload it cannot read: 200 with an empty body. A
/// registered URL that misses by a trailing slash, a path or a method would
/// otherwise be a 404 or a 405, and a non-2xx is a hook error in the user's
/// Claude session just as surely as a non-JSON body is. The extension's
/// endpoints are on the same listener because there is one port to discover and
/// one thing to be running, not because they answer the same way — theirs carry
/// a body, which is how the extension tells them apart from that fallback.
pub fn router(state: ListenerState) -> Router {
    Router::new()
        .route(HOOK_PATH, post(receive_hook))
        .route(EXTENSION_REGISTER_PATH, post(register_extension))
        .route(EXTENSION_HEARTBEAT_PATH, post(extension_heartbeat))
        .route(EXTENSION_UNREGISTER_PATH, post(unregister_extension))
        .route(EXTENSION_FOCUS_PATH, post(extension_focus))
        .fallback(accept_quietly)
        .method_not_allowed_fallback(accept_quietly)
        .with_state(state)
}

/// Answers a request that reached no route with the only response that cannot
/// degrade the caller's session.
async fn accept_quietly() -> StatusCode {
    StatusCode::OK
}

/// Applies queued inputs to the registry, one at a time, off the request path.
///
/// Blocking, and run on the blocking pool by [`spawn_pump`]: waiting for the
/// registry lock on an async worker would hold that worker hostage, and a
/// blocked worker can starve the runtime's IO and timer driver — which would
/// stop the listener answering at all, the one thing this module exists to
/// prevent.
///
/// A panic applying one input is caught by [`apply_catching`] so that the input
/// is dropped rather than the pump, and every later one is still tracked.
pub fn pump_events(
    registry: Arc<Mutex<SessionRegistry>>,
    mut inputs: mpsc::UnboundedReceiver<RegistryInput>,
) {
    while let Some(input) = inputs.blocking_recv() {
        apply_catching(|| apply_input(&registry, input));
    }
}

/// Applies one queued input, holding the registry lock for exactly as long as
/// it takes.
fn apply_input(registry: &Mutex<SessionRegistry>, input: RegistryInput) {
    let mut registry = lock_recovering(registry);

    match input {
        RegistryInput::Focus(focus) => registry.apply_window_focus(focus),
        RegistryInput::WindowClosed(window_id) => registry.remove_window(window_id),
    }
}

/// Runs one input's `apply`, catching a panic it raises.
///
/// The pump has to outlive any single input: a panic applying one of them costs
/// that input and nothing else, because a dead pump would silently stop
/// tracking every session for the rest of the app's life.
pub fn apply_catching(apply: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(apply)).is_err() {
        eprintln!("aydy: dropped a listener event after a panic applying it");
    }
}

/// Starts the registry pump on the blocking pool.
pub fn spawn_pump(
    registry: Arc<Mutex<SessionRegistry>>,
    inputs: mpsc::UnboundedReceiver<RegistryInput>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || pump_events(registry, inputs))
}

/// Serves `state` on `listener` until `shutdown` fires.
pub async fn serve(
    listener: TcpListener,
    state: ListenerState,
    shutdown: oneshot::Receiver<()>,
) -> io::Result<()> {
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
        })
        .await
}

/// Binds a loopback port and applies what the companion extension reports to
/// `registry` until `shutdown` fires.
///
/// The remembered port is preferred so the registered hook URLs usually stay
/// valid across launches; when it is taken, the OS assigns another, the new one
/// is remembered, and the move is published for the hook installer to rewrite
/// the URLs and for the popover to report.
pub async fn run(
    memory: PortMemory,
    ports: Arc<PortState>,
    registry: Arc<Mutex<SessionRegistry>>,
    extensions: Arc<ExtensionCommands>,
    shutdown: oneshot::Receiver<()>,
) {
    let bound = match bind_loopback(memory.remembered()).await {
        Ok(bound) => bound,
        Err(error) => {
            eprintln!("aydy: could not bind a loopback port for hook events: {error}");
            return;
        }
    };

    if let Err(error) = memory.remember(bound.port) {
        eprintln!(
            "aydy: could not remember port {}, the next launch will take a new one: {error}",
            bound.port
        );
    }

    ports.set_bound(bound.port, bound.moved_from);

    let (inputs, queued) = mpsc::unbounded_channel();
    let pump = spawn_pump(registry, queued);

    if let Err(error) = serve(
        bound.listener,
        ListenerState::new(inputs, extensions),
        shutdown,
    )
    .await
    {
        eprintln!("aydy: the hook listener stopped: {error}");
    }

    ports.set_unbound();
    let _ = pump.await;
}

/// The graceful-shutdown trigger for the running listener.
#[derive(Debug)]
pub struct ListenerShutdown {
    signal: Mutex<Option<oneshot::Sender<()>>>,
}

impl ListenerShutdown {
    /// A trigger that fires `signal`.
    pub fn new(signal: oneshot::Sender<()>) -> Self {
        Self {
            signal: Mutex::new(Some(signal)),
        }
    }

    /// Asks the listener to stop serving. Firing twice does nothing.
    pub fn trigger(&self) {
        if let Some(signal) = lock_recovering(&self.signal).take() {
            let _ = signal.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc as blocking;
    use std::time::{Duration, Instant};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::clock::SystemClock;
    use crate::discovery::process_tree::FixedParents;
    use crate::discovery::session_files::SessionFile;
    use crate::discovery::window_map::associate;
    use crate::extension::command::{LoopbackChannel, COMMAND_PATH};
    use crate::extension::registry::{ExtensionPid, ExtensionRegistry};
    use crate::listener::port::PortMove;
    use crate::session::event::{HookEvent, HookEventKind, SessionEnrichment};

    static SCRATCH: AtomicU32 = AtomicU32::new(0);

    /// A running listener, its registry, and the scratch directory holding its
    /// remembered port.
    struct Harness {
        port: u16,
        ports: Arc<PortState>,
        registry: Arc<Mutex<SessionRegistry>>,
        windows: Arc<ExtensionRegistry>,
        memory: PortMemory,
        shutdown: ListenerShutdown,
        serving: JoinHandle<()>,
        home: PathBuf,
    }

    impl Harness {
        /// A listener started with `remembered` written to its port memory.
        async fn start(remembered: Option<u16>) -> Self {
            let home = std::env::temp_dir().join(format!(
                "aydy-listener-{}-{}",
                std::process::id(),
                SCRATCH.fetch_add(1, Ordering::Relaxed)
            ));
            let memory = PortMemory::new(home.join("listener-port"));

            if let Some(port) = remembered {
                memory.remember(port).expect("the port to be remembered");
            }

            let ports = Arc::new(PortState::new());
            let registry = Arc::new(Mutex::new(SessionRegistry::new(Arc::new(SystemClock))));
            // The real channel, so the confirming ping the registration
            // endpoint makes goes over a real socket rather than a stand-in.
            let windows = Arc::new(ExtensionRegistry::new(Arc::new(SystemClock)));
            let extensions = Arc::new(ExtensionCommands::new(
                Arc::clone(&windows),
                Arc::new(LoopbackChannel::new()),
            ));
            let (signal, shutdown) = oneshot::channel();
            let serving = tokio::spawn(run(
                memory.clone(),
                Arc::clone(&ports),
                Arc::clone(&registry),
                extensions,
                shutdown,
            ));
            let port = tokio::time::timeout(Duration::from_secs(10), ports.wait_for_port())
                .await
                .expect("the listener to bind within ten seconds")
                .expect("the listener to bind a port");

            Self {
                port,
                ports,
                registry,
                windows,
                memory,
                shutdown: ListenerShutdown::new(signal),
                serving,
                home,
            }
        }

        /// Posts `body` to the hook endpoint and returns the raw response.
        async fn post(&self, body: &str) -> String {
            post_to(self.port, body).await
        }

        /// Posts `body` to one of the companion extension's endpoints.
        async fn post_extension(&self, path: &str, body: &str) -> String {
            request_to(self.port, "POST", path, body).await
        }

        /// Registers `window` the way the extension does.
        async fn register(&self, window: &FakeWindow) -> String {
            self.post_extension(
                EXTENSION_REGISTER_PATH,
                &format!(
                    r#"{{"pid":{},"callbackPort":{},"workspaceFolders":["/Users/eric/Code/aydy"],"claudeCodeVersion":"2.1.238"}}"#,
                    window.pid, window.port
                ),
            )
            .await
        }

        /// Gives the pump time to apply anything already queued, for a test
        /// asserting that something did not happen.
        async fn settle(&self) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        /// Waits for `condition` to hold, failing after ten seconds.
        async fn eventually(&self, condition: impl Fn(&Self) -> bool) {
            for _ in 0..2_000 {
                if condition(self) {
                    return;
                }

                tokio::time::sleep(Duration::from_millis(5)).await;
            }

            panic!("the registry never reached the expected state");
        }

        /// How many sessions the registry is tracking.
        fn tracked(&self) -> usize {
            lock_recovering(&self.registry).sessions().count()
        }

        /// Tracks and labels `sessions`, and places every one of them in
        /// `window` — the state a scan and a transcript pass leave behind
        /// before any tab has been reported.
        fn place(&self, window: ExtensionPid, sessions: &[(&str, &str)]) {
            let files: Vec<SessionFile> = sessions
                .iter()
                .enumerate()
                .map(|(index, (session_id, _))| SessionFile {
                    pid: 90_000 + index as u32,
                    session_id: (*session_id).to_string(),
                    cwd: None,
                    entrypoint: None,
                })
                .collect();
            let parents = FixedParents::new(
                files
                    .iter()
                    .map(|file| (file.pid, window))
                    .collect::<Vec<(u32, ExtensionPid)>>(),
            );
            let mut registry = lock_recovering(&self.registry);

            for (session_id, label) in sessions {
                registry.apply_hook_event(HookEvent::new(*session_id, HookEventKind::SessionStart));
                registry.apply_enrichment(
                    session_id,
                    SessionEnrichment {
                        label: Some((*label).to_string()),
                        ..SessionEnrichment::default()
                    },
                );
            }

            registry.apply_window_map(&associate(&files, &[window], &parents));
        }

        /// The session the registry believes the user is in.
        fn current(&self) -> Option<String> {
            lock_recovering(&self.registry)
                .current_session()
                .map(str::to_string)
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            self.shutdown.trigger();
            self.serving.abort();
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    async fn post_to(port: u16, body: &str) -> String {
        request_to(port, "POST", HOOK_PATH, body).await
    }

    async fn request_to(port: u16, method: &str, path: &str, body: &str) -> String {
        tokio::time::timeout(
            Duration::from_secs(10),
            raw_request(port, method, path, body),
        )
        .await
        .expect("the endpoint to answer within ten seconds")
    }

    async fn raw_request(port: u16, method: &str, path: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("the listener to accept a connection");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );

        stream
            .write_all(request.as_bytes())
            .await
            .expect("the request to be written");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("a response");

        response
    }

    /// Asserts the response is the only shape Claude Code accepts without
    /// putting a hook error in the user's session: a 200, and a body that is
    /// empty or a JSON object.
    fn assert_accepted(response: &str) {
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "expected a 200, got {response:?}"
        );

        let body = body_of(response);

        assert!(
            body.is_empty() || body.starts_with('{'),
            "expected an empty body or a JSON object, got {body:?}"
        );
    }

    /// The body of a raw response.
    fn body_of(response: &str) -> &str {
        response
            .split_once("\r\n\r\n")
            .expect("a response with a body separator")
            .1
    }

    fn event(session_id: &str, name: &str) -> String {
        format!(r#"{{"session_id":"{session_id}","hook_event_name":"{name}"}}"#)
    }

    /// A stand-in for one VS Code window's extension host: a loopback server
    /// answering commands the way the companion extension does, and recording
    /// what it was sent.
    struct FakeWindow {
        pid: ExtensionPid,
        port: u16,
        received: Arc<Mutex<Vec<String>>>,
        serving: JoinHandle<()>,
    }

    impl FakeWindow {
        /// A window that answers as itself.
        async fn open(pid: ExtensionPid) -> Self {
            Self::answering_as(pid, pid).await
        }

        /// A window that answers naming `answers_as` rather than `pid`, which
        /// is what a stranger on a recycled port looks like.
        async fn answering_as(pid: ExtensionPid, answers_as: ExtensionPid) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback port");
            let port = listener.local_addr().expect("a local address").port();
            let received = Arc::new(Mutex::new(Vec::new()));
            let recording = Arc::clone(&received);
            let router = Router::new().route(
                COMMAND_PATH,
                post(move |body: String| {
                    let recording = Arc::clone(&recording);

                    async move {
                        lock_recovering(&recording).push(body);

                        format!(r#"{{"pid":{answers_as},"ok":true}}"#)
                    }
                }),
            );
            let serving = tokio::spawn(async move {
                let _ = axum::serve(listener, router).await;
            });

            Self {
                pid,
                port,
                received,
                serving,
            }
        }

        /// A window that has already gone: a port nothing is listening on.
        async fn closed(pid: ExtensionPid) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback port");
            let port = listener.local_addr().expect("a local address").port();
            drop(listener);

            Self {
                pid,
                port,
                received: Arc::new(Mutex::new(Vec::new())),
                serving: tokio::spawn(async {}),
            }
        }

        fn received(&self) -> Vec<String> {
            lock_recovering(&self.received).clone()
        }
    }

    impl Drop for FakeWindow {
        fn drop(&mut self) {
            self.serving.abort();
        }
    }

    /// The legacy hook entries keep posting until the app removes them, and a
    /// non-2xx is a hook error in the user's session, so every post is still
    /// accepted. Session state comes from the status files now.
    #[tokio::test]
    async fn a_hook_post_is_accepted_and_leaves_the_registry_alone() {
        let listener = Harness::start(None).await;

        for body in [
            event("s1", "SessionStart"),
            event("s1", "UserPromptSubmit"),
            event("s1", "Notification"),
            event("s1", "Stop"),
            event("s1", "SessionEnd"),
            event("s1", "PreToolUse"),
            r#"{"hook_event_name":"Stop"}"#.to_string(),
            "{not json at all".to_string(),
            String::new(),
        ] {
            let response = listener.post(&body).await;

            assert_accepted(&response);
            assert_eq!(body_of(&response), "");
        }

        listener.settle().await;
        assert_eq!(listener.tracked(), 0);
    }

    /// A hook URL or method that misses the endpoint is the installer's
    /// mistake, and the user's session must not pay for it: a 404 or a 405 is a
    /// hook error in their transcript exactly as a non-JSON body is.
    #[tokio::test]
    async fn a_request_that_misses_the_hook_endpoint_is_still_accepted() {
        let listener = Harness::start(None).await;

        for (method, path) in [
            ("POST", "/hook/"),
            ("POST", "/"),
            ("POST", "/hooks"),
            ("GET", HOOK_PATH),
            ("PUT", HOOK_PATH),
        ] {
            let response = request_to(listener.port, method, path, "").await;

            assert_accepted(&response);
            assert_eq!(
                body_of(&response),
                "",
                "expected an empty body from {method} {path}, got {response:?}"
            );
        }
    }

    #[test]
    fn a_panic_applying_one_event_drops_that_event_and_not_the_pump() {
        let reached = Arc::new(Mutex::new(Vec::new()));
        let applied = Arc::new(Mutex::new(Vec::new()));

        for event in ["panicking", "next"] {
            let reached = Arc::clone(&reached);
            let applied = Arc::clone(&applied);

            apply_catching(move || {
                lock_recovering(&reached).push(event);

                if event == "panicking" {
                    panic!("applying a hook event");
                }

                lock_recovering(&applied).push(event);
            });
        }

        assert_eq!(*lock_recovering(&reached), ["panicking", "next"]);
        assert_eq!(*lock_recovering(&applied), ["next"]);
    }

    #[tokio::test]
    async fn the_bound_port_is_remembered_for_the_next_launch() {
        let listener = Harness::start(None).await;

        assert_eq!(listener.memory.remembered(), Some(listener.port));
    }

    #[tokio::test]
    async fn a_busy_remembered_port_is_left_behind_and_the_move_is_signalled() {
        let occupied = bind_loopback(None).await.expect("a loopback port");

        let listener = Harness::start(Some(occupied.port)).await;

        assert_ne!(listener.port, occupied.port);
        assert_eq!(
            listener.ports.moved(),
            Some(PortMove {
                from: occupied.port,
                to: listener.port
            })
        );
        assert_eq!(listener.memory.remembered(), Some(listener.port));
    }

    /// Runs on a multi-threaded runtime, as the app does.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_response_does_not_wait_for_the_registry() {
        let listener = Harness::start(None).await;
        let busy = Arc::clone(&listener.registry);
        let (locked, is_locked) = oneshot::channel();
        let (release, released) = blocking::channel();
        let holder = std::thread::spawn(move || {
            let _guard = lock_recovering(&busy);
            locked.send(()).expect("the test to be waiting");
            let _ = released.recv();
        });
        is_locked.await.expect("the registry to be held");

        let response = tokio::time::timeout(
            Duration::from_secs(5),
            post_to(listener.port, &event("s1", "Stop")),
        )
        .await
        .expect("the endpoint to answer while the registry is busy");

        assert_accepted(&response);
        release.send(()).expect("the holding thread to be waiting");
        holder.join().expect("the holding thread to finish");
    }

    #[tokio::test]
    async fn a_burst_of_events_is_answered_without_approaching_a_hook_timeout() {
        let listener = Harness::start(None).await;
        let mut slowest = Duration::ZERO;

        for index in 0..50 {
            let started = Instant::now();
            let response = listener.post(&event(&format!("s{index}"), "Stop")).await;
            slowest = slowest.max(started.elapsed());

            assert_accepted(&response);
        }

        assert!(
            slowest < Duration::from_millis(100),
            "the slowest response in a burst took {slowest:?}"
        );
    }

    #[tokio::test]
    async fn a_poisoned_registry_lock_does_not_stop_later_reports_from_being_applied() {
        let listener = Harness::start(None).await;
        let poisoned = Arc::clone(&listener.registry);
        std::thread::spawn(move || {
            let _guard = lock_recovering(&poisoned);
            panic!("poisoning the registry lock");
        })
        .join()
        .expect_err("the poisoning thread to have panicked");
        let window = FakeWindow::open(65_921).await;
        listener.register(&window).await;
        listener.place(65_921, &[("s1", "Fix the auth flow")]);

        listener
            .post_extension(
                EXTENSION_FOCUS_PATH,
                r#"{"pid":65921,"focused":true,"activeTabLabel":"Fix the auth flow"}"#,
            )
            .await;

        listener
            .eventually(|listener| listener.current() == Some("s1".to_string()))
            .await;
    }

    #[tokio::test]
    async fn the_listener_stops_accepting_connections_on_shutdown() {
        let listener = Harness::start(None).await;

        listener.shutdown.trigger();
        tokio::time::timeout(Duration::from_secs(5), async {
            while listener.ports.port().is_some() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the listener to stop serving");

        assert!(TcpStream::connect(("127.0.0.1", listener.port))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_window_that_registers_is_stored_under_its_process_id() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;

        let response = listener.register(&window).await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":true,"heartbeatSeconds":10}"#
        );
        let stored = listener
            .windows
            .registration(65_921)
            .expect("a registration");
        assert_eq!(stored.callback_port, window.port);
        assert_eq!(
            stored.workspace_folders,
            vec![PathBuf::from("/Users/eric/Code/aydy")]
        );
        assert_eq!(stored.claude_code_version, Some("2.1.238".to_string()));
    }

    /// The round trip, over the real listener and a real callback port: the
    /// registration is only accepted once the window has answered a command,
    /// so `registered` means the channel Tasks 5.3 and 5.4 need is open.
    #[tokio::test]
    async fn registering_is_confirmed_by_a_command_the_window_answers() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;

        listener.register(&window).await;

        assert_eq!(
            window.received(),
            vec![r#"{"pid":65921,"command":"ping"}"#.to_string()]
        );
    }

    /// A window whose callback port cannot be reached is not a window AYDY can
    /// jump to, and saying otherwise would leave that failure to be discovered
    /// by a click that silently does nothing.
    #[tokio::test]
    async fn a_window_aydy_cannot_reach_is_refused_rather_than_stored() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::closed(65_921).await;

        let response = listener.register(&window).await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
        assert_eq!(listener.windows.registration(65_921), None);
    }

    /// Something else took the port the window used to hold. Trusting it would
    /// have AYDY send this window's reveals to a stranger.
    #[tokio::test]
    async fn a_callback_port_answering_for_a_different_window_is_refused() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::answering_as(65_921, 17_804).await;

        let response = listener.register(&window).await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
        assert_eq!(listener.windows.registration(65_921), None);
    }

    /// Two windows, two registrations, and the command that confirms each one
    /// goes only to the window it belongs to.
    #[tokio::test]
    async fn each_window_is_registered_and_confirmed_on_its_own_port() {
        let listener = Harness::start(None).await;
        let first = FakeWindow::open(17_804).await;
        let second = FakeWindow::open(65_921).await;

        listener.register(&first).await;
        listener.register(&second).await;

        assert_eq!(
            first.received(),
            vec![r#"{"pid":17804,"command":"ping"}"#.to_string()]
        );
        assert_eq!(
            second.received(),
            vec![r#"{"pid":65921,"command":"ping"}"#.to_string()]
        );
        assert_eq!(listener.windows.window_count(), 2);
    }

    #[tokio::test]
    async fn a_heartbeat_from_a_registered_window_is_acknowledged() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;
        listener.register(&window).await;

        let response = listener
            .post_extension(EXTENSION_HEARTBEAT_PATH, r#"{"pid":65921}"#)
            .await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":true,"heartbeatSeconds":10}"#
        );
    }

    /// AYDY was restarted while the window stayed open. The refusal is the only
    /// thing that tells the extension to register again.
    #[tokio::test]
    async fn a_heartbeat_from_a_window_aydy_never_registered_is_refused() {
        let listener = Harness::start(None).await;

        let response = listener
            .post_extension(EXTENSION_HEARTBEAT_PATH, r#"{"pid":65921}"#)
            .await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
    }

    #[tokio::test]
    async fn a_window_that_says_it_is_closing_is_dropped_without_waiting_for_the_timeout() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;
        listener.register(&window).await;

        let response = listener
            .post_extension(EXTENSION_UNREGISTER_PATH, r#"{"pid":65921}"#)
            .await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
        assert_eq!(listener.windows.registration(65_921), None);
    }

    /// One window closing must leave the other registered, or two windows would
    /// be one window with a shared fate.
    #[tokio::test]
    async fn one_window_closing_leaves_the_other_registered() {
        let listener = Harness::start(None).await;
        let first = FakeWindow::open(65_921).await;
        let second = FakeWindow::open(17_804).await;
        listener.register(&first).await;
        listener.register(&second).await;

        listener
            .post_extension(EXTENSION_UNREGISTER_PATH, r#"{"pid":65921}"#)
            .await;

        assert_eq!(listener.windows.registration(65_921), None);
        assert!(listener.windows.registration(17_804).is_some());
    }

    /// The whole path over the real listener: a label the window can see, the
    /// titles only AYDY holds, and the session the two identify between them.
    #[tokio::test]
    async fn a_focus_report_from_a_registered_window_names_where_the_user_is() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;
        listener.register(&window).await;
        listener.place(
            65_921,
            &[("s1", "Fix the auth flow"), ("s2", "Rewrite the parser")],
        );

        let response = listener
            .post_extension(
                EXTENSION_FOCUS_PATH,
                r#"{"pid":65921,"focused":true,"activeTabLabel":"Fix the auth flow"}"#,
            )
            .await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":true,"heartbeatSeconds":10}"#
        );
        listener
            .eventually(|listener| listener.current() == Some("s1".to_string()))
            .await;
    }

    /// The two facts stay two all the way to the registry: a tab in front of a
    /// window the user is not looking at is nobody's current session.
    #[tokio::test]
    async fn a_focus_report_from_an_unfocused_window_names_nobody_as_current() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;
        listener.register(&window).await;
        listener.place(65_921, &[("s1", "Fix the auth flow")]);

        listener
            .post_extension(
                EXTENSION_FOCUS_PATH,
                r#"{"pid":65921,"focused":false,"activeTabLabel":"Fix the auth flow"}"#,
            )
            .await;

        listener.settle().await;
        assert_eq!(listener.current(), None);
    }

    /// Focus is a fact about every window at once, so it is only taken from a
    /// window AYDY confirmed it could reach. The refusal is also what tells the
    /// extension to register again after AYDY restarted.
    #[tokio::test]
    async fn a_focus_report_from_a_window_aydy_never_registered_is_refused() {
        let listener = Harness::start(None).await;
        listener.place(65_921, &[("s1", "Fix the auth flow")]);

        let response = listener
            .post_extension(
                EXTENSION_FOCUS_PATH,
                r#"{"pid":65921,"focused":true,"activeTabLabel":"Fix the auth flow"}"#,
            )
            .await;

        assert_eq!(
            body_of(&response),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
        listener.settle().await;
        assert_eq!(listener.current(), None);
    }

    /// A closed window's last report must stop being the truth about where the
    /// user is, or the session it left in front goes on silencing its own
    /// notifications with nothing left to correct it.
    #[tokio::test]
    async fn a_window_that_says_it_is_closing_stops_being_where_the_user_is() {
        let listener = Harness::start(None).await;
        let window = FakeWindow::open(65_921).await;
        listener.register(&window).await;
        listener.place(65_921, &[("s1", "Fix the auth flow")]);
        listener
            .post_extension(
                EXTENSION_FOCUS_PATH,
                r#"{"pid":65921,"focused":true,"activeTabLabel":"Fix the auth flow"}"#,
            )
            .await;
        listener
            .eventually(|listener| listener.current() == Some("s1".to_string()))
            .await;

        listener
            .post_extension(EXTENSION_UNREGISTER_PATH, r#"{"pid":65921}"#)
            .await;

        listener
            .eventually(|listener| listener.current().is_none())
            .await;
    }

    #[tokio::test]
    async fn a_focus_report_naming_no_window_is_rejected() {
        let listener = Harness::start(None).await;

        let response = listener
            .post_extension(EXTENSION_FOCUS_PATH, "not json at all")
            .await;

        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "expected a 400, got {response:?}"
        );
    }

    #[tokio::test]
    async fn a_registration_naming_no_window_is_rejected() {
        let listener = Harness::start(None).await;

        let response = listener
            .post_extension(EXTENSION_REGISTER_PATH, "not json at all")
            .await;

        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "expected a 400, got {response:?}"
        );
        assert_eq!(
            body_of(&response),
            r#"{"registered":false,"heartbeatSeconds":10}"#
        );
    }
}
