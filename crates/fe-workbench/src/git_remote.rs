//! Talking to a remote: `git.fetch`, `git.pull`, `git.push` and
//! `git.push_force_with_lease`, as VS Code's Git: Fetch, Pull, Push and Push
//! (Force with Lease), routed here by `git_commands`.
//!
//! - **Fetch** leaves pruning to the repository's `fetch.prune`.
//! - **Pull** only fast-forwards, so the editor never starts a merge or
//!   rebase unasked. It changes the index and the working tree, so it takes
//!   its turn among the work that changes a repository (`git_stage`'s queue);
//!   fetches and pushes touch neither, and don't hold that work up for as long
//!   as the network takes. Once it's run, the open files are checked against
//!   disk, reloading those it changed.
//! - **Push** goes to the upstream, never with a lease. With none, it
//!   publishes the branch to `origin`, or the only remote, and says the
//!   upstream was set; with several remotes and no `origin`, it asks for one
//!   to be set by hand.
//! - **Push (Force with Lease)** is its own command, and asks first, naming
//!   the branch and upstream. A yes holds for those and the branch's commit
//!   alone: if any has changed by then, nothing is pushed, and what's pushed is
//!   that commit to that destination, whatever HEAD does meanwhile. It doesn't
//!   publish.
//!
//! One runs at a time in a repository: another asked for meanwhile answers
//! [`ALREADY_RUNNING`], in any window into it, by any path or linked worktree,
//! so finding the repository comes first, in the background. Each runs git in
//! the background, with a toast saying it's going and one saying how it went.
//! How it went is said even if the root's changed or the window's closed by
//! then, as nothing else would tell of a push that failed. A pull's open files
//! are checked whatever the root is by then, as a new root keeps the tabs but
//! not the watcher that would have noticed; the git state is only read again
//! if the root's still the one it ran in.

use std::{
    path::{Path, PathBuf},
    process::Command,
    rc::{Rc, Weak},
};

use fe_git::{Head, PullMode, RemoteError, Repo};
use gpui::{App, AppContext as _, AsyncApp, Context, EntityId, Global, SharedString, Window};
use kiln_l10n::tr;

use super::{
    super::{
        git_stage,
        toast::{self, Toast},
    },
    GitCommand, GitOp,
};
use crate::Workbench;

/// What a command answers outside a repository.
fn no_repo() -> SharedString {
    tr!("workbench-git-not-a-repository")
}

/// What a command answers while another's running in its repository.
pub(crate) fn already_running() -> SharedString {
    tr!("workbench-git-remote-already-running")
}

/// What force-pushing says under its question.
fn force_detail() -> SharedString {
    tr!("workbench-git-remote-force-detail")
}

/// What a force-push answers when a yes no longer holds.
pub(crate) fn target_changed() -> SharedString {
    tr!("workbench-git-remote-target-changed")
}

/// What a push answers on a branch with nothing to push.
fn no_commits() -> SharedString {
    tr!("workbench-git-remote-no-commits")
}

/// Runs `op`, a remote command, once its repository's been found in the
/// background.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let Some(root) = workbench.root().map(Path::to_owned) else {
        return workbench.fail_git_op(&op, no_repo().into(), cx);
    };
    let origin = Origin {
        root,
        git: workbench.git.entity_id(),
    };
    let (root, command) = (origin.root.clone(), op.command);
    let found = cx.background_spawn(async move { find(&root, command) });
    cx.spawn_in(window, async move |workbench, cx| {
        let found = found.await;
        workbench
            .update_in(cx, |workbench, window, cx| {
                workbench.remote_found(origin, op, found, window, cx)
            })
            .ok();
    })
    .detach();
}

/// The root a command started in, and the git state that root's, which a new
/// root replaces.
#[derive(Clone, Debug)]
struct Origin {
    root: PathBuf,
    git: EntityId,
}

/// What a force-push does, as its question is asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    branch: String,
    /// The branch's commit, what's pushed.
    commit: String,
    remote: String,
    /// The full name of the branch replaced there.
    merge: String,
}

impl Target {
    /// Where it's pushed, as `origin/main`.
    fn upstream(&self) -> String {
        let merge = self.merge.strip_prefix("refs/heads/");
        format!("{}/{}", self.remote, merge.unwrap_or(&self.merge))
    }
}

/// What a command found before it starts.
#[derive(Debug)]
struct Found {
    /// The repository's shared git directory, the same by any path into it
    /// and from any of its linked worktrees.
    repository: PathBuf,
    /// For a force-push, what it would replace, or why it can't. An answer
    /// only once nothing's running, which is the answer otherwise.
    target: Result<Option<Target>, RemoteError>,
}

/// How a command went.
#[derive(Debug)]
enum Outcome {
    /// It ran; what its toast says.
    Done(String),
    NoRepo,
    Failed(RemoteError),
}

/// The repositories a command's running in, each until its claim's gone.
#[derive(Default)]
struct Running(Vec<(PathBuf, Weak<()>)>);

impl Global for Running {}

/// Whether a command's running in `repository`.
fn is_running(repository: &Path, cx: &mut App) -> bool {
    let running = &mut cx.default_global::<Running>().0;
    running.retain(|(_, claim)| claim.strong_count() > 0);
    running.iter().any(|(busy, _)| busy == repository)
}

/// Marks a command as running in `repository` for as long as what's returned
/// lives, which a task going with its window can't outlast; none if one
/// already is.
fn claim(repository: &Path, cx: &mut App) -> Option<Rc<()>> {
    if is_running(repository, cx) {
        return None;
    }
    let claim = Rc::new(());
    let running = &mut cx.default_global::<Running>().0;
    running.push((repository.to_owned(), Rc::downgrade(&claim)));
    Some(claim)
}

impl Workbench {
    /// `op`'s repository has been looked for: answers, asks or starts, unless
    /// the root's changed since.
    fn remote_found(
        &mut self,
        origin: Origin,
        op: GitOp,
        found: Result<Option<Found>, RemoteError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git.entity_id() != origin.git {
            return log::info!("not running {:?}: the root changed", op.command);
        }
        let Found { repository, target } = match found {
            Ok(Some(found)) => found,
            Ok(None) => return self.fail_git_op(&op, no_repo().into(), cx),
            Err(error) => return toast::show(cx, failure(op.command, error, &origin.root)),
        };
        if is_running(&repository, cx) {
            return toast::show(cx, Toast::info(already_running()));
        }
        match target {
            Err(error) => toast::show(cx, failure(op.command, error, &origin.root)),
            Ok(Some(target)) => self.ask_force_push(origin, repository, target, window, cx),
            Ok(None) => self.start_remote(origin, repository, None, op.command, window, cx),
        }
    }

    /// Asks whether to force-push to `target`, then does on a yes. Nothing is
    /// running meanwhile, so a question left unanswered holds nothing up.
    fn ask_force_push(
        &mut self,
        origin: Origin,
        repository: PathBuf,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (answer, answered) = futures::channel::oneshot::channel();
        let titles = vec![
            tr!("workbench-git-yes").into(),
            tr!("workbench-git-no").into(),
        ];
        let question = (Toast::warning(force_question(&target)).detail(force_detail())).choices(
            titles,
            move |choice| {
                answer.send(choice).ok();
            },
        );
        toast::show(cx, question);
        cx.spawn_in(window, async move |workbench, cx| {
            if answered.await != Ok(Some(0)) {
                return;
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    let command = GitCommand::PushForceWithLease;
                    workbench.start_remote(origin, repository, Some(target), command, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Runs `command` in `origin`'s repository in the background, unless the
    /// root's changed or one's running there already.
    fn start_remote(
        &mut self,
        origin: Origin,
        repository: PathBuf,
        confirmed: Option<Target>,
        command: GitCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git.entity_id() != origin.git {
            return log::info!("not running {command:?}: the root changed");
        }
        let Some(claim) = claim(&repository, cx) else {
            return toast::show(cx, Toast::info(already_running()));
        };
        let going = Toast::info(progress(command));
        let going = self.toasts.update(cx, |layer, cx| layer.push(going, cx));
        let workbench = cx.weak_entity();
        let window = window.window_handle();
        let start = move |cx: &mut App| {
            #[cfg(test)]
            let gate = git_stage::test_gate(cx);
            let root = origin.root.clone();
            let outcome = cx.background_spawn(async move {
                #[cfg(test)]
                gate.await;
                run_remote(&root, command, confirmed.as_ref())
            });
            cx.spawn(async move |cx: &mut AsyncApp| {
                let outcome = outcome.await;
                (cx.update_window(window, |_, window, cx| {
                    (workbench.update(cx, |workbench, cx| {
                        workbench.toasts.update(cx, |layer, cx| {
                            layer.dismiss(going, cx);
                        });
                        workbench.remote_ran(&origin, command, window, cx)
                    }))
                    .ok();
                }))
                .ok();
                let told = told(command, outcome, &origin.root);
                cx.update(|cx| toast::show(cx, told));
                drop(claim);
            })
        };
        match command {
            GitCommand::Pull => git_stage::mutate(cx, start),
            _ => start(cx).detach(),
        }
    }

    /// `command`, from `origin`, has run: reads git again, unless the root's
    /// changed since, and after a pull (conflicts change files too) checks the
    /// open files, which a new root keeps.
    fn remote_ran(
        &mut self,
        origin: &Origin,
        command: GitCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git.entity_id() == origin.git {
            self.git.update(cx, |git, cx| git.refresh(cx));
        }
        if command == GitCommand::Pull {
            self.check_open_files(window, cx);
        }
    }
}

/// The repository containing `root`, if there's one, and what `command` needs
/// to know of it before it starts. Runs on the background executor, and blocks
/// until git exits.
fn find(root: &Path, command: GitCommand) -> Result<Option<Found>, RemoteError> {
    let Some(repo) = Repo::discover(root)? else {
        return Ok(None);
    };
    let shared = git_read(&repo, &["rev-parse", "--git-common-dir"])?;
    let repository = (dunce::canonicalize(repo.workdir().join(shared))).map_err(|error| {
        failed(&tr!(
            "workbench-git-remote-git-directory",
            error = error.to_string()
        ))
    })?;
    let target = match command {
        GitCommand::PushForceWithLease => force_target(&repo).map(Some),
        _ => Ok(None),
    };
    Ok(Some(Found { repository, target }))
}

/// What a force-push from HEAD would do, as [`Repo::push`] finds it.
fn force_target(repo: &Repo) -> Result<Target, RemoteError> {
    let branch = match repo.current_branch()? {
        Head::Branch(branch) => branch,
        Head::Unborn(_) => return Err(failed(&no_commits())),
        Head::Detached(_) => return Err(RemoteError::Detached),
    };
    let config = |key: &str| {
        let key = format!("branch.{branch}.{key}");
        git_read(repo, &["config", "--default", "", "--get", &key])
    };
    let (remote, merge) = (config("remote")?, config("merge")?);
    if remote.is_empty() || merge.is_empty() {
        return Err(RemoteError::NoUpstream);
    }
    let name = format!("refs/heads/{branch}^{{commit}}");
    let commit = git_read(repo, &["rev-parse", "--verify", &name])?;
    Ok(Target {
        branch,
        commit,
        remote,
        merge,
    })
}

/// What force-pushing to `target` asks first.
fn force_question(target: &Target) -> String {
    let (branch, upstream) = (&target.branch, target.upstream());
    tr!(
        "workbench-git-remote-force-question",
        branch = branch.as_str(),
        upstream = upstream.as_str()
    )
    .into()
}

/// Force-pushes what was `confirmed`, if it's still what a force-push from
/// HEAD would do; what its toast says.
fn force_push(repo: &Repo, confirmed: Option<&Target>) -> Result<String, RemoteError> {
    let target = force_target(repo)?;
    if confirmed != Some(&target) {
        return Err(failed(&target_changed()));
    }
    #[cfg(test)]
    after_force_check(repo.workdir());
    repo.force_push_commit(&target.commit, &target.remote, &target.merge)?;
    let (branch, upstream) = (&target.branch, target.upstream());
    Ok(tr!(
        "workbench-git-remote-force-pushed",
        branch = branch.as_str(),
        upstream = upstream.as_str()
    )
    .into())
}

/// What a test has happen in a working tree once a force-push's checks have
/// passed.
#[cfg(test)]
static AFTER_FORCE_CHECK: std::sync::Mutex<Vec<(PathBuf, ForceCheckHook)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
type ForceCheckHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
fn after_force_check(workdir: &Path) {
    let mut waiting = AFTER_FORCE_CHECK.lock().unwrap();
    let hook = (waiting.iter()).position(|(dir, _)| dir == workdir);
    let hook = hook.map(|ix| waiting.remove(ix).1);
    drop(waiting);
    if let Some(hook) = hook {
        hook();
    }
}

/// Runs `command` in the repository containing `root`. Runs on the background
/// executor, and blocks until git exits.
fn run_remote(root: &Path, command: GitCommand, confirmed: Option<&Target>) -> Outcome {
    let repo = match Repo::discover(root) {
        Ok(Some(repo)) => repo,
        Ok(None) => return Outcome::NoRepo,
        Err(error) => return Outcome::Failed(error.into()),
    };
    let ran = match command {
        GitCommand::Fetch => {
            (repo.fetch(false)).map(|()| tr!("workbench-git-remote-fetched").into())
        }
        GitCommand::Pull => (repo.pull(PullMode::FastForwardOnly))
            .map(|()| tr!("workbench-git-remote-pulled").into()),
        GitCommand::PushForceWithLease => force_push(&repo, confirmed),
        _ => match repo.push(false) {
            Err(RemoteError::NoUpstream) => publish(&repo),
            pushed => pushed.map(|()| tr!("workbench-git-remote-pushed").into()),
        },
    };
    match ran {
        Ok(done) => Outcome::Done(done),
        Err(error) => Outcome::Failed(error),
    }
}

/// Publishes HEAD's branch, which has no upstream, to the remote to use; what
/// its toast says.
fn publish(repo: &Repo) -> Result<String, RemoteError> {
    let branch = match repo.current_branch()? {
        Head::Branch(branch) => branch,
        Head::Unborn(_) => return Err(failed(&no_commits())),
        Head::Detached(_) => return Err(RemoteError::Detached),
    };
    let remote = publish_remote(repo)?;
    repo.publish(&remote)?;
    Ok(tr!(
        "workbench-git-remote-published",
        branch = branch.as_str(),
        remote = remote.as_str()
    )
    .into())
}

/// `origin`, else the only remote.
fn publish_remote(repo: &Repo) -> Result<String, RemoteError> {
    let listed = git_read(repo, &["remote"])?;
    let remotes: Vec<&str> = listed.lines().collect();
    match remotes.as_slice() {
        [] => Err(RemoteError::NoRemote),
        [only] => Ok((*only).to_owned()),
        _ if remotes.contains(&"origin") => Ok("origin".to_owned()),
        _ => Err(failed(&tr!("workbench-git-remote-several-remotes"))),
    }
}

/// What `git args` prints in `repo`'s working tree, trimmed, for what fe-git
/// doesn't offer. Blocks until `git` exits.
fn git_read(repo: &Repo, args: &[&str]) -> Result<String, RemoteError> {
    let output = crate::process_group::no_window(&mut Command::new("git"))
        .current_dir(repo.workdir())
        .args(args)
        .output()
        .map_err(|error| failed(&tr!("workbench-git-error", error = error.to_string())))?;
    if !output.status.success() {
        return Err(failed(&String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn failed(output: &str) -> RemoteError {
    RemoteError::Failed {
        output: output.to_owned(),
    }
}

/// What `command` says while it runs.
fn progress(command: GitCommand) -> SharedString {
    match command {
        GitCommand::Fetch => tr!("workbench-git-remote-fetching"),
        GitCommand::Pull => tr!("workbench-git-remote-pulling"),
        GitCommand::PushForceWithLease => tr!("workbench-git-remote-force-pushing"),
        _ => tr!("workbench-git-remote-pushing"),
    }
}

/// Which of `git.ftl`'s `-git-remote-verb` forms `command` selects.
fn verb(command: GitCommand) -> &'static str {
    match command {
        GitCommand::Fetch => "fetch",
        GitCommand::Pull => "pull",
        GitCommand::PushForceWithLease => "force-push",
        _ => "push",
    }
}

/// The toast saying how `command`, run in `root`, went.
fn told(command: GitCommand, outcome: Outcome, root: &Path) -> Toast {
    match outcome {
        Outcome::Done(done) => Toast::info(done),
        Outcome::NoRepo => Toast::info(no_repo()),
        Outcome::Failed(error) => failure(command, error, root),
    }
}

/// The toast for `command`, run in `root`, failing with `error`: one sentence
/// on the cause and what to do, under it anything git said.
fn failure(command: GitCommand, error: RemoteError, root: &Path) -> Toast {
    let verb = verb(command);
    let (sentence, detail) = match error {
        RemoteError::NoRemote => (tr!("workbench-git-remote-no-remote", verb = verb), None),
        RemoteError::NoUpstream => (tr!("workbench-git-remote-no-upstream", verb = verb), None),
        RemoteError::Detached => (tr!("workbench-git-remote-detached", verb = verb), None),
        RemoteError::Conflicts { paths } => {
            let detail = (paths.iter())
                .map(|path| {
                    path.strip_prefix(root)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            let count = paths.len();
            (
                tr!("workbench-git-remote-conflicts", verb = verb, count = count),
                Some(detail),
            )
        }
        RemoteError::NotFastForward => (tr!("workbench-git-remote-diverged", verb = verb), None),
        RemoteError::Rejected { output } if command == GitCommand::PushForceWithLease => (
            tr!("workbench-git-remote-lease-rejected", verb = verb),
            Some(output),
        ),
        RemoteError::Rejected { output } => (
            tr!("workbench-git-remote-rejected", verb = verb),
            Some(output),
        ),
        RemoteError::Auth { output } => {
            (tr!("workbench-git-remote-auth", verb = verb), Some(output))
        }
        // Git's own first line, the cause when it's signing refused or a
        // local change in the way: it says what to do too.
        RemoteError::Failed { output } => {
            let git_failed = tr!("workbench-git-remote-failed");
            let first = (output.lines())
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or(&git_failed);
            let first = (first.strip_prefix("error: "))
                .or_else(|| first.strip_prefix("fatal: "))
                .unwrap_or(first);
            let sentence = tr!("workbench-git-remote-cant", verb = verb, cause = first);
            let detail = (output.trim().lines().count() > 1).then_some(output);
            (sentence, detail)
        }
        RemoteError::Git(error) => (
            tr!(
                "workbench-git-remote-cant",
                verb = verb,
                cause = error.to_string()
            ),
            None,
        ),
    };
    match detail {
        Some(detail) => Toast::error(sentence).detail(detail),
        None => Toast::error(sentence),
    }
}

#[cfg(test)]
#[path = "git_remote_tests.rs"]
mod git_remote_tests;
