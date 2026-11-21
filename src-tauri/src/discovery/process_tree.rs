//! Walking a process's ancestors, which is how a Claude session is matched to
//! the VS Code window running it.
//!
//! Every Claude session process observed on this machine is a **direct child**
//! of a VS Code extension-host process, so reading one parent answers every
//! case there is today. The chain is walked anyway. If Claude Code ever spawns
//! its sessions through an intermediate helper, a parent check stops matching
//! without raising anything and without logging anything, and every session
//! quietly becomes unjumpable. Matching at any depth costs one lookup per level
//! and cannot fail that way.

use std::collections::HashMap;

/// How far above a session's own process the walk goes.
///
/// A session sits one level below its extension host today, so this is headroom
/// rather than a measurement: enough room for a helper process or two, and
/// short enough that a tree which somehow loops back on itself ends the walk
/// instead of hanging the scan.
pub const MAX_ANCESTOR_DEPTH: usize = 8;

/// Reads one process's parent.
///
/// Injected rather than called directly so the join can be tested against a
/// process tree the test lays out, rather than against whatever this machine
/// happens to be running.
pub trait ProcessParents: Send + Sync {
    /// The parent of `pid`, or `None` when there is none to read: the process
    /// has gone, the table cannot be read, or the walk has reached the top of
    /// the tree.
    fn parent(&self, pid: u32) -> Option<u32>;
}

/// The process tree of this machine.
pub struct SystemParents;

impl ProcessParents for SystemParents {
    /// `proc_pidinfo` with the short BSD flavour is the unprivileged way to ask
    /// for one process's parent: it needs no entitlement, allocates nothing,
    /// and does not list every process on the machine to find one of them.
    ///
    /// Process id 0 is never asked about — it is the kernel, and nothing
    /// descends from it that this join cares about — and a call that fills
    /// anything other than the whole record is treated as no answer rather than
    /// as a parent read out of a half-written buffer.
    fn parent(&self, pid: u32) -> Option<u32> {
        let pid = libc::c_int::try_from(pid).ok()?;

        if pid <= 0 {
            return None;
        }

        let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
        let size = libc::c_int::try_from(size_of::<libc::proc_bsdshortinfo>()).ok()?;

        // SAFETY: the buffer is a `proc_bsdshortinfo` and `size` is its size,
        // which is the record the short BSD flavour fills.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDT_SHORTBSDINFO,
                0,
                std::ptr::from_mut(&mut info).cast::<libc::c_void>(),
                size,
            )
        };

        if written != size {
            return None;
        }

        Some(info.pbsi_ppid)
    }
}

/// A process tree of fixed child-to-parent links.
///
/// Public rather than test-only, the way [`FixedProcesses`] is, so the tests of
/// the join and of the registry that consumes it share one fixture tree.
///
/// [`FixedProcesses`]: crate::discovery::session_files::FixedProcesses
pub struct FixedParents {
    parents: HashMap<u32, u32>,
}

impl FixedParents {
    /// A tree in which each `(child, parent)` pair is a link and every other
    /// process has no parent at all.
    pub fn new(links: impl IntoIterator<Item = (u32, u32)>) -> Self {
        Self {
            parents: links.into_iter().collect(),
        }
    }
}

impl ProcessParents for FixedParents {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.parents.get(&pid).copied()
    }
}

/// The ancestors of `pid`, nearest first.
///
/// The process itself is never in the list. A session's own process is not the
/// window hosting it, and leaving it out is what stops a session being matched
/// to a registration that happens to carry its process id — which would aim a
/// reveal at a window chosen by a coincidence.
pub fn ancestors(parents: &dyn ProcessParents, pid: u32) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut current = pid;

    for _ in 0..MAX_ANCESTOR_DEPTH {
        let Some(parent) = parents.parent(current) else {
            break;
        };

        // A parent of 0 is the top of the tree: launchd reports it, and so does
        // a record that was never filled in.
        if parent == 0 {
            break;
        }

        chain.push(parent);
        current = parent;
    }

    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The extension host, the Claude session under it, and the helper the
    /// walk exists to survive.
    const EXTENSION_HOST: u32 = 65_921;
    const SESSION_PROCESS: u32 = 95_646;
    const HELPER: u32 = 95_600;

    #[test]
    fn a_process_with_no_parent_on_record_has_no_ancestors() {
        let parents = FixedParents::new([]);

        assert_eq!(ancestors(&parents, SESSION_PROCESS), Vec::<u32>::new());
    }

    #[test]
    fn the_ancestors_of_a_process_are_listed_nearest_first() {
        let parents = FixedParents::new([(SESSION_PROCESS, HELPER), (HELPER, EXTENSION_HOST)]);

        assert_eq!(
            ancestors(&parents, SESSION_PROCESS),
            vec![HELPER, EXTENSION_HOST]
        );
    }

    #[test]
    fn the_walk_stops_at_the_top_of_the_tree() {
        let parents = FixedParents::new([(SESSION_PROCESS, EXTENSION_HOST), (EXTENSION_HOST, 0)]);

        assert_eq!(ancestors(&parents, SESSION_PROCESS), vec![EXTENSION_HOST]);
    }

    #[test]
    fn the_walk_stops_at_a_bounded_depth() {
        let deep = MAX_ANCESTOR_DEPTH + 5;
        let parents = FixedParents::new((1..=deep).map(|step| (step as u32, step as u32 + 1)));

        assert_eq!(ancestors(&parents, 1).len(), MAX_ANCESTOR_DEPTH);
    }

    #[test]
    fn a_process_tree_that_loops_back_on_itself_still_ends_the_walk() {
        let parents = FixedParents::new([
            (SESSION_PROCESS, EXTENSION_HOST),
            (EXTENSION_HOST, SESSION_PROCESS),
        ]);

        assert_eq!(
            ancestors(&parents, SESSION_PROCESS).len(),
            MAX_ANCESTOR_DEPTH
        );
    }

    /// The one test of the real reader: everything above runs against a
    /// fixture, so without this nothing would notice `proc_pidinfo` being
    /// called wrongly.
    #[test]
    fn the_nearest_ancestor_of_this_process_is_the_process_that_started_it() {
        // SAFETY: `getppid` takes nothing and only reads this process's record.
        let expected = u32::try_from(unsafe { libc::getppid() }).expect("a parent process id");

        assert_eq!(
            ancestors(&SystemParents, std::process::id())
                .first()
                .copied(),
            Some(expected)
        );
    }

    /// The three ways there is no answer: the kernel, a number too large to be
    /// a process id at all, and one that reaches the kernel and comes back
    /// empty. The last is the one that matters — a record left unfilled reads
    /// as a parent of 0, which is a plausible-looking answer rather than an
    /// obvious failure.
    #[test]
    fn a_process_id_that_addresses_no_process_has_no_parent() {
        let above_every_process_id = u32::try_from(libc::c_int::MAX).expect("a positive maximum");

        assert_eq!(SystemParents.parent(0), None);
        assert_eq!(SystemParents.parent(u32::MAX), None);
        assert_eq!(SystemParents.parent(above_every_process_id), None);
    }
}
