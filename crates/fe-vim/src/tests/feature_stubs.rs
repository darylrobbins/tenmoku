//! The M7 breadth cards' motions, text objects and operators, before their
//! cards: each reaches its stub module and changes nothing.

use serde::{Serialize, de::DeserializeOwned};

use super::invocation;
use crate::{
    HeadlessVim, Motion, MoveArgs, OperateArgs, Operator, SelectArgs, Target, TextObject, context,
};

const MOTIONS: [(Motion, &str); 14] = [
    (Motion::ParagraphForward, "paragraph_forward"),
    (Motion::ParagraphBackward, "paragraph_backward"),
    (Motion::SentenceForward, "sentence_forward"),
    (Motion::SentenceBackward, "sentence_backward"),
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

const OBJECTS: [(TextObject, &str); 12] = [
    (TextObject::InnerParagraph, "inner_paragraph"),
    (TextObject::AParagraph, "a_paragraph"),
    (TextObject::InnerSentence, "inner_sentence"),
    (TextObject::ASentence, "a_sentence"),
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

/// The `g` operators, over a motion.
const G_OPERATORS: [Operator; 6] = [
    Operator::Lowercase,
    Operator::Uppercase,
    Operator::ToggleCase,
    Operator::Rot13,
    Operator::Format,
    Operator::FormatKeep,
];

const TEXT: &str = "Some text. More (text) [here]\n\n<a>`x`</a> end\nlast row";

/// A vim with one change made (so there's something to undo), in a visual
/// mode if `visual`.
fn vim(visual: bool) -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("xjl").unwrap();
    if visual {
        vim.feed("vl").unwrap();
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
    for (motion, name) in MOTIONS {
        for extend in [false, true] {
            let args = MoveArgs {
                motion,
                extend,
                count: None,
            };
            changes_nothing(extend, "vim.move", args, &format!("move {name}"));
        }
        for operator in G_OPERATORS {
            let args = OperateArgs {
                operator,
                target: Target::Motion(motion),
                count: None,
                register: None,
            };
            changes_nothing(false, "vim.operate", args, &format!("{operator:?} {name}"));
        }
        round_trips(motion, name);
    }
    for (object, name) in OBJECTS {
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
    assert_eq!(names(marks::settable), "'`abcdefghijklmnopqrstuvwxyz");
    assert_eq!(names(marks::jumpable), "'<>`abcdefghijklmnopqrstuvwxyz");
}
