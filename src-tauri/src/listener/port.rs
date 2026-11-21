//! Port selection, the remembered port, and the move signal.

use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use tokio::net::TcpListener;
use tokio::sync::watch;

/// A loopback listener, and the port it had to give up to get there.
#[derive(Debug)]
pub struct BoundListener {
    pub listener: TcpListener,
    pub port: u16,
    /// The preferred port, when it was unavailable and the OS assigned another
    /// one instead.
    pub moved_from: Option<u16>,
}

/// Binds a loopback listener, preferring `preferred` and falling back to an
/// OS-assigned port when it is taken.
///
/// Binding is loopback-only. Session details — working directories, repository
/// names, what a session is waiting for — would otherwise be readable by
/// anything on the network.
pub async fn bind_loopback(preferred: Option<u16>) -> io::Result<BoundListener> {
    let preferred = preferred.filter(|port| *port != 0);

    if let Some(port) = preferred {
        if let Ok(listener) = TcpListener::bind(loopback(port)).await {
            return Ok(BoundListener {
                listener,
                port,
                moved_from: None,
            });
        }
    }

    let listener = TcpListener::bind(loopback(0)).await?;
    let port = listener.local_addr()?.port();

    Ok(BoundListener {
        listener,
        port,
        moved_from: preferred,
    })
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// The port the app listened on last, remembered across launches so the
/// registered hook URLs usually stay valid.
#[derive(Debug, Clone)]
pub struct PortMemory {
    path: PathBuf,
}

impl PortMemory {
    /// Remembers the port in the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file the port is remembered in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The remembered port, or `None` when nothing is remembered yet or the
    /// file does not hold a port.
    pub fn remembered(&self) -> Option<u16> {
        fs::read_to_string(&self.path).ok()?.trim().parse().ok()
    }

    /// Remembers `port` for the next launch.
    pub fn remember(&self, port: u16) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.path, port.to_string())
    }
}

/// A port the app had to leave, and the one it took instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortMove {
    pub from: u16,
    pub to: u16,
}

/// What the rest of the app knows about the listener's port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PortStatus {
    /// The port currently bound and accepting, or `None` while the listener is
    /// not bound.
    pub port: Option<u16>,
    /// The move this launch made, when it could not take the remembered port.
    /// It outlives the binding, so the notice does not vanish when the listener
    /// stops.
    pub moved: Option<PortMove>,
}

/// The current port and the move signal, shared with the hook installer, which
/// writes the port into the hook URLs, and with the popover, which reports a
/// move.
///
/// The port is deliberately not a setting: the app picks it and tells everyone
/// who needs it.
#[derive(Debug)]
pub struct PortState {
    status: watch::Sender<PortStatus>,
}

impl PortState {
    /// A state with nothing bound and no move to report.
    pub fn new() -> Self {
        Self {
            status: watch::Sender::new(PortStatus::default()),
        }
    }

    /// Everything currently known about the port.
    pub fn status(&self) -> PortStatus {
        *self.status.borrow()
    }

    /// The port currently bound and accepting.
    pub fn port(&self) -> Option<u16> {
        self.status().port
    }

    /// The move this launch made, if any.
    pub fn moved(&self) -> Option<PortMove> {
        self.status().moved
    }

    /// Records that the listener is bound on `port`, having been unable to take
    /// `moved_from`.
    pub fn set_bound(&self, port: u16, moved_from: Option<u16>) {
        self.status.send_modify(|status| {
            status.port = Some(port);

            if let Some(from) = moved_from {
                status.moved = Some(PortMove { from, to: port });
            }
        });
    }

    /// Records that the listener is no longer accepting connections.
    pub fn set_unbound(&self) {
        self.status.send_modify(|status| status.port = None);
    }

    /// Watches the port, so a caller can act on a move.
    pub fn subscribe(&self) -> watch::Receiver<PortStatus> {
        self.status.subscribe()
    }

    /// The bound port, waiting for the listener to bind if it has not yet.
    ///
    /// This is how the hook installer gets a port to write into the hook URLs
    /// without racing the listener's startup. `None` only once this state has
    /// been dropped, which cannot happen while the app is running.
    pub async fn wait_for_port(&self) -> Option<u16> {
        let mut updates = self.subscribe();

        loop {
            let port = updates.borrow_and_update().port;

            if port.is_some() {
                return port;
            }

            updates.changed().await.ok()?;
        }
    }
}

impl Default for PortState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aydy-port-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).expect("the scratch directory to be creatable");

        dir
    }

    #[tokio::test]
    async fn the_listener_binds_loopback_only() {
        let bound = bind_loopback(None).await.expect("a loopback port");

        let address = bound.listener.local_addr().expect("a local address");

        assert!(
            address.ip().is_loopback(),
            "expected a loopback address, got {address}"
        );
    }

    #[tokio::test]
    async fn an_unremembered_port_is_assigned_by_the_operating_system() {
        let bound = bind_loopback(None).await.expect("a loopback port");

        assert_ne!(bound.port, 0);
        assert_eq!(bound.moved_from, None);
    }

    #[tokio::test]
    async fn the_preferred_port_is_reused_when_it_is_free() {
        let free = bind_loopback(None).await.expect("a loopback port").port;

        let bound = bind_loopback(Some(free)).await.expect("a loopback port");

        assert_eq!(bound.port, free);
        assert_eq!(bound.moved_from, None);
    }

    #[tokio::test]
    async fn another_port_is_taken_when_the_preferred_one_is_busy() {
        let occupied = bind_loopback(None).await.expect("a loopback port");

        let bound = bind_loopback(Some(occupied.port))
            .await
            .expect("a loopback port");

        assert_ne!(bound.port, occupied.port);
        assert_eq!(bound.moved_from, Some(occupied.port));
        assert!(bound
            .listener
            .local_addr()
            .expect("a local address")
            .ip()
            .is_loopback());
    }

    #[test]
    fn nothing_is_remembered_before_the_first_launch() {
        let memory = PortMemory::new(scratch_dir("unremembered").join("listener-port"));

        assert_eq!(memory.remembered(), None);
    }

    #[test]
    fn the_remembered_port_survives_being_written_and_read_back() {
        let memory = PortMemory::new(scratch_dir("roundtrip").join("nested/listener-port"));

        memory.remember(51_234).expect("the port to be written");

        assert_eq!(memory.remembered(), Some(51_234));
    }

    #[test]
    fn a_file_that_does_not_hold_a_port_remembers_nothing() {
        let memory = PortMemory::new(scratch_dir("garbage").join("listener-port"));
        fs::write(memory.path(), "not a port").expect("the file to be written");

        assert_eq!(memory.remembered(), None);
    }

    #[test]
    fn the_port_is_unbound_and_unmoved_before_the_listener_starts() {
        let state = PortState::new();

        assert_eq!(state.port(), None);
        assert_eq!(state.moved(), None);
    }

    #[test]
    fn binding_without_a_move_reports_a_port_and_no_move() {
        let state = PortState::new();

        state.set_bound(51_234, None);

        assert_eq!(state.port(), Some(51_234));
        assert_eq!(state.moved(), None);
    }

    #[test]
    fn binding_after_a_move_names_both_ports() {
        let state = PortState::new();

        state.set_bound(51_235, Some(51_234));

        assert_eq!(
            state.moved(),
            Some(PortMove {
                from: 51_234,
                to: 51_235
            })
        );
    }

    #[test]
    fn unbinding_clears_the_port_but_keeps_the_move_worth_reporting() {
        let state = PortState::new();
        state.set_bound(51_235, Some(51_234));

        state.set_unbound();

        assert_eq!(state.port(), None);
        assert_eq!(
            state.moved(),
            Some(PortMove {
                from: 51_234,
                to: 51_235
            })
        );
    }

    #[tokio::test]
    async fn waiting_for_the_port_resolves_once_the_listener_binds() {
        let state = std::sync::Arc::new(PortState::new());
        let binding = std::sync::Arc::clone(&state);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            binding.set_bound(51_234, None);
        });

        assert_eq!(state.wait_for_port().await, Some(51_234));
    }

    #[tokio::test]
    async fn waiting_for_the_port_returns_immediately_when_it_is_already_bound() {
        let state = PortState::new();
        state.set_bound(51_234, None);

        assert_eq!(state.wait_for_port().await, Some(51_234));
    }
}
