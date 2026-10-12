//! The M7 breadth cards' motions, text objects and operators, before their
//! cards: each reaches its stub module and changes nothing.

use serde::{Serialize, de::DeserializeOwned};

use super::invocation;
use crate::{HeadlessVim, Motion, OperateArgs, Operator, SelectArgs, Target, TextObject, context};

/// The prose card's motions and objects (T415), which now work: only their
/// names are checked here.
const PROSE_MOTIONS: [(Motion, &str); 4] = [
    (Motion::ParagraphForward, "paragraph_forward"),
    (Motion::ParagraphBackward, "paragraph_backward"),
    (Motion::SentenceForward, "sentence_forward"),
    (Motion::SentenceBackward, "sentence_backward"),
];

const PROSE_OBJECTS: [(TextObject, &str); 4] = [
    (TextObject::InnerParagraph, "inner_paragraph"),
    (TextObject::AParagraph, "a_paragraph"),
    (TextObject::InnerSentence, "inner_sentence"),
    (TextObject::ASentence, "a_sentence"),
];

const MOTIONS: [(Motion, &str); 10] = [
    (Motion::WordEndBackward, "word_end_backward"),
    (Motion::BigWordEndBackward, "big_word_end_backward"),
    (Motion::NextLineFirstNonBlank, "next_line_first_non_blank"),
    (Motion::PrevLineFirstNonBlank, "prev_line_first_non_blank"),
    (Motion::Column, "column"),
    (Motion::WindowTop, "window_top"),
    (Motion::WindowMiddle, "window_middle"),
    (Motion::WindowBottom, "window_bottom"),
    (Motion::DisplayDown, "display_down"),
    (Motion::DisplayUp, "display_up"),
];

const OBJECTS: [(TextObject, &str); 8] = [
    (TextObject::InnerBracket, "inner_bracket"),
    (TextObject::ABracket, "a_bracket"),
    (TextObject::InnerAngle, "inner_angle"),
    (TextObject::AAngle, "a_angle"),
    (TextObject::InnerTag, "inner_tag"),
    (TextObject::ATag, "a_tag"),
    (TextObject::InnerBacktick, "inner_backtick"),
    (TextObject::ABacktick, "a_backtick"),
];

const OPERATORS: [(Operator, &str); 3] = [
    (Operator::Rot13, "rot13"),
    (Operator::Format, "format"),
    (Operator::FormatKeep, "format_keep"),
];

const TEXT: &str = "Some text. More (text) [here]\n\n<a>`x`</a> end\nlast row";

/// A vim with one change made (so there's something to undo), on an interior
/// column of a populated row, with distinct anchor and cursor if `visual`.
fn vim(visual: bool) -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("x3l").unwrap();
    assert_eq!(vim.cursor(), (0, 3), "fixture: interior of a populated row");
    if visual {
        vim.feed("vl").unwrap();
        assert_eq!(vim.visual_anchor(), Some((0, 3)), "fixture: anchor");
        assert_eq!(vim.cursor(), (0, 4), "fixture: cursor apart from anchor");
    }
    vim
}

/// Runs `command` with `args` (failing or not) and checks the text, the
/// caret and the undo stack didn't change: the text's version is the same,
/// and `u` then undoes the one change before it and nothing more.
fn changes_nothing(visual: bool, command: &str, args: impl Serialize, what: &str) {
    let mut vim = vim(visual);
    let before = (
        vim.text(),
        vim.cursor(),
        vim.visual_anchor(),
        vim.model().snapshot().version(),
    );
    let invocation = invocation(command, args).context(context(vim.state()));
    let _ = vim.dispatch(&invocation);
    let after = (
        vim.text(),
        vim.cursor(),
        vim.visual_anchor(),
        vim.model().snapshot().version(),
    );
    assert_eq!(after, before, "{what}");
    vim.feed("<Esc>u").unwrap();
    assert_eq!(vim.text(), TEXT, "{what}: u undoes the change before it");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), TEXT, "{what}: and there's nothing more to undo");
}

/// `value` serializes as `name` and back.
fn round_trips<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: T,
    name: &str,
) {
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json, serde_json::json!(name));
    assert_eq!(serde_json::from_value::<T>(json).unwrap(), value);
}

#[test]
fn new_variants_change_nothing() {
    // T416's line motions work: only their names are checked.
    for (motion, name) in MOTIONS {
        round_trips(motion, name);
    }
    for (object, name) in OBJECTS {
        // T417 made the delimiter objects work.
        if name.ends_with("_bracket")
            || name.ends_with("_angle")
            || name.ends_with("_tag")
            || name.ends_with("_backtick")
        {
            round_trips(object, name);
            continue;
        }
        for operator in [Operator::Delete, Operator::Yank, Operator::Change] {
            let args = OperateArgs {
                operator,
                target: Target::TextObject(object),
                count: None,
                register: None,
            };
            changes_nothing(false, "vim.operate", args, &format!("{operator:?} {name}"));
        }
        let args = SelectArgs {
            text_object: object,
            count: None,
        };
        changes_nothing(true, "vim.select", args, &format!("select {name}"));
        round_trips(object, name);
    }
    for (operator, name) in OPERATORS {
        round_trips(operator, name);
    }
    for (motion, name) in PROSE_MOTIONS {
        round_trips(motion, name);
    }
    for (object, name) in PROSE_OBJECTS {
        round_trips(object, name);
    }
}

#[test]
fn hooks_commit_as_keys_do() {
    use crate::{CommandLineArgs, CommandLineKind, Pending, Step, VimKey, marks};

    let waiting = |step| match step {
        Step::Wait(pending) => pending,
        other => panic!("{other:?}, not Wait"),
    };
    // `with_operator` then `motion` is `dw`; `operate` over `line` is `dd`.
    let d = waiting(Pending::new().with_operator(Operator::Delete));
    assert_eq!(d.operator(), Some(Operator::Delete));
    assert_eq!(d.clone().motion(Motion::WordForward), super::run("dw"));
    assert_eq!(
        Pending::new().operate(Operator::Delete, Target::Motion(Motion::Line)),
        super::run("dd")
    );
    // `?` after `g?` is left to `step`; `/` still opens a line.
    let rot13 = waiting(Pending::new().with_operator(Operator::Rot13));
    assert!(rot13.clone().command_line(VimKey::Char('?')).is_err());
    assert_eq!(
        rot13.command_line(VimKey::Char('/')),
        Ok(crate::reducer::invocation(
            "vim.command_line",
            CommandLineArgs {
                kind: CommandLineKind::SearchForward,
                count: None,
                register: None,
                operator: Some(Operator::Rot13),
            }
        ))
    );
    assert!(d.command_line(VimKey::Char('?')).is_ok());
    // The mark names are today's.
    let names = |of: fn(char) -> bool| (' '..='~').filter(|&ch| of(ch)).collect::<String>();
    let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let lower = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(names(marks::settable), format!("'{upper}[]`{lower}"));
    assert_eq!(names(marks::jumpable), format!("'.<>{upper}[]^`{lower}"));
}
