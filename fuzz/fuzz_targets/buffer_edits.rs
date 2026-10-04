//! Streams of edits, undos and redos from the user and an AI, checked against
//! a `String`. Selective undo across origins has no simple model, so after an
//! undo or redo the model takes the buffer's text; what's checked is that
//! edits apply exactly and a refused undo or redo changes nothing. With a
//! single origin undo is plain, so undoing everything must restore the
//! original text and redoing it the final text.

#![no_main]

use arbitrary::Arbitrary;
use fe_buffer::{Bias, Buffer, Origin, Text, UndoError};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    single_origin: bool,
    text: Vec<u8>,
    ops: Vec<Op>,
}

#[derive(Debug, Arbitrary)]
enum Op {
    Edit {
        ai: bool,
        start: u16,
        end: u16,
        text: Vec<u8>,
    },
    Undo {
        ai: bool,
    },
    Redo {
        ai: bool,
    },
}

fuzz_target!(|input: Input| run(input));

fn run(input: Input) {
    let original = decode(&input.text);
    let mut buffer = Buffer::new(&original);
    let mut model = original.clone();
    let origin = |ai: bool| {
        if ai && !input.single_origin {
            Origin::Ai {
                agent: "agent".into(),
                session: "session".into(),
            }
        } else {
            Origin::User
        }
    };

    for op in &input.ops {
        match op {
            Op::Edit {
                ai,
                start,
                end,
                text,
            } => {
                let start = boundary(buffer.text(), *start);
                let end = boundary(buffer.text(), *end);
                let range = start.min(end)..start.max(end);
                let text = decode(text);
                model.replace_range(range.clone(), &text);
                buffer.edit([(range, text)], origin(*ai));
                assert_eq!(buffer.text().to_string(), model, "after {op:?}");
            }
            Op::Undo { ai } | Op::Redo { ai } => {
                let origin = origin(*ai);
                let result = if matches!(op, Op::Undo { .. }) {
                    buffer.undo(|o| *o == origin)
                } else {
                    buffer.redo(|o| *o == origin)
                };
                match result {
                    Ok(Some(_)) => model = buffer.text().to_string(),
                    Ok(None) | Err(UndoError::Conflict(_)) => {
                        assert_eq!(buffer.text().to_string(), model, "{op:?} -> {result:?}");
                    }
                }
                assert!(
                    !input.single_origin || result.is_ok(),
                    "single-origin {op:?} refused"
                );
            }
        }
    }

    if !input.single_origin {
        return;
    }
    let mut undone = 0;
    while buffer.undo(|_| true).expect("single-origin undo").is_some() {
        undone += 1;
    }
    assert_eq!(buffer.text().to_string(), original, "undoing everything");
    for _ in 0..undone {
        let redone = buffer.redo(|_| true).expect("single-origin redo");
        assert!(redone.is_some(), "redo after {undone} undos");
    }
    assert_eq!(buffer.text().to_string(), model, "redoing everything");
}

/// `bytes` as UTF-8 with `\n` line endings, as a buffer holds it.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// An offset into `text` from a raw one, moved back to a char boundary.
fn boundary(text: &Text, raw: u16) -> usize {
    text.clip_offset(usize::from(raw) % (text.len() + 1), Bias::Left)
}
