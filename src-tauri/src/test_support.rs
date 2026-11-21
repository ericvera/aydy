//! Helpers shared by the crate's test modules.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static SCRATCH: AtomicU32 = AtomicU32::new(0);

/// A directory of this test's own, deleted by nothing and shared with no other
/// test, so no test ever reads or writes a file the developer cares about.
pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aydy-test-{}-{name}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the scratch directory to be creatable");

    dir
}
