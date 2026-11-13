//! Credential prompts for fetches, through an [`Askpass`]: git's own, from
//! a local http server that wants a username and password, and ssh's, from
//! a fake `ssh` on `PATH`. The helper git runs is this test binary,
//! rerun as [`askpass_helper`], which asks over the socket as an app's
//! `--askpass` mode does.
//!
//! The library's git calls inherit the process environment, so each test
//! runs again in a child process whose git sees no global or system config.
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use fe_git::{Askpass, AskpassRequest, RemoteError, Repo};

const USER: &str = "ada";
const SECRET: &str = "s3cret-Pa55";

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Set in the child process a test reruns itself in.
const ISOLATED: &str = "FE_GIT_ASKPASS_TEST_ISOLATED";

/// `true` in the child process, where the test's body runs. In the test
/// process itself, runs test `name` in a child whose git sees no global or
/// system config, no proxy and no ssh command of the user's, with the fake
/// `ssh` first on `PATH`, fails if it fails, and returns `false`.
fn isolated(name: &str) -> bool {
    if std::env::var_os(ISOLATED).is_some() {
        return true;
    }
    let bin = tempfile::tempdir().unwrap();
    fake_ssh(bin.path());
    let mut path = OsString::from(bin.path());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(ISOLATED, "1")
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for (var, value) in [
        ("GIT_AUTHOR_NAME", "t"),
        ("GIT_AUTHOR_EMAIL", "t@t"),
        ("GIT_COMMITTER_NAME", "t"),
        ("GIT_COMMITTER_EMAIL", "t@t"),
    ] {
        command.env(var, value);
    }
    for var in [
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "SSH_ASKPASS_REQUIRE",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
    ] {
        command.env_remove(var);
    }
    let output = command.output().expect("rerun the test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{name} failed in isolation:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

/// Set, with the prompt, when git runs this binary as its askpass helper.
const HELPER_SOCKET: &str = "FE_GIT_ASKPASS_TEST_SOCKET";
const HELPER_PROMPT: &str = "FE_GIT_ASKPASS_TEST_PROMPT";

/// The askpass helper, when git runs this binary as one: asks over the
/// socket and writes the answer to fd 3, which the script made git's pipe
/// (stdout is the test harness's), then exits as `--askpass` would.
#[test]
fn askpass_helper() {
    let (Some(socket), Some(prompt)) = (
        std::env::var_os(HELPER_SOCKET),
        std::env::var(HELPER_PROMPT).ok(),
    ) else {
        return;
    };
    let mut out = fs::OpenOptions::new()
        .write(true)
        .open("/dev/fd/3")
        .unwrap();
    let args = [socket, prompt.into()];
    std::process::exit(fe_git::askpass::helper(args, &mut out));
}

/// What an [`Askpass`] runs as its helper: this binary, as
/// [`askpass_helper`], given the socket and the prompt.
fn helper() -> Vec<OsString> {
    let script = format!(
        "{HELPER_SOCKET}=\"$1\" {HELPER_PROMPT}=\"$2\" exec \"$0\" askpass_helper --exact \
         --nocapture --test-threads=1 3>&1 >/dev/null 2>/dev/null"
    );
    vec![
        "/bin/sh".into(),
        "-c".into(),
        script.into(),
        std::env::current_exe().unwrap().into(),
    ]
}

/// An `ssh` that asks `SSH_ASKPASS` for a passphrase, as a key with one
/// would, and runs git's command on this machine if it's [`SECRET`]. It
/// refuses batch mode, which would never ask. The host says how it goes:
///
/// - `host`: as above, failing on a wrong passphrase or none.
/// - `retry`: as OpenSSH does, a helper that fails is an empty answer: it
///   asks three times, then gets in with another key, asking nothing.
/// - `vanish`: shows a notice (`SSH_ASKPASS_PROMPT=none`) and asks for a
///   passphrase, stopping each helper after a moment, unanswered, as
///   OpenSSH does once a security key's touched, then gets in.
fn fake_ssh(bin: &Path) {
    let script = format!(
        "#!/bin/sh\n\
         for arg; do\n\
         \x20 case $arg in *BatchMode*) echo 'Permission denied (batch).' >&2; exit 255;; esac\n\
         \x20 host=$last; last=$arg\n\
         done\n\
         [ \"$SSH_ASKPASS_REQUIRE\" = force ] || {{ echo 'Permission denied (no askpass).' >&2; exit 255; }}\n\
         ask() {{ \"$SSH_ASKPASS\" \"Enter passphrase for key '/k/id': \"; }}\n\
         case $host in\n\
         retry)\n\
         \x20 for try in 1 2 3; do\n\
         \x20   pass=$(ask) || pass=\n\
         \x20   [ \"$pass\" = '{SECRET}' ] && exec sh -c \"$last\"\n\
         \x20 done\n\
         \x20 exec sh -c \"$last\";;\n\
         vanish)\n\
         \x20 SSH_ASKPASS_PROMPT=none \"$SSH_ASKPASS\" 'Confirm user presence for key /k/id' & notice=$!\n\
         \x20 \"$SSH_ASKPASS\" \"Enter passphrase for key '/k/id': \" >/dev/null & helper=$!\n\
         \x20 sleep 1; kill $notice $helper 2>/dev/null; wait\n\
         \x20 exec sh -c \"$last\";;\n\
         esac\n\
         pass=$(ask) || {{ echo 'Permission denied (publickey).' >&2; exit 255; }}\n\
         [ \"$pass\" = '{SECRET}' ] || {{ echo 'Permission denied (publickey).' >&2; exit 255; }}\n\
         exec sh -c \"$last\"\n"
    );
    let ssh = bin.join("ssh");
    fs::write(&ssh, script).unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A bare repository with one commit, served over dumb http by [`Server`],
/// and a repository with it as `origin` and nothing fetched.
struct Fixture {
    dir: tempfile::TempDir,
    bare: PathBuf,
    work: PathBuf,
    /// Where askpass directories go.
    state: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("seed");
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-q"]);
    fs::write(seed.join("f"), "one\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "one"]);
    git(dir.path(), &["clone", "-q", "--bare", "seed", "x.git"]);
    let bare = dir.path().join("x.git");
    git(&bare, &["update-server-info"]);
    let work = dir.path().join("work");
    fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q"]);
    let state = dir.path().join("state");
    Fixture {
        bare,
        work,
        state,
        dir,
    }
}

/// An http server for a [`Fixture`]'s bare repository that wants
/// [`USER`] and [`SECRET`] (or, `refusing`, never takes any), recording
/// each `Authorization` it's sent.
struct Server {
    port: u16,
    authorizations: Arc<Mutex<Vec<String>>>,
}

fn serve(bare: &Path, refusing: bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let authorizations = Arc::new(Mutex::new(Vec::new()));
    let (bare, seen) = (bare.to_owned(), authorizations.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            respond(stream, &bare, refusing, &seen);
        }
    });
    Server {
        port,
        authorizations,
    }
}

fn respond(mut stream: TcpStream, bare: &Path, refusing: bool, seen: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let mut authorization = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("authorization")
        {
            authorization = Some(value.trim().to_owned());
        }
    }
    let wanted = format!("Basic {}", base64(&format!("{USER}:{SECRET}")));
    if let Some(authorization) = &authorization {
        seen.lock().unwrap().push(authorization.clone());
    }
    if refusing || authorization.as_deref() != Some(&wanted) {
        let _ = stream.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\n\
              Content-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    // `GET /x.git/<file>?<query> HTTP/1.1`
    let target = first.split_whitespace().nth(1).unwrap_or("");
    let path = target.split('?').next().unwrap_or("");
    let file = path.strip_prefix("/x.git/").map(|rela| bare.join(rela));
    let body = file.and_then(|file| {
        let mut body = Vec::new();
        fs::File::open(file).ok()?.read_to_end(&mut body).ok()?;
        Some(body)
    });
    let response = match body {
        Some(body) => {
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            response.extend(body);
            response
        }
        None => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
    };
    let _ = stream.write_all(&response);
}

fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// An askpass for `state` that records what it's asked, answering [`USER`]
/// to a username's prompt and `password` to any other, or cancelling with
/// `None`.
fn recording(state: &Path, password: Option<&str>) -> (Askpass, Arc<Mutex<Vec<AskpassRequest>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (seen, password) = (asked.clone(), password.map(str::to_owned));
    let askpass = Askpass::new(helper(), state.to_owned(), move |request, _| {
        seen.lock().unwrap().push(request.clone());
        let password = password.clone()?;
        Some(if request.prompt.starts_with("Username") {
            USER.to_owned()
        } else {
            password
        })
    });
    (askpass, asked)
}

fn open(root: &Path, askpass: Option<Askpass>) -> Repo {
    let mut repo = Repo::discover(root).unwrap().expect("a repository");
    repo.set_askpass(askpass);
    repo
}

/// What's left in `state`: nothing once a command's done.
fn left_in(state: &Path) -> Vec<PathBuf> {
    match fs::read_dir(state) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn a_credential_request_reaches_the_prompt_and_the_answer_reaches_git() {
    if !isolated("a_credential_request_reaches_the_prompt_and_the_answer_reaches_git") {
        return;
    }
    let fixture = fixture();
    let server = serve(&fixture.bare, false);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    let repo = open(&fixture.work, Some(askpass));

    repo.fetch(false).unwrap();
    let asked = std::mem::take(&mut *asked.lock().unwrap());
    let host = format!("127.0.0.1:{}", server.port);
    assert_eq!(
        asked,
        [
            AskpassRequest {
                remote: "origin".into(),
                prompt: format!("Username for 'http://{host}': "),
            },
            AskpassRequest {
                remote: "origin".into(),
                prompt: format!("Password for 'http://{USER}@{host}': "),
            },
        ]
    );
    let wanted = format!("Basic {}", base64(&format!("{USER}:{SECRET}")));
    assert!(
        server.authorizations.lock().unwrap().contains(&wanted),
        "git never sent the answer"
    );
    assert_eq!(
        git(&fixture.work, &["rev-parse", "origin/main"]),
        git(&fixture.bare, &["rev-parse", "main"])
    );
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());

    // Ssh's passphrase too, in place of batch mode.
    let ssh_url = format!("ssh://host{}", fixture.bare.display());
    git(&fixture.work, &["remote", "add", "byssh", &ssh_url]);
    git(&fixture.work, &["config", "branch.main.remote", "byssh"]);
    git(
        &fixture.work,
        &["config", "branch.main.merge", "refs/heads/main"],
    );
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    let repo = open(&fixture.work, Some(askpass));
    repo.fetch(false).unwrap();
    let asked = std::mem::take(&mut *asked.lock().unwrap());
    assert_eq!(
        asked,
        [AskpassRequest {
            remote: "byssh".into(),
            prompt: "Enter passphrase for key '/k/id': ".into(),
        }]
    );
    assert!(!git(&fixture.work, &["rev-parse", "byssh/main"]).is_empty());
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
    drop(fixture.dir);
}

#[test]
fn cancelling_fails_the_op_with_the_old_credentials_message() {
    if !isolated("cancelling_fails_the_op_with_the_old_credentials_message") {
        return;
    }
    let fixture = fixture();
    let server = serve(&fixture.bare, false);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);

    // As it failed with no prompt.
    let unprompted = match open(&fixture.work, None).fetch(false) {
        Err(error @ RemoteError::Auth { .. }) => error.to_string(),
        other => panic!("expected Auth, got {other:?}"),
    };
    let (askpass, asked) = recording(&fixture.state, None);
    match open(&fixture.work, Some(askpass)).fetch(false) {
        Err(error @ RemoteError::Auth { .. }) => {
            let (cancelled, before) = (error.to_string(), unprompted.lines().next());
            assert_eq!(cancelled.lines().next(), before, "{cancelled}");
            assert!(cancelled.contains("prompt was cancelled"), "{cancelled}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
    // Asked once, then nothing more.
    assert_eq!(asked.lock().unwrap().len(), 1);
    assert!(server.authorizations.lock().unwrap().is_empty());
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());

    // Ssh's too, as batch mode refused it.
    let ssh_url = format!("ssh://host{}", fixture.bare.display());
    git(&fixture.work, &["remote", "set-url", "origin", &ssh_url]);
    let (askpass, asked) = recording(&fixture.state, None);
    match open(&fixture.work, Some(askpass)).fetch(false) {
        Err(RemoteError::Auth { output }) => {
            assert!(output.contains("prompt was cancelled"), "{output}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
    assert_eq!(asked.lock().unwrap().len(), 1);
}

#[test]
fn no_prompt_is_attempted_when_askpass_is_unset_by_the_caller() {
    if !isolated("no_prompt_is_attempted_when_askpass_is_unset_by_the_caller") {
        return;
    }
    let fixture = fixture();
    let server = serve(&fixture.bare, false);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    let mut repo = open(&fixture.work, Some(askpass));
    repo.set_askpass(None);

    match repo.fetch(false) {
        Err(RemoteError::Auth { output }) => {
            assert!(output.contains("could not read Username"), "{output}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
    // Ssh stays in batch mode, which the fake refuses.
    let ssh_url = format!("ssh://host{}", fixture.bare.display());
    git(&fixture.work, &["remote", "set-url", "origin", &ssh_url]);
    match repo.fetch(false) {
        Err(RemoteError::Auth { output }) => {
            assert!(output.contains("Permission denied (batch)"), "{output}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
    assert!(asked.lock().unwrap().is_empty());
    assert!(!fixture.state.exists(), "{:?}", left_in(&fixture.state));
}

#[test]
fn nothing_secret_is_logged_or_in_the_toast() {
    if !isolated("nothing_secret_is_logged_or_in_the_toast") {
        return;
    }
    let fixture = fixture();
    // It takes no credentials, so git says what it was refused.
    let server = serve(&fixture.bare, true);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    assert!(!format!("{askpass:?}").contains(SECRET));

    let error = open(&fixture.work, Some(askpass)).fetch(false).unwrap_err();
    assert!(matches!(error, RemoteError::Auth { .. }), "{error:?}");
    // The toast's sentence and detail come from these.
    for shown in [error.to_string(), format!("{error:?}")] {
        assert!(shown.contains("Authentication failed"), "{shown}");
        assert!(!shown.contains(SECRET), "{shown}");
    }
    assert_eq!(asked.lock().unwrap().len(), 2);
    assert!(!server.authorizations.lock().unwrap().is_empty());
    // Nothing's left on disk, the socket and script included, and nothing
    // in the repository has it.
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
    let mut dirs = vec![fixture.work.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                let text = String::from_utf8_lossy(&bytes);
                assert!(!text.contains(SECRET), "{}", path.display());
            }
        }
    }
}

#[test]
fn prompted_answers_arent_stored_and_stored_ones_still_answer() {
    if !isolated("prompted_answers_arent_stored_and_stored_ones_still_answer") {
        return;
    }
    let fixture = fixture();
    let server = serve(&fixture.bare, false);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let store = fixture.dir.path().join("credentials");
    let helper = format!("store --file={}", store.display());
    git(&fixture.work, &["config", "credential.helper", &helper]);

    // Asked for, and taken, but not handed to the helper to keep.
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    open(&fixture.work, Some(askpass)).fetch(false).unwrap();
    assert_eq!(asked.lock().unwrap().len(), 2);
    let stored = fs::read_to_string(&store).unwrap_or_default();
    assert!(!stored.contains(SECRET), "stored: {stored}");

    // What the helper does keep still answers, with no prompt.
    fs::write(
        &store,
        format!("http://{USER}:{SECRET}@127.0.0.1:{}\n", server.port),
    )
    .unwrap();
    let (askpass, asked) = recording(&fixture.state, None);
    open(&fixture.work, Some(askpass)).fetch(false).unwrap();
    assert!(asked.lock().unwrap().is_empty());
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
}

/// OpenSSH takes a helper that fails as an empty answer, then retries or
/// tries another key: cancelling stops it before it can.
#[test]
fn a_cancel_stops_ssh_retrying_or_trying_another_key() {
    if !isolated("a_cancel_stops_ssh_retrying_or_trying_another_key") {
        return;
    }
    let fixture = fixture();
    let url = format!("ssh://retry{}", fixture.bare.display());
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let (askpass, asked) = recording(&fixture.state, None);

    match open(&fixture.work, Some(askpass)).fetch(false) {
        Err(RemoteError::Auth { output }) => {
            assert!(output.contains("prompt was cancelled"), "{output}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
    assert_eq!(asked.lock().unwrap().len(), 1);
    let fetched = Command::new("git")
        .args(["rev-parse", "--verify", "-q", "origin/main"])
        .current_dir(&fixture.work)
        .output()
        .unwrap();
    assert!(!fetched.status.success(), "the other key got in");
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
}

/// A helper stopped unanswered withdraws its request, without cancelling
/// the command, and ssh's notice asks nothing.
#[test]
fn a_helper_gone_withdraws_its_request_and_a_notice_asks_nothing() {
    if !isolated("a_helper_gone_withdraws_its_request_and_a_notice_asks_nothing") {
        return;
    }
    let fixture = fixture();
    let url = format!("ssh://vanish{}", fixture.bare.display());
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let askpass = Askpass::new(
        helper(),
        fixture.state.clone(),
        move |request, withdrawn| {
            // An answer that never comes, until there's no one to take it.
            let start = std::time::Instant::now();
            while !withdrawn.is_withdrawn() && start.elapsed() < std::time::Duration::from_secs(10)
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let told = Arc::new(Mutex::new(false));
            let heard = told.clone();
            withdrawn.on_withdrawn(move || *heard.lock().unwrap() = true);
            seen.lock()
                .unwrap()
                .push((request.prompt.clone(), *told.lock().unwrap()));
            None
        },
    );

    open(&fixture.work, Some(askpass)).fetch(false).unwrap();
    // Heard on the session's thread, which may be just behind.
    for _ in 0..500 {
        if !asked.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        *asked.lock().unwrap(),
        [("Enter passphrase for key '/k/id': ".to_owned(), true)]
    );
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
}

/// Git runs the scripts from the working tree, so a state directory given
/// relative to somewhere else still works.
#[test]
fn a_relative_state_dir_works_from_the_working_tree() {
    if !isolated("a_relative_state_dir_works_from_the_working_tree") {
        return;
    }
    let fixture = fixture();
    let server = serve(&fixture.bare, false);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);
    // The test runs alone in this process.
    std::env::set_current_dir(fixture.dir.path()).unwrap();
    let (askpass, asked) = recording(Path::new("state"), Some(SECRET));

    open(&fixture.work, Some(askpass)).fetch(false).unwrap();
    assert_eq!(asked.lock().unwrap().len(), 2);
    assert!(fixture.state.is_dir());
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
}

/// A helper's partial answer is used, git asking only for the rest, and its
/// quit is kept, as without the session's own helper in between.
#[test]
fn a_helpers_partial_answer_and_quit_are_kept() {
    if !isolated("a_helpers_partial_answer_and_quit_are_kept") {
        return;
    }
    let fixture = fixture();
    let server = serve(&fixture.bare, false);
    let url = format!("http://127.0.0.1:{}/x.git", server.port);
    git(&fixture.work, &["remote", "add", "origin", &url]);
    let host = format!("127.0.0.1:{}", server.port);
    let helper = |answers: &str| {
        let helper = format!("!f() {{ test \"$1\" = get && printf '{answers}'; }}; f");
        git(&fixture.work, &["config", "credential.helper", &helper]);
    };

    helper(&format!("username={USER}\\n"));
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    open(&fixture.work, Some(askpass)).fetch(false).unwrap();
    let prompts: Vec<String> = (asked.lock().unwrap().iter())
        .map(|request| request.prompt.clone())
        .collect();
    assert_eq!(prompts, [format!("Password for 'http://{USER}@{host}': ")]);

    helper(&format!("password={SECRET}\\n"));
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    open(&fixture.work, Some(askpass)).fetch(false).unwrap();
    let prompts: Vec<String> = (asked.lock().unwrap().iter())
        .map(|request| request.prompt.clone())
        .collect();
    assert_eq!(prompts, [format!("Username for 'http://{host}': ")]);

    helper("quit=1\\n");
    let (askpass, asked) = recording(&fixture.state, Some(SECRET));
    let error = open(&fixture.work, Some(askpass)).fetch(false).unwrap_err();
    assert!(error.to_string().contains("told us to quit"), "{error}");
    assert!(asked.lock().unwrap().is_empty());
    assert_eq!(left_in(&fixture.state), Vec::<PathBuf>::new());
}
