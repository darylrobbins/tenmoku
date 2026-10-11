//! `ViewerModel`'s word moves: the editor's selections, from reading only
//! the text around the caret.

use std::{
    borrow::Cow,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use fe_buffer::{Bias, Buffer, ResolvedSelection, Text};
use fe_command::Invocation;
use fe_editor::{
    EditorModel, Outcome, ViewerModel,
    commands::{self, EditorTarget, HeadlessHost},
};
use fe_text::TextSource;
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};

/// Up to 60 pieces of the alphabet `tests/viewer.rs` draws on, with
/// punctuation added.
fn random_text(rng: &mut StdRng) -> String {
    const PIECES: [&str; 14] = [
        "a", "word ", "é", "日本", "😀", "\t", "  ", "\u{3000}", "\n", "\n\n", "x\ty", "ab\u{a0}",
        "-", "_.",
    ];
    (0..rng.random_range(0..60))
        .map(|_| PIECES[rng.random_range(0..PIECES.len())])
        .collect()
}

#[test]
fn word_moves_match_the_editor() {
    let registry = commands::registry();
    let mut rng = StdRng::seed_from_u64(396);
    let commands = [
        "editor.move_word_left",
        "editor.move_word_right",
        "editor.select_word_left",
        "editor.select_word_right",
    ];
    for _ in 0..25 {
        let source = random_text(&mut rng);
        let text = Text::new(source.as_str());
        let offsets: Vec<usize> = (0..=source.len())
            .filter(|&ix| source.is_char_boundary(ix))
            .collect();
        for &from in &offsets {
            for reversed in [false, true] {
                for to in [from, offsets[rng.random_range(0..offsets.len())]] {
                    let mut runs: Vec<(&str, Value)> =
                        commands.iter().map(|id| (*id, Value::Null)).collect();
                    for motion in ["word_left", "word_right"] {
                        for extend in [false, true] {
                            for count in [1, 2, 5] {
                                runs.push((
                                    "editor.move",
                                    json!({ "motion": motion, "extend": extend, "count": count }),
                                ));
                            }
                        }
                    }
                    for (id, args) in runs {
                        let mut editor = EditorModel::new(Buffer::new(&source), None);
                        let mut host = HeadlessHost {
                            clipboard: None,
                            page_rows: 20,
                        };
                        let mut viewer = ViewerModel::new(Arc::new(text.clone()), 4);
                        let (anchor, head) = if reversed { (to, from) } else { (from, to) };
                        viewer.set_selection(anchor, head);
                        editor.set_selections([ResolvedSelection {
                            range: anchor.min(head)..anchor.max(head),
                            reversed: head < anchor,
                        }]);
                        let outcome = viewer.run(id, &args);
                        assert!(
                            matches!(outcome, Outcome::Changed | Outcome::Nothing),
                            "{id}: {outcome:?}"
                        );
                        let invocation = Invocation::of(id)
                            .unwrap()
                            .args(args.clone())
                            .context(commands::context());
                        registry
                            .dispatch(&mut EditorTarget::new(&mut editor, &mut host), &invocation)
                            .unwrap();
                        assert_eq!(
                            viewer.resolved_selections(),
                            editor.selections(),
                            "{id} {args} from {anchor}..{head} over {source:?}"
                        );
                    }
                }
            }
        }
    }
}

/// A text that counts the bytes its chunks yield.
struct Counting {
    text: Text,
    read: Arc<AtomicUsize>,
}

impl TextSource for Counting {
    fn len(&self) -> usize {
        self.text.len()
    }
    fn row_count(&self) -> u32 {
        TextSource::row_count(&self.text)
    }
    fn row_start(&self, row: u32) -> usize {
        TextSource::row_start(&self.text, row)
    }
    fn row_len(&self, row: u32) -> u32 {
        TextSource::row_len(&self.text, row)
    }
    fn clip_offset(&self, offset: usize, bias: Bias) -> usize {
        TextSource::clip_offset(&self.text, offset, bias)
    }
    fn is_char_boundary(&self, offset: usize) -> bool {
        TextSource::is_char_boundary(&self.text, offset)
    }
    fn char_at(&self, offset: usize) -> Option<char> {
        TextSource::char_at(&self.text, offset)
    }
    fn chunks_in_range(&self, range: Range<usize>) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_> {
        let read = Arc::clone(&self.read);
        Box::new(
            TextSource::chunks_in_range(&self.text, range).inspect(move |chunk| {
                read.fetch_add(chunk.len(), Ordering::Relaxed);
            }),
        )
    }
    fn share(&self) -> Arc<dyn TextSource> {
        Arc::new(Counting {
            text: self.text.clone(),
            read: Arc::clone(&self.read),
        })
    }
}

#[test]
fn reads_only_around_the_caret() {
    const LEN: usize = 64 * 1024 * 1024;
    let source = "word, ".repeat(LEN / 6);
    let read = Arc::new(AtomicUsize::new(0));
    let text = Counting {
        text: Text::new(source.as_str()),
        read: Arc::clone(&read),
    };
    let middle = text.clip_offset(source.len() / 2 + 1, Bias::Left);
    let mut viewer = ViewerModel::new(Arc::new(text), 4);
    viewer.set_selection(middle, middle);
    for id in ["editor.move_word_right", "editor.move_word_left"] {
        read.store(0, Ordering::Relaxed);
        let before = viewer.head();
        assert_eq!(viewer.run(id, &Value::Null), Outcome::Changed);
        assert_ne!(viewer.head(), before);
        let bytes = read.load(Ordering::Relaxed);
        assert!(bytes < 1024 * 1024, "{id} read {bytes} bytes");
    }
}

#[test]
fn crosses_crlf_line_breaks_whole() {
    use fe_fileview::{FileText, FileView, IndexState, LineIndex};
    use std::{thread, time::Duration};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crlf.txt");
    std::fs::write(&path, b"ab\r\ncd\r\n\r\nef").unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let index = Arc::new(LineIndex::build(Arc::clone(&view)));
    while index.progress().state == IndexState::Building {
        thread::sleep(Duration::from_millis(1));
    }
    let text = FileText::new(Arc::clone(&view), index);
    let mut viewer = ViewerModel::new(Arc::new(text.clone()), 4);

    let mut stops = vec![viewer.head()];
    while viewer.run("editor.move_word_right", &Value::Null) == Outcome::Changed {
        stops.push(viewer.head());
    }
    for &stop in &stops {
        assert!(text.is_char_boundary(stop), "{stops:?}");
    }
    let mut back = vec![viewer.head()];
    while viewer.run("editor.move_word_left", &Value::Null) == Outcome::Changed {
        back.push(viewer.head());
    }
    for &stop in &back {
        assert!(text.is_char_boundary(stop), "{back:?}");
    }
    assert_eq!(back.last(), Some(&0));
    // A selecting move over a break lands on boundaries as well.
    viewer.set_selection(2, 2);
    viewer.run("editor.select_word_right", &Value::Null);
    assert!(text.is_char_boundary(viewer.head()));
}

#[test]
fn long_runs_cross_backward_windows() {
    // Runs and gaps longer than a read window, with a boundary just inside
    // a later one.
    for gap in [0, 1, 65_535, 65_536, 65_537, 200_000] {
        let source = format!(
            "ab{}{}{}",
            " ".repeat(gap),
            "x".repeat(150_000),
            "-".repeat(3)
        );
        let text = Text::new(source.as_str());
        let mut editor = EditorModel::new(Buffer::new(&source), None);
        let mut host = HeadlessHost {
            clipboard: None,
            page_rows: 20,
        };
        let registry = commands::registry();
        let mut viewer = ViewerModel::new(Arc::new(text), 4);
        let end = source.len();
        viewer.set_selection(end, end);
        editor.set_selections([ResolvedSelection {
            range: end..end,
            reversed: false,
        }]);
        for _ in 0..4 {
            viewer.run("editor.move_word_left", &Value::Null);
            let invocation = Invocation::of("editor.move_word_left")
                .unwrap()
                .context(commands::context());
            registry
                .dispatch(&mut EditorTarget::new(&mut editor, &mut host), &invocation)
                .unwrap();
            assert_eq!(
                viewer.resolved_selections(),
                editor.selections(),
                "gap {gap}"
            );
        }
    }
}
