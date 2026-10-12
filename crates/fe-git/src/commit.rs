//! Committing the index, through the `git` executable (see [`crate::cli`]), so
//! hooks run and `commit.gpgsign` is honoured as in a terminal.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cli::{Access, run_raw};
use crate::{GitError, Repo};

/// Why a commit didn't happen.
#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    /// The message has nothing but whitespace and `#` lines.
    #[error("empty commit message")]
    EmptyMessage,
    /// Nothing is staged and the commit isn't an amend.
    #[error("nothing staged to commit")]
    NothingStaged,
    /// A hook exited non-zero; `output` is what it printed, as git relayed it.
    #[error("{hook} hook failed:\n{output}")]
    HookFailed { hook: String, output: String },
    /// Git failed for another reason; `output` is all it printed.
    #[error("git commit failed ({code:?}):\n{output}")]
    Failed { code: Option<i32>, output: String },
    #[error(transparent)]
    Git(#[from] GitError),
}

/// Makes each trace file name unique within the process.
static TRACE_SEQ: AtomicU32 = AtomicU32::new(0);

impl Repo {
    /// Whether the index differs from HEAD (from the empty tree while HEAD is
    /// unborn).
    pub fn has_staged(&self) -> Result<bool, GitError> {
        let args = ["diff", "--cached", "--quiet"].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        match output.status.code() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            code => Err(GitError::Command {
                args: args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
                code,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            }),
        }
    }

    /// Commits the index with `message` (`#` lines stripped), replacing HEAD
    /// when `amend`, and returns the new HEAD's id. `pre-commit`, `commit-msg`
    /// and `post-commit` hooks run. Blocks until git exits.
    pub fn commit(&self, message: &str, amend: bool) -> Result<String, CommitError> {
        if message
            .lines()
            .all(|line| line.trim().is_empty() || line.starts_with('#'))
        {
            return Err(CommitError::EmptyMessage);
        }
        if !amend && !self.has_staged()? {
            return Err(CommitError::NothingStaged);
        }
        let mut args: Vec<OsString> = ["commit", "-q", "--cleanup=strip"]
            .map(OsString::from)
            .into();
        if amend {
            args.push("--amend".into());
        }
        args.extend(["-F", "-"].map(OsString::from));

        // Git names no hook when one fails; its trace2 events do.
        let trace = TraceDir::create()?;
        let output = run_raw(
            &self.workdir,
            Access::Write,
            &args,
            Some(message.as_bytes()),
            &[
                ("GIT_EDITOR", "true".as_ref()),
                ("GIT_TRACE2_EVENT", &trace.file()),
            ],
        );
        let events = std::fs::read_to_string(trace.file()).unwrap_or_default();
        drop(trace);
        let output = output?;

        if output.status.success() {
            return self
                .head_id()
                .ok_or_else(|| GitError::Io("HEAD unreadable after commit".into()).into());
        }
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let text = text.trim().to_owned();
        Err(match failed_hook(&events) {
            Some(hook) => CommitError::HookFailed { hook, output: text },
            None => CommitError::Failed {
                code: output.status.code(),
                output: text,
            },
        })
    }
}

/// The name of the first hook of the top-level git process (the first event's
/// session) that exited non-zero in a trace2 event log. Nested git commands a
/// hook runs log into the same file under their own sessions, with child ids
/// of their own.
fn failed_hook(events: &str) -> Option<String> {
    let top = str_field(events.lines().next()?, "sid")?;
    let mut hooks: Vec<(i64, String)> = Vec::new();
    for line in events.lines() {
        if str_field(line, "sid").as_deref() != Some(top.as_str()) {
            continue;
        }
        let Some(id) = int_field(line, "child_id") else {
            continue;
        };
        if line.contains("\"event\":\"child_start\"") {
            if let Some(name) = str_field(line, "hook_name") {
                hooks.push((id, name));
            }
        } else if line.contains("\"event\":\"child_exit\"")
            && int_field(line, "code").is_some_and(|code| code != 0)
            && let Some((_, name)) = hooks.iter().find(|(hook_id, _)| *hook_id == id)
        {
            return Some(name.clone());
        }
    }
    None
}

/// A private directory for one commit's trace2 log, removed on drop. Created
/// exclusively (so an existing path, a symlink included, is never reused) and
/// readable by the owner only.
struct TraceDir(PathBuf);

impl TraceDir {
    fn create() -> Result<TraceDir, GitError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        for _ in 0..16 {
            let dir = std::env::temp_dir().join(format!(
                "fe-git-trace-{}-{nanos:x}-{}",
                std::process::id(),
                TRACE_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(&dir) {
                Ok(()) => return Ok(TraceDir(dir)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(GitError::Io(err.to_string())),
            }
        }
        Err(GitError::Io("no unused trace directory name".into()))
    }

    fn file(&self) -> PathBuf {
        self.0.join("trace2.json")
    }
}

impl Drop for TraceDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn int_field(line: &str, key: &str) -> Option<i64> {
    let rest = line.split_once(&format!("\"{key}\":"))?.1;
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '-'))?;
    rest[..end].parse().ok()
}

fn str_field(line: &str, key: &str) -> Option<String> {
    let rest = line.split_once(&format!("\"{key}\":\""))?.1;
    Some(rest[..rest.find('"')?].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(sid: &str, id: u32, hook: &str) -> String {
        format!(
            "{{\"event\":\"child_start\",\"sid\":\"{sid}\",\"child_id\":{id},\"child_class\":\"hook\",\"hook_name\":\"{hook}\"}}\n"
        )
    }

    fn exit(sid: &str, id: u32, code: i32) -> String {
        format!(
            "{{\"event\":\"child_exit\",\"sid\":\"{sid}\",\"child_id\":{id},\"code\":{code}}}\n"
        )
    }

    #[test]
    fn nested_sessions_and_stale_lines_do_not_blame_a_hook() {
        // pre-commit (child 0) passes; a nested git it ran, in session
        // "top/inner", has its own failing child 0.
        let log = [
            start("top", 0, "pre-commit"),
            exit("top/inner", 0, 1),
            exit("top", 0, 0),
        ]
        .concat();
        assert_eq!(failed_hook(&log), None);
        let log = [log, start("top", 1, "commit-msg"), exit("top", 1, 1)].concat();
        assert_eq!(failed_hook(&log).as_deref(), Some("commit-msg"));
        assert_eq!(failed_hook(""), None);
    }

    #[test]
    fn trace_dirs_are_private_and_unique() {
        let a = TraceDir::create().unwrap();
        let b = TraceDir::create().unwrap();
        assert_ne!(a.0, b.0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&a.0).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        let path = a.0.clone();
        drop(a);
        assert!(!path.exists());
    }
}
