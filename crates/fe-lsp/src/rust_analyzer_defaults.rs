//! The built-in rust-analyzer's memory-conscious defaults
//! (docs/decisions/ztdenw59dh-rust-analyzer-memory-defaults.md): its
//! `initializationOptions` before the user's, which are deep-merged on top
//! ([`merged_under`]).
//!
//! - `files.exclude` lists `target` and every nested git worktree under
//!   the root ([`nested_worktrees`]), worked out as each instance starts.
//! - Cache priming stays on (nothing is sent): without it the peak fell
//!   by nine tenths on fastedit, but the first go-to-definition into
//!   another crate took 26 s instead of under 0.1 s.
//! - Proc macros stay on (gpui's macros need them), so nothing is sent
//!   for them.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The built-in server these defaults are for.
pub(crate) const RUST_ANALYZER: &str = "rust-analyzer";

/// The defaults for an instance at `root`, before the user's options.
pub(crate) fn defaults(root: &Path) -> Value {
    let mut exclude = vec![Value::from("target")];
    exclude.extend(nested_worktrees(root).into_iter().map(Value::from));
    json!({ "files": { "exclude": exclude } })
}

/// The nested git worktrees under `root` (a directory whose `.git` is a
/// file, as `git worktree add` makes them), relative to it with `/`
/// between names, sorted: those among its children, and among the
/// children of its other children (`.worktrees/<lane>`). Nothing deeper
/// is read, nor anything under a worktree, `target` or `.git`, so a start
/// costs one listing per child folder. Symlinks are skipped, and so are
/// names that aren't UTF-8, which the settings can't carry.
pub(crate) fn nested_worktrees(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for (name, child) in folders(root) {
        if name == "target" || name == ".git" {
            continue;
        }
        if is_worktree(&child) {
            found.push(name);
            continue;
        }
        for (inner, grandchild) in folders(&child) {
            if is_worktree(&grandchild) {
                found.push(format!("{name}/{inner}"));
            }
        }
    }
    found.sort();
    found
}

/// The folders directly in `dir` (not symlinks), with UTF-8 names. None
/// if it can't be listed.
fn folders(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .collect()
}

/// Whether `dir` is a linked git worktree: its `.git` is a file.
fn is_worktree(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join(".git")).is_ok_and(|meta| meta.is_file())
}

/// `defaults` with `user`'s options on top: tables merge key by key, at
/// every depth, and any other value the user gives (a list such as
/// `files.exclude`, a number, false, null) replaces the default there.
/// No options (or null ones) keep the defaults; options that aren't a
/// table are the user's alone.
pub(crate) fn merged_under(defaults: Value, user: Option<Value>) -> Value {
    match user {
        None | Some(Value::Null) => defaults,
        Some(user) => merge(defaults, user),
    }
}

fn merge(base: Value, over: Value) -> Value {
    match (base, over) {
        (Value::Object(mut base), Value::Object(over)) => {
            for (key, value) in over {
                match base.get_mut(&key) {
                    Some(given) => *given = merge(given.take(), value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
            Value::Object(base)
        }
        (_, over) => over,
    }
}
