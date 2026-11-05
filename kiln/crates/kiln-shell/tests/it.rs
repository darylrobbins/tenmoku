// SPDX-License-Identifier: Apache-2.0

//! kiln-shell's integration tests, one binary: each tests/*.rs is a module,
//! listed by build.rs (kiln/scripts/it_mods.rs), so a test's ID is
//! `<file>::<test>` and a new test is a new file, with no list to edit.

include!(concat!(env!("OUT_DIR"), "/it_mods.rs"));

// The files share one copy (`use crate::support;`): GPUI registers an action
// once per binary, and a copy per file would register `pad::*` again.
mod support;

/// A stale build would leave a new file out without a word: the list it was
/// built with must name every tests/*.rs there is.
#[test]
fn it_lists_every_test_file() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files: Vec<String> = std::fs::read_dir(&tests)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file() && path.extension().is_some_and(|extension| extension == "rs")
        })
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
        .filter(|stem| stem != "it")
        .collect();
    files.sort();
    assert_eq!(
        IT_MODS, files,
        "rebuild: tests/ and the list built in disagree"
    );
}
