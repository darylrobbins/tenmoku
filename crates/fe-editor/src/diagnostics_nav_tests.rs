use std::path::PathBuf;

use fe_buffer::Buffer;

use super::*;

const ID: (u32, u32) = (1, 1);

fn model(items: Vec<(Range<usize>, Severity)>) -> EditorModel {
    let mut model = EditorModel::new(Buffer::new("aaaa\nbbbb\ncccc\ndddd\n"), None);
    let items = (items.into_iter())
        .map(|(range, severity)| (range, Diagnostic::new(severity, "m")))
        .collect();
    let version = model.snapshot().version();
    assert!(model.apply_diagnostics(1u32, ID, version, items, Some(ID)));
    model
}

#[test]
fn next_previous() {
    use Severity::*;
    let none = model(Vec::new());
    assert_eq!(none.next_diagnostic(3, true), None);
    assert_eq!(none.next_diagnostic(3, false), None);

    let m = model(vec![
        (2..3, Hint),
        (7..8, Warning),
        (7..9, Error),
        (12..13, Hint),
    ]);
    // Forward skips one at `from`, and takes the most severe at a start.
    assert_eq!(m.next_diagnostic(2, true), Some(7..9));
    assert_eq!(m.next_diagnostic(0, true), Some(2..3));
    assert_eq!(m.next_diagnostic(7, true), Some(12..13));
    // Wraps once, to the first.
    assert_eq!(m.next_diagnostic(12, true), Some(2..3));
    assert_eq!(m.next_diagnostic(19, true), Some(2..3));
    // Backward.
    assert_eq!(m.next_diagnostic(12, false), Some(7..9));
    assert_eq!(m.next_diagnostic(8, false), Some(7..9));
    assert_eq!(m.next_diagnostic(7, false), Some(2..3));
    assert_eq!(m.next_diagnostic(2, false), Some(12..13));
    assert_eq!(m.next_diagnostic(0, false), Some(12..13));
    // One diagnostic, at `from`: wraps onto itself.
    let one = model(vec![(5..6, Error)]);
    assert_eq!(one.next_diagnostic(5, true), Some(5..6));
    assert_eq!(one.next_diagnostic(5, false), Some(5..6));
}

#[test]
fn counts() {
    use Severity::*;
    assert_eq!(
        model(Vec::new()).diagnostic_counts(),
        DiagnosticCounts::default()
    );
    let m = model(vec![
        (0..1, Error),
        (2..3, Error),
        (5..6, Warning),
        (7..8, Information),
        (9..10, Hint),
        (11..12, Hint),
        (13..14, Hint),
    ]);
    assert_eq!(
        m.diagnostic_counts(),
        DiagnosticCounts {
            errors: 2,
            warnings: 1,
            infos: 1,
            hints: 3
        }
    );
}

fn problem(row: u32, column: u32, severity: Severity) -> Problem {
    Problem {
        range: 0..1,
        row,
        column,
        severity,
        message: format!("{row}:{column}"),
        source: None,
        related: Vec::new(),
        code_href: None,
    }
}

#[test]
fn grouped() {
    use Severity::*;
    let groups = group_problems(vec![
        (
            PathBuf::from("b.rs"),
            vec![
                problem(2, 0, Error),
                problem(1, 5, Hint),
                problem(1, 5, Error),
                problem(1, 2, Warning),
            ],
        ),
        (PathBuf::from("empty.rs"), Vec::new()),
        (PathBuf::from("a.rs"), vec![problem(0, 0, Hint)]),
        // Listed twice: merged into one group, sorted together.
        (PathBuf::from("b.rs"), vec![problem(0, 9, Hint)]),
    ]);
    let paths: Vec<_> = groups.iter().map(|g| g.path.clone()).collect();
    assert_eq!(paths, [PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
    let order: Vec<_> = (groups[1].problems.iter())
        .map(|p| (p.row, p.column, p.severity))
        .collect();
    assert_eq!(
        order,
        [
            (0, 9, Hint),
            (1, 2, Warning),
            (1, 5, Error),
            (1, 5, Hint),
            (2, 0, Error)
        ]
    );

    let m = model(vec![(6..8, Warning)]);
    let problems = m.problems();
    assert_eq!((problems[0].row, problems[0].column), (1, 1));
    assert_eq!(problems[0].range, 6..8);
}
