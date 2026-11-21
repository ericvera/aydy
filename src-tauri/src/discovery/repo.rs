//! Which repository a session's working directory belongs to.
//!
//! The obvious implementation — the last component of the working directory —
//! is wrong for every session in a linked worktree, and this user works in
//! worktrees for most of their sessions. A session in
//! `/Users/eric/Code/okven.worktrees/eric-attribution-2` belongs to **okven**,
//! and the group header the design calls for reads `okven · eric/attribution-2`.
//!
//! A linked worktree's `.git` is a **file**, not a directory:
//!
//! ```text
//! gitdir: /Users/eric/Code/okven/.git/worktrees/eric-attribution-2
//! ```
//!
//! The repository is that path with the trailing `.git/worktrees/<name>`
//! removed. Code that only handles the directory case finds nothing at all
//! here, because there is no `.git` directory anywhere above a linked worktree.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// The directory or file git leaves at the top of a working tree.
const GIT_ENTRY: &str = ".git";

/// The directory a git directory keeps its linked worktrees in.
const WORKTREES_DIR: &str = "worktrees";

/// The prefix of the line a linked worktree's `.git` file carries.
const GITDIR_PREFIX: &str = "gitdir:";

/// The repository a working directory belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    /// The repository's root — the main checkout, never a linked worktree.
    pub root: PathBuf,
    /// The repository's name, which is the root's last component.
    pub name: String,
    /// The linked worktree the working directory sits in, when it sits in one.
    ///
    /// The group header shows the repository and the branch, so this is not
    /// part of the grouping; it is the distinction worth showing in detail,
    /// where two sessions on the same repository sit in different directories.
    pub worktree: Option<String>,
}

/// The repository `cwd` belongs to.
///
/// The search walks up from `cwd`, because a session may be started in a
/// subdirectory rather than at the repository root. A working directory outside
/// any repository, or one whose `.git` cannot be read, has no repository —
/// which is a supported state, not a failure.
pub fn identify(cwd: &Path) -> Option<RepoIdentity> {
    for directory in cwd.ancestors() {
        let entry = directory.join(GIT_ENTRY);

        if entry.is_dir() {
            return identity(directory, None);
        }

        if entry.is_file() {
            return from_pointer(directory, &entry);
        }
    }

    None
}

/// Resolves the `gitdir:` pointer a linked worktree's `.git` file holds.
fn from_pointer(directory: &Path, pointer: &Path) -> Option<RepoIdentity> {
    let contents = fs::read_to_string(pointer).ok()?;
    let gitdir = contents
        .lines()
        .find_map(|line| line.trim().strip_prefix(GITDIR_PREFIX))?
        .trim();
    let gitdir = absolute(directory, Path::new(gitdir));

    match linked_worktree(&gitdir) {
        Some((root, worktree)) => identity(&root, Some(worktree)),
        // A pointer of some other kind — a submodule's, say. The directory
        // holding it is still the top of a working tree, so it names the
        // repository as well as anything available does.
        None => identity(directory, None),
    }
}

/// The repository root and worktree name a linked worktree's git directory
/// implies, or `None` when the path is not one.
fn linked_worktree(gitdir: &Path) -> Option<(PathBuf, String)> {
    let worktree = gitdir.file_name()?.to_str()?.to_string();
    let worktrees = gitdir.parent()?;

    if worktrees.file_name()? != WORKTREES_DIR {
        return None;
    }

    Some((working_tree_of(worktrees.parent()?)?, worktree))
}

/// The working tree a git directory belongs to: the parent of a `.git`
/// directory, or a bare repository itself.
fn working_tree_of(git_directory: &Path) -> Option<PathBuf> {
    if git_directory.file_name()? == GIT_ENTRY {
        return Some(git_directory.parent()?.to_path_buf());
    }

    Some(git_directory.to_path_buf())
}

fn identity(root: &Path, worktree: Option<String>) -> Option<RepoIdentity> {
    Some(RepoIdentity {
        root: root.to_path_buf(),
        name: name_of(root)?,
        worktree,
    })
}

/// A repository's name: its root's last component, without the `.git` suffix a
/// bare repository carries.
fn name_of(root: &Path) -> Option<String> {
    let name = root.file_name()?.to_str()?;
    let trimmed = name
        .strip_suffix(".git")
        .filter(|trimmed| !trimmed.is_empty())
        .unwrap_or(name);

    Some(trimmed.to_string())
}

/// Resolves `path` against `base` and flattens `.` and `..`, so a relative
/// pointer does not leave a repository named `..`.
fn absolute(base: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };

    let mut resolved = PathBuf::new();

    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other),
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::test_support::{checkout, scratch_dir, worktree};

    #[test]
    fn a_working_directory_in_a_linked_worktree_belongs_to_the_repository_it_points_at() {
        let dir = scratch_dir("worktree");
        let root = checkout(&dir, "okven");
        let linked = worktree(&dir.join("okven.worktrees"), &root, "eric-attribution-2");

        let identity = identify(&linked).expect("the worktree to resolve to a repository");

        assert_eq!(identity.name, "okven");
        assert_eq!(identity.root, root);
        assert_eq!(identity.worktree, Some("eric-attribution-2".to_string()));
    }

    #[test]
    fn a_subdirectory_of_a_linked_worktree_still_resolves_to_the_repository() {
        let dir = scratch_dir("worktree-subdirectory");
        let root = checkout(&dir, "okven");
        let linked = worktree(&dir.join("okven.worktrees"), &root, "eric-attribution-2");
        let inside = linked.join("functions/src");
        fs::create_dir_all(&inside).expect("the subdirectory to be creatable");

        let identity = identify(&inside).expect("the subdirectory to resolve to a repository");

        assert_eq!(identity.name, "okven");
        assert_eq!(identity.worktree, Some("eric-attribution-2".to_string()));
    }

    #[test]
    fn a_working_directory_in_a_normal_checkout_takes_the_checkouts_name() {
        let dir = scratch_dir("checkout");
        let root = checkout(&dir, "aydy");

        let identity = identify(&root).expect("the checkout to resolve to a repository");

        assert_eq!(identity.name, "aydy");
        assert_eq!(identity.root, root);
        assert_eq!(identity.worktree, None);
    }

    #[test]
    fn a_subdirectory_of_a_checkout_still_finds_the_root() {
        let dir = scratch_dir("subdirectory");
        let root = checkout(&dir, "aydy");
        let inside = root.join("src-tauri/src/discovery");
        fs::create_dir_all(&inside).expect("the subdirectory to be creatable");

        let identity = identify(&inside).expect("the subdirectory to resolve to a repository");

        assert_eq!(identity.name, "aydy");
        assert_eq!(identity.root, root);
    }

    #[test]
    fn a_working_directory_outside_any_repository_has_no_repository() {
        let dir = scratch_dir("no-repository");

        assert_eq!(identify(&dir), None);
    }

    #[test]
    fn a_working_directory_that_does_not_exist_has_no_repository() {
        let dir = scratch_dir("gone").join("was-here");

        assert_eq!(identify(&dir), None);
    }

    #[test]
    fn a_git_file_carrying_no_pointer_leaves_the_session_without_a_repository() {
        let dir = scratch_dir("no-pointer");
        let working_tree = dir.join("mystery");
        fs::create_dir_all(&working_tree).expect("the working tree to be creatable");
        fs::write(working_tree.join(GIT_ENTRY), "no pointer here\n")
            .expect("the git file to be writable");

        assert_eq!(identify(&working_tree), None);
    }

    #[test]
    fn a_git_file_that_cannot_be_read_leaves_the_session_without_a_repository() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch_dir("unreadable");
        let working_tree = dir.join("sealed");
        fs::create_dir_all(&working_tree).expect("the working tree to be creatable");
        let pointer = working_tree.join(GIT_ENTRY);
        fs::write(
            &pointer,
            "gitdir: /Users/eric/Code/okven/.git/worktrees/sealed\n",
        )
        .expect("the git file to be writable");
        fs::set_permissions(&pointer, fs::Permissions::from_mode(0o000))
            .expect("the git file to be sealable");

        let identity = identify(&working_tree);

        fs::set_permissions(&pointer, fs::Permissions::from_mode(0o600))
            .expect("the git file to be unsealable");

        assert_eq!(identity, None);
    }

    #[test]
    fn a_pointer_that_is_not_a_worktrees_pointer_names_the_working_tree() {
        let dir = scratch_dir("submodule");
        let working_tree = dir.join("shared-types");
        fs::create_dir_all(&working_tree).expect("the working tree to be creatable");
        fs::write(
            working_tree.join(GIT_ENTRY),
            "gitdir: /Users/eric/Code/okven/.git/modules/shared-types\n",
        )
        .expect("the git file to be writable");

        let identity = identify(&working_tree).expect("the working tree to name itself");

        assert_eq!(identity.name, "shared-types");
        assert_eq!(identity.worktree, None);
    }

    #[test]
    fn a_relative_pointer_resolves_against_the_worktree_rather_than_leaving_a_dotted_name() {
        let dir = scratch_dir("relative-pointer");
        let root = checkout(&dir, "okven");
        let linked = dir.join("okven.worktrees").join("eric-fab-snippet");
        fs::create_dir_all(&linked).expect("the worktree to be creatable");
        fs::write(
            linked.join(GIT_ENTRY),
            "gitdir: ../../okven/.git/worktrees/eric-fab-snippet\n",
        )
        .expect("the git file to be writable");

        let identity = identify(&linked).expect("the relative pointer to resolve");

        assert_eq!(identity.name, "okven");
        assert_eq!(identity.root, root);
        assert_eq!(identity.worktree, Some("eric-fab-snippet".to_string()));
    }

    #[test]
    fn a_worktree_of_a_bare_repository_takes_the_repositorys_name_without_the_git_suffix() {
        let dir = scratch_dir("bare");
        let linked = dir.join("checkouts").join("eric-fix-logo-size");
        fs::create_dir_all(&linked).expect("the worktree to be creatable");
        fs::write(
            linked.join(GIT_ENTRY),
            format!(
                "gitdir: {}/okven.git/worktrees/eric-fix-logo-size\n",
                dir.display()
            ),
        )
        .expect("the git file to be writable");

        let identity = identify(&linked).expect("the bare repository to resolve");

        assert_eq!(identity.name, "okven");
        assert_eq!(identity.root, dir.join("okven.git"));
    }
}
