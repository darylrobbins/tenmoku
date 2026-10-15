//! Typed `:Git switch` names, on the fixture of `git_branch_tests` (remote
//! `origin` with `feat`): a remote whose name holds a slash, and a local
//! branch named like a remote-tracking one.

use std::{fs, path::Path};

use gpui::TestAppContext;

use super::OVERWRITE;
use super::git_branch_tests::{FEAT_TEXT, draw, ex, fixture, git, init, stashes, window};

fn symbolic_head(work: &Path) -> String {
    git(work, &["symbolic-ref", "HEAD"])
}

fn upstream_of_feat(work: &Path) -> String {
    git(
        work,
        &["rev-parse", "--symbolic-full-name", "feat@{upstream}"],
    )
}

/// Remote `team/origin` with `feat`, and a local `origin/feat`.
#[gpui::test]
fn typed_switch_under_a_remote_with_a_slash(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    let origin = dir.path().join("origin.git");
    git(
        &work,
        &["remote", "add", "team/origin", origin.to_str().unwrap()],
    );
    git(&work, &["fetch", "-q", "team/origin"]);
    git(&work, &["branch", "-q", "-D", "feat"]);
    git(&work, &["branch", "-q", "origin/feat"]);
    init(cx);
    let (_workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;

    ex("Git switch team/origin/feat", cx);
    assert_eq!(symbolic_head(&work), "refs/heads/feat");
    assert_eq!(upstream_of_feat(&work), "refs/remotes/team/origin/feat");
}

/// A local `origin/feat` made while the overwrite question for the remote
/// one is open doesn't take the retry.
#[gpui::test]
fn overwrite_retries_keep_the_typed_remote(cx: &mut TestAppContext) {
    for answer in ["Force Checkout", "Stash & Checkout"] {
        let dir = fixture();
        let work = dir.path().join("work");
        git(&work, &["branch", "-q", "-D", "feat"]);
        init(cx);
        let (_workbench, mut cx) = window(cx, &work);
        let cx = &mut cx;
        fs::write(work.join("a.txt"), "mine\n").unwrap();

        ex("Git switch origin/feat", cx);
        let (question, _) = cx.pending_prompt().expect("a question");
        assert_eq!(question, OVERWRITE);
        git(&work, &["branch", "-q", "origin/feat"]);
        cx.simulate_prompt_answer(answer);
        draw(cx);

        assert_eq!(symbolic_head(&work), "refs/heads/feat", "{answer}");
        assert_eq!(upstream_of_feat(&work), "refs/remotes/origin/feat");
        assert_eq!(stashes(&work), usize::from(answer.starts_with("Stash")));
        assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), FEAT_TEXT);
    }
}
