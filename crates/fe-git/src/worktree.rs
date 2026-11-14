//! Linked worktrees and the revisions frozen from them, through the `git`
//! executable (see [`crate::cli`]): an agent lane's worktree is added
//! detached at a commit, and its whole state, untracked files included, is
//! frozen as a commit on a private ref under the lane's [`RefNamespace`]
//! (`refs/tenmoku/lanes/<layout key>/<n>`, its revisions `…/r<k>`).
//!
//! Freezing writes a temporary index (`GIT_INDEX_FILE` in a temp
//! directory), never the worktree's own, and moves no branch and no HEAD.
//! Refs are shared by every linked worktree of a repository, so each lane's
//! revisions live under its own namespace, and clearing one deletes nothing
//! outside it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cli::{Access, run, run_raw};
use crate::{ChangeKind, FileStatus, GitError, Repo};

/// The author and committer of a frozen revision: it is the worktree's
/// state, not anyone's commit, and freezing must not fail where no identity
/// is configured.
const FREEZE_NAME: &str = "tenmoku";
const FREEZE_EMAIL: &str = "tenmoku@localhost";

/// Makes each temporary index directory name unique within the process.
static INDEX_SEQ: AtomicU32 = AtomicU32::new(0);

/// One of the repository's worktrees, as `git worktree list` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Its root, absolute, as git recorded it.
    pub path: PathBuf,
    /// Its HEAD's commit id, `None` for a bare repository or an unborn HEAD.
    pub head: Option<String>,
    /// The branch it has checked out, a full ref name (`refs/heads/…`);
    /// `None` when detached or bare.
    pub branch: Option<String>,
    pub detached: bool,
    /// The main repository entry, when it is bare (it has no working tree).
    pub bare: bool,
    pub locked: bool,
    /// Its directory is gone, so `git worktree prune` would drop it.
    pub prunable: bool,
}

/// The refs one lane's revisions live under, such as
/// `refs/tenmoku/lanes/<layout key>/2`: revision `k` is `<namespace>/r<k>`.
/// Matching is by whole components, so lane 2's namespace holds nothing of
/// lane 20's.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefNamespace(String);

impl RefNamespace {
    /// `prefix` (a trailing `/` dropped) as a namespace, or `None` unless it
    /// is under `refs/` and every component is one git allows in a ref name.
    pub fn new(prefix: &str) -> Option<RefNamespace> {
        let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
        let rest = prefix.strip_prefix("refs/")?;
        rest.split('/')
            .all(valid_component)
            .then(|| RefNamespace(prefix.to_owned()))
    }

    /// The namespace, `refs/…`, without a trailing `/`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Revision `k`'s full ref name.
    pub fn revision_ref(&self, k: u32) -> String {
        format!("{}/r{k}", self.0)
    }
}

/// Whether git takes `component` as one component of a ref name (see
/// `git check-ref-format`).
fn valid_component(component: &str) -> bool {
    !component.is_empty()
        && !component.starts_with('.')
        && !component.ends_with(".lock")
        && !component.contains("..")
        && !component.contains("@{")
        && component != "@"
        && !component
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f || b" ~^:?*[\\".contains(&byte))
}

/// A frozen revision: the commit on its ref, and that commit's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub commit: String,
    pub tree: String,
}

impl Repo {
    /// Adds a linked worktree at `path` (absolute; it must not exist or must
    /// be empty) with HEAD detached at `commit` (a commit id or anything
    /// else that names one), checked out. Git runs its `post-checkout` hook
    /// as in a terminal. Blocks until git exits.
    pub fn add_worktree_detached(&self, path: &Path, commit: &str) -> Result<(), GitError> {
        let id = self.resolve_commit(commit)?;
        let args: [OsString; 5] = [
            "worktree".into(),
            "add".into(),
            "--detach".into(),
            path.into(),
            id.into(),
        ];
        run(&self.workdir, Access::Write, &args, None)?;
        Ok(())
    }

    /// Removes the linked worktree at `path`, its directory and its admin
    /// files; with `force`, even when it has changes or untracked files
    /// (not when it is locked). One whose directory is already gone loses
    /// just its admin files; no other worktree's are pruned. A path that
    /// isn't a linked worktree is an error. This repository must not be
    /// that worktree. Blocks until git exits.
    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), GitError> {
        let mut args: Vec<OsString> = vec!["worktree".into(), "remove".into()];
        if force {
            args.push("--force".into());
        }
        args.push(path.into());
        run(&self.workdir, Access::Write, &args, None)?;
        Ok(())
    }

    /// The repository's worktrees, the main one first.
    pub fn worktrees(&self) -> Result<Vec<Worktree>, GitError> {
        let args = ["worktree", "list", "--porcelain", "-z"].map(OsString::from);
        let stdout = run(&self.workdir, Access::Read, &args, None)?;
        Ok(parse_worktrees(&stdout))
    }

    /// The tree id of this working tree's whole state: tracked files as
    /// they are on disk, and untracked ones that aren't ignored, written
    /// through a temporary index. The worktree's own index isn't touched.
    /// Blocks until git exits.
    pub fn worktree_tree(&self) -> Result<String, GitError> {
        let temp = TempDir::create()?;
        let index = temp.0.join("index");
        let own = self.repo.to_thread_local().index_path();
        // Its stat cache spares hashing every unchanged file again.
        copy_index(&own, &index).map_err(|err| GitError::Io(err.to_string()))?;
        self.unhide_entries(&index)?;
        // `--sparse` stages materialized files outside a sparse checkout's
        // cone, which `add` otherwise skips; git before 2.34 lacks it, and
        // there is no cone to skip them with.
        if self
            .temp_index_git(&index, &["add", "--all", "--sparse"], None)
            .is_err()
        {
            self.temp_index_git(&index, &["add", "--all"], None)?;
        }
        let stdout = self.temp_index_git(&index, &["write-tree"], None)?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    /// Runs `git <args>` on the temporary index at `index`, returning its
    /// stdout. Every command that reads or writes it goes through here:
    /// with `core.splitIndex` on, a write would add a shared index to the
    /// real git directory and expire ones the real index still names, and
    /// `core.ignoreStat` would mark what `add` stages assume-unchanged.
    fn temp_index_git(
        &self,
        index: &Path,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<Vec<u8>, GitError> {
        let args: Vec<OsString> = ["-c", "core.splitIndex=false", "-c", "core.ignoreStat=false"]
            .iter()
            .chain(args)
            .map(OsString::from)
            .collect();
        let envs = [("GIT_INDEX_FILE", index)];
        checked(
            &args,
            run_raw(&self.workdir, Access::Write, &args, stdin, &envs)?,
        )
    }

    /// Clears, in the temporary index at `index`, the flags that make git
    /// skip an entry's file on disk: assume-unchanged on every entry, and
    /// skip-worktree on each whose file is there. A skip-worktree entry
    /// with no file (outside a sparse checkout) keeps its index content.
    fn unhide_entries(&self, index: &Path) -> Result<(), GitError> {
        let stdout = self.temp_index_git(index, &["ls-files", "-v", "-z"], None)?;
        let mut assumed = Vec::new();
        let mut skipped = Vec::new();
        for record in stdout.split(|&byte| byte == 0) {
            // `<tag> <path>`: lowercase for assume-unchanged, `S` for
            // skip-worktree.
            let (Some(&tag), Some(path)) = (record.first(), record.get(2..)) else {
                continue;
            };
            if tag.is_ascii_lowercase() {
                assumed.extend_from_slice(path);
                assumed.push(0);
            }
            if tag.eq_ignore_ascii_case(&b's') {
                let Ok(rela) = gix::path::from_byte_slice(path) else {
                    continue;
                };
                if std::fs::symlink_metadata(self.workdir.join(rela)).is_ok() {
                    skipped.extend_from_slice(path);
                    skipped.push(0);
                }
            }
        }
        for (flag, paths) in [
            ("--no-assume-unchanged", assumed),
            ("--no-skip-worktree", skipped),
        ] {
            if !paths.is_empty() {
                let args = ["update-index", flag, "-z", "--stdin"];
                self.temp_index_git(index, &args, Some(&paths))?;
            }
        }
        Ok(())
    }

    /// Freezes this working tree's whole state (see [`Repo::worktree_tree`])
    /// as a commit with `message`, on `parent` when given, and points
    /// revision `k`'s ref in `namespace` at it, replacing any commit there.
    /// No branch, HEAD or index moves. Signing is off: the commit is private
    /// to the editor. Blocks until git exits.
    pub fn freeze(
        &self,
        namespace: &RefNamespace,
        k: u32,
        parent: Option<&str>,
        message: &str,
    ) -> Result<Revision, GitError> {
        let tree = self.worktree_tree()?;
        let mut args: Vec<OsString> = vec!["commit-tree".into(), "--no-gpg-sign".into()];
        if let Some(parent) = parent {
            args.extend(["-p".into(), self.resolve_commit(parent)?.into()]);
        }
        args.extend(["-m".into(), message.into(), tree.clone().into()]);
        let name = Path::new(FREEZE_NAME);
        let email = Path::new(FREEZE_EMAIL);
        let envs = [
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
        ];
        let stdout = checked(
            &args,
            run_raw(&self.workdir, Access::Write, &args, None, &envs)?,
        )?;
        let commit = String::from_utf8_lossy(&stdout).trim().to_owned();
        // `--no-deref`: a symbolic ref there is replaced, its target untouched.
        let args = [
            "update-ref".into(),
            "--no-deref".into(),
            namespace.revision_ref(k),
            commit.clone(),
        ]
        .map(OsString::from);
        run(&self.workdir, Access::Write, &args, None)?;
        Ok(Revision { commit, tree })
    }

    /// The revisions under `namespace`, as `(k, commit id)` sorted by `k`.
    /// Other refs in it, not named `r<k>`, are left out.
    pub fn revisions(&self, namespace: &RefNamespace) -> Result<Vec<(u32, String)>, GitError> {
        let mut revisions: Vec<(u32, String)> = (self.namespace_refs(namespace)?.into_iter())
            .filter_map(|(name, id)| {
                let rest = name.strip_prefix(namespace.as_str())?.strip_prefix("/r")?;
                // Digits only, as `revision_ref` writes them: `u32::from_str`
                // would take a `+`, and `r01` would read as `r1`.
                let canonical = rest == "0" || !rest.starts_with('0');
                if rest.is_empty() || !canonical || !rest.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return None;
                }
                Some((rest.parse().ok()?, id))
            })
            .collect();
        revisions.sort();
        Ok(revisions)
    }

    /// Deletes every ref under `namespace`, and nothing else, in one
    /// transaction. Returns how many there were. Blocks until git exits.
    pub fn delete_namespace(&self, namespace: &RefNamespace) -> Result<usize, GitError> {
        let refs = self.namespace_refs(namespace)?;
        if refs.is_empty() {
            return Ok(0);
        }
        let mut stdin = Vec::new();
        for (name, id) in &refs {
            stdin.extend_from_slice(b"delete ");
            stdin.extend_from_slice(name.as_bytes());
            stdin.push(0);
            stdin.extend_from_slice(id.as_bytes());
            stdin.push(0);
        }
        // `--no-deref`: a symbolic ref is deleted itself, never its target.
        let args = ["update-ref", "--no-deref", "--stdin", "-z"].map(OsString::from);
        run(&self.workdir, Access::Write, &args, Some(&stdin))?;
        Ok(refs.len())
    }

    /// The tree id of `commit` (a commit id, ref name or anything else that
    /// names one).
    pub fn tree_id(&self, commit: &str) -> Result<String, GitError> {
        let tree = format!("{}^{{tree}}", self.resolve_commit(commit)?);
        let args = ["rev-parse", "--verify", tree.as_str()].map(OsString::from);
        let stdout = run(&self.workdir, Access::Read, &args, None)?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }

    /// The files that differ between commits `from` and `to`, sorted by
    /// path: added, deleted, or modified (a type change included). Renames
    /// show as a deletion and an addition.
    pub fn changed_files(&self, from: &str, to: &str) -> Result<Vec<FileStatus>, GitError> {
        let from = self.resolve_commit(from)?;
        let to = self.resolve_commit(to)?;
        let args = [
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--name-status",
            "--no-relative",
            from.as_str(),
            to.as_str(),
        ]
        .map(OsString::from);
        let stdout = run(&self.workdir, Access::Read, &args, None)?;
        let mut files = Vec::new();
        let mut tokens = stdout.split(|&byte| byte == 0);
        while let (Some(status), Some(path)) = (tokens.next(), tokens.next()) {
            let kind = match status.first() {
                Some(b'A') => ChangeKind::Added,
                Some(b'D') => ChangeKind::Deleted,
                Some(b'M' | b'T') => ChangeKind::Modified,
                _ => continue,
            };
            let Ok(path) = gix::path::from_byte_slice(path) else {
                continue;
            };
            files.push(FileStatus {
                path: self.workdir.join(path),
                kind,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Every ref under `namespace`, as `(name, id)`.
    fn namespace_refs(&self, namespace: &RefNamespace) -> Result<Vec<(String, String)>, GitError> {
        // A pattern without wildcards matches whole leading components;
        // the namespace has none, being a valid ref prefix.
        let args = [
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            namespace.as_str(),
        ]
        .map(OsString::from);
        let stdout = run(&self.workdir, Access::Read, &args, None)?;
        let prefix = format!("{}/", namespace.as_str());
        Ok((String::from_utf8_lossy(&stdout).lines())
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, name)| name.starts_with(&prefix))
            .map(|(id, name)| (name.to_owned(), id.to_owned()))
            .collect())
    }

    /// `commit`'s full id, so no argument handed to git can read as an
    /// option.
    fn resolve_commit(&self, commit: &str) -> Result<String, GitError> {
        let missing = || {
            GitError::Gix(
                kiln_l10n::tr!("git-commit-missing", commit = commit.to_string()).to_string(),
            )
        };
        if commit.is_empty() || commit.starts_with('-') {
            return Err(missing());
        }
        let spec = format!("{commit}^{{commit}}");
        let args = ["rev-parse", "--verify", "--quiet", spec.as_str()].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        // `--quiet` exits 1, printing nothing, for a revision that names no commit.
        if output.status.code() == Some(1) && output.stderr.is_empty() {
            return Err(missing());
        }
        let stdout = checked(&args, output)?;
        Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
    }
}

/// Copies the index at `from` to `to`, if there is one, keeping its
/// modification time: git takes an entry whose file changed no earlier than
/// the index was written as racily clean and hashes it again, and a copy
/// stamped now would hide such a change.
fn copy_index(from: &Path, to: &Path) -> std::io::Result<()> {
    let modified = match std::fs::metadata(from) {
        Ok(metadata) => metadata.modified()?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    std::fs::copy(from, to)?;
    std::fs::File::options()
        .write(true)
        .open(to)?
        .set_modified(modified)
}

/// `stdout` when git exited 0, else [`GitError::Command`].
fn checked(args: &[OsString], output: std::process::Output) -> Result<Vec<u8>, GitError> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(GitError::Command {
        args: args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

/// `git worktree list --porcelain -z`: each attribute ends in a NUL, and
/// each worktree's attributes in one more.
fn parse_worktrees(stdout: &[u8]) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in stdout.split(|&byte| byte == 0) {
        if field.is_empty() {
            worktrees.extend(current.take());
            continue;
        }
        let (key, value) = match field.iter().position(|&byte| byte == b' ') {
            Some(space) => (&field[..space], Some(&field[space + 1..])),
            None => (field, None),
        };
        if key == b"worktree" {
            worktrees.extend(current.take());
            let Some(path) = value.and_then(|value| gix::path::from_byte_slice(value).ok()) else {
                continue;
            };
            current = Some(Worktree {
                path: path.to_path_buf(),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(worktree) = current.as_mut() else {
            continue;
        };
        let text = || value.map(|value| String::from_utf8_lossy(value).into_owned());
        match key {
            b"HEAD" => {
                worktree.head = text().filter(|id| id.bytes().any(|byte| byte != b'0'));
            }
            b"branch" => worktree.branch = text(),
            b"detached" => worktree.detached = true,
            b"bare" => worktree.bare = true,
            b"locked" => worktree.locked = true,
            b"prunable" => worktree.prunable = true,
            _ => {}
        }
    }
    worktrees.extend(current);
    worktrees
}

/// A private directory for the temporary index, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn create() -> Result<TempDir, GitError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let mut last = None;
        for _ in 0..16 {
            let dir = std::env::temp_dir().join(format!(
                "fe-git-index-{}-{nanos:x}-{}",
                std::process::id(),
                INDEX_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(&dir) {
                Ok(()) => return Ok(TempDir(dir)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => last = Some(err),
                Err(err) => return Err(GitError::Io(err.to_string())),
            }
        }
        Err(GitError::Io(
            last.map(|err| err.to_string()).unwrap_or_default(),
        ))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_needs_refs_and_valid_components() {
        let lane = RefNamespace::new("refs/tenmoku/lanes/k1/2/").unwrap();
        assert_eq!(lane.as_str(), "refs/tenmoku/lanes/k1/2");
        assert_eq!(lane.revision_ref(3), "refs/tenmoku/lanes/k1/2/r3");
        for bad in [
            "HEAD",
            "refs",
            "refs/",
            "heads/main",
            "refs//x",
            "refs/a/../b",
            "refs/a*",
            "refs/a b",
            "refs/.a",
            "refs/a.lock",
            "refs/a@{1}",
        ] {
            assert_eq!(RefNamespace::new(bad), None, "{bad}");
        }
    }

    #[test]
    fn porcelain_list_parses() {
        let stdout = b"worktree /r\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
worktree /r/lanes/2\0HEAD 2222222222222222222222222222222222222222\0detached\0locked why\0\0\
worktree /gone\0HEAD 3333333333333333333333333333333333333333\0detached\0prunable gitdir file points to non-existent location\0\0";
        let worktrees = parse_worktrees(stdout);
        assert_eq!(worktrees.len(), 3);
        assert_eq!(worktrees[0].path, Path::new("/r"));
        assert_eq!(worktrees[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(!worktrees[0].detached);
        assert!(worktrees[1].detached && worktrees[1].locked);
        assert_eq!(
            worktrees[1].head.as_deref(),
            Some("2222222222222222222222222222222222222222")
        );
        assert!(worktrees[2].prunable);
    }
}
