//! The language each side of a git or file diff is highlighted in.

use super::*;

fn language(side: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    side.read_with(cx, |side, _| side.syntax().map(|s| s.language().name()))
}

fn languages(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [Option<&'static str>; 2] {
    sides(diff, cx).map(|side| language(&side, cx))
}

#[gpui::test]
fn a_compared_file_is_highlighted_in_its_own_language(cx: &mut TestAppContext) {
    let dir = repo(&[("a.rs", "fn a() {}\n"), ("b.py", "def b():\n    pass\n")]);
    let (workbench, mut cx) = window(cx, dir.path(), Some("a.rs"));
    let cx = &mut cx;
    ex("diffsplit b.py", cx);
    cx.run_until_parked();
    draw(cx);
    let compare = diff(&workbench, cx);
    assert_eq!(languages(&compare, cx), [Some("python"), Some("rust")]);
}

#[gpui::test]
fn git_sides_take_the_files_language(cx: &mut TestAppContext) {
    let dir = repo(&[("a.toml", "a = 1\n")]);
    let root = dir.path();
    fs::write(root.join("a.toml"), "a = 2\n").unwrap();
    git(root, &["add", "a.toml"]);
    fs::write(root.join("a.toml"), "a = 3\n").unwrap();
    let (workbench, mut cx) = window(cx, root, Some("a.toml"));
    let cx = &mut cx;
    for id in ["git.diff_index", "git.diff_head"] {
        dispatch(id, cx);
        cx.run_until_parked();
        let compare = diff(&workbench, cx);
        assert_eq!(
            languages(&compare, cx),
            [Some("toml"), Some("toml")],
            "{id}"
        );
    }
}
