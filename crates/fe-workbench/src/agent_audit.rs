//! The agents' audit trail (the agents record's § 7, card AG8): every
//! decided request is one JSON line in
//! `<state_dir>/agents/<layout_key(root)>/audit.jsonl` ([`audit_file`]),
//! the file `0600` and its folder `0700`.
//!
//! - **A line** ([`Entry`]) holds the time, the agent (`fastedit.chat` or
//!   `acp.claude-agent`) and its chat or session id, the class and target
//!   (a path, pattern, or argv or command string; never a file's
//!   contents), the decision and who made it (`policy`, `rule:<pattern>`,
//!   `user`, `session` or `hard`), and for a command the program it
//!   resolved to and how it ended: its exit code, the `signal` that ended
//!   it, `killed` with why (`timeout`, `stop` or `trust_withdrawn`), or why
//!   it never started (`not_started`). A command's line is written once it
//!   has ended, so it holds both.
//! - **The worker** ([`AuditLog`]): the app's one thread writes every line,
//!   in the order recorded, as `chat_store_worker`'s does for chats, and is
//!   joined at quit. Under test it is a queue the test runs by hand
//!   ([`AuditLog::run_pending`]).
//! - **Rotation**: a line that would take the file past [`MAX_LOG`] moves it
//!   to `audit.jsonl.1` first, replacing the one there; one is kept.
//! - **Untrusted folders write nothing** ([`destination`]), and have nothing
//!   to log anyway (record § 9). A request's [`Destination`] is taken while
//!   it is trusted, so a command killed because trust was withdrawn still
//!   gets its line.
//! - **`agent.show_audit_log`** opens the window's folder's log, the kept
//!   `.1` first, in a read-only tab.

use std::{
    collections::VecDeque,
    fmt, fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{SystemTime, UNIX_EPOCH},
};

use fe_buffer::Buffer;
use fe_chat::permissions::Class;
use fe_editor::EditorModel;
use gpui::{App, AppContext as _, Context, Focusable as _, Global, Window};
use kiln::{Cx, platform::command_actions::dispatch_window};
use kiln_l10n::tr;
use serde::Serialize;

use crate::{
    Workbench,
    agent_command::{Decider, End, KillReason, Prepared, Ran},
    chat_panel::ChatStateDir,
    workbench::toast::{self, Toast},
};

#[cfg(test)]
#[path = "agent_audit_tests.rs"]
mod tests;

/// The size a log grows to before it is rotated.
pub(crate) const MAX_LOG: u64 = 1 << 20;

/// The log's file name.
const LOG_NAME: &str = "audit.jsonl";

/// The rotated log's file name.
const ROTATED_NAME: &str = "audit.jsonl.1";

/// The API chat's agent name.
pub(crate) const API_CHAT: &str = "fastedit.chat";

/// The Claude Agent's agent name.
pub(crate) const CLAUDE_AGENT: &str = "acp.claude-agent";

/// The audit log of the folder at canonical `root`:
/// `<state_dir>/agents/<layout_key(root)>/audit.jsonl`.
pub(crate) fn audit_file(state_dir: &Path, root: &Path) -> PathBuf {
    state_dir
        .join("agents")
        .join(kiln::core::layout::layout_key(root))
        .join(LOG_NAME)
}

/// The rotated log beside `file`.
pub(crate) fn rotated(file: &Path) -> PathBuf {
    file.with_file_name(ROTATED_NAME)
}

/// What a request was about, as a line records it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum Target {
    /// A path, a search pattern, a URL or query, a tool's title, or Claude
    /// Agent's command string.
    Text(String),
    /// The API chat's `run_command` argv.
    Argv(Vec<String>),
}

/// What was decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    Allow,
    Deny,
    /// A prompt closed unanswered: Stop, or trust withdrawn.
    Cancelled,
}

/// Why a command was killed, as a line records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Killed {
    Timeout,
    Stop,
    TrustWithdrawn,
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Entry {
    /// RFC 3339, UTC, to the millisecond.
    pub(crate) time: String,
    pub(crate) agent: String,
    /// The chat's id, or the Claude Agent session's.
    pub(crate) session: String,
    pub(crate) class: &'static str,
    pub(crate) target: Target,
    pub(crate) decision: Verdict,
    pub(crate) decided_by: String,
    /// The program a command resolved to (API chat), or ran as given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) program: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) killed: Option<Killed>,
    /// The signal that ended a command on its own (not Tenmoku's kill).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) signal: Option<i32>,
    /// Why a command allowed to run never started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) not_started: Option<String>,
}

/// A class's name, as the settings spell it.
pub(crate) fn class_name(class: Class) -> &'static str {
    match class {
        Class::Read => "read",
        Class::Search => "search",
        Class::ProposeEdit => "propose_edit",
        Class::RunCommand => "run_command",
        Class::Fetch => "fetch",
        Class::Other => "other",
    }
}

impl Entry {
    /// A request of `agent`'s, in chat or session `session`, decided now.
    pub(crate) fn new(
        agent: &str,
        session: impl fmt::Display,
        class: Class,
        target: Target,
        decision: Verdict,
        by: &Decider,
    ) -> Entry {
        Entry {
            time: rfc3339(SystemTime::now()),
            agent: agent.to_owned(),
            session: session.to_string(),
            class: class_name(class),
            target,
            decision,
            decided_by: by.to_string(),
            program: None,
            exit: None,
            killed: None,
            signal: None,
            not_started: None,
        }
    }

    /// The API chat's `run_command` of `prepared`, in chat `session`.
    pub(crate) fn command(
        session: impl fmt::Display,
        prepared: &Prepared,
        decision: Verdict,
        by: &Decider,
    ) -> Entry {
        Entry {
            program: Some(prepared.program.path().display().to_string()),
            ..Entry::new(
                API_CHAT,
                session,
                Class::RunCommand,
                Target::Argv(prepared.argv.clone()),
                decision,
                by,
            )
        }
    }

    /// With how the command ended: its exit code, or why it was killed. A
    /// cancel is `stop` unless `cancelled` says otherwise (trust withdrawn).
    pub(crate) fn ended(mut self, ran: &Ran, cancelled: Killed) -> Entry {
        match &ran.end {
            End::Exited(code) => self.exit = Some(*code),
            End::Signalled(signal) => self.signal = Some(*signal),
            End::Killed(KillReason::Timeout) => self.killed = Some(Killed::Timeout),
            End::Killed(KillReason::Cancelled) => self.killed = Some(cancelled),
            End::NotStarted(why) => self.not_started = Some(why.clone()),
        }
        self
    }

    /// The line, without its line break.
    pub(crate) fn line(&self) -> String {
        serde_json::to_string(self).expect("an audit entry serializes")
    }
}

/// `time` as RFC 3339 in UTC, to the millisecond:
/// `2026-11-14T09:05:07.123Z`.
pub(crate) fn rfc3339(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, of_day) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60,
        since.subsec_millis()
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Appends `line` and a line break to the log `file`, rotating it first if
/// that would take it past [`MAX_LOG`]. Makes its folder (`0700`) and the
/// file (`0600`).
pub(crate) fn append(file: &Path, line: &str) -> io::Result<()> {
    let dir = file
        .parent()
        .ok_or_else(|| io::Error::other("the log has no folder"))?;
    make_private_dir(dir)?;
    let size = match fs::metadata(file) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    let adding = line.len() as u64 + 1;
    if size > 0 && size + adding > MAX_LOG {
        fs::rename(file, rotated(file))?;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut log = options.open(file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        log.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    log.write_all(&bytes)
}

/// Makes `dir` and its missing parents, `dir` itself `0700`.
fn make_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// The log `file` as it is kept: the rotated one's lines, then its own.
/// Empty if neither exists.
pub(crate) fn read_log(file: &Path) -> io::Result<String> {
    let mut text = String::new();
    for path in [rotated(file), file.to_owned()] {
        match fs::read(&path) {
            Ok(bytes) => text.push_str(&String::from_utf8_lossy(&bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(text)
}

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The app's audit worker: a thread writing the lines in order, or under
/// test a queue the test runs by hand.
pub(crate) struct AuditLog {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
struct AuditHandle(Arc<AuditLog>);

impl Global for AuditHandle {}

impl AuditLog {
    /// A worker: a thread of its own unless `manual`.
    pub(crate) fn new(manual: bool) -> Arc<AuditLog> {
        let shared = Arc::new(Shared::default());
        let thread = (!manual).then(|| {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("agent-audit".into())
                .spawn(move || {
                    loop {
                        let job = {
                            let mut queue = shared.lock();
                            loop {
                                if let Some(job) = queue.jobs.pop_front() {
                                    break Some(job);
                                }
                                if queue.closed {
                                    break None;
                                }
                                queue = (shared.ready.wait(queue))
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                            }
                        };
                        match job {
                            Some(job) => job(),
                            None => return,
                        }
                    }
                })
                .expect("the agent audit thread starts")
        });
        Arc::new(AuditLog {
            shared,
            thread: Mutex::new(thread),
        })
    }

    /// The app's worker, made on first use and joined at quit. Under test
    /// it is the by-hand kind.
    pub(crate) fn of(cx: &mut App) -> Arc<AuditLog> {
        if let Some(AuditHandle(log)) = cx.try_global::<AuditHandle>() {
            return log.clone();
        }
        let log = AuditLog::new(cfg!(test));
        cx.set_global(AuditHandle(log.clone()));
        let quitting = log.clone();
        cx.on_app_quit(move |cx| {
            let quitting = quitting.clone();
            cx.background_spawn(async move { quitting.shutdown() })
        })
        .detach();
        log
    }

    fn submit(&self, job: Job) -> bool {
        let mut queue = self.shared.lock();
        if queue.closed {
            return false;
        }
        queue.jobs.push_back(job);
        self.shared.ready.notify_one();
        true
    }

    /// Queues `entry`'s line for the log `file`. False once shut down.
    pub(crate) fn write(&self, file: PathBuf, entry: &Entry) -> bool {
        let line = entry.line();
        self.submit(Box::new(move || {
            if let Err(error) = append(&file, &line) {
                log::warn!(
                    "an agent audit line wasn't written to {}: {error}",
                    file.display()
                );
            }
        }))
    }

    /// The log `file` as kept ([`read_log`]), once every line queued before
    /// is written.
    pub(crate) fn read(&self, file: PathBuf) -> async_channel::Receiver<io::Result<String>> {
        let (sender, reply) = async_channel::bounded(1);
        let queued = self.submit(Box::new(move || {
            let _ = sender.try_send(read_log(&file));
        }));
        if !queued {
            // The job and its sender are gone: the receiver sees a closed
            // channel.
            log::info!("the agent audit log was asked for after quit");
        }
        reply
    }

    /// Runs what is queued, on this thread: for the by-hand kind.
    #[cfg(test)]
    pub(crate) fn run_pending(&self) {
        loop {
            let job = self.shared.lock().jobs.pop_front();
            match job {
                Some(job) => job(),
                None => return,
            }
        }
    }

    /// Writes what was queued and stops the thread. Later requests are
    /// dropped.
    pub(crate) fn shutdown(&self) {
        self.shared.lock().closed = true;
        self.shared.ready.notify_all();
        let thread = (self.thread.lock())
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

/// The state directory, unless set otherwise ([`ChatStateDir`]). Under
/// test, none unless set, so no test touches the user's.
fn state_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<ChatStateDir>() {
        Some(ChatStateDir(dir)) => dir.clone(),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    }
}

/// `root` with its symlinks resolved, or as it is if that fails.
fn canonical(root: &Path) -> PathBuf {
    dunce::canonicalize(root).unwrap_or_else(|_| root.to_owned())
}

/// The log of the folder at `root`, if there's a state directory.
pub(crate) fn file_for(root: &Path, cx: &App) -> Option<PathBuf> {
    Some(audit_file(&state_dir(cx)?, &canonical(root)))
}

/// Where a request's line goes, taken when the request is made, while its
/// folder is trusted ([`destination`]). A command's line is written once
/// it has ended, which may be after trust was withdrawn (its kill is what
/// the line records), so it is written here whatever the trust is by then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Destination {
    file: PathBuf,
}

impl Destination {
    /// The log this goes to.
    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// Queues `entry`'s line. Whether it was queued.
    pub(crate) fn record(&self, entry: &Entry, cx: &mut App) -> bool {
        AuditLog::of(cx).write(self.file.clone(), entry)
    }
}

/// Where the lines of a request made now in the folder at `root` go: none
/// if the folder isn't trusted, or there's no state directory, so an
/// untrusted folder writes nothing.
pub(crate) fn destination(root: &Path, cx: &App) -> Option<Destination> {
    let root = canonical(root);
    if kiln_shell::trust::decided(&root, cx) != Some(true) {
        return None;
    }
    Some(Destination {
        file: file_for(&root, cx)?,
    })
}

/// Records `entry`, a request decided now, in the log of the folder at
/// `root` ([`destination`]). Whether it was queued.
pub(crate) fn record(root: &Path, entry: &Entry, cx: &mut App) -> bool {
    destination(root, cx).is_some_and(|destination| destination.record(entry, cx))
}

/// The command's declaration: see [`init`]. It's handled by [`init`]'s
/// listener, as `save_commands`' are.
#[allow(dead_code)]
struct AgentAuditCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl AgentAuditCommands {
    /// Show Agent Audit Log
    #[command(id = "agent.show_audit_log")]
    fn show_audit_log(&mut self, _: &mut Cx<Self>) {}
}

/// Handles `agent.show_audit_log` on the window's workbench.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &ShowAuditLog, cx| {
        let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
            return;
        };
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    if let Some(Some(workbench)) = window.root::<Workbench>() {
                        workbench.update(cx, |workbench, cx| {
                            workbench.show_agent_audit_log(window, cx)
                        });
                    }
                })
                .ok();
        });
    });
}

impl Workbench {
    /// Opens the folder's agent audit log, as kept, in a read-only tab, and
    /// focuses it; a toast says so if there's no folder or nothing logged.
    pub(crate) fn show_agent_audit_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.root().and_then(|root| file_for(root, cx)) else {
            toast::show(cx, Toast::info(tr!("workbench-agent-audit-no-folder")));
            return;
        };
        let reply = AuditLog::of(cx).read(file);
        cx.spawn_in(window, async move |this, cx| {
            let text = reply.recv().await;
            this.update_in(cx, |workbench, window, cx| match text {
                Ok(Ok(text)) if text.is_empty() => {
                    toast::show(cx, Toast::info(tr!("workbench-agent-audit-empty")));
                }
                Ok(Ok(text)) => workbench.open_audit_text(&text, window, cx),
                Ok(Err(error)) => toast::show(
                    cx,
                    Toast::error(tr!("workbench-agent-audit-unreadable")).detail(error.to_string()),
                ),
                Err(_) => {}
            })
            .ok();
        })
        .detach();
    }

    /// Opens `text` in a read-only tab titled as the audit log.
    fn open_audit_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let model = EditorModel::new(Buffer::from_file_contents(text), None);
        let editor = self.pane().update(cx, |pane, cx| {
            let editor = pane.new_editor(model, cx);
            editor.update(cx, |editor, cx| {
                editor.set_read_only(true, cx);
                editor.set_tab_title(tr!("workbench-agent-audit-title"), LOG_NAME, cx);
            });
            pane.add_item(Box::new(editor.clone()), cx);
            editor
        });
        window.focus(&editor.focus_handle(cx), cx);
    }
}
