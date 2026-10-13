//! `.editorconfig` files that start with a byte order mark.

use std::fs;

use fe_config::{EndOfLine, IndentKind, IndentSize, editorconfig_for};

#[test]
fn editorconfig_with_a_bom_before_its_first_section_applies() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "\u{feff}[*]\nindent_style = tab\n[*.py]\nindent_style = space\nindent_size = 4\n",
    )
    .unwrap();
    let rs = editorconfig_for(&dir.path().join("a.rs"));
    assert_eq!(rs.indent_style, Some(IndentKind::Tab));
    let py = editorconfig_for(&dir.path().join("b.py"));
    assert_eq!(py.indent_style, Some(IndentKind::Space));
    assert_eq!(py.indent_size, Some(IndentSize::Columns(4)));
}

#[test]
fn nested_editorconfig_with_a_bom_before_its_first_section_applies_over_its_parent() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "[*]\nend_of_line = crlf\n",
    )
    .unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join(".editorconfig"),
        "\u{feff}[*]\nindent_style = tab\n",
    )
    .unwrap();
    let c = editorconfig_for(&proj.join("a.rs"));
    assert_eq!(c.indent_style, Some(IndentKind::Tab));
    assert_eq!(c.end_of_line, Some(EndOfLine::Crlf));
}

#[test]
fn editorconfig_with_a_bom_before_its_preamble_applies() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "\u{feff}root = true\n[*]\nindent_style = tab\n",
    )
    .unwrap();
    let c = editorconfig_for(&dir.path().join("a.rs"));
    assert_eq!(c.indent_style, Some(IndentKind::Tab));
}

#[cfg(unix)]
#[test]
fn dot_dot_after_a_symlinked_directory_reads_the_link_targets_parent() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join("proj")).unwrap();
    fs::create_dir_all(p.join("other/sub")).unwrap();
    fs::write(p.join("proj/.editorconfig"), "[*]\nindent_style = space\n").unwrap();
    fs::write(p.join("other/.editorconfig"), "[*]\nindent_style = tab\n").unwrap();
    std::os::unix::fs::symlink(p.join("other/sub"), p.join("proj/link")).unwrap();
    let c = editorconfig_for(&p.join("proj/link/../a.go"));
    assert_eq!(c.indent_style, Some(IndentKind::Tab));
}
