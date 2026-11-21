//! The build id behind `fe_remote_proto::BUILD`, and what cargo must watch
//! to keep it current. `build.rs` uses it. `tests/build_script.rs` calls it
//! directly, and `tests/build_rerun.rs` builds a crate with it through cargo.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where to read the commit from: `program` (git) run in `dir`.
pub(crate) struct Git {
    pub(crate) program: OsString,
    pub(crate) dir: PathBuf,
}

pub(crate) struct BuildId {
    /// `<version>+<commit>[-dirty]`.
    pub(crate) id: String,
    /// Paths whose change can change `id`, for `cargo:rerun-if-changed`.
    pub(crate) watch: Vec<PathBuf>,
}

/// The build id for `version`. The commit is `commit_override` when given
/// (a source tree without git), else HEAD's 12-character abbreviation, with
/// `-dirty` when anything under a tracked top-level entry differs from HEAD,
/// untracked files there included. That is exactly what `watch` covers, so
/// the id can't go stale between reruns. Without a commit there is no id:
/// `<version>+unknown` would let any two builds of a version pass the
/// handshake's strict match.
pub(crate) fn build_id(
    version: &str,
    commit_override: Option<&str>,
    git: &Git,
) -> Result<BuildId, String> {
    if let Some(commit) = commit_override.map(str::trim).filter(|c| !c.is_empty()) {
        return Ok(BuildId {
            id: format!("{version}+{commit}"),
            watch: Vec::new(),
        });
    }
    let commit = git
        .run(&["rev-parse", "--short=12", "HEAD"])
        .ok_or_else(|| {
            format!(
                "fe-remote-proto needs the commit for its build id, and `git rev-parse HEAD` \
             failed in {}. Building from a tree without git? Set TENMOKU_BUILD_COMMIT to \
             the commit it was made from.",
                git.dir.display()
            )
        })?;
    let entries = git.top_level_entries();
    let mut watch = git.watch_refs();
    if let Some(top) = git.run(&["rev-parse", "--show-toplevel"]) {
        // Missing entries too: cargo reruns on every build while one is
        // gone, so restoring it reruns as well.
        watch.extend(entries.iter().map(|entry| Path::new(&top).join(entry)));
    }
    let dirty = !entries.is_empty() && git.changed_under(&entries)?;
    Ok(BuildId {
        id: format!("{version}+{commit}{}", if dirty { "-dirty" } else { "" }),
        watch,
    })
}

impl Git {
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.arg("-C").arg(&self.dir).args(args);
        cmd
    }

    fn run(&self, args: &[&str]) -> Option<String> {
        let out = self.command(args).output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        (out.status.success() && !text.trim().is_empty()).then(|| text.trim().to_owned())
    }

    fn git_path(&self, name: &str) -> Option<PathBuf> {
        self.run(&["rev-parse", "--path-format=absolute", "--git-path", name])
            .map(PathBuf::from)
    }

    /// HEAD, the index and the branch HEAD names, so a commit, a checkout,
    /// staging or a moved branch reruns the script. Only existing paths:
    /// these come and go in ordinary use (a packed branch has no loose ref),
    /// and a missing one would rerun the script on every build.
    fn watch_refs(&self) -> Vec<PathBuf> {
        let mut watch: Vec<PathBuf> = ["HEAD", "index", "packed-refs"]
            .into_iter()
            .filter_map(|name| self.git_path(name))
            .filter(|p| p.exists())
            .collect();
        if let Some(branch) = self.run(&["symbolic-ref", "-q", "HEAD"])
            && let Some(path) = self.git_path(&branch)
        {
            // A packed branch has no loose ref until it moves: watch the
            // nearest directory that holds it now.
            watch.extend(path.ancestors().find(|p| p.exists()).map(Path::to_owned));
        }
        watch
    }

    /// The first component of every path tracked in HEAD or the index.
    /// cargo scans a watched directory whole.
    fn top_level_entries(&self) -> BTreeSet<String> {
        let listings = [
            self.command(&["ls-tree", "-z", "--full-tree", "--name-only", "HEAD"]),
            self.command(&["ls-files", "-z", "--full-name", "--", ":(top)"]),
        ];
        let mut entries = BTreeSet::new();
        for mut listing in listings {
            let Ok(out) = listing.output() else { continue };
            for path in out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
                let first = path.split(|b| *b == b'/').next().unwrap_or(path);
                entries.insert(String::from_utf8_lossy(first).into_owned());
            }
        }
        entries
    }

    /// Whether `git status` reports anything under `entries`, untracked
    /// files included.
    fn changed_under(&self, entries: &BTreeSet<String>) -> Result<bool, String> {
        let pathspecs: Vec<String> = entries
            .iter()
            .map(|e| format!(":(top,literal){e}"))
            .collect();
        let mut args = vec!["status", "--porcelain", "--untracked-files=normal", "--"];
        args.extend(pathspecs.iter().map(String::as_str));
        // Optional locks off: a status that refreshed the index would touch a
        // watched file, and cargo would rerun this on every build.
        let status = self
            .command(&args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .map_err(|e| format!("git status failed: {e}"))?;
        if !status.status.success() {
            return Err(format!(
                "git status failed in {}: {}",
                self.dir.display(),
                String::from_utf8_lossy(&status.stderr).trim()
            ));
        }
        Ok(!status.stdout.is_empty())
    }
}
