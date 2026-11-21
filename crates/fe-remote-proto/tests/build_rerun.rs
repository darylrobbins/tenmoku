//! The build id through cargo's own rerun check: a throwaway crate built
//! with this build script, in a throwaway repository, rebuilt after each
//! change to the tree. The id it prints must be what the tree is now, never
//! what it was at the last run (D § 2.5).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Probe {
    repo: tempfile::TempDir,
    target: tempfile::TempDir,
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn cargo() -> Command {
    let cargo = PathBuf::from(env!("CARGO"));
    let mut cmd = Command::new(&cargo);
    // The probe builds with the toolchain building this test, uncached.
    let rustc = cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    if rustc.exists() {
        cmd.env("RUSTC", rustc);
    }
    cmd.env_remove("RUSTC_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .env_remove("TENMOKU_BUILD_COMMIT");
    cmd
}

impl Probe {
    /// A committed crate whose binary prints its `TENMOKU_BUILD`, with a
    /// top-level README and a `src` directory.
    fn new() -> Probe {
        let probe = Probe {
            repo: tempfile::tempdir().unwrap(),
            target: tempfile::tempdir().unwrap(),
        };
        let files = [
            (
                "Cargo.toml",
                "[package]\nname = \"probe\"\nversion = \"1.2.3\"\nedition = \"2024\"\n\n[workspace]\n",
            ),
            ("build.rs", include_str!("../build.rs")),
            ("build_id.rs", include_str!("../build_id.rs")),
            (
                "src/main.rs",
                "fn main() {\n    print!(\"{}\", env!(\"TENMOKU_BUILD\"));\n}\n",
            ),
            ("README.md", "probe\n"),
        ];
        for (path, text) in files {
            probe.write(path, text);
        }
        git(probe.dir(), &["init", "-q", "-b", "main"]);
        let lock = cargo()
            .args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(probe.dir().join("Cargo.toml"))
            .status()
            .unwrap();
        assert!(lock.success());
        probe.commit("one");
        probe
    }

    fn dir(&self) -> &Path {
        self.repo.path()
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.dir().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn commit(&self, message: &str) {
        git(self.dir(), &["add", "-A"]);
        git(self.dir(), &["commit", "-q", "-m", message]);
    }

    /// Builds the probe if cargo thinks it's stale, and returns the id the
    /// binary was built with.
    fn build(&self) -> String {
        let out = cargo()
            .args(["run", "-q", "--offline", "--manifest-path"])
            .arg(self.dir().join("Cargo.toml"))
            .arg("--target-dir")
            .arg(self.target.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

fn is_clean_id(id: &str) -> bool {
    id.strip_prefix("1.2.3+")
        .is_some_and(|c| c.len() == 12 && c.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[test]
fn a_committed_tree_builds_with_its_commit() {
    let probe = Probe::new();
    assert!(is_clean_id(&probe.build()));
}

#[test]
fn an_unstaged_edit_and_its_revert_rerun_the_script() {
    let probe = Probe::new();
    let clean = probe.build();
    let main = "fn main() {\n    print!(\"{}\", env!(\"TENMOKU_BUILD\"));\n}\n";
    probe.write("src/main.rs", &format!("// edited\n{main}"));
    assert_eq!(probe.build(), format!("{clean}-dirty"));
    probe.write("src/main.rs", main);
    assert_eq!(probe.build(), clean);
}

#[test]
fn deleting_and_restoring_a_tracked_top_level_file_reruns_the_script() {
    let probe = Probe::new();
    let clean = probe.build();
    fs::remove_file(probe.dir().join("README.md")).unwrap();
    assert_eq!(probe.build(), format!("{clean}-dirty"));
    probe.write("README.md", "probe\n");
    assert_eq!(probe.build(), clean);
}

#[test]
fn an_untracked_file_in_a_tracked_directory_reruns_the_script() {
    let probe = Probe::new();
    let clean = probe.build();
    probe.write("src/new.rs", "");
    assert_eq!(probe.build(), format!("{clean}-dirty"));
    fs::remove_file(probe.dir().join("src/new.rs")).unwrap();
    assert_eq!(probe.build(), clean);
}

/// The script doesn't watch the work tree's root (that would scan the
/// target directory too), so an untracked top-level entry must not count
/// towards `-dirty`: a forced rerun agrees with the build that missed it.
#[test]
fn an_untracked_top_level_file_leaves_the_id_alone() {
    let probe = Probe::new();
    let clean = probe.build();
    probe.write("NOTES.txt", "scratch\n");
    assert_eq!(probe.build(), clean);
    probe.write("build.rs", include_str!("../build.rs"));
    assert_eq!(probe.build(), clean);
}

#[test]
fn a_commit_reruns_the_script() {
    let probe = Probe::new();
    let first = probe.build();
    probe.write("README.md", "probe, two\n");
    probe.commit("two");
    let second = probe.build();
    assert!(is_clean_id(&second) && second != first, "{second}");
}

#[test]
fn moving_a_packed_branch_reruns_the_script() {
    let probe = Probe::new();
    let first = probe.build();
    probe.write("README.md", "probe, two\n");
    probe.commit("two");
    git(probe.dir(), &["pack-refs", "--all"]);
    let second = probe.build();
    assert_ne!(second, first);
    git(probe.dir(), &["reset", "-q", "--hard", "HEAD~1"]);
    assert_eq!(probe.build(), first);
}
