//! Publishing the listener's port where the companion extension can find it.
//!
//! The port is chosen by the app and moves when it has to, so the extension
//! cannot be told it once and cannot be asked to hold it as a setting — that
//! would be configuration, which the extension is required not to have. It
//! reads this file instead, and reads it again whenever it cannot reach what
//! the file last said.
//!
//! This is a different fact from the remembered port in
//! [`crate::listener::port::PortMemory`]: that one survives the app so the next
//! launch can prefer it, whereas this one exists only while the listener is
//! accepting. Its absence is how a reader learns AYDY is not running.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::atomic_file::write_atomically;
use crate::listener::port::PortState;

/// The published file's name, inside the app config directory.
pub const PORT_FILE: &str = "listener.json";

/// What the published file says.
///
/// JSON rather than the bare number the port memory holds, so that a later
/// field can be added without every reader having to be replaced at the same
/// moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedPort {
    pub port: u16,
}

/// The file naming the port the listener is accepting on right now.
#[derive(Debug, Clone)]
pub struct PortFile {
    path: PathBuf,
}

impl PortFile {
    /// A file published at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where the port is published.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Publishes `port` as the one the listener is accepting on.
    ///
    /// Written atomically: a reader that catches a rewrite mid-flight would
    /// otherwise read a truncated file, decide AYDY is not running, and wait
    /// out a whole retry interval for no reason.
    pub fn publish(&self, port: u16) -> io::Result<()> {
        let contents = serde_json::to_string(&PublishedPort { port })
            .map_err(|error| io::Error::other(error.to_string()))?;

        write_atomically(&self.path, &contents)
    }

    /// Withdraws the published port, because nothing is accepting any more.
    ///
    /// A file left behind names a port AYDY has let go of, and something else
    /// may take it; removing it is what makes absence mean what it says.
    pub fn withdraw(&self) {
        let _ = fs::remove_file(&self.path);
    }

    /// The published port, or `None` when nothing is published or the file does
    /// not name one.
    pub fn published(&self) -> Option<u16> {
        let contents = fs::read_to_string(&self.path).ok()?;

        serde_json::from_str::<PublishedPort>(&contents)
            .ok()
            .map(|published| published.port)
    }
}

/// Keeps `file` saying exactly what `ports` says, for as long as the app runs.
///
/// Publishing from the port watch rather than from the bind means a move is
/// followed here the same way the hook URLs follow it, so the extension and the
/// hooks are never pointed at two different ports.
pub async fn publish_port(file: PortFile, ports: Arc<PortState>) {
    let mut updates = ports.subscribe();

    loop {
        match updates.borrow_and_update().port {
            Some(port) => {
                if let Err(error) = file.publish(port) {
                    eprintln!(
                        "aydy: could not publish port {port} for the companion extension: {error}"
                    );
                }
            }
            None => file.withdraw(),
        }

        if updates.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::*;

    static SCRATCH: AtomicU32 = AtomicU32::new(0);

    fn scratch_file() -> PortFile {
        let dir = std::env::temp_dir().join(format!(
            "aydy-port-file-{}-{}",
            std::process::id(),
            SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));

        PortFile::new(dir.join(PORT_FILE))
    }

    /// Waits for `expected`, so the assertion is about what the publisher
    /// settles on rather than about how quickly its task was scheduled.
    async fn settles_on(file: &PortFile, expected: Option<u16>) {
        for _ in 0..600 {
            if file.published() == expected {
                return;
            }

            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        panic!(
            "expected the published port to settle on {expected:?}, it is {:?}",
            file.published()
        );
    }

    #[test]
    fn nothing_is_published_before_the_listener_binds() {
        assert_eq!(scratch_file().published(), None);
    }

    #[test]
    fn the_published_port_survives_being_written_and_read_back() {
        let file = scratch_file();

        file.publish(51_234).expect("the port to be published");

        assert_eq!(file.published(), Some(51_234));
    }

    /// The reader is a separate program on its own release cadence, so the
    /// shape it parses is pinned here rather than left to whatever `serde`
    /// happens to emit.
    #[test]
    fn the_published_file_is_a_json_object_naming_the_port() {
        let file = scratch_file();

        file.publish(51_234).expect("the port to be published");

        assert_eq!(
            fs::read_to_string(file.path()).expect("the file to be readable"),
            r#"{"port":51234}"#
        );
    }

    #[test]
    fn withdrawing_leaves_nothing_published() {
        let file = scratch_file();
        file.publish(51_234).expect("the port to be published");

        file.withdraw();

        assert_eq!(file.published(), None);
        assert!(!file.path().exists());
    }

    #[test]
    fn withdrawing_what_was_never_published_is_not_an_error() {
        let file = scratch_file();

        file.withdraw();

        assert_eq!(file.published(), None);
    }

    #[test]
    fn a_file_that_does_not_name_a_port_publishes_nothing() {
        let file = scratch_file();
        fs::create_dir_all(file.path().parent().expect("a parent directory"))
            .expect("the scratch directory to be creatable");
        fs::write(file.path(), "not json at all").expect("the file to be written");

        assert_eq!(file.published(), None);
    }

    #[tokio::test]
    async fn the_bound_port_is_published_as_soon_as_the_listener_takes_it() {
        let file = scratch_file();
        let ports = Arc::new(PortState::new());
        tokio::spawn(publish_port(file.clone(), Arc::clone(&ports)));

        ports.set_bound(51_234, None);

        settles_on(&file, Some(51_234)).await;
    }

    #[tokio::test]
    async fn a_port_move_is_republished_so_the_extension_follows_it() {
        let file = scratch_file();
        let ports = Arc::new(PortState::new());
        tokio::spawn(publish_port(file.clone(), Arc::clone(&ports)));
        ports.set_bound(51_234, None);
        settles_on(&file, Some(51_234)).await;

        ports.set_bound(51_235, Some(51_234));

        settles_on(&file, Some(51_235)).await;
    }

    #[tokio::test]
    async fn a_listener_that_stops_accepting_withdraws_the_port() {
        let file = scratch_file();
        let ports = Arc::new(PortState::new());
        tokio::spawn(publish_port(file.clone(), Arc::clone(&ports)));
        ports.set_bound(51_234, None);
        settles_on(&file, Some(51_234)).await;

        ports.set_unbound();

        settles_on(&file, None).await;
    }
}
