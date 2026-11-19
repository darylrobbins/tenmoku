//! The `# %%` cell parser: which lines are markers, and what a cell runs.

use std::ops::Range;

use super::{Cell, CellKind, CellSource, cell_at_row, cell_source, markdown_text, parse_cells};

/// A cell's marker row, body rows, kind and title.
type Summary = (Option<u32>, Range<u32>, CellKind, Option<String>);

/// The cells' summaries.
fn summary(text: &str) -> Vec<Summary> {
    parse_cells(text)
        .into_iter()
        .map(
            |Cell {
                 marker_row,
                 rows,
                 kind,
                 title,
             }| (marker_row, rows, kind, title),
        )
        .collect()
}

fn title(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn splits_on_percent_lines_and_markdown_variants() {
    use CellKind::{Code, Markdown};
    let text = "\
import os
# %%
x = 1
#%%
y = 2
# %% Load the data
z = 3
# %% [markdown]
# # A heading
# Some *text*.
# %% [md] Notes
# more
";
    assert_eq!(
        summary(text),
        vec![
            (None, 0..1, Code, None),
            (Some(1), 2..3, Code, None),
            (Some(3), 4..5, Code, None),
            (Some(5), 6..7, Code, title("Load the data")),
            (Some(7), 8..10, Markdown, None),
            (Some(10), 11..13, Markdown, title("Notes")),
        ]
    );
    let cells = parse_cells(text);
    assert_eq!(markdown_text(&cells[4], text), "# A heading\nSome *text*.");
    assert_eq!(markdown_text(&cells[5], text), "more");
    assert_eq!(markdown_text(&cells[1], text), "");

    // Not markers: a third %, a word straight after, an indented marker, a
    // marker further along a line, and one inside a triple-quoted string
    // (either quote), even after a one-line string or comment that holds the
    // other delimiter.
    let text = "\
# %%
a = 1
# %%%
# %%x
    # %%
b = 2  # %% not here
s = \"\"\"
# %% in a string
\"\"\"
t = '''x
# %% also in a string
'''
u = \"'''\"  # '''
# %% after
c = 3
";
    assert_eq!(
        summary(text),
        vec![
            (Some(0), 1..13, Code, None),
            (Some(13), 14..16, Code, title("after")),
        ]
    );

    // An escaped quote doesn't end a string; CRLF line endings are rows too.
    let text = "# %%\ns = \"\"\"\\\"\"\"\n# %% hidden\n\"\"\"\r\n# %%\r\nd = 4\r\n";
    assert_eq!(
        summary(text),
        vec![(Some(0), 1..4, Code, None), (Some(4), 5..7, Code, None)]
    );

    // A one-quote string continued by a backslash hides a marker, either
    // quote; an escaped backslash at a line's end doesn't continue one.
    let text =
        "# %%\ns = 'a\\\n# %% in a string'\nt = \"b\\\n# %% also\"\nu = 'c\\\\'\n# %% real\n";
    assert_eq!(
        summary(text),
        vec![
            (Some(0), 1..6, Code, None),
            (Some(6), 7..8, Code, title("real"))
        ]
    );

    // A markdown tag is one only on its own, or before whitespace and a
    // title.
    for marker in ["# %% [markdown]ing", "# %% [md]notes"] {
        let text = format!("{marker}\nx = 1\n");
        let cells = parse_cells(&text);
        assert_eq!(cells[0].kind, Code, "{marker}");
        assert_eq!(cell_source(&cells[0], &text).code, "x = 1");
    }
    assert_eq!(summary("# %% [md]\tTabbed\n")[0].2, Markdown);

    // No marker, no cells; a blank preamble is no cell.
    assert_eq!(summary("x = 1\nprint(x)\n"), vec![]);
    assert_eq!(summary(""), vec![]);
    assert_eq!(
        summary("\n  \n# %%\nx\n"),
        vec![(Some(2), 3..5, Code, None)]
    );
    // A file that ends on a marker has an empty last cell.
    assert_eq!(
        summary("x\n# %%"),
        vec![(None, 0..1, Code, None), (Some(1), 2..2, Code, None)]
    );
}

#[test]
fn cell_source_keeps_file_line_numbers() {
    let text = "\
import math
# %% first

x = 2
y = x * 3


# %% [markdown]
# Not code
# %%
print(y)";
    let cells = parse_cells(text);
    assert_eq!(cells.len(), 4);
    // The preamble starts at line 1.
    assert_eq!(
        cell_source(&cells[0], text),
        CellSource {
            code: "import math".into(),
            first_line: 1,
        }
    );
    // A leading blank row stays, so the lines line up; trailing ones go.
    assert_eq!(
        cell_source(&cells[1], text),
        CellSource {
            code: "\nx = 2\ny = x * 3".into(),
            first_line: 3,
        }
    );
    // Markdown runs nothing.
    assert_eq!(
        cell_source(&cells[2], text),
        CellSource {
            code: String::new(),
            first_line: 9,
        }
    );
    // The last cell, with no newline at the end.
    assert_eq!(
        cell_source(&cells[3], text),
        CellSource {
            code: "print(y)".into(),
            first_line: 11,
        }
    );
    // A row of a marker or body finds its cell.
    assert_eq!(cell_at_row(&cells, 1), Some(&cells[1]));
    assert_eq!(cell_at_row(&cells, 6), Some(&cells[1]));
    assert_eq!(cell_at_row(&cells, 10), Some(&cells[3]));
    assert_eq!(cell_at_row(&cells, 11), None);
}
