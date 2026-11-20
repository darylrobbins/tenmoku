//! Terminal profiles: the shells a new terminal can start, as Windows
//! Terminal's profiles are (docs/decisions/7csjgt8ynm-terminal-profiles.md).
//!
//! One model on every platform; only detection differs. On Windows,
//! [`detect_windows`] finds PowerShell 7, Windows PowerShell 5.1, Command
//! Prompt, Git Bash and each WSL distro, in that order and each only when
//! it's there; on macOS and Linux, [`detect_unix`] gives "Default", the
//! login shell a terminal runs today, then each shell `/etc/shells` lists.
//! Detection asks a [`ProfileProbe`], so tests fake the machine;
//! [`SystemProbe`] asks the real one, running `wsl.exe` with a timeout.
//! [`detected`] runs it once per process on a thread of its own and caches
//! it, so asking never blocks, and hands it over in stages
//! ([`detect_in_stages`]): the shells, found by looking at files, before
//! `wsl.exe` has answered, so a terminal of one never waits on WSL.
//!
//! A profile resolves to the [`TerminalOptions`] a session runs
//! ([`Profile::options`]); a WSL profile's working folder is mapped into the
//! distro ([`wsl_cwd`]) and passed as `--cd`.

use std::{
    collections::BTreeMap,
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use crate::TerminalOptions;

/// How long `wsl.exe -l -q` may take before detection gives up on WSL.
pub const WSL_TIMEOUT: Duration = Duration::from_secs(5);

/// The canonical names of the detected profiles, which the
/// `terminal.default_profile` setting and a user profile's name match.
pub mod names {
    /// The login shell, on macOS and Linux.
    pub const DEFAULT: &str = "Default";
    pub const POWERSHELL_7: &str = "PowerShell 7";
    pub const WINDOWS_POWERSHELL: &str = "Windows PowerShell";
    pub const COMMAND_PROMPT: &str = "Command Prompt";
    pub const GIT_BASH: &str = "Git Bash";
}

/// The stable ids of the detected profiles that aren't per shell or distro.
pub mod ids {
    pub const DEFAULT: &str = "default";
    pub const POWERSHELL_7: &str = "pwsh";
    pub const WINDOWS_POWERSHELL: &str = "powershell";
    pub const COMMAND_PROMPT: &str = "cmd";
    pub const GIT_BASH: &str = "git-bash";
    /// A user's profile from settings: `user:<name>`.
    pub const USER_PREFIX: &str = "user:";
    /// A WSL distro's: `wsl:<distro>`.
    pub const WSL_PREFIX: &str = "wsl:";
    /// An `/etc/shells` entry's: `shell:<path>`.
    pub const SHELL_PREFIX: &str = "shell:";
}

/// What a profile runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    /// The user's login shell, as a terminal with no profile runs it:
    /// `program` only names it.
    Login,
    /// `program` with `args`.
    Shell,
    /// A WSL distro, through `wsl.exe -d <distro>`.
    Wsl { distro: String },
}

/// One way to start a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Stable across runs: `pwsh`, `wsl:Ubuntu`, `shell:/bin/zsh`,
    /// `user:<name>`.
    pub id: String,
    /// What the menu shows and settings match.
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    /// Where it starts, if not where a new terminal would.
    pub cwd: Option<PathBuf>,
    /// What it adds to the environment. Not carried into a WSL distro.
    pub env: BTreeMap<String, String>,
    /// An icon's name from the app's icon set, if it has its own.
    pub icon: Option<String>,
    pub kind: ProfileKind,
}

impl Profile {
    /// A profile running `program` with `args`.
    pub fn shell(
        id: impl Into<String>,
        name: impl Into<String>,
        program: impl Into<String>,
        args: Vec<String>,
    ) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            program: program.into(),
            args,
            cwd: None,
            env: BTreeMap::new(),
            icon: None,
            kind: ProfileKind::Shell,
        }
    }

    /// The profile of WSL distro `distro`: `wsl.exe -d <distro>`.
    pub fn wsl(distro: &str) -> Profile {
        Profile {
            id: format!("{}{distro}", ids::WSL_PREFIX),
            name: distro.to_owned(),
            program: "wsl.exe".to_owned(),
            args: vec!["-d".to_owned(), distro.to_owned()],
            cwd: None,
            env: BTreeMap::new(),
            icon: None,
            kind: ProfileKind::Wsl {
                distro: distro.to_owned(),
            },
        }
    }

    /// "Default", the login shell `program` names.
    pub fn login(program: impl Into<String>) -> Profile {
        Profile {
            kind: ProfileKind::Login,
            ..Profile::shell(ids::DEFAULT, names::DEFAULT, program, Vec::new())
        }
    }

    /// Whether `name` picks it: its name or its id, either ignoring case.
    pub fn is_named(&self, name: &str) -> bool {
        let name = name.trim();
        self.name.eq_ignore_ascii_case(name) || self.id.eq_ignore_ascii_case(name)
    }

    /// The program and arguments it runs in `cwd`, `None` for the login
    /// shell. A WSL distro's gets `--cd` with `cwd` mapped into it, if it
    /// maps ([`wsl_cwd`]); its own `cwd`, if it's a distro path, goes there
    /// as it is.
    pub fn command(&self, cwd: &Path) -> Option<(String, Vec<String>)> {
        match &self.kind {
            ProfileKind::Login => None,
            ProfileKind::Shell => Some((command_program(&self.program), self.args.clone())),
            ProfileKind::Wsl { distro } => {
                let mut args = self.args.clone();
                let own = (self.cwd.as_ref())
                    .and_then(|dir| dir.to_str())
                    .filter(|dir| dir.starts_with('/') || dir.starts_with('~'))
                    .map(str::to_owned);
                let dir = own.or_else(|| wsl_cwd(&cwd.to_string_lossy(), distro));
                if let Some(dir) = dir {
                    args.extend(["--cd".to_owned(), dir]);
                }
                Some((command_program(&self.program), args))
            }
        }
    }

    /// The session it starts, in its own `cwd` if it has one that is a
    /// directory, else in `cwd`, keeping `scrollback` lines. A WSL
    /// profile's own `cwd` is the distro's, so its session starts in
    /// `cwd`, and `--cd` takes it there.
    pub fn options(&self, cwd: PathBuf, scrollback: usize) -> TerminalOptions {
        let own = (self.cwd.clone())
            .filter(|dir| !matches!(self.kind, ProfileKind::Wsl { .. }) && dir.is_dir());
        let cwd = own.unwrap_or(cwd);
        TerminalOptions {
            shell: self.command(&cwd),
            scrollback,
            env: self.env.clone(),
            ..TerminalOptions::new(cwd)
        }
    }
}

/// `program` as a session's command line starts with it. ConPTY's command
/// line is the program and its arguments joined, the program unquoted, so on
/// Windows one with a space in its path (`C:\Program Files\...`) is quoted;
/// elsewhere it's run as it is.
fn command_program(program: &str) -> String {
    if cfg!(windows) && program.contains(char::is_whitespace) && !program.starts_with('"') {
        format!("\"{program}\"")
    } else {
        program.to_owned()
    }
}

/// What detection asks the machine.
pub trait ProfileProbe {
    /// `program` on `PATH`, if it's there (with `.exe` on Windows if the
    /// name has none).
    fn find_on_path(&self, program: &str) -> Option<PathBuf>;
    /// Whether `path` is a file.
    fn exists(&self, path: &Path) -> bool;
    /// An environment variable, if set and not empty.
    fn var(&self, name: &str) -> Option<String>;
    /// A file's text, if it can be read.
    fn read(&self, path: &Path) -> Option<String>;
    /// What `program` with `args` writes to its stdout, if it exits with 0
    /// within `timeout`; `None` if it can't start, fails or takes longer.
    fn output(&self, program: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>>;
}

/// The profiles on this machine, as [`detect_windows`] or [`detect_unix`]
/// finds them.
pub fn detect(probe: &dyn ProfileProbe) -> Vec<Profile> {
    if cfg!(windows) {
        detect_windows(probe)
    } else {
        detect_unix(probe)
    }
}

/// What detection has found so far ([`detect_in_stages`], [`detected`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detected {
    /// The profiles found, in their order.
    pub profiles: Vec<Profile>,
    /// Whether detection has finished: until it has, more may follow.
    pub done: bool,
}

/// Detects this machine's profiles as [`detect`] does, handing each stage
/// to `land` as it's found: on Windows, [`detect_windows_in_stages`]'s two;
/// elsewhere, everything at once.
pub fn detect_in_stages(probe: &dyn ProfileProbe, land: &mut dyn FnMut(Detected)) {
    if cfg!(windows) {
        detect_windows_in_stages(probe, land);
    } else {
        land(Detected {
            profiles: detect_unix(probe),
            done: true,
        });
    }
}

/// [`detect_windows`] in two stages: first the shells, found by looking at
/// files and the environment, which takes no time; then those and the WSL
/// distros, once `wsl.exe` has answered or [`WSL_TIMEOUT`] has passed. So a
/// shell's terminal never waits on `wsl.exe`.
pub fn detect_windows_in_stages(probe: &dyn ProfileProbe, land: &mut dyn FnMut(Detected)) {
    let mut profiles = detect_windows_shells(probe);
    land(Detected {
        profiles: profiles.clone(),
        done: false,
    });
    profiles.extend(detect_wsl(probe));
    land(Detected {
        profiles,
        done: true,
    });
}

/// Windows' profiles, each only when it's there, in this order:
/// PowerShell 7 (`pwsh` on `PATH`, or under `Program Files\PowerShell\7`),
/// Windows PowerShell 5.1, Command Prompt (`%ComSpec%`), Git Bash
/// (`Program Files\Git\bin\bash.exe --login -i`), then a profile per WSL
/// distro `wsl.exe -l -q` lists. A `wsl.exe` that fails, or takes longer
/// than [`WSL_TIMEOUT`], gives none.
pub fn detect_windows(probe: &dyn ProfileProbe) -> Vec<Profile> {
    let mut found = detect_windows_shells(probe);
    found.extend(detect_wsl(probe));
    found
}

/// [`detect_windows`]'s shells, without the WSL distros: no program runs.
pub fn detect_windows_shells(probe: &dyn ProfileProbe) -> Vec<Profile> {
    let mut found = Vec::new();
    let program_files = probe
        .var("ProgramFiles")
        .unwrap_or_else(|| r"C:\Program Files".to_owned());
    let system_root = probe
        .var("SystemRoot")
        .unwrap_or_else(|| r"C:\Windows".to_owned());
    let pwsh = probe.find_on_path("pwsh").or_else(|| {
        let path = windows_join(&program_files, &[r"PowerShell", "7", "pwsh.exe"]);
        probe.exists(&path).then_some(path)
    });
    if let Some(pwsh) = pwsh {
        found.push(Profile::shell(
            ids::POWERSHELL_7,
            names::POWERSHELL_7,
            pwsh.to_string_lossy(),
            Vec::new(),
        ));
    }
    let powershell = windows_join(
        &system_root,
        &["System32", "WindowsPowerShell", "v1.0", "powershell.exe"],
    );
    if probe.exists(&powershell) {
        found.push(Profile::shell(
            ids::WINDOWS_POWERSHELL,
            names::WINDOWS_POWERSHELL,
            powershell.to_string_lossy(),
            Vec::new(),
        ));
    }
    let cmd = (probe.var("ComSpec").map(PathBuf::from))
        .unwrap_or_else(|| windows_join(&system_root, &["System32", "cmd.exe"]));
    if probe.exists(&cmd) {
        found.push(Profile::shell(
            ids::COMMAND_PROMPT,
            names::COMMAND_PROMPT,
            cmd.to_string_lossy(),
            Vec::new(),
        ));
    }
    let bash = windows_join(&program_files, &["Git", "bin", "bash.exe"]);
    if probe.exists(&bash) {
        found.push(Profile::shell(
            ids::GIT_BASH,
            names::GIT_BASH,
            bash.to_string_lossy(),
            vec!["--login".to_owned(), "-i".to_owned()],
        ));
    }
    found
}

/// A profile per WSL distro `wsl.exe -l -q` lists; none if it fails, or
/// takes longer than [`WSL_TIMEOUT`].
pub fn detect_wsl(probe: &dyn ProfileProbe) -> Vec<Profile> {
    let Some(list) = probe.output("wsl.exe", &["-l", "-q"], WSL_TIMEOUT) else {
        return Vec::new();
    };
    (parse_wsl_list(&list).iter())
        .map(|distro| Profile::wsl(distro))
        .collect()
}

/// `base` with `parts` after it, `\`-separated whatever this platform's
/// separator is, so detection reads the same in tests everywhere.
fn windows_join(base: &str, parts: &[&str]) -> PathBuf {
    let mut path = base.trim_end_matches(['\\', '/']).to_owned();
    for part in parts {
        path.push('\\');
        path.push_str(part);
    }
    PathBuf::from(path)
}

/// macOS's and Linux's profiles: "Default", the login shell (`$SHELL`, else
/// `/bin/sh`, by name; it runs as a terminal with no profile does), then
/// one per `/etc/shells` entry that's there, skipping comments, blank
/// lines and `nologin`. An entry is named for its file, or by its path if
/// an earlier one took that name.
pub fn detect_unix(probe: &dyn ProfileProbe) -> Vec<Profile> {
    let login = probe.var("SHELL").unwrap_or_else(|| "/bin/sh".to_owned());
    let mut found = vec![Profile::login(login)];
    let shells = probe.read(Path::new("/etc/shells")).unwrap_or_default();
    for path in parse_etc_shells(&shells) {
        if !probe.exists(Path::new(&path)) {
            continue;
        }
        if found
            .iter()
            .any(|profile| profile.program == path && profile.kind == ProfileKind::Shell)
        {
            continue;
        }
        let file = Path::new(&path)
            .file_name()
            .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
        let name = if found.iter().any(|profile| profile.name == file) {
            path.clone()
        } else {
            file
        };
        found.push(Profile::shell(
            format!("{}{path}", ids::SHELL_PREFIX),
            name,
            path,
            Vec::new(),
        ));
    }
    found
}

/// The shells an `/etc/shells` file lists: each line's path, without
/// comments (`#` to the end of the line), blank lines or `nologin`.
pub fn parse_etc_shells(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let name = Path::new(line).file_name().unwrap_or_default();
            name != "nologin"
        })
        .map(str::to_owned)
        .collect()
}

/// The distros `wsl.exe -l -q` lists: its output is UTF-16LE (with a byte
/// order mark, CRLF line ends and blank lines), or UTF-8 under
/// `WSL_UTF8=1`. Each name is trimmed, and a stray NUL or BOM dropped.
pub fn parse_wsl_list(output: &[u8]) -> Vec<String> {
    let utf16 = output.starts_with(&[0xFF, 0xFE])
        || (output.len() >= 2 && output.chunks(2).any(|pair| pair.get(1) == Some(&0)));
    let text = if utf16 {
        let (pairs, _) = output.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(output).into_owned()
    };
    text.split(['\r', '\n'])
        .map(|line| line.replace(['\u{feff}', '\0'], ""))
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Windows folder `cwd` as WSL distro `distro` sees it, for `wsl.exe --cd`:
/// `C:\a\b` is `/mnt/c/a/b`, and `\\wsl$\<distro>\x` or
/// `\\wsl.localhost\<distro>\x` is `/x`. `None` for another distro's
/// folder, a network share, a relative path or anything else that doesn't
/// map.
pub fn wsl_cwd(cwd: &str, distro: &str) -> Option<String> {
    let cwd = cwd.replace('/', "\\");
    let cwd = (cwd
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}")))
    .or_else(|| cwd.strip_prefix(r"\\?\").map(str::to_owned))
    .unwrap_or(cwd);
    if let Some(unc) = cwd.strip_prefix(r"\\") {
        let mut parts = unc.split('\\').filter(|part| !part.is_empty());
        let host = parts.next()?;
        if !host.eq_ignore_ascii_case("wsl$") && !host.eq_ignore_ascii_case("wsl.localhost") {
            return None;
        }
        if !parts.next()?.eq_ignore_ascii_case(distro) {
            return None;
        }
        let rest: Vec<&str> = parts.collect();
        return Some(format!("/{}", rest.join("/")));
    }
    let mut chars = cwd.chars();
    let (drive, colon) = (chars.next()?, chars.next()?);
    if !drive.is_ascii_alphabetic() || colon != ':' {
        return None;
    }
    let rest = chars.as_str();
    if !(rest.is_empty() || rest.starts_with('\\')) {
        return None;
    }
    let parts: Vec<&str> = rest.split('\\').filter(|part| !part.is_empty()).collect();
    let mut path = format!("/mnt/{}", drive.to_ascii_lowercase());
    for part in parts {
        path.push('/');
        path.push_str(part);
    }
    Some(path)
}

/// The machine itself: `PATH`, the file system, the environment, and
/// programs run with a timeout.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProbe;

impl ProfileProbe for SystemProbe {
    fn find_on_path(&self, program: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        let names: Vec<String> = if cfg!(windows) && Path::new(program).extension().is_none() {
            vec![format!("{program}.exe")]
        } else {
            vec![program.to_owned()]
        };
        std::env::split_paths(&path)
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file())
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn read(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn output(&self, program: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
        run_with_timeout(program, args, timeout)
    }
}

/// Runs `program` with `args`, its stdin and stderr closed, and gives its
/// stdout if it exits with 0 within `timeout`; one that takes longer is
/// killed. Its output is read on a thread of its own, so a full pipe never
/// holds the wait up.
fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // CREATE_NO_WINDOW: no console flashes up for it.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (sent, read) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        sent.send(stdout.read_to_end(&mut bytes).ok().map(|_| bytes))
            .ok();
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                child.kill().ok();
                child.wait().ok();
                break None;
            }
        }
    };
    if !status?.success() {
        return None;
    }
    // Its output, unless something it started still holds the pipe open
    // past the deadline.
    let left = deadline.saturating_duration_since(Instant::now());
    read.recv_timeout(left.max(Duration::from_millis(100)))
        .ok()
        .flatten()
}

/// The profiles found on this machine, stage by stage: the first call
/// starts detection ([`detect_in_stages`]) on a thread of its own, once per
/// process, and its result is kept. `update` has what's found so far at
/// once if any is, then each later stage as it lands, the last with `done`
/// set; it runs on the detecting thread, or the caller's. Without a thread
/// to detect on, nothing is found.
pub fn detected(update: impl FnMut(&Detected) + Send + 'static) {
    let mut update: Watcher = Box::new(update);
    let mut state = detection();
    let Some(found) = state.found.clone() else {
        state.watchers.push(update);
        if !std::mem::replace(&mut state.started, true) {
            drop(state);
            let spawned = std::thread::Builder::new()
                .name("terminal-profiles".to_owned())
                .spawn(|| detect_in_stages(&SystemProbe, &mut land));
            if spawned.is_err() {
                land(Detected {
                    profiles: Vec::new(),
                    done: true,
                });
            }
        }
        return;
    };
    // Outside the lock: `update` may ask again.
    drop(state);
    update(&found);
    if found.done {
        return;
    }
    let mut state = detection();
    match state.found.clone().filter(|found| found.done) {
        // The last stage landed while `update` ran.
        Some(done) => {
            drop(state);
            update(&done);
        }
        None => state.watchers.push(update),
    }
}

/// Whether [`detected`] has been asked, and so detection started, in this
/// process.
pub fn detection_started() -> bool {
    detection().started
}

type Watcher = Box<dyn FnMut(&Detected) + Send>;

/// [`detected`]'s state: what's found so far and who waits for more.
struct Detection {
    started: bool,
    found: Option<Detected>,
    watchers: Vec<Watcher>,
}

fn detection() -> MutexGuard<'static, Detection> {
    static DETECTION: Mutex<Detection> = Mutex::new(Detection {
        started: false,
        found: None,
        watchers: Vec::new(),
    });
    DETECTION
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Keeps a stage [`detected`] found and hands it to each watcher, outside
/// the lock; they wait on for the next unless it's the last.
fn land(found: Detected) {
    let mut watchers = {
        let mut state = detection();
        state.found = Some(found.clone());
        std::mem::take(&mut state.watchers)
    };
    for update in &mut watchers {
        update(&found);
    }
    if !found.done {
        detection().watchers.extend(watchers);
    }
}
