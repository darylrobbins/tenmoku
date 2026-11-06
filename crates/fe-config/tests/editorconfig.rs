//! `.editorconfig` lookup and values.

use std::fs;

use fe_config::{EndOfLine, IndentKind, IndentSize, editorconfig_for};

#[test]
fn sections_and_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path();
    // Above the root = true file: must be ignored.
    fs::write(
        top.join(".editorconfig"),
        "[*]\nindent_style = tab\nend_of_line = cr\n",
    )
    .unwrap();
    let proj = top.join("proj");
    fs::create_dir_all(proj.join("sub")).unwrap();
    fs::write(
        proj.join(".editorconfig"),
        "root = true\n[*]\nindent_style = space\nindent_size = 4\n[*.go]\nindent_style = tab\n[Makefile]\nindent_style = tab\n",
    )
    .unwrap();
    fs::write(proj.join("sub/.editorconfig"), "[*.js]\nindent_size = 2\n").unwrap();

    let go = editorconfig_for(&proj.join("a.go"));
    assert_eq!(go.indent_style, Some(IndentKind::Tab));
    assert_eq!(go.end_of_line, None);
    let js = editorconfig_for(&proj.join("sub/x.js"));
    assert_eq!(js.indent_style, Some(IndentKind::Space));
    assert_eq!(js.indent_size, Some(IndentSize::Columns(2)));
    let make = editorconfig_for(&proj.join("Makefile"));
    assert_eq!(make.indent_style, Some(IndentKind::Tab));
}

#[test]
fn values_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(
        p.join(".editorconfig"),
        "root = true\n[a.txt]\nend_of_line = crlf\ninsert_final_newline = true\ntrim_trailing_whitespace = false\n\
         [tab.txt]\nindent_size = tab\ntab_width = 8\n\
         [three.txt]\nindent_size = 3\n\
         [bad.txt]\nindent_style = bogus\nend_of_line = nope\ninsert_final_newline = maybe\n\
         [unset.txt]\nindent_style = unset\nindent_size = unset\n\
         [hard.txt]\nindent_style = tab\n\
         [hard8.txt]\nindent_style = tab\ntab_width = 8\n\
         [tabnone.txt]\nindent_size = tab\n\
         [sizeunset.txt]\nindent_style = tab\nindent_size = unset\n\
         [sizebad.txt]\nindent_style = tab\nindent_size = lots\n\
         [widthunset.txt]\nindent_size = 3\ntab_width = unset\n\
         [widthbad.txt]\nindent_size = 3\ntab_width = wide\n",
    )
    .unwrap();

    let a = editorconfig_for(&p.join("a.txt"));
    assert_eq!(a.end_of_line, Some(EndOfLine::Crlf));
    assert_eq!(a.insert_final_newline, Some(true));
    assert_eq!(a.trim_trailing_whitespace, Some(false));

    let t = editorconfig_for(&p.join("tab.txt"));
    assert_eq!(
        (t.indent_size, t.tab_width),
        (Some(IndentSize::Columns(8)), Some(8))
    );

    let three = editorconfig_for(&p.join("three.txt"));
    assert_eq!(
        (three.indent_size, three.tab_width),
        (Some(IndentSize::Columns(3)), Some(3))
    );

    let hard = editorconfig_for(&p.join("hard.txt"));
    assert_eq!(hard.indent_size, Some(IndentSize::Tab));

    let hard8 = editorconfig_for(&p.join("hard8.txt"));
    assert_eq!(hard8.indent_size, Some(IndentSize::Columns(8)));
    let tabnone = editorconfig_for(&p.join("tabnone.txt"));
    assert_eq!(
        (tabnone.indent_size, tabnone.tab_width),
        (Some(IndentSize::Tab), None)
    );

    // Explicitly unset or invalid values are not replaced by defaults.
    for name in ["sizeunset.txt", "sizebad.txt"] {
        let c = editorconfig_for(&p.join(name));
        assert_eq!(
            (c.indent_style, c.indent_size),
            (Some(IndentKind::Tab), None),
            "{name}"
        );
    }
    for name in ["widthunset.txt", "widthbad.txt"] {
        let c = editorconfig_for(&p.join(name));
        assert_eq!(
            (c.indent_size, c.tab_width),
            (Some(IndentSize::Columns(3)), None),
            "{name}"
        );
    }

    for name in ["bad.txt", "unset.txt", "none.txt"] {
        let c = editorconfig_for(&p.join(name));
        assert_eq!(c.indent_style, None, "{name}");
        assert_eq!(c.indent_size, None, "{name}");
        assert_eq!(c.end_of_line, None, "{name}");
        assert_eq!(c.insert_final_newline, None, "{name}");
    }
}

#[test]
fn broken_files_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(
        p.join(".editorconfig"),
        "root = true\n[*]\nindent_style = space\n",
    )
    .unwrap();
    let sub = p.join("sub");
    fs::create_dir_all(sub.join("deeper")).unwrap();
    // Unreadable: a directory where the file should be.
    fs::create_dir(sub.join(".editorconfig")).unwrap();
    // Unparseable: invalid UTF-8.
    fs::write(
        sub.join("deeper/.editorconfig"),
        b"[*]\nindent_size = \xff\xfe\n[\n",
    )
    .unwrap();

    let c = editorconfig_for(&sub.join("deeper/x.rs"));
    assert_eq!(c.indent_style, Some(IndentKind::Space));
}

#[test]
fn a_broken_root_file_does_not_hide_the_ones_above() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join(".editorconfig"), "[*]\nindent_style = space\n").unwrap();
    fs::create_dir(p.join("sub")).unwrap();
    fs::write(
        p.join("sub/.editorconfig"),
        "root = true\n[*]\nindent_style = tab\nnot a property\n",
    )
    .unwrap();
    let c = editorconfig_for(&p.join("sub/x.rs"));
    assert_eq!(c.indent_style, Some(IndentKind::Space));
}

#[test]
fn dot_dot_paths_match_their_resolved_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join(".editorconfig"), "[*]\nindent_style = space\n").unwrap();
    fs::create_dir(p.join("sub")).unwrap();
    fs::write(
        p.join("sub/.editorconfig"),
        "root = true\n[*]\nindent_style = tab\n",
    )
    .unwrap();
    let direct = editorconfig_for(&p.join("a.go"));
    assert_eq!(direct.indent_style, Some(IndentKind::Space));
    assert_eq!(editorconfig_for(&p.join("sub/../a.go")), direct);
    assert_eq!(editorconfig_for(&p.join("./sub/./../a.go")), direct);
}

#[cfg(unix)]
struct Capture(std::sync::Mutex<Vec<String>>);

#[cfg(unix)]
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0
            .lock()
            .unwrap()
            .push(format!("{}: {}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

#[cfg(unix)]
static CAPTURE: Capture = Capture(std::sync::Mutex::new(Vec::new()));

#[cfg(unix)]
fn warnings_naming(needle: &str) -> usize {
    CAPTURE
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.starts_with("WARN") && m.contains(needle))
        .count()
}

#[test]
#[cfg(unix)]
fn skipped_files_are_warned_about_once() {
    use std::os::unix::fs::PermissionsExt;
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let locked = p.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join(".editorconfig"), "[*]\nindent_style = tab\n").unwrap();
    fs::write(p.join(".editorconfig"), "[*]\nindent_style = space\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let denied = editorconfig_for(&locked.join("x.rs"));
    let missing = editorconfig_for(&p.join("nothing/x.rs"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(denied.indent_style, Some(IndentKind::Space));
    assert_eq!(missing.indent_style, Some(IndentKind::Space));
    let name = locked.join(".editorconfig");
    assert_eq!(warnings_naming(name.to_str().unwrap()), 1);
    // Files that simply don't exist are not worth a warning.
    assert_eq!(warnings_naming(p.join("nothing").to_str().unwrap()), 0);
}
