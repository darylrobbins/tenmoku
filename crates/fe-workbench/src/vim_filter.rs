//! Vim's `!` filters and `:r !{cmd}` ([`HostCommand::Filter`],
//! [`HostCommand::ReadCommand`]), run through the user's shell.
//!
//! The shell is [`resolve_shell`]'s: the `vim.shell` setting if set, else
//! on Unix `$SHELL` when it is an absolute path, else `/bin/sh`, run as
//! `<shell> -c <command>`; on Windows `%COMSPEC%`, else `cmd.exe`, as
//! `<shell> /C <command>`. Its `PATH` is the login shell's
//! ([`fe_lsp::LoginPath::system`]) when it has one, so a fastedit started
//! from the Dock finds the same tools a terminal does.
//!
//! Each of these is refused before anything runs, with a vim message: a
//! command anyone but the user asked for
//! ([`fe_vim::messages::shell_refused`]); one in a window whose root isn't
//! trusted, or for a file whose real path (symlinks followed) isn't under
//! the root's ([`UNTRUSTED`]); one in a
//! read-only editor (`E21`); and one while another runs in the same editor
//! ([`BUSY`]).
//!
//! `%` is the buffer's path relative to the root (an untitled buffer fails
//! `E499`), quoted as one word for the shell's family: single quotes for
//! the POSIX shells (`sh`, `bash`, `zsh`, `ksh`, `dash` and the like), with
//! `'` as `'\''`; fish's single quotes, with `\` and `'` escaped. A quoted
//! word is one literal word only where a bare word goes, which fastedit
//! doesn't parse a shell's grammar to find: a command with `%` may hold none
//! of the characters that open quotes, substitutions, expansions or groups
//! (`'`, `"`, `` ` ``, `$`, `\`, `(`, `)`, `{`, `}`), nor start a comment
//! (`#`) or a here-document (`<<`), and `%` may not follow `~` or an `=`
//! that starts a word; else, or for another shell, it's refused
//! ([`NO_NAME_HERE`], [`UNKNOWN_SHELL`]). Nor may the name hold a line
//! break or another control character ([`UNNAMEABLE`]). A command without
//! `%` runs as typed.
//! `#` fails `E194`, as nvim's does with no alternate file. On Windows `%`
//! and `#` are refused: cmd.exe's quoting isn't safe to build.
//!
//! fe-vim counts the empty row after a buffer's final line break as a
//! line, which nvim has no such row for: a range on it is taken as on the
//! row before, and `:$r !` puts its rows above it, so `:%!sort` sorts the
//! file's lines and keeps its line break, and `:$!` filters the last line.
//!
//! The command runs on a thread of its own ([`run`]), in the root, its
//! rows on stdin from another thread (so one that never reads can't
//! block), stdout and stderr read apart, each capped at [`OUTPUT_LIMIT`].
//! It leads a process group of its own, which a stop kills: `SIGTERM`, then
//! after [`GRACE`] `SIGKILL`, to whatever is left of the group, the group's
//! leader not reaped until then so its id can't pass to another process.
//! That happens past `vim.filter_time_limit`, past the cap, on a cancel,
//! once the editor's tab closes or the editor goes (seen within [`WATCH`]),
//! once its window closes, and as the app quits, which waits for the
//! groups to go (at most [`GRACE`] and a little); none of them changes the
//! text. A cancel is
//! `workbench.cancel_vim_filter`, for the focused editor's command (with
//! none, every one in the window), or the Cancel button of the info toast
//! a command shows once it has run [`TOAST_AFTER`], for that command; the
//! toast goes when the command does.
//!
//! Exit 0 hands stdout to [`fe_vim::VimState::apply_filter`] (one undo
//! step) or [`fe_vim::VimState::apply_read_command`], with any stderr in a
//! warning toast's detail. Any other exit changes nothing, says `shell
//! returned N`, and shows an error toast with stderr as its detail. Unlike
//! nvim, neither puts stderr in the text. If the text changed while the
//! command ran, it's kept as it is ([`CHANGED`]); so it is if vim isn't in
//! normal mode as it ends ([`NOT_NORMAL`]), where an insert or another
//! command under way would take the output into its own undo step. Toasts
//! show in the command's window, whichever window is active.

use crate::process_group::{self, GroupId};
use std::collections::HashMap;
use std::io::{Read, Write as _};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use fe_buffer::Version;
use fe_command::Actor;
use fe_gpui::EditorView;
use fe_vim::{HostCommand, ShellCommand, ShellPart};
use gpui::{
    App, AsyncApp, Context, Entity, EntityId, Focusable as _, Global, Subscription, WeakEntity,
    Window, WindowId,
};
use kiln_shell::ItemHandle as _;

use super::super::git_diff::E21;
use super::super::toast::{Toast, ToastId};
use super::message;
use crate::Workbench;

/// What a shell command in an untrusted folder, or for a file outside the
/// root, says.
pub(crate) const UNTRUSTED: &str = "Shell commands run only in a trusted folder";
/// What a shell command says while another runs in the same editor.
pub(crate) const BUSY: &str = "A shell command is already running in this editor";
/// `%` in an untitled buffer, as nvim says it.
pub(crate) const E499: &str = "E499: Empty file name for '%' or '#', only works with \":p:h\"";
/// `#`, with no alternate file: fastedit keeps none yet.
pub(crate) const E194: &str = "E194: No alternate file name to substitute for '#'";
/// `%` or `#` with the Windows shell.
pub(crate) const NO_NAMES_ON_WINDOWS: &str =
    "% and # aren't put in a shell command on Windows: cmd.exe's quoting isn't safe to build";
/// `%` with a shell whose quoting fastedit doesn't build.
pub(crate) const UNKNOWN_SHELL: &str =
    "% goes in a shell command only for sh, bash, zsh, ksh, dash or fish: vim.shell names another";
/// `%` in a command whose text could put it inside quotes, an expansion,
/// a comment or a here-document.
pub(crate) const NO_NAME_HERE: &str = "% goes only in a command without quotes, backticks, $, \\, #, <<, (…) or {…}, and not after ~ or a leading =";
/// `%` for a file whose name has a line break or another control
/// character, which could end a comment or start a here-document's body.
pub(crate) const UNNAMEABLE: &str =
    "% can't name a file whose name has a line break or another control character";
/// Vim was in another mode as the command ended.
pub(crate) const NOT_NORMAL: &str =
    "The shell command ended outside normal mode, so the text is kept as it was";
/// The text changed while the command ran.
pub(crate) const CHANGED: &str =
    "The text changed while the shell command ran, so it's kept as it was";
/// A cancel stopped it.
pub(crate) const CANCELLED: &str = "Shell command cancelled: nothing changed";
/// The most stdout, or stderr, a command may write: fe-vim's put limit.
pub(crate) const OUTPUT_LIMIT: usize = 64 << 20;
/// How long a command runs before a toast offers to cancel it.
pub(crate) const TOAST_AFTER: Duration = Duration::from_millis(500);
/// How often the window looks for a command's tab, editor or window gone.
pub(crate) const WATCH: Duration = Duration::from_millis(100);
/// How long a stopped command has between `SIGTERM` and `SIGKILL`.
pub(crate) const GRACE: Duration = Duration::from_secs(1);
/// The command that cancels the focused editor's command, or the window's.
#[cfg(all(test, unix))]
pub(crate) const CANCEL: &str = "workbench.cancel_vim_filter";

/// Whose shell rules apply: [`resolve_shell`] takes it as data, so both
/// are tested on any host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Platform {
    Unix,
    Windows,
}

impl Platform {
    /// This host's.
    pub(crate) const HOST: Platform = match cfg!(windows) {
        true => Platform::Windows,
        false => Platform::Unix,
    };
}

/// The program a command runs with, as `program flag command`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shell {
    pub program: String,
    pub flag: &'static str,
    pub platform: Platform,
}

/// The shell for `platform`: `setting` (`vim.shell`) if it isn't empty;
/// else on Unix `$SHELL` if it is an absolute path, else `/bin/sh`, with
/// `-c`; on Windows `%COMSPEC%`, else `cmd.exe`, with `/C`. `env` reads an
/// environment variable.
pub(crate) fn resolve_shell(
    setting: &str,
    platform: Platform,
    env: impl Fn(&str) -> Option<String>,
) -> Shell {
    let setting = setting.trim();
    let (program, flag) = match platform {
        Platform::Unix => {
            let shell = env("SHELL").filter(|shell| shell.starts_with('/'));
            (shell.unwrap_or_else(|| "/bin/sh".to_string()), "-c")
        }
        Platform::Windows => {
            let shell = env("COMSPEC").filter(|shell| !shell.is_empty());
            (shell.unwrap_or_else(|| "cmd.exe".to_string()), "/C")
        }
    };
    let program = match setting.is_empty() {
        true => program,
        false => setting.to_string(),
    };
    Shell {
        program,
        flag,
        platform,
    }
}

/// A shell family's quoting, which [`Shell::line`] builds `%` with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Quoting {
    /// `sh`, `bash`, `zsh`, `ksh`, `dash` and the like.
    Posix,
    /// fish, whose single quotes take `\\` and `\'`.
    Fish,
}

impl Quoting {
    /// The family of the shell `program` names, by its file name; `None`
    /// for one fastedit doesn't quote for.
    pub(crate) fn of(program: &str) -> Option<Quoting> {
        let name = program.rsplit('/').next().unwrap_or(program);
        match name {
            "sh" | "bash" | "dash" | "zsh" | "ksh" | "ksh93" | "mksh" | "pdksh" | "oksh"
            | "ash" | "yash" | "posh" => Some(Quoting::Posix),
            "fish" => Some(Quoting::Fish),
            _ => None,
        }
    }

    /// `name` as one word of this family's, taken literally.
    pub(crate) fn quote(self, name: &str) -> String {
        match self {
            Quoting::Posix => format!("'{}'", name.replace('\'', r"'\''")),
            Quoting::Fish => format!("'{}'", name.replace('\\', r"\\").replace('\'', r"\'")),
        }
    }
}

impl Shell {
    /// `command` as this shell gets it: `%` as `current` (the buffer's
    /// path, `None` untitled) quoted as one word, or the vim message that
    /// refuses it, as the module describes.
    pub(crate) fn line(
        &self,
        command: &ShellCommand,
        current: Option<&str>,
    ) -> Result<String, &'static str> {
        let names = (command.parts.iter()).any(|part| !matches!(part, ShellPart::Text(_)));
        if names && self.platform == Platform::Windows {
            return Err(NO_NAMES_ON_WINDOWS);
        }
        let mut line = String::new();
        if command.parts.contains(&ShellPart::CurrentFile) {
            let quoting = Quoting::of(&self.program).ok_or(UNKNOWN_SHELL)?;
            if !Self::plain(command) {
                return Err(NO_NAME_HERE);
            }
            for part in &command.parts {
                match part {
                    ShellPart::Text(text) => line.push_str(text),
                    ShellPart::CurrentFile => {
                        let current = current.ok_or(E499)?;
                        if current.chars().any(char::is_control) {
                            return Err(UNNAMEABLE);
                        }
                        line.push_str(&quoting.quote(current));
                    }
                    ShellPart::AlternateFile => return Err(E194),
                }
            }
            return Ok(line);
        }
        for part in &command.parts {
            match part {
                ShellPart::Text(text) => line.push_str(text),
                ShellPart::CurrentFile => unreachable!("no % here"),
                ShellPart::AlternateFile => return Err(E194),
            }
        }
        Ok(line)
    }

    /// Whether `command`'s text leaves each `%` a word of its own, taken
    /// literally, in any shell fastedit quotes for: it opens nothing that
    /// quotes, substitutes, expands or groups, no comment (`#`) and no
    /// here-document (`<<`), and no `%` follows `~` (a user's home) or an
    /// `=` that starts a word (zsh's command path).
    fn plain(command: &ShellCommand) -> bool {
        let opens = |text: &str| {
            text.contains(['\'', '"', '`', '$', '\\', '(', ')', '{', '}', '#'])
                || text.contains("<<")
                || text.chars().any(char::is_control)
        };
        let mut before: Vec<char> = Vec::new();
        for part in &command.parts {
            match part {
                ShellPart::Text(text) if opens(text) => return false,
                ShellPart::Text(text) => before.extend(text.chars()),
                ShellPart::CurrentFile | ShellPart::AlternateFile => {
                    let starts = |ch: Option<&char>| ch.is_none_or(|ch| " \t;|&<>".contains(*ch));
                    match before.as_slice() {
                        [.., '~'] => return false,
                        [rest @ .., '='] if starts(rest.last()) => return false,
                        _ => {}
                    }
                    before.push('\'');
                }
            }
        }
        true
    }
}

/// A command to run, and how.
pub(crate) struct Run {
    pub shell: Shell,
    pub line: String,
    /// Where it runs: the root.
    pub cwd: PathBuf,
    /// Its stdin; `None` for none (`:r !`).
    pub input: Option<String>,
    pub limit: Duration,
    /// Set to stop it.
    pub cancel: Arc<AtomicBool>,
    /// Its place among the app's processes, for its quit.
    pub group: Group,
}

/// The processes an app's shell commands start, from before each starts
/// until its group's leader is reaped, whatever the windows showing them
/// have done since: what the app's quit stops and waits for.
#[derive(Clone, Default)]
pub(crate) struct Lives(Arc<Mutex<Live>>);

#[derive(Default)]
struct Live {
    /// Set as the app quits: nothing starts after.
    quitting: bool,
    /// Each command's group's id, once it has started, until its leader is
    /// reaped, so the id is still the group's.
    groups: HashMap<u64, Option<GroupId>>,
    next: u64,
}

impl Lives {
    fn lock(&self) -> MutexGuard<'_, Live> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A command about to start, listed from now.
    fn register(&self) -> Group {
        let mut live = self.lock();
        live.next += 1;
        let key = live.next;
        live.groups.insert(key, None);
        Group {
            lives: self.clone(),
            key,
        }
    }

    /// Stops them all as the app quits: nothing starts after this, each
    /// group is sent `SIGTERM`, and once their own stops have killed and
    /// reaped them (at most a little past [`GRACE`]), or the wait is over,
    /// what's left is sent `SIGKILL`.
    fn quit(&self) {
        {
            let mut live = self.lock();
            live.quitting = true;
            for &group in live.groups.values().flatten() {
                // A group whose leader isn't reaped while it's listed and the
                // lock is held, so it keeps its id.
                process_group::terminate(group);
            }
        }
        let deadline = Instant::now() + GRACE + Duration::from_millis(200);
        while !self.lock().groups.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        for &group in self.lock().groups.values().flatten() {
            // As above.
            process_group::kill(group);
        }
    }
}

/// A command's place in [`Lives`].
pub(crate) struct Group {
    lives: Lives,
    key: u64,
}

impl Default for Group {
    /// One of its own: for a [`run`] no app watches.
    fn default() -> Self {
        Lives::default().register()
    }
}

impl Group {
    /// Starts the command with `spawn`, its group listed as it starts;
    /// `None` if the app is quitting, when nothing starts.
    fn start(
        &self,
        spawn: impl FnOnce() -> std::io::Result<Child>,
    ) -> Option<std::io::Result<Child>> {
        let mut live = self.lives.lock();
        if live.quitting {
            live.groups.remove(&self.key);
            return None;
        }
        let spawned = spawn();
        match &spawned {
            Ok(child) => {
                let group = process_group::group_of(child);
                live.groups.insert(self.key, group);
            }
            Err(_) => {
                live.groups.remove(&self.key);
            }
        }
        Some(spawned)
    }

    /// Signals the group with `send` ([`process_group::terminate`] or
    /// [`process_group::kill`]), if its leader isn't reaped.
    fn signal(&self, send: fn(GroupId)) {
        let live = self.lives.lock();
        if let Some(&Some(group)) = live.groups.get(&self.key) {
            // The group's leader, not reaped while it's listed and the lock
            // is held, keeps its id.
            send(group);
        }
    }

    /// Takes the group off the list, for its leader to be reaped.
    fn reaping(&self) {
        self.lives.lock().groups.remove(&self.key);
    }
}

/// How a [`Run`] ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// It ran to the end, with this exit code (`128 + n` for signal `n`).
    Exited {
        code: i32,
        stdout: String,
        stderr: String,
    },
    /// It couldn't start: why.
    Failed(String),
    /// Stopped past its time limit.
    TimedOut,
    /// Stopped by its cancel.
    Cancelled,
    /// Stopped once stdout or stderr passed [`OUTPUT_LIMIT`].
    TooMuch,
}

/// Runs `run` to its end, blocking: see the module docs. A stopped
/// command's group is killed after [`GRACE`] on a thread of its own, once
/// this has returned.
pub(crate) fn run(run: Run) -> Outcome {
    let mut command = Command::new(&run.shell.program);
    command
        .arg(run.shell.flag)
        .arg(&run.line)
        .current_dir(&run.cwd)
        .stdin(match run.input {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process_group::lead_group(&mut command);
    if let Some(path) = fe_lsp::LoginPath::system().path() {
        command.env("PATH", path);
    }
    if run.cancel.load(Ordering::SeqCst) {
        run.group.reaping();
        return Outcome::Cancelled;
    }
    let started = Instant::now();
    let mut child = match run.group.start(|| command.spawn()) {
        Some(Ok(child)) => child,
        Some(Err(error)) => {
            return Outcome::Failed(format!("Can't run {}: {error}", run.shell.program));
        }
        None => return Outcome::Cancelled,
    };
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), run.input) {
        // A command that stops reading (or never starts) closes the pipe:
        // that's its business.
        thread::spawn(move || drop(stdin.write_all(input.as_bytes())));
    }
    let over = Arc::new(AtomicBool::new(false));
    let stdout = read_capped(child.stdout.take(), over.clone());
    let stderr = read_capped(child.stderr.take(), over.clone());
    let deadline = started + run.limit;
    let stop = loop {
        if over.load(Ordering::SeqCst) {
            break Outcome::TooMuch;
        }
        if run.cancel.load(Ordering::SeqCst) {
            break Outcome::Cancelled;
        }
        if exited(&child) && stdout.is_finished() && stderr.is_finished() {
            let read = |reader: JoinHandle<Vec<u8>>| reader.join().unwrap_or_default();
            let (stdout, stderr) = (read(stdout), read(stderr));
            // A reader may pass the cap as it ends, after the check above:
            // stopped as any run past it, the leader still not reaped.
            if stdout.len() > OUTPUT_LIMIT || stderr.len() > OUTPUT_LIMIT {
                stop_group(child, run.group);
                return Outcome::TooMuch;
            }
            run.group.reaping();
            let status = child.wait();
            let code = match status {
                Ok(status) => status
                    .code()
                    .unwrap_or_else(|| 128 + process_group::signal_of(&status).unwrap_or(0)),
                Err(error) => return Outcome::Failed(format!("Can't wait for it: {error}")),
            };
            let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
            return Outcome::Exited {
                code,
                stdout: text(stdout),
                stderr: text(stderr),
            };
        }
        if Instant::now() >= deadline {
            break Outcome::TimedOut;
        }
        thread::sleep(Duration::from_millis(5));
    };
    // The readers are left to end as the pipes close: a process that left
    // the group may hold them open.
    stop_group(child, run.group);
    stop
}

/// Whether `child` has exited, without reaping it: until it's reaped, its
/// id, which is its group's, can't go to another process.
fn exited(child: &Child) -> bool {
    process_group::exited(child)
}

/// Reads `pipe` to its end on a thread of its own, up to one byte past
/// [`OUTPUT_LIMIT`], which sets `over`.
fn read_capped(
    pipe: Option<impl Read + Send + 'static>,
    over: Arc<AtomicBool>,
) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut read = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(OUTPUT_LIMIT as u64 + 1).read_to_end(&mut read);
        }
        if read.len() > OUTPUT_LIMIT {
            over.store(true, Ordering::SeqCst);
        }
        read
    })
}

/// Stops `child`'s process group: `SIGTERM` now, and after [`GRACE`]
/// `SIGKILL` to whatever is left of it, such as a process that ignores
/// `SIGTERM`, on a thread of its own. The child, the group's leader, is
/// reaped only after that, so the group's id is still its own.
fn stop_group(mut child: Child, group: Group) {
    group.signal(process_group::terminate);
    thread::spawn(move || {
        thread::sleep(GRACE);
        group.signal(process_group::kill);
        group.reaping();
        let _ = child.wait();
    });
}

/// What a filter does with the output: `rows` (from 0) replaced, or put
/// below line `below` (from 1).
#[derive(Clone, Debug)]
enum Job {
    Filter(Range<u32>),
    Read(u32),
}

/// The shell commands running, by editor.
struct Running {
    runs: HashMap<EntityId, Filter>,
    next: u64,
    lives: Lives,
    _quit: Subscription,
    _closed: Subscription,
}

impl Global for Running {}

impl Running {
    /// The running commands, made the first time with the hooks that stop
    /// them as their window closes and as the app quits.
    fn of(cx: &mut App) -> &mut Running {
        if !cx.has_global::<Running>() {
            let quit = cx.on_app_quit(|cx| {
                stop_all(cx);
                async {}
            });
            let closed = cx.on_window_closed(|cx, window| {
                if let Some(running) = cx.try_global::<Running>() {
                    (running.runs.values())
                        .filter(|run| run.window == window)
                        .for_each(|run| run.cancel.store(true, Ordering::SeqCst));
                }
            });
            cx.set_global(Running {
                runs: HashMap::new(),
                next: 0,
                lives: Lives::default(),
                _quit: quit,
                _closed: closed,
            });
        }
        cx.global_mut::<Running>()
    }
}

/// As the app quits: cancels every command, and stops every process
/// they started, or may yet ([`Lives::quit`]).
fn stop_all(cx: &mut App) {
    let Some(running) = cx.try_global::<Running>() else {
        return;
    };
    for run in running.runs.values() {
        run.cancel.store(true, Ordering::SeqCst);
    }
    running.lives.quit();
}

/// A running shell command.
struct Filter {
    /// Tells it from a later one in the same editor.
    id: u64,
    /// The workbench it runs for, and its window.
    workbench: EntityId,
    window: WindowId,
    cancel: Arc<AtomicBool>,
    /// The toast that offers to cancel it, once shown.
    toast: Option<ToastId>,
}

/// Whether a shell command runs in `editor`.
fn running_in(editor: EntityId, cx: &App) -> bool {
    cx.try_global::<Running>()
        .is_some_and(|running| running.runs.contains_key(&editor))
}

/// The command `id` in `editor`, if it still runs.
fn run_of(editor: EntityId, id: u64, cx: &mut App) -> Option<&mut Filter> {
    if !cx.has_global::<Running>() {
        return None;
    }
    let running = cx.global_mut::<Running>();
    running.runs.get_mut(&editor).filter(|run| run.id == id)
}

/// How many shell commands run, in every window.
#[cfg(all(test, unix))]
pub(crate) fn running(cx: &App) -> usize {
    cx.try_global::<Running>()
        .map_or(0, |running| running.runs.len())
}

/// Where `path` is under `root`, relative to it, if its real path (every
/// symlink followed; for a file not made yet, its nearest folder's) is
/// under the root's: as typed when that holds as it's spelled, else the
/// real one.
pub(crate) fn within(path: &Path, root: &Path) -> Option<PathBuf> {
    let real_root = dunce::canonicalize(root).ok()?;
    let real = real_path(path)?;
    let inside = real.strip_prefix(&real_root).ok()?.to_owned();
    let plain =
        |relative: &Path| (relative.components()).all(|part| matches!(part, Component::Normal(_)));
    match path.strip_prefix(root) {
        Ok(relative) if plain(relative) && !relative.as_os_str().is_empty() => {
            Some(relative.to_owned())
        }
        _ => Some(inside),
    }
}

/// `path` with every symlink followed: its real path, or for a file not
/// made yet (nothing at all there, not even a symlink), its folder's real
/// path with its name. Anything else, such as a dangling symlink, is none.
fn real_path(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = dunce::canonicalize(path) {
        return Some(real);
    }
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return None,
    }
    let name = match path.components().next_back()? {
        Component::Normal(name) => name,
        _ => return None,
    };
    Some(dunce::canonicalize(path.parent()?).ok()?.join(name))
}

impl Workbench {
    /// Runs [`HostCommand::Filter`] or [`HostCommand::ReadCommand`],
    /// typed in `editor`, in this workbench's `window`, as the module
    /// describes; any other command does nothing.
    pub(super) fn vim_shell(
        &mut self,
        editor: &Entity<EditorView>,
        command: HostCommand,
        window: WindowId,
        cx: &mut Context<Self>,
    ) {
        let (job, command, actor) = match command {
            HostCommand::Filter {
                rows,
                command,
                actor,
            } => (Job::Filter(rows), command, actor),
            HostCommand::ReadCommand {
                below,
                command,
                actor,
            } => (Job::Read(below), command, actor),
            _ => return,
        };
        if actor != Actor::User {
            return message(editor, fe_vim::messages::shell_refused(), cx);
        }
        let path = editor.path(cx).map(|path| path.to_path_buf());
        let root = match &self.root {
            Some(root) if self.root_trusted() => root.clone(),
            _ => return message(editor, UNTRUSTED, cx),
        };
        let current = match &path {
            Some(path) => match within(path, &root) {
                Some(relative) => Some(relative),
                None => return message(editor, UNTRUSTED, cx),
            },
            None => None,
        };
        if editor.read(cx).is_read_only() {
            return message(editor, E21, cx);
        }
        if running_in(editor.entity_id(), cx) {
            return message(editor, BUSY, cx);
        }
        let settings = kiln::platform::config::settings::<fe_config::Settings>(cx);
        let shell = resolve_shell(&settings.vim.shell, Platform::HOST, |name| {
            std::env::var(name).ok()
        });
        let current = match current.as_deref().map(Path::to_str) {
            Some(Some(current)) => Some(current.to_string()),
            Some(None) if command.parts.contains(&ShellPart::CurrentFile) => {
                return message(
                    editor,
                    "The file's name isn't UTF-8, so % can't name it",
                    cx,
                );
            }
            _ => None,
        };
        let line = match shell.line(&command, current.as_deref()) {
            Ok(line) => line,
            Err(refused) => return message(editor, refused, cx),
        };
        let (job, input, version) = {
            let model = editor.read(cx).model();
            let text = model.text();
            let count = text.row_count();
            // The empty row after a final line break is no line of nvim's:
            // a range on it is on the row before, and `:$r !` puts its rows
            // above it, so the file keeps its line break.
            let lines = match count > 1 && text.row_start(count - 1) == text.len() {
                true => count - 1,
                false => count,
            };
            let (job, input) = match job {
                Job::Filter(rows) => {
                    let first = rows.start.min(lines - 1);
                    let end = rows.end.clamp(first + 1, lines);
                    let to = match end < count {
                        true => text.row_start(end) - 1,
                        false => text.len(),
                    };
                    let input = text.slice_to_string(text.row_start(first)..to) + "\n";
                    (Job::Filter(first..end), Some(input))
                }
                Job::Read(below) => (Job::Read(below.min(lines)), None),
            };
            (job, input, model.snapshot().version())
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let running = Running::of(cx);
        let group = running.lives.register();
        running.next += 1;
        let id = running.next;
        let run = Run {
            shell,
            line: line.clone(),
            cwd: root,
            input,
            limit: Duration::from_millis(settings.vim.filter_time_limit),
            cancel: cancel.clone(),
            group,
        };
        let (sender, receiver) = async_channel::bounded(1);
        let spawned = thread::Builder::new()
            .name("vim filter".into())
            .spawn(move || drop(sender.send_blocking(self::run(run))));
        if let Err(error) = spawned {
            return message(editor, format!("Can't run {line}: {error}"), cx);
        }
        let workbench = cx.entity_id();
        Running::of(cx).runs.insert(
            editor.entity_id(),
            Filter {
                id,
                workbench,
                window,
                cancel,
                toast: None,
            },
        );
        let weak = editor.downgrade();
        let editor_id = editor.entity_id();
        let offered = format!("Filtering through {line}…");
        cx.spawn(async move |workbench, cx| {
            watch(workbench, weak, window, id, offered, cx).await;
        })
        .detach();
        let weak = editor.downgrade();
        let limit = settings.vim.filter_time_limit;
        cx.spawn(async move |workbench, cx| {
            let outcome = receiver.recv().await.unwrap_or(Outcome::Cancelled);
            // Gone from the running whether or not the window is still
            // there to show the outcome.
            let ran = cx.update(|cx| {
                let toast = run_of(editor_id, id, cx).map(|run| run.toast);
                if toast.is_some() {
                    cx.global_mut::<Running>().runs.remove(&editor_id);
                }
                toast
            });
            let Some(toast) = ran else {
                return;
            };
            workbench
                .update(cx, |workbench, cx| {
                    if let Some(toast) = toast {
                        (workbench.toasts).update(cx, |layer, cx| layer.dismiss(toast, cx));
                    }
                    let done = Done {
                        editor: weak,
                        job,
                        line,
                        version,
                        limit,
                    };
                    workbench.vim_shell_done(done, outcome, cx);
                })
                .ok();
        })
        .detach();
    }

    /// Stops the focused editor's shell command, or with none there, every
    /// one running in this window's editors; then they change nothing.
    /// `workbench.cancel_vim_filter`.
    pub(crate) fn cancel_vim_filter(&mut self, window: &Window, cx: &mut Context<Self>) {
        let workbench = cx.entity_id();
        let focused = (self.file_editors(cx).into_iter())
            .find(|editor| editor.focus_handle(cx).contains_focused(window, cx))
            .map(|editor| editor.entity_id());
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let ours = |(editor, run): &(&EntityId, &Filter)| match focused {
            Some(focused) if running.runs.contains_key(&focused) => **editor == focused,
            _ => run.workbench == workbench,
        };
        (running.runs.iter())
            .filter(ours)
            .for_each(|(_, run)| run.cancel.store(true, Ordering::SeqCst));
    }

    /// Puts `outcome`, the end of `done`'s command, in its editor, if it
    /// still has a tab here.
    fn vim_shell_done(&mut self, done: Done, outcome: Outcome, cx: &mut Context<Self>) {
        let Done {
            editor,
            job,
            line,
            version,
            limit,
        } = done;
        let Some(editor) = editor.upgrade() else {
            return;
        };
        if self.tab_of(&editor, cx).is_none() {
            return;
        }
        let (stdout, stderr) = match outcome {
            Outcome::Exited {
                code: 0,
                stdout,
                stderr,
            } => (stdout, stderr),
            Outcome::Exited { code, stderr, .. } => {
                let mut failed = Toast::error(format!("{line} exited with {code}"));
                if !stderr.trim().is_empty() {
                    failed = failed.detail(stderr);
                }
                self.toasts.update(cx, |layer, cx| layer.push(failed, cx));
                return message(&editor, format!("shell returned {code}"), cx);
            }
            Outcome::Failed(why) => return message(&editor, why, cx),
            Outcome::TimedOut => {
                let said = format!(
                    "{line} ran past {limit} ms (vim.filter_time_limit): stopped, nothing changed"
                );
                return message(&editor, said, cx);
            }
            Outcome::Cancelled => return message(&editor, CANCELLED, cx),
            Outcome::TooMuch => {
                let said = format!("{line} wrote more than 64 MiB: stopped, nothing changed");
                return message(&editor, said, cx);
            }
        };
        if editor.read(cx).model().snapshot().version() != version {
            return message(&editor, CHANGED, cx);
        }
        match editor.read(cx).vim_mode() {
            Some("normal") => {}
            Some(_) => return message(&editor, NOT_NORMAL, cx),
            None => {
                let off = Toast::warning(format!("{line} ended with vim off: nothing changed"));
                self.toasts.update(cx, |layer, cx| layer.push(off, cx));
                return;
            }
        }
        editor.update(cx, |editor, cx| {
            editor.update_vim(
                |state, model| match job {
                    Job::Filter(rows) => state.apply_filter(model, rows, &stdout),
                    Job::Read(below) => state.apply_read_command(model, below, &stdout),
                },
                cx,
            )
        });
        if !stderr.trim().is_empty() {
            let warned = Toast::warning(format!("{line} wrote to stderr")).detail(stderr);
            self.toasts.update(cx, |layer, cx| layer.push(warned, cx));
        }
    }
}

/// Watches command `id` in `editor` while it runs, every [`WATCH`]:
/// cancels it once `editor`'s tab in `workbench` has closed, either has
/// gone, or `window` has closed (which [`Running::of`]'s hook sees first),
/// and after [`TOAST_AFTER`] shows `offered` in the workbench's
/// window, with a Cancel button for this command.
async fn watch(
    workbench: WeakEntity<Workbench>,
    editor: WeakEntity<EditorView>,
    window: WindowId,
    id: u64,
    offered: String,
    cx: &mut AsyncApp,
) {
    let editor_id = editor.entity_id();
    let mut waited = Duration::ZERO;
    loop {
        cx.background_executor().timer(WATCH).await;
        waited += WATCH;
        let watched =
            cx.update(|cx| {
                let cancel = run_of(editor_id, id, cx)?.cancel.clone();
                let open = (cx.windows().iter()).any(|open| open.window_id() == window);
                let shown = (workbench.upgrade()).zip(editor.upgrade()).is_some_and(
                    |(workbench, editor)| workbench.read(cx).tab_of(&editor, cx).is_some(),
                );
                let shown = open && shown;
                if !shown {
                    cancel.store(true, Ordering::SeqCst);
                    return None;
                }
                let toast = run_of(editor_id, id, cx)?.toast;
                if waited >= TOAST_AFTER && toast.is_none() {
                    let reply = {
                        let cancel = cancel.clone();
                        move |choice: Option<usize>| {
                            if choice == Some(0) {
                                cancel.store(true, Ordering::SeqCst);
                            }
                        }
                    };
                    let toast = Toast::info(offered.clone()).choices(vec!["Cancel".into()], reply);
                    let layer = workbench.upgrade()?.read(cx).toasts.clone();
                    let shown = layer.update(cx, |layer, cx| layer.push(toast, cx));
                    run_of(editor_id, id, cx)?.toast = Some(shown);
                }
                Some(())
            });
        if watched.is_none() {
            return;
        }
    }
}

/// What [`Workbench::vim_shell_done`] needs of the command that ran.
struct Done {
    editor: WeakEntity<EditorView>,
    job: Job,
    line: String,
    /// The text's version as it started.
    version: Version,
    /// `vim.filter_time_limit`, in milliseconds, as it started.
    limit: u64,
}
