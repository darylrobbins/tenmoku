use fe_chat::{Budgeted, ContextItem, ContextKind, Fit, Rows, budget, render_user_content};

fn item(kind: ContextKind, label: &str, text: &str) -> ContextItem {
    ContextItem {
        kind,
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

fn file(path: &str, text: &str) -> ContextItem {
    let kind = ContextKind::File {
        path: path.to_owned(),
        rows: None,
    };
    item(kind, path, text)
}

/// `count` rows of 9 bytes each, newline included: 3 tokens a row.
fn rows(count: usize) -> String {
    (0..count).map(|row| format!("row {row:04}\n")).collect()
}

fn labels(budgeted: &[Budgeted]) -> Vec<&str> {
    budgeted.iter().map(|b| b.item.label.as_str()).collect()
}

fn fits(budgeted: &[Budgeted]) -> Vec<Fit> {
    budgeted.iter().map(|b| b.fit).collect()
}

#[test]
fn budget_fills_in_order_and_truncates_at_rows() {
    // The estimate: a token for every 3 bytes, rounded up.
    assert_eq!(file("a", "").estimate(), 0);
    assert_eq!(file("a", "ab").estimate(), 1);
    assert_eq!(file("a", "abc").estimate(), 1);
    assert_eq!(file("a", "abcd").estimate(), 2);
    assert_eq!(file("a", &rows(100)).estimate(), 300);

    let selection = item(
        ContextKind::Selection {
            path: "src/lib.rs".to_owned(),
            rows: Rows { first: 5, last: 6 },
        },
        "lib.rs:5-6",
        "fn a() {}\n}\n",
    );
    let diagnostics = item(ContextKind::Diagnostics, "Problems", "error 3: bad\n");
    let active = item(
        ContextKind::ActiveFile {
            path: "src/main.rs".to_owned(),
            rows: None,
        },
        "main.rs",
        &rows(100),
    );
    let open = item(ContextKind::OpenEditors, "Open Editors", "src/main.rs\n");
    let (first, second) = (file("b.rs", "b\n"), file("a.rs", "a\n"));
    // Attached in any order, filled selection, files in the order given,
    // diagnostics, the active file, then the open editors.
    let items = [
        open.clone(),
        active.clone(),
        first.clone(),
        diagnostics.clone(),
        second.clone(),
        selection.clone(),
    ];
    let order = [
        "lib.rs:5-6",
        "b.rs",
        "a.rs",
        "Problems",
        "main.rs",
        "Open Editors",
    ];
    let whole = budget(&items, 10_000);
    assert_eq!(labels(&whole), order);
    assert_eq!(fits(&whole), [Fit::Whole; 6]);
    let indices: Vec<usize> = whole.iter().map(|b| b.index).collect();
    assert_eq!(indices, [5, 2, 4, 3, 1, 0]);
    assert!(whole.iter().all(|b| b.item == items[b.index]));
    let estimates: Vec<u32> = whole.iter().map(|b| b.estimate).collect();
    assert_eq!(estimates, [4, 1, 1, 5, 300, 4]);

    // Room for the first four and 50 rows of the active file's 100: it's
    // cut after the most rows that fit with the truncation line, 47 rows
    // and 25 bytes in 150 tokens, which names the rest. The open editors
    // don't fit after it.
    let before = 4 + 1 + 1 + 5;
    let limit = before + 50 * 3;
    let cut = budget(&items, limit);
    assert_eq!(labels(&cut), order);
    assert_eq!(
        fits(&cut),
        [
            Fit::Whole,
            Fit::Whole,
            Fit::Whole,
            Fit::Whole,
            Fit::Truncated {
                rows_kept: 47,
                rows_total: 100
            },
            Fit::Dropped,
        ]
    );
    let truncated = &cut[4];
    assert_eq!(
        truncated.item.text,
        format!("{}[truncated: 53 more rows]", rows(47))
    );
    assert_eq!(truncated.item.estimate(), 150);
    assert_eq!(truncated.estimate, 300, "the whole item's");
    assert_eq!(
        truncated.item.kind,
        ContextKind::ActiveFile {
            path: "src/main.rs".to_owned(),
            rows: Some(Rows { first: 1, last: 47 }),
        }
    );
    // The tag carries the rows kept; the dropped item isn't sent.
    let sent: Vec<ContextItem> = (cut.iter())
        .filter(|b| b.sent())
        .map(|b| b.item.clone())
        .collect();
    let content = render_user_content("Why?", &sent);
    assert_eq!(content.len(), 6);
    assert_eq!(
        content[4]["text"],
        format!(
            "<file path=\"src/main.rs\" rows=\"1-47\">\n{}[truncated: 53 more rows]\n</file>",
            rows(47)
        )
    );
    assert_eq!(content[5]["text"], "Why?");

    // A selection cut keeps its own first row.
    let long = item(
        ContextKind::Selection {
            path: "src/lib.rs".to_owned(),
            rows: Rows {
                first: 20,
                last: 119,
            },
        },
        "lib.rs:20-119",
        &rows(100),
    );
    let cut = budget(std::slice::from_ref(&long), 40);
    assert_eq!(
        cut[0].item.kind,
        ContextKind::Selection {
            path: "src/lib.rs".to_owned(),
            rows: Rows {
                first: 20,
                last: 29
            },
        }
    );
    assert_eq!(
        cut[0].fit,
        Fit::Truncated {
            rows_kept: 10,
            rows_total: 100
        }
    );

    // Fewer than 10 rows fit: dropped, and what's after still fills.
    let short = budget(&[long.clone(), first.clone()], 30);
    assert_eq!(fits(&short), [Fit::Dropped, Fit::Whole]);
    assert_eq!(short[0].item, long, "a dropped item is left whole");
    // As is one with fewer than 10 rows at all.
    let few = budget(&[file("c.rs", &rows(9))], 26);
    assert_eq!(fits(&few), [Fit::Dropped]);
    // An empty budget sends only what estimates to nothing.
    let none = budget(&[file("d.rs", ""), first], 0);
    assert_eq!(fits(&none), [Fit::Whole, Fit::Dropped]);
}
