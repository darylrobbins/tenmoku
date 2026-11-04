//! How much work [`outline_scopes`] does: matches outside the rows are
//! dropped before they're kept or their predicates run, the C++ namespace
//! index isn't built, and Markdown's scopes are found once a parse.

use std::{cell::Cell, fmt::Write as _, time::Duration};

use fe_buffer::{Buffer, Origin};

use super::*;
use crate::language_by_name;

thread_local! {
    /// How many times this thread has built a C++ namespace index.
    pub(super) static CPP_INDEXES: Cell<usize> = const { Cell::new(0) };
}

fn parsed(language: &str, buffer: &Buffer) -> Syntax {
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    assert!(syntax.parse_until(Duration::MAX, || false));
    syntax
}

#[test]
fn an_impls_methods_outside_the_rows_are_dropped_before_they_are_kept() {
    let mut text = String::from("impl S {\n");
    for i in 0..12_000 {
        writeln!(text, "    fn m{i}() {{}}").unwrap();
    }
    text.push_str("    fn last() {\n        x();\n    }\n}\n");
    let buffer = Buffer::new(&text);
    let syntax = parsed("rust", &buffer);
    let job = outline_job(&syntax).unwrap();
    let rows = 12_002..12_003;
    let bytes = job.text.row_start(rows.start)..job.text.row_start(rows.end);
    let compiled = job.query.outline(job.language);
    let (found, truncated) = find(
        job.language,
        compiled,
        &job.tree,
        &job.text,
        Find::Overlapping(bytes),
    );
    assert!(!truncated);
    let mut names: Vec<String> = (found.iter())
        .map(|found| name(&job.text, found.name_range.clone()))
        .collect();
    names.sort();
    assert_eq!(names, ["impl S", "last"]);
    assert_eq!(
        outline_scopes(&syntax, rows).unwrap(),
        vec![0..=12_004, 12_001..=12_003]
    );
}

#[test]
#[cfg(feature = "cpp")]
fn scopes_do_not_build_the_cpp_namespace_index() {
    let text = "namespace N {\nint f();\n}\nint N::f() {\n    return 1;\n}\n";
    let buffer = Buffer::new(text);
    let syntax = parsed("cpp", &buffer);
    let before = CPP_INDEXES.with(Cell::get);
    assert_eq!(outline_scopes(&syntax, 4..5).unwrap(), vec![3..=5]);
    assert_eq!(CPP_INDEXES.with(Cell::get), before);
    // The outline needs it, to tell N::f is a function.
    let outline = outline(&syntax).unwrap();
    assert_eq!(CPP_INDEXES.with(Cell::get), before + 1);
    let f = outline.items.iter().find(|item| item.row == 3).unwrap();
    assert_eq!(f.kind, OutlineKind::Function);
}

#[test]
fn markdown_scopes_are_found_once_a_parse() {
    let mut buffer = Buffer::new("# A\ntext\n## B\ntext\n");
    let mut syntax = parsed("markdown", &buffer);
    assert_eq!(outline_scopes(&syntax, 3..4).unwrap(), vec![0..=3, 2..=3]);
    let index = syntax.section_scopes(|| panic!("found again"));
    assert_eq!(outline_scopes(&syntax, 1..2).unwrap(), vec![0..=3]);
    assert!(Arc::ptr_eq(
        &index,
        &syntax.section_scopes(|| panic!("found again"))
    ));

    // An edit forgets them.
    buffer.edit([(buffer.len()..buffer.len(), "# C\ntext\n")], Origin::User);
    syntax.edit(buffer.current());
    assert!(syntax.parse_until(Duration::MAX, || false));
    assert_eq!(outline_scopes(&syntax, 3..4).unwrap(), vec![0..=3, 2..=3]);
    assert_eq!(outline_scopes(&syntax, 4..6).unwrap(), vec![4..=5]);
    assert!(!Arc::ptr_eq(
        &index,
        &syntax.section_scopes(|| panic!("found again"))
    ));
}

/// `scope_matches` for the last row of a container holding `count`
/// unrelated members before its one multi-row member, which `before` and
/// `after` wrap.
fn matches_past(language: &str, before: &str, member: &str, after: &str, count: usize) -> usize {
    let mut text = String::from(before);
    for i in 0..count {
        text.push_str(&member.replace('N', &i.to_string()));
    }
    text.push_str(&member.replace('N', "last"));
    text.push_str(after);
    let buffer = Buffer::new(&text);
    let syntax = parsed(language, &buffer);
    let last = syntax.text().row_count() - 2;
    scope_matches(&syntax, last..last + 1)
}

#[test]
fn matches_enumerated_do_not_grow_with_unrelated_siblings() {
    for (language, before, member, after) in [
        (
            "rust",
            "impl S {\n",
            "    fn mN() {\n        x();\n    }\n",
            "}\n",
        ),
        (
            "python",
            "class C:\n",
            "    def mN(self):\n        x()\n",
            "\n",
        ),
    ] {
        let few = matches_past(language, before, member, after, 20);
        let many = matches_past(language, before, member, after, 2_000);
        assert_eq!(
            few, many,
            "{language}: {few} matches for 20 siblings, {many} for 2000"
        );
    }
}
