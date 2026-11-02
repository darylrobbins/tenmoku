//! Git's own files are known by name or, where the name says nothing
//! (`config`, `attributes`), by the end of their path.

use std::path::Path;

use fe_syntax::language_name_for_path;

fn language(path: &str) -> Option<&'static str> {
    language_name_for_path(Path::new(path))
}

#[cfg(feature = "gitcommit")]
#[test]
fn commit_message_files_are_gitcommit() {
    for name in [
        "COMMIT_EDITMSG",
        "MERGE_MSG",
        "TAG_EDITMSG",
        "SQUASH_MSG",
        "NOTES_EDITMSG",
        "EDIT_DESCRIPTION",
        "repo/.git/COMMIT_EDITMSG",
    ] {
        assert_eq!(language(name), Some("gitcommit"), "{name}");
    }
}

#[cfg(feature = "gitrebase")]
#[test]
fn the_rebase_todo_is_gitrebase() {
    assert_eq!(language("git-rebase-todo"), Some("gitrebase"));
    assert_eq!(
        language("repo/.git/rebase-merge/git-rebase-todo"),
        Some("gitrebase")
    );
}

#[cfg(feature = "gitconfig")]
#[test]
fn config_files_are_gitconfig() {
    for name in [
        ".gitconfig",
        ".gitmodules",
        "repo/.git/config",
        "home/.config/git/config",
        ".git/config",
        "./.git/config",
        "/abs/.git/config",
        "REPO/.GIT/CONFIG",
    ] {
        assert_eq!(language(name), Some("gitconfig"), "{name}");
    }
}

#[cfg(feature = "gitattributes")]
#[test]
fn attributes_files_are_gitattributes() {
    for name in [
        ".gitattributes",
        "repo/.git/info/attributes",
        "home/.config/git/attributes",
    ] {
        assert_eq!(language(name), Some("gitattributes"), "{name}");
    }
}

#[cfg(feature = "gitignore")]
#[test]
fn exclude_files_are_gitignore() {
    for name in ["repo/.git/info/exclude", "home/.config/git/ignore"] {
        assert_eq!(language(name), Some("gitignore"), "{name}");
    }
}

#[test]
fn a_tail_matches_whole_components() {
    for name in [
        "proj/config",
        "config",
        "git/config",
        "attributes",
        "exclude",
        // Part of a component isn't a match: a bare repo's `foo.git/config`.
        "foo.git/config",
        "x.config/git/config",
        // `..` leaves the directory the tail names.
        ".git/../config",
    ] {
        assert_eq!(language(name), None, "{name}");
    }
}

/// With its language left out of the build, a tail falls through to the
/// other rules: here, nothing knows the name `config`.
#[cfg(not(feature = "gitconfig"))]
#[test]
fn a_tail_of_a_language_left_out_is_skipped() {
    assert_eq!(language("repo/.git/config"), None);
}
