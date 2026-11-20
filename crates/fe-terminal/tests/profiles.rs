//! Terminal profiles: detection on Windows and on macOS and Linux against a
//! fake machine, `wsl.exe -l -q`'s output, and a WSL profile's working
//! folder; on Windows, the real machine's Command Prompt through ConPTY.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use fe_terminal::profiles::{
    self, Detected, Profile, ProfileKind, ProfileProbe, detect_unix, detect_windows,
    detect_windows_in_stages, parse_etc_shells, parse_wsl_list, wsl_cwd,
};

/// A machine as a test has it: what's on `PATH`, which files are there, the
/// environment, file texts, and what `wsl.exe -l -q` does.
#[derive(Default)]
struct FakeProbe {
    on_path: BTreeMap<String, PathBuf>,
    files: BTreeSet<String>,
    vars: BTreeMap<String, String>,
    texts: BTreeMap<String, String>,
    /// `wsl.exe -l -q`'s stdout; `None` fails, as one that times out does.
    wsl: Option<Vec<u8>>,
    /// The timeouts each run was given.
    timeouts: RefCell<Vec<Duration>>,
}

impl ProfileProbe for FakeProbe {
    fn find_on_path(&self, program: &str) -> Option<PathBuf> {
        self.on_path.get(program).cloned()
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.contains(&path.to_string_lossy().into_owned())
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn read(&self, path: &Path) -> Option<String> {
        self.texts
            .get(&path.to_string_lossy().into_owned())
            .cloned()
    }

    fn output(&self, program: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
        assert_eq!((program, args), ("wsl.exe", &["-l", "-q"][..]));
        self.timeouts.borrow_mut().push(timeout);
        self.wsl.clone()
    }
}

/// `text` as `wsl.exe -l -q` writes it: UTF-16LE with a byte order mark.
fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

/// A Windows machine with every shell there and two WSL distros.
fn windows_with_everything() -> FakeProbe {
    let mut probe = FakeProbe::default();
    probe
        .vars
        .insert("ProgramFiles".into(), r"C:\Program Files".into());
    probe.vars.insert("SystemRoot".into(), r"C:\WINDOWS".into());
    probe
        .vars
        .insert("ComSpec".into(), r"C:\WINDOWS\system32\cmd.exe".into());
    probe.on_path.insert(
        "pwsh".into(),
        PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe"),
    );
    for file in [
        r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
        r"C:\WINDOWS\system32\cmd.exe",
        r"C:\Program Files\Git\bin\bash.exe",
    ] {
        probe.files.insert(file.into());
    }
    probe.wsl = Some(utf16le_with_bom("Ubuntu-24.04\r\nDebian\r\n\r\n"));
    probe
}

fn names(found: &[Profile]) -> Vec<&str> {
    found.iter().map(|profile| profile.name.as_str()).collect()
}

#[test]
fn wsl_list_in_utf16le_parses_to_the_distro_names() {
    let output = utf16le_with_bom("Ubuntu\r\n\r\nDebian\r\n  \r\nkali-linux\r\n\r\n");
    assert_eq!(parse_wsl_list(&output), ["Ubuntu", "Debian", "kali-linux"]);
    // Without its byte order mark, and in UTF-8 as WSL_UTF8=1 has it.
    let bare: Vec<u8> = "Ubuntu\r\nDebian\r\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(parse_wsl_list(&bare), ["Ubuntu", "Debian"]);
    assert_eq!(
        parse_wsl_list(b"Ubuntu\r\nDebian\r\n"),
        ["Ubuntu", "Debian"]
    );
    assert_eq!(parse_wsl_list(&utf16le_with_bom("\r\n")), [] as [&str; 0]);
    assert_eq!(parse_wsl_list(b""), [] as [&str; 0]);
}

#[test]
fn each_wsl_profile_runs_wsl_exe_on_its_distro() {
    let profile = Profile::wsl("Ubuntu-24.04");
    assert_eq!(profile.program, "wsl.exe");
    assert_eq!(profile.args, ["-d", "Ubuntu-24.04"]);
    assert_eq!(
        profile.kind,
        ProfileKind::Wsl {
            distro: "Ubuntu-24.04".into()
        }
    );
    // In a folder that doesn't map, no --cd.
    let (program, args) = profile.command(Path::new("relative")).unwrap();
    assert_eq!(program, "wsl.exe");
    assert_eq!(args, ["-d", "Ubuntu-24.04"]);
}

#[test]
fn windows_detection_finds_each_shell_in_order() {
    let probe = windows_with_everything();
    let found = detect_windows(&probe);
    assert_eq!(
        names(&found),
        [
            "PowerShell 7",
            "Windows PowerShell",
            "Command Prompt",
            "Git Bash",
            "Ubuntu-24.04",
            "Debian"
        ]
    );
    let programs: Vec<(&str, Vec<&str>)> = found
        .iter()
        .map(|p| {
            (
                p.program.as_str(),
                p.args.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        programs,
        [
            (r"C:\Program Files\PowerShell\7\pwsh.exe", vec![]),
            (
                r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
                vec![]
            ),
            (r"C:\WINDOWS\system32\cmd.exe", vec![]),
            (r"C:\Program Files\Git\bin\bash.exe", vec!["--login", "-i"]),
            ("wsl.exe", vec!["-d", "Ubuntu-24.04"]),
            ("wsl.exe", vec!["-d", "Debian"]),
        ]
    );
    let ids: Vec<&str> = found.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "pwsh",
            "powershell",
            "cmd",
            "git-bash",
            "wsl:Ubuntu-24.04",
            "wsl:Debian"
        ]
    );
    // wsl.exe ran with a timeout.
    assert_eq!(*probe.timeouts.borrow(), [profiles::WSL_TIMEOUT]);
}

/// Each stage `detect_windows_in_stages` landed: the names, whether it
/// was the last, and how many times `wsl.exe` had been run by then.
fn stages(probe: &FakeProbe) -> Vec<(Vec<String>, bool, usize)> {
    let mut stages = Vec::new();
    detect_windows_in_stages(probe, &mut |found: Detected| {
        let names = (found.profiles.iter()).map(|p| p.name.clone()).collect();
        stages.push((names, found.done, probe.timeouts.borrow().len()));
    });
    stages
}

#[test]
fn the_shells_land_before_wsl_exe_is_run() {
    let shells = [
        "PowerShell 7",
        "Windows PowerShell",
        "Command Prompt",
        "Git Bash",
    ]
    .map(String::from)
    .to_vec();
    let mut every = shells.clone();
    every.extend(["Ubuntu-24.04", "Debian"].map(String::from));
    // The shells, with wsl.exe not yet run, however long it then takes;
    // then everything.
    let probe = windows_with_everything();
    assert_eq!(
        stages(&probe),
        [(shells.clone(), false, 0), (every.clone(), true, 1)]
    );
    assert_eq!(detect_windows(&windows_with_everything()), {
        let mut last = Vec::new();
        detect_windows_in_stages(&probe, &mut |found| last = found.profiles);
        last
    });
    // A wsl.exe that stalls past its timeout: the shells alone, done.
    let mut stalled = windows_with_everything();
    stalled.wsl = None;
    assert_eq!(
        stages(&stalled),
        [(shells.clone(), false, 0), (shells, true, 1)]
    );
}

/// The real machine's profiles, in stages: what's found so far at once to a
/// later caller, the last stage marked done, the same for every caller.
#[test]
fn detected_hands_every_caller_the_last_stage() {
    let (sent, got) = std::sync::mpsc::channel();
    for caller in 0..2 {
        let sent = sent.clone();
        profiles::detected(move |found| sent.send((caller, found.clone())).unwrap());
    }
    let mut last = [None, None];
    while last.iter().any(Option::is_none) {
        let (caller, found) = got
            .recv_timeout(Duration::from_secs(30))
            .expect("detection finishes");
        assert!(last[caller].is_none(), "nothing after the last stage");
        if found.done {
            last[caller] = Some(found);
        }
    }
    assert_eq!(last[0], last[1]);
    // Asked again, it's the same, at once.
    profiles::detected(move |found| sent.send((2, found.clone())).unwrap());
    let (_, again) = got.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(Some(again), last[0]);
}

#[test]
fn windows_detection_finds_pwsh_in_program_files_when_not_on_path() {
    let mut probe = windows_with_everything();
    probe.on_path.clear();
    assert_ne!(detect_windows(&probe)[0].name, "PowerShell 7");
    probe
        .files
        .insert(r"C:\Program Files\PowerShell\7\pwsh.exe".into());
    let found = detect_windows(&probe);
    assert_eq!(found[0].name, "PowerShell 7");
    assert_eq!(found[0].program, r"C:\Program Files\PowerShell\7\pwsh.exe");
}

#[test]
fn windows_detection_omits_each_absent_shell() {
    let mut probe = windows_with_everything();
    probe.on_path.clear();
    probe.files.remove(r"C:\Program Files\Git\bin\bash.exe");
    assert_eq!(
        names(&detect_windows(&probe)),
        [
            "Windows PowerShell",
            "Command Prompt",
            "Ubuntu-24.04",
            "Debian"
        ]
    );
    probe
        .files
        .remove(r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe");
    probe.files.remove(r"C:\WINDOWS\system32\cmd.exe");
    probe.wsl = Some(utf16le_with_bom(""));
    assert_eq!(names(&detect_windows(&probe)), [] as [&str; 0]);
}

#[test]
fn a_failing_or_timed_out_wsl_exe_gives_no_wsl_profiles() {
    // The probe gives `None` for a wsl.exe that timed out, failed to start
    // or exited with an error, as `SystemProbe` does.
    let mut probe = windows_with_everything();
    probe.wsl = None;
    let found = detect_windows(&probe);
    assert_eq!(
        names(&found),
        [
            "PowerShell 7",
            "Windows PowerShell",
            "Command Prompt",
            "Git Bash"
        ]
    );
    assert!(
        found
            .iter()
            .all(|p| !matches!(p.kind, ProfileKind::Wsl { .. }))
    );
}

/// The real probe gives up on a program that takes longer than its
/// timeout, and on one that fails.
#[cfg(unix)]
#[test]
fn the_system_probe_gives_up_on_a_slow_or_failing_program() {
    use fe_terminal::profiles::SystemProbe;
    let probe = SystemProbe;
    let start = std::time::Instant::now();
    let slow = probe.output("/bin/sh", &["-c", "sleep 10"], Duration::from_millis(200));
    assert_eq!(slow, None);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(
        probe.output(
            "/bin/sh",
            &["-c", "echo out; exit 3"],
            Duration::from_secs(5)
        ),
        None
    );
    assert_eq!(
        probe.output("/bin/sh", &["-c", "echo out"], Duration::from_secs(5)),
        Some(b"out\n".to_vec())
    );
    assert_eq!(
        probe.output("/no/such/program", &[], Duration::from_secs(5)),
        None
    );
    // One that exits but leaves its output held open by a child it started
    // isn't waited on past the timeout.
    let start = std::time::Instant::now();
    let held = probe.output(
        "/bin/sh",
        &["-c", "sleep 10 & echo out"],
        Duration::from_millis(300),
    );
    assert_eq!(held, None);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn etc_shells_skips_comments_blank_lines_and_nologin() {
    let text = "# /etc/shells: valid login shells\n/bin/sh\n\n/bin/bash # the usual\n  /usr/bin/zsh  \n/usr/sbin/nologin\n/sbin/nologin\n";
    assert_eq!(
        parse_etc_shells(text),
        ["/bin/sh", "/bin/bash", "/usr/bin/zsh"]
    );
}

#[test]
fn unix_detection_is_the_login_shell_then_the_shells_that_are_there() {
    let mut probe = FakeProbe::default();
    probe.vars.insert("SHELL".into(), "/usr/bin/fish".into());
    probe.texts.insert(
        "/etc/shells".into(),
        "# comment\n/bin/sh\n/bin/bash\n/usr/bin/bash\n/opt/missing/shell\n/usr/sbin/nologin\n/usr/bin/fish\n".into(),
    );
    for file in [
        "/bin/sh",
        "/bin/bash",
        "/usr/bin/bash",
        "/usr/bin/fish",
        "/usr/sbin/nologin",
    ] {
        probe.files.insert(file.into());
    }
    let found = detect_unix(&probe);
    assert_eq!(
        names(&found),
        ["Default", "sh", "bash", "/usr/bin/bash", "fish"]
    );
    // Default is the login shell: it runs as a terminal with no profile.
    assert_eq!(found[0].kind, ProfileKind::Login);
    assert_eq!(found[0].program, "/usr/bin/fish");
    assert_eq!(found[0].command(Path::new("/")), None);
    let options = found[0].options(PathBuf::from("/"), 100);
    assert_eq!(options.shell, None);
    assert_eq!(
        found[1].command(Path::new("/")),
        Some(("/bin/sh".into(), vec![]))
    );
}

#[test]
fn unix_detection_without_etc_shells_is_the_login_shell_alone() {
    let probe = FakeProbe::default();
    let found = detect_unix(&probe);
    assert_eq!(names(&found), ["Default"]);
    assert_eq!(found[0].program, "/bin/sh");
    assert_eq!(found[0].kind, ProfileKind::Login);
}

#[test]
fn a_windows_cwd_maps_into_the_distro() {
    assert_eq!(wsl_cwd(r"C:\a\b", "D").as_deref(), Some("/mnt/c/a/b"));
    assert_eq!(wsl_cwd(r"c:\a\b\", "D").as_deref(), Some("/mnt/c/a/b"));
    assert_eq!(wsl_cwd(r"D:\", "D").as_deref(), Some("/mnt/d"));
    assert_eq!(wsl_cwd("C:/a/b", "D").as_deref(), Some("/mnt/c/a/b"));
    assert_eq!(wsl_cwd(r"\\?\C:\a\b", "D").as_deref(), Some("/mnt/c/a/b"));
    assert_eq!(wsl_cwd(r"\\wsl.localhost\D\x", "D").as_deref(), Some("/x"));
    assert_eq!(wsl_cwd(r"\\wsl$\D\x", "D").as_deref(), Some("/x"));
    assert_eq!(wsl_cwd(r"\\wsl$\d\x\y", "D").as_deref(), Some("/x/y"));
    assert_eq!(wsl_cwd(r"\\wsl$\D", "D").as_deref(), Some("/"));
    assert_eq!(
        wsl_cwd(r"\\?\UNC\wsl.localhost\D\x", "D").as_deref(),
        Some("/x")
    );
}

#[test]
fn an_unmappable_cwd_passes_no_cd() {
    for cwd in [
        r"\\server\share\x",
        r"\\wsl$\Other\x",
        "relative\\x",
        "C:relative",
        "",
        "/home/me",
    ] {
        assert_eq!(wsl_cwd(cwd, "D"), None, "{cwd}");
    }
    let profile = Profile::wsl("D");
    let (_, args) = profile.command(Path::new(r"\\server\share\x")).unwrap();
    assert_eq!(args, ["-d", "D"]);
    let (_, args) = profile.command(Path::new(r"C:\a\b")).unwrap();
    assert_eq!(args, ["-d", "D", "--cd", "/mnt/c/a/b"]);
    let (_, args) = profile.command(Path::new(r"\\wsl.localhost\D\x")).unwrap();
    assert_eq!(args, ["-d", "D", "--cd", "/x"]);
}

#[test]
fn a_profile_resolves_to_its_session_options() {
    let dir = tempfile::tempdir().unwrap();
    let mut profile = Profile::shell("user:x", "x", "/bin/sh", vec!["-l".into()]);
    profile.env.insert("A".into(), "1".into());
    let options = profile.options(PathBuf::from("/"), 42);
    assert_eq!(options.cwd, PathBuf::from("/"));
    assert_eq!(options.shell, Some(("/bin/sh".into(), vec!["-l".into()])));
    assert_eq!(options.scrollback, 42);
    assert_eq!(options.env, BTreeMap::from([("A".into(), "1".into())]));
    // Its own folder, if it's one, wins.
    profile.cwd = Some(dir.path().to_owned());
    assert_eq!(profile.options(PathBuf::from("/"), 42).cwd, dir.path());
    profile.cwd = Some(dir.path().join("gone"));
    assert_eq!(
        profile.options(PathBuf::from("/"), 42).cwd,
        PathBuf::from("/")
    );
    // A distro path is the distro's: --cd takes it there.
    let mut wsl = Profile::wsl("D");
    wsl.cwd = Some(PathBuf::from("~"));
    let options = wsl.options(PathBuf::from("/"), 42);
    assert_eq!(options.cwd, PathBuf::from("/"));
    assert_eq!(options.shell.unwrap().1, ["-d", "D", "--cd", "~"]);
}

#[test]
fn a_profile_is_picked_by_name_or_id() {
    let profile = Profile::wsl("Ubuntu");
    assert!(profile.is_named("Ubuntu"));
    assert!(profile.is_named("ubuntu"));
    assert!(profile.is_named("wsl:Ubuntu"));
    assert!(!profile.is_named("Debian"));
}

/// The real machine: Command Prompt is found, and its profile starts
/// through ConPTY and runs.
#[cfg(windows)]
#[test]
fn the_real_probe_finds_command_prompt_and_it_runs_in_conpty() {
    use std::time::Instant;

    use fe_terminal::{GridSize, Terminal, profiles::SystemProbe};

    let found = detect_windows(&SystemProbe);
    let cmd = found
        .iter()
        .find(|profile| profile.id == profiles::ids::COMMAND_PROMPT)
        .unwrap_or_else(|| panic!("no Command Prompt in {:?}", names(&found)));
    assert_eq!(cmd.name, "Command Prompt");
    let mut profile = cmd.clone();
    profile.args = vec!["/C".into(), "echo profile-marker-%OS%".into()];
    let options = profile.options(std::env::temp_dir(), 1000);
    let size = GridSize {
        cols: 80,
        rows: 24,
        cell_width: 8,
        cell_height: 16,
    };
    let (terminal, events) = Terminal::spawn(options, size).expect("spawn");
    let start = Instant::now();
    loop {
        while events.try_recv().is_ok() {}
        let screen = terminal.snapshot().text();
        if screen.contains("profile-marker-Windows_NT") {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out; screen:\n{screen}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// ConPTY joins the program and its arguments into one command line, the
/// program unquoted: on Windows, a path with a space is quoted for it.
#[test]
fn a_program_with_a_space_is_quoted_on_windows_only() {
    let bash = Profile::shell(
        "git-bash",
        "Git Bash",
        r"C:\Program Files\Git\bin\bash.exe",
        vec!["--login".into(), "-i".into()],
    );
    let (program, args) = bash.command(Path::new(r"C:\a")).unwrap();
    if cfg!(windows) {
        assert_eq!(program, r#""C:\Program Files\Git\bin\bash.exe""#);
    } else {
        assert_eq!(program, r"C:\Program Files\Git\bin\bash.exe");
    }
    assert_eq!(args, ["--login", "-i"]);
    let cmd = Profile::shell(
        "cmd",
        "Command Prompt",
        r"C:\WINDOWS\system32\cmd.exe",
        vec![],
    );
    assert_eq!(
        cmd.command(Path::new(r"C:\a")).unwrap().0,
        r"C:\WINDOWS\system32\cmd.exe"
    );
}
