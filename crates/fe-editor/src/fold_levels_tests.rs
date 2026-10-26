use std::ops::{Range, RangeInclusive};

use fe_command::{Effect, Invocation, Value};

use crate::{
    EditorModel,
    commands::{self, EditorHost, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

/// Classes holding fns, one of them holding a block, as syntax folds them:
/// a `{ … }` ends on the row before its `}`.
const NESTED: &str = "\
class A {
    fn a() {
        if x {
            y
        }
    }
    fn b() {
        z
    }
}
class B {
    fn c() {
        w
    }
}
";

/// `NESTED`'s candidates: the classes, their fns and the block.
const CANDIDATES: [RangeInclusive<u32>; 6] = [0..=8, 1..=4, 2..=3, 6..=7, 10..=13, 11..=12];

/// A host whose syntax gives these candidates.
struct Syntax(Vec<RangeInclusive<u32>>);

impl EditorHost for Syntax {
    fn page_rows(&self) -> u32 {
        20
    }

    fn read_clipboard(&mut self) -> Option<String> {
        None
    }

    fn write_clipboard(&mut self, _: String) {}

    /// As `fe_syntax::fold_range_at`: the innermost headed by `row`, else
    /// the innermost containing it.
    fn fold_range_at(&self, row: u32) -> Option<RangeInclusive<u32>> {
        let innermost = |ranges: Vec<RangeInclusive<u32>>| {
            ranges
                .into_iter()
                .min_by_key(|range| range.end() - range.start())
        };
        let candidates = self.0.iter().cloned();
        let headed = candidates.clone().filter(|range| *range.start() == row);
        innermost(headed.collect())
            .or_else(|| innermost(candidates.filter(|r| r.contains(&row)).collect()))
    }

    fn fold_ranges(&self, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
        let mut ranges: Vec<_> = (self.0.iter())
            .filter(|range| rows.contains(range.start()))
            .cloned()
            .collect();
        ranges.sort_by_key(|range| (*range.start(), *range.end()));
        ranges
    }
}

fn run(model: &mut EditorModel, host: &mut dyn EditorHost, command: &str) -> Effect {
    let invocation = Invocation::of(command)
        .unwrap()
        .args(Value::Null)
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, host), &invocation)
        .unwrap_or_else(|error| panic!("{command}: {error}"))
        .effect
}

fn folds(model: &EditorModel) -> Vec<(u32, u32)> {
    let folds = model.display().folds();
    folds.iter().map(|fold| (fold.start, fold.end)).collect()
}

#[test]
fn level_n_folds_that_depth() {
    // The caret is on the last row, in no candidate.
    let mut model = parse(&format!("{NESTED}|"));
    let host = &mut Syntax(CANDIDATES.to_vec());
    assert_eq!(
        run(&mut model, host, "editor.fold_level_1"),
        Effect::Changed
    );
    assert_eq!(folds(&model), [(0, 8), (10, 13)]);
    model.unfold_all();
    run(&mut model, host, "editor.fold_level_2");
    assert_eq!(folds(&model), [(1, 4), (6, 7), (11, 12)], "level 1 open");
    model.unfold_all();
    for level in 4..=7 {
        let command = format!("editor.fold_level_{level}");
        assert_eq!(run(&mut model, host, &command), Effect::Nothing, "{level}");
    }
    assert_eq!(folds(&model), []);
    // Folds at other levels stay.
    run(&mut model, host, "editor.fold_level_3");
    assert_eq!(folds(&model), [(2, 3)]);
    run(&mut model, host, "editor.fold_level_1");
    assert_eq!(folds(&model), [(0, 8), (2, 3), (10, 13)]);
    assert_eq!(render(&model), format!("{NESTED}|"));
}

#[test]
fn level_skips_the_caret_row() {
    let host = &mut Syntax(CANDIDATES.to_vec());
    // Carets in `fn a`'s block and in `fn c`: their fns and classes stay
    // open, `fn b` folds.
    let marked = NESTED.replace("    y", "    |y").replace("    w", "    w|");
    let mut model = parse(&marked);
    assert_eq!(
        run(&mut model, host, "editor.fold_level_1"),
        Effect::Nothing
    );
    run(&mut model, host, "editor.fold_level_2");
    assert_eq!(folds(&model), [(6, 7)]);
    assert_eq!(
        run(&mut model, host, "editor.fold_level_3"),
        Effect::Nothing
    );
    assert_eq!(render(&model), marked, "no caret moved");
    // A caret on a header counts, as does a selection's end.
    for marked in [
        NESTED.replace("    fn b", "|    fn b"),
        NESTED
            .replace("    }\n    fn b", "  [  }\n    fn b")
            .replace("z", "z]"),
        NESTED
            .replace("    }\n    fn b", "  [  }\n    fn b")
            .replace("}\nclass B", "}]\nclass B"),
    ] {
        let mut model = parse(&marked);
        run(&mut model, host, "editor.fold_level_2");
        let expected = if marked.contains("}]\nclass") {
            // The selection holds `fn b` but ends past it.
            vec![(1, 4), (6, 7), (11, 12)]
        } else {
            vec![(1, 4), (11, 12)]
        };
        assert_eq!(folds(&model), expected, "{marked}");
        assert_eq!(render(&model), marked, "no selection moved");
    }
}

#[test]
fn recursive() {
    let host = &mut Syntax(CANDIDATES.to_vec());
    // On class A's header: it and everything in it, not class B.
    let mut model = parse(&format!("|{NESTED}"));
    run(&mut model, host, "editor.fold_recursively");
    assert_eq!(folds(&model), [(0, 8), (1, 4), (2, 3), (6, 7)]);
    // A sibling folded.
    model.fold_rows(11..=12);
    assert_eq!(
        run(&mut model, host, "editor.unfold_recursively"),
        Effect::Changed
    );
    assert_eq!(folds(&model), [(11, 12)], "class B's fn stays folded");
    assert_eq!(render(&model), format!("|{NESTED}"));

    // In `fn a`, beside a folded `fn b`.
    let mut model = parse(&NESTED.replace("fn a", "fn |a"));
    model.fold_rows(6..=7);
    run(&mut model, host, "editor.fold_recursively");
    assert_eq!(folds(&model), [(1, 4), (2, 3), (6, 7)]);
    run(&mut model, host, "editor.unfold_recursively");
    assert_eq!(folds(&model), [(6, 7)], "fn b stays folded");

    // A caret inside moves to the header.
    let mut model = parse(&NESTED.replace("z", "z|"));
    run(&mut model, host, "editor.fold_recursively");
    assert_eq!(folds(&model), [(6, 7)]);
    assert_eq!(render(&model), NESTED.replace("fn b() {", "fn b() {|"));
    // Opening from the header opens it.
    run(&mut model, host, "editor.unfold_recursively");
    assert_eq!(folds(&model), []);

    // Past every candidate: nothing to fold or open.
    let mut model = parse(&format!("{NESTED}|"));
    model.fold_rows(0..=8);
    assert_eq!(
        run(&mut model, host, "editor.fold_recursively"),
        Effect::Nothing
    );
    assert_eq!(
        run(&mut model, host, "editor.unfold_recursively"),
        Effect::Nothing
    );
    assert_eq!(folds(&model), [(0, 8)]);
}

#[test]
fn recursive_unfold_keeps_a_sibling_sharing_a_row() {
    // One candidate ends on the row the next starts on.
    let host = &mut Syntax(vec![0..=1, 1..=2]);
    let mut model = parse("|a\nb\nc\nd");
    model.fold_rows(1..=2);
    model.fold_rows(0..=1);
    assert_eq!(folds(&model), [(0, 1), (1, 2)]);
    assert_eq!(render(&model), "|a\nb\nc\nd");
    run(&mut model, host, "editor.unfold_recursively");
    assert_eq!(folds(&model), [(1, 2)]);
}

/// `a` with `b` and `c` under it once another model indents `b`, the caret
/// on `a`, and a candidate elsewhere: `fold` is made before the edit,
/// which the model hasn't caught up with.
fn indented_elsewhere(fold: Option<RangeInclusive<u32>>) -> EditorModel {
    let mut model = parse("|a\nb\n  c\nend\nx {\n  y\n}\n");
    if let Some(rows) = fold {
        model.fold_rows(rows);
    }
    let mut other = model.split();
    other.set_cursor(2);
    other.insert("  ");
    assert_eq!(model.text().row_to_string(1), "b", "not caught up");
    model
}

#[test]
fn recursive_fallback_follows_edits_not_caught_up() {
    let host = &mut Syntax(std::iter::once(4..=5).collect());
    let mut model = indented_elsewhere(None);
    run(&mut model, host, "editor.fold_recursively");
    // As the text was, `a` headed no block.
    assert_eq!(folds(&model), [(0, 2)]);
    let mut model = indented_elsewhere(Some(1..=2));
    assert_eq!(folds(&model), [(1, 2)]);
    run(&mut model, host, "editor.unfold_recursively");
    assert_eq!(folds(&model), [], "the fold is in a's block now");
}

#[test]
fn recursive_fallback_folds_the_candidates_inside() {
    // Indentation doesn't see the list; syntax does.
    let host = &mut Syntax(std::iter::once(1..=3).collect());
    let marked = "|a\n list(\n x\n y\n )\nend";
    let mut model = parse(marked);
    run(&mut model, host, "editor.fold_recursively");
    assert_eq!(folds(&model), [(0, 4), (1, 3)]);
    // The candidate is followed through a row another model inserted in it.
    let mut model = parse(marked);
    let mut other = model.split();
    other.set_cursor("a\n list(\n".len());
    other.insert(" w\n");
    assert_eq!(model.text().row_count(), 6, "not caught up");
    run(&mut model, host, "editor.fold_recursively");
    assert_eq!(model.text().row_to_string(2), " w");
    assert_eq!(folds(&model), [(0, 5), (1, 4)]);
}

/// Classes, defs and a block, by indentation alone.
const INDENTED: &str = "\
class A:
    def a():
        if x:
            y
    def b():
        z
class B:
    def c():
        w
end
";

#[test]
fn indent_fallback() {
    let host = &mut HeadlessHost::new();
    let mut model = parse(&format!("{INDENTED}|"));
    run(&mut model, host, "editor.fold_level_1");
    assert_eq!(folds(&model), [(0, 5), (6, 8)]);
    model.unfold_all();
    run(&mut model, host, "editor.fold_level_2");
    assert_eq!(folds(&model), [(1, 3), (4, 5), (7, 8)]);
    run(&mut model, host, "editor.fold_level_3");
    run(&mut model, host, "editor.fold_level_1");
    assert_eq!(
        run(&mut model, host, "editor.fold_level_4"),
        Effect::Nothing
    );
    // Every level folded is what Fold All folds.
    let mut all = parse(&format!("{INDENTED}|"));
    run(&mut all, host, "editor.fold_all");
    assert_eq!(folds(&model), folds(&all));

    let mut model = parse(&INDENTED.replace("def a", "def |a"));
    run(&mut model, host, "editor.fold_level_2");
    assert_eq!(folds(&model), [(4, 5), (7, 8)], "def a holds the caret");
    run(&mut model, host, "editor.fold_recursively");
    assert_eq!(folds(&model), [(1, 3), (2, 3), (4, 5), (7, 8)]);
    run(&mut model, host, "editor.unfold_recursively");
    assert_eq!(folds(&model), [(4, 5), (7, 8)]);
}
