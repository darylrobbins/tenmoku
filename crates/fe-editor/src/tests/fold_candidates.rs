//! Folding with candidates from a syntax tree, as fe-syntax's
//! `fold_range_at` and `fold_ranges` give them; here, fixed lists.

use std::ops::RangeInclusive;

use super::*;

fn folds(model: &EditorModel) -> Vec<(u32, u32)> {
    model
        .display()
        .folds()
        .iter()
        .map(|fold| (fold.start, fold.end))
        .collect()
}

/// A candidate source giving the first of `candidates` that `row` heads.
fn headed_by(candidates: &[RangeInclusive<u32>]) -> impl Fn(u32) -> Option<RangeInclusive<u32>> {
    move |row| {
        candidates
            .iter()
            .find(|range| *range.start() == row)
            .cloned()
    }
}

#[test]
fn fold_uses_candidates_then_indentation() {
    let fold = |marked: &str, candidate_at: &dyn Fn(u32) -> Option<RangeInclusive<u32>>| {
        let mut model = parse(marked);
        model.fold(candidate_at);
        (folds(&model), render(&model))
    };

    // A brace block indented unevenly: the candidate folds to the row
    // before its `}`, where indentation finds a different block.
    let braces = "fn f() {\nx();\n  if y {\nz();\n  }\n}";
    let blocks = [0..=4, 2..=3];
    let at = |row: usize| {
        let start = braces
            .split('\n')
            .take(row)
            .map(|line| line.len() + 1)
            .sum();
        format!("{}|{}", &braces[..start], &braces[start..])
    };
    assert_eq!(fold(&at(0), &headed_by(&blocks)).0, [(0, 4)]);
    assert_eq!(fold(&at(2), &headed_by(&blocks)).0, [(2, 3)]);
    assert_eq!(fold(&at(2), &|_| None).0, [(1, 2)]);

    // A Markdown section isn't indented at all. A caret folded away moves
    // to the header.
    let section = "# A\n\ntext\n\n## B\n\nmo|re\n# C";
    assert_eq!(
        fold(section, &|row| (row <= 6).then_some(0..=6)),
        (
            vec![(0, 6)],
            "# A|\n\ntext\n\n## B\n\nmore\n# C".to_string()
        )
    );

    // Without a candidate, the indented block the caret heads.
    assert_eq!(fold("de|f a():\n    one\nend", &|_| None).0, [(0, 1)]);

    // A selection over rows folds exactly those, whatever the candidates.
    let any = |_| Some(0..=6);
    assert_eq!(fold("[# A\n\ntext\n]\n## B", &any).0, [(0, 2)]);
    assert_eq!(fold("# A\n[\ntext\n\n## ]B", &any).0, [(1, 4)]);
}

#[test]
fn fold_all_and_unfold_all() {
    // Every candidate, nested ones included. Carets folded away move to
    // their outermost fold's header; others stay.
    let text = "fn f() {\n    if x {\n        y|();\n    }\n}\nfn g() {\n    z|();\n}\n|";
    let mut model = parse(text);
    model.fold_all([0..=3, 1..=2, 5..=6]);
    assert_eq!(folds(&model), [(0, 3), (1, 2), (5, 6)]);
    assert_eq!(
        render(&model),
        "fn f() {|\n    if x {\n        y();\n    }\n}\nfn g() {|\n    z();\n}\n|"
    );
    assert_eq!(model.display().row_count(), 5);
    model.unfold_all();
    assert_eq!(folds(&model), []);
    assert_eq!(model.display().row_count(), 9);

    // Without candidates, every indented block, nested ones included,
    // without trailing blank rows.
    let mut model = parse("a:\n  b:\n    c\n\n  d\n\ne:\n  f\ng\n  |h");
    model.fold_all([]);
    assert_eq!(folds(&model), [(0, 4), (1, 2), (6, 7), (8, 9)]);
    assert_eq!(render(&model), "a:\n  b:\n    c\n\n  d\n\ne:\n  f\ng|\n  h");
    model.unfold_all();
    assert_eq!(folds(&model), []);
    assert_eq!(model.display().row_count(), 10);
}

#[test]
fn fold_rows_and_unfold_rows() {
    let mut model = parse("a\nb\n|c\nd");
    model.fold_rows(1..=2);
    assert_eq!(folds(&model), [(1, 2)]);
    assert_eq!(render(&model), "a\nb|\nc\nd");
    model.unfold_rows(2..=2);
    assert_eq!(folds(&model), []);
}
