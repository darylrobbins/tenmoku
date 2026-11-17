//! Words for the completion menu (fe-editor's `word_completion`): a
//! buffer's identifiers of three chars or more, offered after a server's
//! items and only those it didn't offer, the word being typed left out.

use std::sync::Arc;

use fe_buffer::{Buffer, Text};
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionSession},
    word_completion::{
        BufferWords, MAX_WORDS, OtherWords, SCAN_BYTES, WORD_GROUP, merge_words, word_at,
        word_items,
    },
};

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

#[test]
fn words_are_identifiers_of_three_chars_or_more() {
    let words = BufferWords::of_str("let ab = abc_d + abc_d; // Ünïcode 42nd x9z _ok fn\n");
    let mut found: Vec<&str> = words.words().collect();
    found.sort_unstable();
    assert_eq!(found, ["_ok", "abc_d", "let", "x9z", "Ünïcode"]);
    assert_eq!(words.count("abc_d"), 2);
    assert_eq!(words.count("ab"), 0, "two chars");
    assert_eq!(words.count("42nd"), 0, "starts with a digit");
}

#[test]
fn a_run_too_long_isnt_a_word() {
    let long = "a".repeat(101);
    let text = format!("{long} fine {}", "b".repeat(100));
    let words = BufferWords::of_str(&text);
    let mut found: Vec<String> = words.words().map(str::to_owned).collect();
    found.sort_unstable();
    assert_eq!(found, ["b".repeat(100), "fine".to_owned()]);
}

#[test]
fn a_scan_keeps_at_most_max_words() {
    let text: String = (0..MAX_WORDS + 50).map(|n| format!("w{n:05} ")).collect();
    let words = BufferWords::of_str(&text);
    assert_eq!(words.len(), MAX_WORDS);
    // The first found are those kept.
    assert_eq!(words.count("w00000"), 1);
    assert_eq!(words.count(&format!("w{:05}", MAX_WORDS + 49)), 0);
}

#[test]
fn a_long_buffer_is_scanned_around_the_caret() {
    let filler = "zz ".repeat(SCAN_BYTES / 3 + 10);
    let text = Text::new(&format!("start {filler}middle {filler}end"));
    let middle = text.to_string().find("middle").unwrap();
    let around = BufferWords::scan(&text, middle);
    assert_eq!(around.count("middle"), 1);
    assert_eq!(around.count("start"), 0);
    assert_eq!(around.count("end"), 0);
    let at_start = BufferWords::scan(&text, 0);
    assert_eq!(at_start.count("start"), 1);
    assert_eq!(at_start.count("end"), 0);
    // A short one whole.
    let short = Text::new("start middle end");
    assert_eq!(BufferWords::scan(&short, 3).len(), 3);
}

#[test]
fn the_word_at_a_caret_spans_its_row() {
    let text = Text::new("one two_three\nfour");
    assert_eq!(word_at(&text, 6), 4..13, "inside");
    assert_eq!(word_at(&text, 13), 4..13, "at its end");
    assert_eq!(word_at(&text, 4), 4..13, "at its start");
    assert_eq!(word_at(&text, 3), 0..3, "touching the one before");
    assert_eq!(word_at(&text, 14), 14..18, "not across the row");
    let spaced = Text::new("a  b");
    assert_eq!(word_at(&spaced, 2), 2..2, "none");
}

#[test]
fn items_leave_out_the_typed_word_and_rank_this_buffer_first() {
    let current = BufferWords::of_str("alpha alpine alp");
    let other = OtherWords {
        words: Arc::new(BufferWords::of_str("alpha alto")),
        detail: Some("Word in lib.rs".into()),
    };
    let items = word_items(&current, "alp", &[other], 10..13, "Text");
    assert_eq!(labels(&items), ["alpha", "alpine", "alto"]);
    for item in &items {
        assert_eq!(item.group, WORD_GROUP);
        assert_eq!(item.kind.as_deref(), Some("Text"));
        assert_eq!(item.edit, (10..13, item.label.clone()));
        assert!(!item.snippet);
    }
    // In this buffer and another: this one's, no detail.
    assert_eq!(items[0].detail, None);
    assert_eq!(items[2].detail.as_deref(), Some("Word in lib.rs"));
    let sort = |ix: usize| items[ix].sort_text.clone().unwrap();
    assert!(sort(1) < sort(2), "this buffer's before others'");
}

#[test]
fn without_a_server_the_words_are_the_list() {
    let items = word_items(&BufferWords::of_str("alpha"), "al", &[], 0..2, "Text");
    let list = merge_words(None, items).expect("a list");
    assert_eq!(labels(&list.items), ["alpha"]);
    assert!(!list.is_incomplete);
    assert!(merge_words(None, Vec::new()).is_none());
}

fn server_item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.into(),
        edit: (0..2, label.into()),
        ..CompletionItem::default()
    }
}

#[test]
fn words_follow_a_server_s_items_without_its_duplicates() {
    let server = CompletionList {
        items: vec![
            server_item("alpha"),
            CompletionItem {
                filter_text: Some("alto".into()),
                ..server_item("alto(…)")
            },
            CompletionItem {
                edit: (0..2, "alright".into()),
                ..server_item("alright — fn")
            },
        ],
        is_incomplete: true,
    };
    let words = BufferWords::of_str("alpha alto alright alpine");
    let items = word_items(&words, "al", &[], 0..2, "Text");
    let list = merge_words(Some(server), items).unwrap();
    assert_eq!(
        labels(&list.items),
        ["alpha", "alto(…)", "alright — fn", "alpine"]
    );
    assert!(list.is_incomplete, "kept the server's");

    // A snippet's text isn't a word it offers.
    let snippet = CompletionList {
        items: vec![CompletionItem {
            snippet: true,
            edit: (0..2, "alpine".into()),
            ..server_item("alp")
        }],
        is_incomplete: false,
    };
    let items = word_items(&words, "al", &[], 0..2, "Text");
    let list = merge_words(Some(snippet), items).unwrap();
    assert!(labels(&list.items).contains(&"alpine"));
}

#[test]
fn a_session_ranks_a_server_s_items_before_better_matching_words() {
    let text = "alpha alto\nal";
    let buffer = Buffer::new(text);
    let snapshot = buffer.snapshot();
    let mut model = EditorModel::new(buffer, None);
    model.set_cursor(text.len());
    let caret = text.len();
    let words = BufferWords::scan(snapshot.text(), caret);
    let items = word_items(&words, "al", &[], caret - 2..caret, "Text");
    let server = CompletionList {
        items: vec![CompletionItem {
            edit: (caret - 2..caret, "xalyz".into()),
            ..server_item("xalyz")
        }],
        is_incomplete: false,
    };
    let list = merge_words(Some(server), items).unwrap();
    let mut session = CompletionSession::new(snapshot, caret, list);
    assert!(session.update(&model));
    let ranked: Vec<&str> = (session.matches().iter())
        .map(|&ix| session.items()[ix].label.as_str())
        .collect();
    assert_eq!(ranked, ["xalyz", "alpha", "alto"]);

    // Accepting a word puts it in place of what was typed.
    session.select(session.matches()[1]);
    session.accept(&mut model).unwrap();
    assert_eq!(model.text().to_string(), "alpha alto\nalpha");
}

#[test]
fn a_word_cut_by_the_scan_s_edge_isnt_offered() {
    // The window, a MiB around the caret, starts and ends inside a word.
    let half = SCAN_BYTES / 2;
    let filler = |n: usize| " ".repeat(n);
    let text = format!(
        "{}alphabet{}middle{}omegaword",
        filler(1000),
        filler(half - 4),
        filler(half - 10)
    );
    let text = Text::new(&text);
    let middle = text.to_string().find("middle").unwrap();
    let words = BufferWords::scan(&text, middle);
    assert_eq!(words.count("middle"), 1);
    let mut found: Vec<&str> = words.words().collect();
    found.sort_unstable();
    assert_eq!(found, ["middle"], "no `abet` nor `omeg`");
}

#[test]
fn a_scan_serves_a_caret_well_inside_what_it_read() {
    let short = Text::new("one two three");
    let words = BufferWords::scan(&short, 0);
    assert!(words.covers(0) && words.covers(13), "all of a short text");

    let long = Text::new(&"ab ".repeat(SCAN_BYTES));
    let len = long.len();
    let at_start = BufferWords::scan(&long, 0);
    assert!(at_start.covers(0));
    assert!(at_start.covers(SCAN_BYTES / 2));
    assert!(!at_start.covers(SCAN_BYTES), "near the window's far edge");
    assert!(!at_start.covers(len - 1));
    let at_end = BufferWords::scan(&long, len);
    assert!(at_end.covers(len));
    assert!(!at_end.covers(0));
}

#[test]
fn the_word_at_a_caret_reads_a_bounded_stretch_of_a_long_row() {
    // One row of two MiB: the word is found all the same.
    let row = "ab cd ".repeat(SCAN_BYTES / 3);
    let text = Text::new(&format!("{row}target {row}"));
    let at = row.len() + 3;
    assert_eq!(word_at(&text, at), row.len()..row.len() + 6);
    // A run far longer than any word is cut, not read whole.
    let run = Text::new(&"x".repeat(SCAN_BYTES));
    let cut = word_at(&run, SCAN_BYTES / 2);
    assert!(cut.len() < 1000, "{cut:?}");
    assert!(cut.contains(&(SCAN_BYTES / 2)));
}
