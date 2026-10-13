//! Discarding many short paths when the argument space is at its 128 KiB floor.

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use fe_git::Repo;

#[cfg(target_os = "linux")]
const TEST: &str = "discard_many_short_paths_under_a_small_arg_max";
#[cfg(target_os = "linux")]
const CHILD: &str = "FE_GIT_SMALL_ARG_MAX_CHILD";

#[cfg(target_os = "linux")]
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn discard_many_short_paths_under_a_small_arg_max() {
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new("sh")
            .args(["-c", r#"ulimit -s 512 && exec "$0" "$@""#])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .expect("run sh");
        assert!(
            output.status.success(),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let out = Command::new("getconf").arg("ARG_MAX").output().unwrap();
    let arg_max: usize = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    assert!(arg_max <= 131072, "ulimit did not take: ARG_MAX {arg_max}");

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    git(&root, &["init", "-q"]);
    let tracked: Vec<_> = (0..12_000).map(|n| root.join(format!("{n:04x}"))).collect();
    for path in &tracked {
        fs::write(path, "one\n").unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    for path in &tracked {
        fs::write(path, "two\n").unwrap();
    }
    let loose: Vec<_> = (0..3).map(|n| root.join(format!("loose{n}.txt"))).collect();
    for path in &loose {
        fs::write(path, "loose\n").unwrap();
    }
    let mut paths = tracked.clone();
    paths.extend(loose.iter().cloned());

    let repo = Repo::discover(&root).unwrap().expect("a repository");
    repo.discard(&paths).unwrap();
    assert!(repo.status_split().unwrap().is_empty());
    assert!(
        tracked
            .iter()
            .all(|path| fs::read_to_string(path).unwrap() == "one\n")
    );
    assert!(loose.iter().all(|path| !path.exists()));
}
