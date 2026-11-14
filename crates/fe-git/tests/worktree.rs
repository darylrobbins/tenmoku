//! Linked worktrees and the revisions frozen from them under ref
//! namespaces, against temp repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fe_git::{ChangeKind, FileStatus, RefNamespace, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repo on `main` with `a.txt` and a `.gitignore` (ignoring `*.log`)
/// committed, under `<tempdir>/main`, so lane worktrees can sit beside it.
fn init() -> (tempfile::TempDir, PathBuf, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap().join("main");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    fs::write(root.join("a.txt"), "one\n").unwrap();
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let repo = Repo::discover(&root).unwrap().expect("a repository");
    (dir, root, repo)
}

fn namespace(prefix: &str) -> RefNamespace {
    RefNamespace::new(prefix).expect("a valid namespace")
}

/// Every ref under `refs/tenmoku/`, as `<name> <id>` lines.
fn tenmoku_refs(root: &Path) -> Vec<String> {
    let out = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/tenmoku/",
        ],
    );
    out.lines().map(str::to_owned).collect()
}

/// `path` in `commit`'s tree, as git shows it.
fn show(root: &Path, commit: &str, path: &str) -> String {
    git(root, &["show", &format!("{commit}:{path}")])
}

/// Adds a lane worktree beside `root`, detached at HEAD.
fn add_lane(repo: &Repo, root: &Path, name: &str) -> (PathBuf, Repo) {
    let path = root.parent().unwrap().join(name);
    let head = repo.head_id().unwrap();
    repo.add_worktree_detached(&path, &head).unwrap();
    let lane = Repo::discover(&path).unwrap().expect("the lane's worktree");
    (path, lane)
}

#[test]
fn add_list_and_remove_a_detached_worktree() {
    let (_dir, root, repo) = init();
    let head = repo.head_id().unwrap();
    let (path, lane) = add_lane(&repo, &root, "lane-2");
    assert_eq!(lane.workdir(), path);
    assert_eq!(lane.head_id().as_deref(), Some(head.as_str()));
    assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "one\n");

    let worktrees = repo.worktrees().unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0].path, root);
    assert_eq!(worktrees[0].branch.as_deref(), Some("refs/heads/main"));
    assert!(!worktrees[0].detached);
    assert_eq!(worktrees[1].path, path);
    assert!(worktrees[1].detached);
    assert_eq!(worktrees[1].branch, None);
    assert_eq!(worktrees[1].head.as_deref(), Some(head.as_str()));

    // A lane's worktree has untracked work: only a forced remove takes it.
    fs::write(path.join("scratch.txt"), "x\n").unwrap();
    assert!(repo.remove_worktree(&path, false).is_err());
    repo.remove_worktree(&path, true).unwrap();
    assert!(!path.exists());
    assert_eq!(repo.worktrees().unwrap().len(), 1);
}

/// A worktree whose directory is gone is removed alone: another gone one
/// keeps its registration, a locked one isn't removed, and a path that was
/// never a worktree is an error.
#[test]
fn removing_a_worktree_whose_directory_is_gone_removes_only_it() {
    let (_dir, root, repo) = init();
    let (path, _lane) = add_lane(&repo, &root, "lane-2");
    let (other, _other) = add_lane(&repo, &root, "lane-3");
    let (locked, _locked) = add_lane(&repo, &root, "lane-4");
    git(&root, &["worktree", "lock", locked.to_str().unwrap()]);
    for gone in [&path, &other, &locked] {
        fs::remove_dir_all(gone).unwrap();
    }
    assert!(repo.worktrees().unwrap()[1].prunable);

    repo.remove_worktree(&path, true).unwrap();
    let paths = |repo: &Repo| -> Vec<PathBuf> {
        (repo.worktrees().unwrap().into_iter())
            .map(|worktree| worktree.path)
            .collect()
    };
    assert_eq!(paths(&repo), [root.clone(), other.clone(), locked.clone()]);

    assert!(repo.remove_worktree(&locked, true).is_err());
    assert!(
        repo.remove_worktree(&root.parent().unwrap().join("never"), true)
            .is_err()
    );
    assert_eq!(paths(&repo), [root, other, locked]);
}

/// Flags that make git skip a file on disk are cleared in the temporary
/// index only: a tracked file marked assume-unchanged or skip-worktree is
/// frozen as it is on disk, a skip-worktree one with no file (outside a
/// sparse checkout) as the index has it, and the real index keeps its flags.
#[test]
fn freeze_reads_files_marked_assume_unchanged_or_skip_worktree() {
    let (_dir, root, repo) = init();
    for name in ["b.txt", "c.txt", "sparse.txt"] {
        fs::write(root.join(name), "one\n").unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "more"]);
    git(&root, &["update-index", "--assume-unchanged", "a.txt"]);
    git(&root, &["update-index", "--skip-worktree", "b.txt"]);
    git(
        &root,
        &[
            "update-index",
            "--assume-unchanged",
            "--skip-worktree",
            "c.txt",
        ],
    );
    git(&root, &["update-index", "--skip-worktree", "sparse.txt"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.join(name), "changed\n").unwrap();
    }
    fs::remove_file(root.join("sparse.txt")).unwrap();
    let flags_before = git(&root, &["ls-files", "-v"]);

    let revision = (repo.freeze(&namespace("refs/tenmoku/lanes/k1/1"), 1, None, "r1")).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(show(&root, &revision.commit, name), "changed", "{name}");
    }
    assert_eq!(show(&root, &revision.commit, "sparse.txt"), "one");
    assert_eq!(git(&root, &["ls-files", "-v"]), flags_before);
    assert_eq!(repo.worktree_tree().unwrap(), revision.tree);
}

/// In a sparse checkout, a modified tracked file and an untracked file
/// outside the cone, both on disk, are frozen; entries absent from disk stay
/// as the index has them (unchanged, so not in the changed-file list); the
/// real index and its flags are untouched.
#[test]
fn freeze_captures_files_outside_a_sparse_cone() {
    let (_dir, root, repo) = init();
    for dir in ["in", "out"] {
        fs::create_dir(root.join(dir)).unwrap();
        for name in ["x.txt", "y.txt"] {
            fs::write(root.join(dir).join(name), "one\n").unwrap();
        }
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "dirs"]);
    git(
        &root,
        &["sparse-checkout", "init", "--cone", "--no-sparse-index"],
    );
    git(&root, &["sparse-checkout", "set", "in"]);
    assert!(!root.join("out").exists(), "out is outside the cone");
    // Materialize two files outside the cone: one tracked, one untracked.
    fs::create_dir(root.join("out")).unwrap();
    fs::write(root.join("out/x.txt"), "changed\n").unwrap();
    fs::write(root.join("out/new.txt"), "new\n").unwrap();
    let head = repo.head_id().unwrap();
    let flags_before = git(&root, &["ls-files", "-v"]);
    let index_before = fs::read(root.join(".git/index")).unwrap();

    let revision = repo
        .freeze(&namespace("refs/tenmoku/lanes/k1/1"), 1, Some(&head), "r1")
        .unwrap();

    assert_eq!(show(&root, &revision.commit, "out/x.txt"), "changed");
    assert_eq!(show(&root, &revision.commit, "out/new.txt"), "new");
    assert_eq!(show(&root, &revision.commit, "out/y.txt"), "one");
    let changed = |kind, name: &str| FileStatus {
        path: root.join(name),
        kind,
    };
    assert_eq!(
        repo.changed_files(&head, &revision.commit).unwrap(),
        [
            changed(ChangeKind::Added, "out/new.txt"),
            changed(ChangeKind::Modified, "out/x.txt"),
        ]
    );
    assert!(!root.join("out/y.txt").exists());
    assert_eq!(git(&root, &["ls-files", "-v"]), flags_before);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
}

/// The real index split, with every write making a new shared index and
/// expiring the old at once: freezing (which clears a flag, stages and
/// writes a tree on its temporary index) leaves the real index and the
/// shared index it names as they were.
#[test]
fn freeze_leaves_a_split_real_index_and_its_shared_index() {
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.splitIndex", "true"]);
    git(&root, &["config", "splitIndex.maxPercentChange", "0"]);
    git(&root, &["config", "splitIndex.sharedIndexExpire", "now"]);
    git(&root, &["update-index", "--split-index"]);
    git(&root, &["update-index", "--assume-unchanged", "a.txt"]);
    let git_dir = root.join(".git");
    let shared = || -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = (fs::read_dir(&git_dir).unwrap())
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                (path.file_name().unwrap().to_str().unwrap()).starts_with("sharedindex.")
            })
            .collect();
        files.sort();
        files
    };
    let shared_before = shared();
    assert!(!shared_before.is_empty());
    // Old enough for any expiry to take them.
    let then = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for file in &shared_before {
        let file = fs::File::options().write(true).open(file).unwrap();
        file.set_modified(then).unwrap();
    }
    let index_before = fs::read(git_dir.join("index")).unwrap();
    fs::write(root.join("a.txt"), "changed\n").unwrap();

    let revision = (repo.freeze(&namespace("refs/tenmoku/lanes/k1/1"), 1, None, "r1")).unwrap();
    assert_eq!(show(&root, &revision.commit, "a.txt"), "changed");
    assert_eq!(shared(), shared_before);
    assert_eq!(fs::read(git_dir.join("index")).unwrap(), index_before);
    git(&root, &["ls-files", "--error-unmatch", "a.txt"]);
}

/// A symbolic ref in a namespace, pointing at another namespace's revision
/// or at a branch, is replaced or deleted itself; its target stays.
#[test]
fn symbolic_refs_in_a_namespace_never_reach_their_targets() {
    let (_dir, root, repo) = init();
    let head = repo.head_id().unwrap();
    let mine = namespace("refs/tenmoku/lanes/rootA/2");
    let theirs = namespace("refs/tenmoku/lanes/rootB/2");
    let b1 = repo.freeze(&theirs, 1, None, "b r1").unwrap();
    git(&root, &["branch", "keep"]);
    git(
        &root,
        &[
            "symbolic-ref",
            &mine.revision_ref(1),
            &theirs.revision_ref(1),
        ],
    );
    git(
        &root,
        &["symbolic-ref", &mine.revision_ref(2), "refs/heads/keep"],
    );

    fs::write(root.join("a.txt"), "two\n").unwrap();
    let a1 = repo.freeze(&mine, 1, None, "a r1").unwrap();
    assert_eq!(repo.revisions(&theirs).unwrap(), [(1, b1.commit.clone())]);
    assert_eq!(git(&root, &["rev-parse", &mine.revision_ref(1)]), a1.commit);

    assert_eq!(repo.delete_namespace(&mine).unwrap(), 2);
    assert_eq!(repo.revisions(&mine).unwrap(), []);
    assert_eq!(repo.revisions(&theirs).unwrap(), [(1, b1.commit)]);
    assert_eq!(git(&root, &["rev-parse", "refs/heads/keep"]), head);
}

#[test]
fn freeze_takes_untracked_files_not_ignored_and_leaves_index_and_head() {
    let (_dir, root, repo) = init();
    let head = repo.head_id().unwrap();
    fs::write(root.join("a.txt"), "two\n").unwrap();
    fs::write(root.join("new.txt"), "new\n").unwrap();
    fs::write(root.join("debug.log"), "ignored\n").unwrap();
    fs::write(root.join(".git/info/exclude"), "secret.txt\n").unwrap();
    fs::write(root.join("secret.txt"), "excluded\n").unwrap();
    let index_before = fs::read(root.join(".git/index")).unwrap();
    let status_before = repo.status().unwrap();

    let lane = namespace("refs/tenmoku/lanes/k1/1");
    let revision = repo.freeze(&lane, 1, Some(&head), "lane 1").unwrap();

    assert_eq!(show(&root, &revision.commit, "a.txt"), "two");
    assert_eq!(show(&root, &revision.commit, "new.txt"), "new");
    let files = git(&root, &["ls-tree", "-r", "--name-only", &revision.commit]);
    assert_eq!(
        files.lines().collect::<Vec<_>>(),
        [".gitignore", "a.txt", "new.txt"]
    );
    assert_eq!(repo.tree_id(&revision.commit).unwrap(), revision.tree);
    assert_eq!(repo.worktree_tree().unwrap(), revision.tree);
    assert_eq!(
        git(&root, &["rev-parse", &format!("{}^", revision.commit)]),
        head
    );
    assert_eq!(
        repo.revisions(&lane).unwrap(),
        [(1, revision.commit.clone())]
    );

    // HEAD, the branch and the index are where they were.
    assert_eq!(repo.head_id().as_deref(), Some(head.as_str()));
    assert_eq!(git(&root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_before);
    assert_eq!(repo.status().unwrap(), status_before);

    // The files changed between HEAD and the revision, and back.
    let changed = |kind, name: &str| FileStatus {
        path: root.join(name),
        kind,
    };
    assert_eq!(
        repo.changed_files(&head, &revision.commit).unwrap(),
        [
            changed(ChangeKind::Modified, "a.txt"),
            changed(ChangeKind::Added, "new.txt"),
        ]
    );
    assert_eq!(
        repo.changed_files(&revision.commit, &head).unwrap(),
        [
            changed(ChangeKind::Modified, "a.txt"),
            changed(ChangeKind::Deleted, "new.txt"),
        ]
    );
}

#[test]
fn freeze_includes_staged_and_deleted_files() {
    let (_dir, root, repo) = init();
    fs::write(root.join("staged.txt"), "s\n").unwrap();
    git(&root, &["add", "staged.txt"]);
    fs::write(root.join("staged.txt"), "s2\n").unwrap();
    fs::remove_file(root.join("a.txt")).unwrap();
    let lane = namespace("refs/tenmoku/lanes/k1/1");
    let revision = repo.freeze(&lane, 1, None, "lane 1").unwrap();
    let files = git(&root, &["ls-tree", "-r", "--name-only", &revision.commit]);
    assert_eq!(
        files.lines().collect::<Vec<_>>(),
        [".gitignore", "staged.txt"]
    );
    assert_eq!(show(&root, &revision.commit, "staged.txt"), "s2");
    // No parent was given.
    assert_eq!(git(&root, &["rev-list", "--count", &revision.commit]), "1");
}

/// A file changed, keeping its size and mtime, no later than the index was
/// written is only told apart by git's racy-clean check, which compares
/// against the index file's own mtime: the temporary index must keep it.
#[test]
fn freeze_sees_a_racily_clean_change() {
    let (_dir, root, repo) = init();
    git(&root, &["config", "core.trustctime", "false"]);
    let file = root.join("a.txt");
    let then = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    let set_mtime = |path: &Path| {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(then)
            .unwrap();
    };
    fs::write(&file, "two\n").unwrap();
    set_mtime(&file);
    git(&root, &["add", "a.txt"]);
    fs::write(&file, "six\n").unwrap();
    set_mtime(&file);
    set_mtime(&root.join(".git/index"));

    let revision = (repo.freeze(&namespace("refs/tenmoku/lanes/k1/1"), 1, None, "r1")).unwrap();
    assert_eq!(show(&root, &revision.commit, "a.txt"), "six");
}

#[test]
fn freezing_twice_unchanged_gives_the_same_tree() {
    let (_dir, root, repo) = init();
    fs::write(root.join("new.txt"), "new\n").unwrap();
    let lane = namespace("refs/tenmoku/lanes/k1/1");
    let r1 = repo.freeze(&lane, 1, None, "r1").unwrap();
    let r2 = repo.freeze(&lane, 2, Some(&r1.commit), "r2").unwrap();
    assert_eq!(r1.tree, r2.tree);
    assert_eq!(repo.changed_files(&r1.commit, &r2.commit).unwrap(), []);
    fs::write(root.join("new.txt"), "newer\n").unwrap();
    assert_ne!(repo.worktree_tree().unwrap(), r2.tree);
    // Refs in the namespace not written as `r<k>` aren't revisions.
    for other in ["r01", "r+3", "r", "x1"] {
        let name = format!("{}/{other}", lane.as_str());
        git(&root, &["update-ref", &name, &r1.commit]);
    }
    assert_eq!(
        repo.revisions(&lane).unwrap(),
        [(1, r1.commit), (2, r2.commit)]
    );
}

/// C1: two linked worktrees of one repository freeze under their own
/// namespaces (two roots' lane 2s), and neither touches the other's refs.
#[test]
fn freezing_in_two_linked_worktrees_leaves_each_others_refs_alone() {
    let (_dir, root, repo) = init();
    let head = repo.head_id().unwrap();
    let (path_a, lane_a) = add_lane(&repo, &root, "lane-a");
    let (path_b, lane_b) = add_lane(&repo, &root, "lane-b");
    let ns_a = namespace("refs/tenmoku/lanes/rootA/2");
    let ns_b = namespace("refs/tenmoku/lanes/rootB/2");

    fs::write(path_a.join("a.txt"), "from a\n").unwrap();
    fs::write(path_a.join("only-a.txt"), "a\n").unwrap();
    fs::write(path_b.join("a.txt"), "from b\n").unwrap();
    fs::write(path_b.join("only-b.txt"), "b\n").unwrap();

    let a1 = lane_a.freeze(&ns_a, 1, Some(&head), "a r1").unwrap();
    let b1 = lane_b.freeze(&ns_b, 1, Some(&head), "b r1").unwrap();
    assert_eq!(show(&root, &a1.commit, "a.txt"), "from a");
    assert_eq!(show(&root, &a1.commit, "only-a.txt"), "a");
    assert_eq!(show(&root, &b1.commit, "a.txt"), "from b");
    assert_eq!(show(&root, &b1.commit, "only-b.txt"), "b");
    let tree_a = git(&root, &["ls-tree", "-r", "--name-only", &a1.commit]);
    assert!(!tree_a.contains("only-b.txt"));
    let tree_b = git(&root, &["ls-tree", "-r", "--name-only", &b1.commit]);
    assert!(!tree_b.contains("only-a.txt"));

    // Refs are shared: each worktree sees both lanes' revisions.
    for repo in [&repo, &lane_a, &lane_b] {
        assert_eq!(repo.revisions(&ns_a).unwrap(), [(1, a1.commit.clone())]);
        assert_eq!(repo.revisions(&ns_b).unwrap(), [(1, b1.commit.clone())]);
    }

    // Freezing A again, a new revision and r1 replaced, leaves B's as it was.
    let b_refs = |refs: Vec<String>| -> Vec<String> {
        refs.into_iter()
            .filter(|line| line.starts_with("refs/tenmoku/lanes/rootB/"))
            .collect()
    };
    let b_before = b_refs(tenmoku_refs(&root));
    fs::write(path_a.join("a.txt"), "from a again\n").unwrap();
    let a2 = lane_a.freeze(&ns_a, 2, Some(&a1.commit), "a r2").unwrap();
    let a1_again = lane_a.freeze(&ns_a, 1, Some(&head), "a r1 again").unwrap();
    assert_eq!(b_refs(tenmoku_refs(&root)), b_before);
    assert_eq!(lane_b.revisions(&ns_b).unwrap(), [(1, b1.commit.clone())]);
    assert_eq!(
        repo.revisions(&ns_a).unwrap(),
        [(1, a1_again.commit), (2, a2.commit)]
    );

    // Neither lane's freezing moved the other's HEAD or touched main's.
    for lane in [&repo, &lane_a, &lane_b] {
        assert_eq!(lane.head_id().as_deref(), Some(head.as_str()));
    }
    assert_eq!(git(&root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
}

/// C2: deleting one namespace's refs deletes no other's: not another
/// root's lane with the same number, not a lane whose number starts with
/// the same digit, not the root's other lanes.
#[test]
fn removing_one_namespaces_refs_removes_no_others() {
    let (_dir, root, repo) = init();
    let (path_a, lane_a) = add_lane(&repo, &root, "lane-a");
    let (path_b, lane_b) = add_lane(&repo, &root, "lane-b");
    fs::write(path_a.join("a.txt"), "from a\n").unwrap();
    fs::write(path_b.join("a.txt"), "from b\n").unwrap();

    let gone = namespace("refs/tenmoku/lanes/rootA/2");
    let kept = [
        namespace("refs/tenmoku/lanes/rootB/2"),
        namespace("refs/tenmoku/lanes/rootA/20"),
        namespace("refs/tenmoku/lanes/rootA/3"),
        namespace("refs/tenmoku/lanes/rootA2/2"),
    ];
    lane_a.freeze(&gone, 1, None, "r1").unwrap();
    lane_a.freeze(&gone, 2, None, "r2").unwrap();
    for (n, ns) in kept.iter().enumerate() {
        let lane = if n == 0 { &lane_b } else { &lane_a };
        lane.freeze(ns, 1, None, "r1").unwrap();
    }
    // A branch and a tag beside them stay too.
    git(&root, &["branch", "keep"]);
    git(&root, &["tag", "keep-tag"]);
    let before = tenmoku_refs(&root);
    assert_eq!(before.len(), 6);

    assert_eq!(lane_b.delete_namespace(&gone).unwrap(), 2);
    assert_eq!(repo.revisions(&gone).unwrap(), []);
    let after = tenmoku_refs(&root);
    let expected: Vec<String> = (before.into_iter())
        .filter(|line| !line.starts_with("refs/tenmoku/lanes/rootA/2/"))
        .collect();
    assert_eq!(after, expected);
    assert_eq!(after.len(), 4);
    for ns in &kept {
        assert_eq!(repo.revisions(ns).unwrap().len(), 1, "{}", ns.as_str());
    }
    assert_eq!(
        git(&root, &["rev-parse", "--verify", "-q", "refs/heads/keep"]).len(),
        40
    );
    assert_eq!(
        git(
            &root,
            &["rev-parse", "--verify", "-q", "refs/tags/keep-tag"]
        )
        .len(),
        40
    );

    // Deleting it again finds nothing.
    assert_eq!(repo.delete_namespace(&gone).unwrap(), 0);
}

#[test]
fn unknown_revisions_are_errors() {
    let (_dir, _root, repo) = init();
    let head = repo.head_id().unwrap();
    assert!(repo.tree_id("no-such-ref").is_err());
    assert!(repo.tree_id("--all").is_err());
    assert!(repo.changed_files(&head, "no-such-ref").is_err());
    assert_eq!(repo.changed_files(&head, "HEAD").unwrap(), []);
}
