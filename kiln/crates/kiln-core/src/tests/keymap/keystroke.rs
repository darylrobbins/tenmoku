// SPDX-License-Identifier: Apache-2.0

use crate::{Key, KeySequence, Keystroke, Modifiers};

fn parse(source: &str) -> Keystroke {
    Keystroke::parse(source).unwrap_or_else(|error| panic!("{source:?}: {error}"))
}

#[test]
fn parse_and_normalise() {
    // Modifiers in any order, printed in a fixed one.
    let a = parse("cmd-shift-left");
    let b = parse("shift-cmd-left");
    assert_eq!(a, b);
    assert_eq!(a.to_string(), "cmd-shift-left");
    assert_eq!(b.to_string(), "cmd-shift-left");
    assert_eq!(
        a,
        Keystroke::new(
            Modifiers {
                cmd: true,
                shift: true,
                ..Modifiers::default()
            },
            Key::Left
        )
    );
    assert_eq!(parse("Fn-Alt-Ctrl-Cmd-K").to_string(), "cmd-ctrl-alt-fn-k");

    // Punctuation, function keys and named keys, each round-tripping.
    for (source, key, printed) in [
        ("cmd--", Key::Char('-'), "cmd--"),
        ("cmd-[", Key::Char('['), "cmd-["),
        ("ctrl-]", Key::Char(']'), "ctrl-]"),
        ("-", Key::Char('-'), "-"),
        ("shift--", Key::Char('-'), "shift--"),
        ("f12", Key::F(12), "f12"),
        ("F20", Key::F(20), "f20"),
        ("space", Key::Space, "space"),
        ("alt-PageDown", Key::PageDown, "alt-pagedown"),
    ] {
        let keystroke = parse(source);
        assert_eq!(keystroke.key, key, "{source}");
        assert_eq!(keystroke.to_string(), printed, "{source}");
        assert_eq!(parse(printed), keystroke, "{printed} parses back");
    }

    // Sequences are whitespace-separated.
    let chord = KeySequence::parse("cmd-k  cmd-0").unwrap();
    assert_eq!(chord.keystrokes(), [parse("cmd-k"), parse("cmd-0")]);
    assert_eq!(chord.to_string(), "cmd-k cmd-0");

    // Unknown modifiers and keys are errors, at the offending part.
    for (source, offset) in [
        ("hyper-k", 0),
        ("cmd-meta-k", 4),
        ("cmd-cmd-k", 4),
        ("cmd-f21", 4),
        ("cmd-f0", 4),
        ("cmd-", 4),
        ("cmd-pgup", 4),
        ("--", 0),
        ("cmd---", 4),
    ] {
        let error = Keystroke::parse(source).expect_err(source);
        assert_eq!(error.offset, offset, "{source}: {error}");
    }
    let error = KeySequence::parse("cmd-k cmd-bogus").unwrap_err();
    assert_eq!(error.offset, 10, "{error}");
    assert!(KeySequence::parse("  ").is_err());
}
