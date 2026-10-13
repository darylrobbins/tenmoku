//! Pulls and pushes never sign, submodules included, while signature
//! verification keeps working.
//!
//! The library's git calls inherit the process environment, so each test runs
//! again in a child process whose git sees no global or system config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{PullMode, RemoteError, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = run_git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .current_dir(dir)
        // Config a test gives the library through the environment.
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git")
}

fn commit(root: &Path, rela: &str, text: &str) {
    fs::write(root.join(rela), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", text]);
}

fn open(root: &Path) -> Repo {
    Repo::discover(root).unwrap().expect("a repository")
}

fn rev(root: &Path, name: &str) -> String {
    git(root, &["rev-parse", name])
}

/// Set in the child process a test reruns itself in.
const ISOLATED: &str = "FE_GIT_REMOTE_TEST_ISOLATED";

/// `true` in the child process, where the test's body runs. In the test
/// process itself, runs test `name` in a child whose git sees no global or
/// system config, no proxy and no ssh command of the user's, fails if it
/// fails, and returns `false`.
fn isolated(name: &str) -> bool {
    isolated_with(name, &[])
}

/// Like [`isolated`], with `envs` set in the child too.
fn isolated_with(name: &str, envs: &[(&str, &str)]) -> bool {
    if std::env::var_os(ISOLATED).is_some() {
        return true;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(ISOLATED, "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .envs(envs.iter().copied());
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

/// Clones `a` and `b` of a bare `origin.git` whose `main` has file f.
struct Remotes {
    _dir: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
}

fn remotes() -> Remotes {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("seed");
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-q"]);
    commit(&seed, "f", "one\n");
    git(dir.path(), &["clone", "-q", "--bare", "seed", "origin.git"]);
    for clone in ["a", "b"] {
        git(dir.path(), &["clone", "-q", "origin.git", clone]);
    }
    Remotes {
        a: dir.path().join("a"),
        b: dir.path().join("b"),
        _dir: dir,
    }
}

/// A signer at `dir/signer` that records it ran (in `dir/signer-ran`, the
/// path returned) and fails.
#[cfg(unix)]
fn sentinel_signer(dir: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let signer = dir.join("signer");
    let ran = dir.join("signer-ran");
    fs::write(
        &signer,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display()),
    )
    .unwrap();
    fs::set_permissions(&signer, fs::Permissions::from_mode(0o755)).unwrap();
    (signer, ran)
}

/// A bare `<name>.git` in `dir` with one commit on `main`.
fn bare(dir: &Path, name: &str) -> PathBuf {
    let seed = dir.join(format!("{name}-seed"));
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-q"]);
    commit(&seed, "s", &format!("{name} 1\n"));
    let bare = format!("{name}.git");
    git(
        dir,
        &["clone", "-q", "--bare", &format!("{name}-seed"), &bare],
    );
    dir.join(bare)
}

fn refs(repo: &Path) -> String {
    git(repo, &["for-each-ref"])
}

fn expect_failed(result: Result<(), RemoteError>, what: &str) {
    assert!(
        matches!(result, Err(RemoteError::Failed { .. })),
        "{what}: expected Failed, got {result:?}"
    );
}

/// Superproject `a` of [`Remotes`] with submodules `sub` and `plain`, each
/// with a commit on `main` that its remote lacks, recorded in a commit on
/// `a`'s `main` that origin lacks. `sub`'s pushes would sign, with an SSH key
/// it has to ask a command for, and every program that could run is a
/// sentinel signer; `plain` has no signing config.
#[cfg(unix)]
struct Submodules {
    r: Remotes,
    sub: PathBuf,
    plain: PathBuf,
    sub_url: PathBuf,
    plain_url: PathBuf,
    sub_refs: String,
    plain_refs: String,
    ran: PathBuf,
}

#[cfg(unix)]
fn submodules() -> Submodules {
    let r = remotes();
    let dir = r.a.parent().unwrap().to_owned();
    let (signer, ran) = sentinel_signer(&dir);

    // sub.git asks for signed pushes.
    let sub_url = bare(&dir, "sub");
    git(&sub_url, &["config", "receive.certNonceSeed", "seed"]);
    let plain_url = bare(&dir, "plain");
    for (url, path) in [(&sub_url, "sub"), (&plain_url, "plain")] {
        git(
            &r.b,
            &["submodule", "add", "-q", url.to_str().unwrap(), path],
        );
    }
    git(&r.b, &["commit", "-q", "-m", "add submodules"]);
    git(&r.b, &["push", "-q"]);
    git(&r.a, &["pull", "-q", "--ff-only"]);
    git(&r.a, &["submodule", "update", "-q", "--init"]);

    let sub = r.a.join("sub");
    let plain = r.a.join("plain");
    let signer = signer.to_str().unwrap();
    for (key, value) in [
        ("push.gpgSign", "true"),
        ("gpg.format", "ssh"),
        ("gpg.ssh.defaultKeyCommand", signer),
        ("gpg.ssh.program", signer),
        ("gpg.program", signer),
    ] {
        git(&sub, &["config", key, value]);
    }
    assert!(
        run_git(&sub, &["config", "--get", "user.signingKey"])
            .stdout
            .is_empty()
    );
    for repo in [&sub, &plain] {
        git(repo, &["config", "protocol.file.allow", "always"]);
        git(repo, &["switch", "-q", "-C", "main"]);
        commit(repo, "t", "from a\n");
    }
    git(&r.a, &["commit", "-q", "-am", "submodules at a"]);
    Submodules {
        sub_refs: refs(&sub_url),
        plain_refs: refs(&plain_url),
        r,
        sub,
        plain,
        sub_url,
        plain_url,
        ran,
    }
}

#[cfg(unix)]
impl Submodules {
    /// Switches the superproject and both submodules to `branch`, making it
    /// when `create`.
    fn switch(&self, branch: &str, create: bool) {
        let args: &[&str] = if create {
            &["switch", "-q", "-c", branch]
        } else {
            &["switch", "-q", branch]
        };
        for repo in [&self.r.a, &self.sub, &self.plain] {
            git(repo, args);
        }
    }

    /// `push(false)` on main and `publish("origin")` on new branch `branch`
    /// are `Ok` when `ok`, else [`RemoteError::Failed`]; neither pushes a
    /// submodule or runs the sentinel.
    fn push_and_publish(&self, what: &str, branch: &str, ok: bool) {
        let a = open(&self.r.a);
        let expect = |result: Result<(), RemoteError>, call: &str| {
            let what = format!("{what}: {call}");
            if ok {
                assert!(result.is_ok(), "{what}: expected Ok, got {result:?}");
            } else {
                expect_failed(result, &what);
            }
        };
        self.switch("main", false);
        expect(a.push(false), "push");
        self.switch(branch, true);
        expect(a.publish("origin"), "publish");
        assert_eq!(refs(&self.sub_url), self.sub_refs, "{what}");
        assert_eq!(refs(&self.plain_url), self.plain_refs, "{what}");
        assert!(!self.ran.exists(), "{what}");
    }
}

/// A push that would push submodules first pushes none of them: nothing runs
/// in a submodule whose config would sign, or find a signing key, and a
/// submodule with no signing config isn't pushed either. Git reads
/// `push.recurseSubmodules` and `submodule.recurse` in config order, the last
/// one deciding, and so does the check.
#[cfg(unix)]
#[test]
fn recursive_push_runs_nothing_in_submodules() {
    if !isolated("recursive_push_runs_nothing_in_submodules") {
        return;
    }
    let s = submodules();
    let config = s.r.a.join(".git/config");
    let base = fs::read_to_string(&config).unwrap();
    let recursing = [
        (
            "push.recurseSubmodules=on-demand",
            "[push]\n\trecurseSubmodules = on-demand\n",
        ),
        (
            "push.recurseSubmodules=only",
            "[push]\n\trecurseSubmodules = only\n",
        ),
        ("submodule.recurse=true", "[submodule]\n\trecurse = true\n"),
        (
            "push.recurseSubmodules=no, then submodule.recurse=true",
            "[push]\n\trecurseSubmodules = no\n[submodule]\n\trecurse = true\n",
        ),
        (
            "submodule.recurse=false, then push.recurseSubmodules=on-demand",
            "[submodule]\n\trecurse = false\n[push]\n\trecurseSubmodules = on-demand\n",
        ),
    ];
    for (i, (what, text)) in recursing.into_iter().enumerate() {
        fs::write(&config, format!("{base}{text}")).unwrap();
        s.push_and_publish(what, &format!("feat{i}"), false);
    }
    // Pushes, as git doesn't recurse; last as it puts main on origin.
    fs::write(
        &config,
        format!("{base}[push]\n\trecurseSubmodules = on-demand\n[submodule]\n\trecurse = false\n"),
    )
    .unwrap();
    s.push_and_publish(
        "push.recurseSubmodules=on-demand, then submodule.recurse=false",
        "plain",
        true,
    );
}

/// Config from the environment comes after the repository's, as git reads it.
#[cfg(unix)]
#[test]
fn recursive_push_obeys_env_config() {
    let envs = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "submodule.recurse"),
        ("GIT_CONFIG_VALUE_0", "true"),
    ];
    if !isolated_with("recursive_push_obeys_env_config", &envs) {
        return;
    }
    let s = submodules();
    git(&s.r.a, &["config", "push.recurseSubmodules", "no"]);
    s.push_and_publish("env submodule.recurse=true", "feat", false);
}

/// With `merge.verifySignatures`, a pull of commits signed by a trusted key
/// goes ahead.
#[test]
fn pull_verifies_signatures() {
    if !isolated("pull_verifies_signatures") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let dir = r.a.parent().unwrap();
    let key = dir.join("key");
    let keygen = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "t@t", "-f"])
        .arg(&key)
        .output();
    let keygen = match keygen {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("skipped: no ssh-keygen");
            return;
        }
        Err(error) => panic!("run ssh-keygen: {error}"),
    };
    assert!(
        keygen.status.success(),
        "ssh-keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );
    let public = fs::read_to_string(dir.join("key.pub")).unwrap();
    let signers = dir.join("allowed_signers");
    fs::write(&signers, format!("t@t {public}")).unwrap();

    for (key, value) in [
        ("gpg.format", "ssh"),
        ("user.signingKey", key.to_str().unwrap()),
        ("commit.gpgSign", "true"),
    ] {
        git(&r.b, &["config", key, value]);
    }
    for (key, value) in [
        ("gpg.format", "ssh"),
        ("gpg.ssh.allowedSignersFile", signers.to_str().unwrap()),
        ("merge.verifySignatures", "true"),
    ] {
        git(&r.a, &["config", key, value]);
    }
    // The helpers turn commit.gpgSign off, so B asks for a signature.
    let signed_commit = |rela: &str, text: &str| {
        fs::write(r.b.join(rela), text).unwrap();
        git(&r.b, &["add", "."]);
        git(&r.b, &["commit", "-q", "-S", "-m", text]);
        git(&r.b, &["push", "-q"]);
    };

    signed_commit("g", "signed by b\n");
    a.pull(PullMode::FastForwardOnly).unwrap();
    assert_eq!(rev(&r.a, "HEAD"), rev(&r.b, "HEAD"));
    assert_eq!(git(&r.a, &["log", "-1", "--format=%G?"]), "G");

    signed_commit("g", "signed again by b\n");
    a.pull(PullMode::Merge).unwrap();
    assert_eq!(rev(&r.a, "HEAD"), rev(&r.b, "HEAD"));
}

/// A merge that `branch.<name>.mergeOptions` asks to sign, in a spelling the
/// preflight doesn't catch, still never runs the signer.
#[cfg(unix)]
#[test]
fn pull_never_signs() {
    if !isolated("pull_never_signs") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let (signer, ran) = sentinel_signer(r.a.parent().unwrap());
    for (key, value) in [
        ("gpg.program", signer.to_str().unwrap()),
        ("commit.gpgSign", "false"),
        ("branch.main.mergeOptions", "\"-S\""),
    ] {
        git(&r.a, &["config", key, value]);
    }
    commit(&r.a, "h", "from a\n");
    commit(&r.b, "g", "from b\n");
    git(&r.b, &["push", "-q"]);

    let result = a.pull(PullMode::Merge);
    assert!(!ran.exists(), "the signer ran; pull gave {result:?}");
    assert!(
        !r.a.join(".git/MERGE_HEAD").exists(),
        "pull gave {result:?}"
    );
}

/// A rebase with the apply backend runs `git am`, which reads `commit.gpgSign`
/// itself, and still never runs the signer.
#[cfg(unix)]
#[test]
fn pull_rebase_apply_never_signs() {
    if !isolated("pull_rebase_apply_never_signs") {
        return;
    }
    let r = remotes();
    let a = open(&r.a);
    let (signer, ran) = sentinel_signer(r.a.parent().unwrap());
    for (key, value) in [
        ("gpg.program", signer.to_str().unwrap()),
        ("commit.gpgSign", "true"),
        ("branch.main.mergeOptions", "--no-gpg-sign"),
        ("rebase.backend", "apply"),
    ] {
        git(&r.a, &["config", key, value]);
    }
    commit(&r.a, "h", "from a\n");
    commit(&r.b, "g", "from b\n");
    git(&r.b, &["push", "-q"]);

    let result = a.pull(PullMode::Rebase);
    assert!(!ran.exists(), "the signer ran; pull gave {result:?}");
    assert!(
        !r.a.join(".git/rebase-apply").exists(),
        "pull gave {result:?}"
    );
}
