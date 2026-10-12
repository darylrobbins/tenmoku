use super::*;
use crate::{
    EditorModel,
    tests::{parse, render},
};

fn detect(s: &str) -> Option<IndentStyle> {
    detect_indent_style(&Text::from_file_contents(s).0)
}

const GO: &str = "\
package main

func main() {
\tfor i := 0; i < 3; i++ {
\t\tif i > 1 {
\t\t\tfmt.Println(i)
\t\t}
\t}
\tdone()
}
";

const JS: &str = "\
function main() {
  for (const x of xs) {
    if (x) {
      log(x);
    }
  }
  done();
}
";

const PYTHON: &str = "\
def main():
    for x in xs:
        if x:
            print(x)

    # A comment, and a continuation lined up with the bracket:
    done(a,
         b)
";

#[test]
fn detects() {
    assert_eq!(detect(GO), Some(IndentStyle::Tabs));
    assert_eq!(detect(JS), Some(IndentStyle::Spaces(2)));
    assert_eq!(detect(PYTHON), Some(IndentStyle::Spaces(4)));
    // Three indented rows aren't enough to tell, nor is nothing at all.
    assert_eq!(detect("a {\n    b\n    c\n    d\n}\n"), None);
    assert_eq!(detect(""), None);
    // Only the first 10,000 rows are read.
    let text = "  a\n".repeat(10_000) + &"\tb\n".repeat(50_000);
    assert_eq!(detect(&text), Some(IndentStyle::Spaces(2)));
    // Ties go to the smaller width; one-space increases count for nothing.
    assert_eq!(
        detect("a\n  b\nc\n    d\n e\n  f\n g\n"),
        Some(IndentStyle::Spaces(2))
    );
    // With no trailing line break, the last row still counts.
    assert_eq!(
        detect("a\n\tb\n\tc\n\td\n\te\n\tf"),
        Some(IndentStyle::Tabs)
    );
}

/// `marked` edited by `run` with `style`.
fn edited(style: Option<IndentStyle>, marked: &str, run: fn(&mut EditorModel)) -> String {
    let mut model = parse(marked);
    model.set_indent_style(style);
    run(&mut model);
    render(&model)
}

#[test]
fn editing_follows_it() {
    let tabs = Some(IndentStyle::Tabs);
    assert_eq!(edited(tabs, "a|b", EditorModel::tab), "a\t|b");
    assert_eq!(edited(tabs, "[a\nb]", EditorModel::tab), "\t[a\n\tb]");
    assert_eq!(edited(tabs, "\t\t|a", EditorModel::outdent_rows), "\t|a");
    // Spaces still outdent by tab_size.
    assert_eq!(edited(tabs, "      |a", EditorModel::outdent_rows), "  |a");

    let two = Some(IndentStyle::Spaces(2));
    assert_eq!(edited(two, "a|b", EditorModel::tab), "a |b");
    assert_eq!(edited(two, "ab|", EditorModel::tab), "ab  |");
    assert_eq!(edited(two, "|a\nb", EditorModel::indent_rows), "  |a\nb");
    assert_eq!(edited(two, "    |a", EditorModel::outdent_rows), "  |a");

    // With no style, tab_size spaces as before.
    assert_eq!(edited(None, "a|b", EditorModel::tab), "a   |b");
    assert_eq!(edited(None, "|a", EditorModel::indent_rows), "    |a");
    assert_eq!(edited(None, "      |a", EditorModel::outdent_rows), "  |a");

    // The unit, and a split keeping the style.
    let mut model = parse("|a");
    assert_eq!(model.indent_unit(), "    ");
    model.set_indent_style(tabs);
    assert_eq!(model.indent_unit(), "\t");
    assert_eq!(model.split().indent_style(), tabs);
    model.set_indent_style(two);
    assert_eq!(model.indent_unit(), "  ");
    // A model detects it when made.
    assert_eq!(parse(GO).indent_style(), tabs);
}
