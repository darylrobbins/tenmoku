//! The ranges and counts of the buffer commands, as nvim 0.12.4 reads them
//! (`nvim --clean`, three buffers listed): what the parse refuses, and what
//! it leaves for the host to check against its buffer numbers.
//!
//! - `:0b`, `:0bd`, `:0b 2` and `:0,2bd` are E16; so is a negative address
//!   (`:2147483648bn` wraps to one), but `:0bn`, `:0,2bf` are fine.
//! - `:9b`, `:9b 2` and `:1,9bd` are E16 (past the highest buffer), but
//!   `:9bn`, `:9bf` and `:1,9bn` are not: the host's to say for `:b`/`:bd`.
//! - `:2,0b`, `:3,0bd 1` and `:9,1b` are E493, not E16: backwards comes first.
//! - `:2,1bn`, `:2,1bf`, `:2,1bl`, `:2,1bn 1`, `:2,1b`, `:2,1bd` are E493.
//!   (nvim only asks to swap when typed; here it is E493 as when sourced.)
//! - `:99999999999bn` is a count of 1215752191, the 32 bits it keeps.

use fe_vim::{BufferAddress, BufferCommand, BufferTarget, HeadlessVim, HostCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn host(line: &str) -> BufferCommand {
    match run(line) {
        (Some(HostCommand::Buffer(command)), None) => command,
        other => panic!(":{line} gave {other:?}"),
    }
}

fn error(line: &str) -> String {
    let (command, message) = run(line);
    assert_eq!(command, None, ":{line}");
    message.unwrap_or_else(|| panic!(":{line} gave no message"))
}

fn n(number: u32) -> BufferAddress {
    BufferAddress::Number(number)
}

#[test]
fn buffer_ranges_match_nvim() {
    for line in [
        "0b",
        "0bd",
        "0b 2",
        "0bd 2",
        "0,2b",
        "0,2bd",
        "0,0b",
        "0,2b 3",
        "2147483648bn",
        "2147483648b",
        "2147483648bd 2",
    ] {
        assert_eq!(error(line), "E16: Invalid range", ":{line}");
    }
    for line in [
        "2,1bn", "2,1bN", "2,1bp", "2,1bf", "2,1br", "2,1bl", "2,1bn 1", "99,3bn", "2,1b", "2,1bd",
    ] {
        assert_eq!(error(line), "E493: Backwards range given", ":{line}");
    }
    // Past the highest buffer, and backwards where `.` or `$` is a buffer,
    // are the host's to check: the range comes with the command.
    let ranged = |first, last, command| BufferCommand::Ranged {
        first,
        last,
        command: Box::new(command),
    };
    let switch = |target| BufferCommand::Switch(target);
    let number = |number| BufferTarget::Number(number);
    assert_eq!(host("9b 2"), ranged(n(9), n(9), switch(number(2))),);
    assert_eq!(
        host("$,.b"),
        ranged(
            BufferAddress::Last,
            BufferAddress::Current,
            switch(BufferTarget::Range {
                first: BufferAddress::Current,
                last: BufferAddress::Current
            })
        )
    );
    assert_eq!(
        host("1,9bd 2"),
        ranged(
            n(1),
            n(9),
            BufferCommand::Delete {
                target: number(2),
                force: false
            }
        )
    );
    // A range whose buffers are certainly there is not wrapped.
    for line in ["1,3b", "3b", "%b", "2,2b"] {
        assert!(!format!("{:?}", host(line)).contains("Ranged"), ":{line}");
    }
    // No number is too high for a count, or too low for the first of `:bn`.
    assert_eq!(host("9bn"), BufferCommand::Next(9));
    assert_eq!(host("0bn"), BufferCommand::Next(1));
    assert_eq!(host("0,2bf"), BufferCommand::First);
    assert_eq!(host("1,9bl"), BufferCommand::Last);
    assert_eq!(host("99999999999bn"), BufferCommand::Next(1_215_752_191));
    assert_eq!(host("2bn 3"), BufferCommand::Next(3));
}

#[test]
fn escaped_filename_target() {
    let name = |text: &str| BufferCommand::Switch(BufferTarget::Name(text.into()));
    assert_eq!(host(r"b a\ b"), name("a b"));
    assert_eq!(host(r"b a\ b.txt"), name("a b.txt"));
    assert_eq!(host(r"b a\#b"), name("a#b"));
    assert_eq!(host(r"b p\%q"), name("p%q"));
    // Other backslashes are kept, and a plain space is part of the name.
    assert_eq!(host(r"b x\\yz"), name(r"x\\yz"));
    assert_eq!(host("b a b"), name("a b"));
    // Escaped, a space doesn't make `:bd` read two names.
    let delete = |text: &str| BufferCommand::Delete {
        target: BufferTarget::Name(text.into()),
        force: false,
    };
    assert_eq!(host(r"bd a\ b"), delete("a b"));
    assert!(error("bd a b").contains("not supported"));
}

#[test]
fn offsets_are_refused_not_ignored() {
    for line in [
        "1+1bn", "2+1,2bn", "1+1bp", "1+1bf", "1+1bl", "1+1b 2", ".+1bd",
    ] {
        assert!(error(line).contains("not supported"), ":{line}");
    }
}

#[test]
fn escaped_trailing_blank_is_kept() {
    let name = |text: &str| BufferTarget::Name(text.into());
    assert_eq!(host("b tail\\ "), BufferCommand::Switch(name("tail ")));
    assert_eq!(
        host("bd tail\\ "),
        BufferCommand::Delete {
            target: name("tail "),
            force: false
        }
    );
}

#[test]
fn backwards_error_precedes_endpoint_error() {
    for line in [
        "2,0b", "3,0bd 1", "9,1b", "9,1bd", "2,0bn", "2,0bf", ".,0b", "$,0bd", ".,0bn",
    ] {
        assert_eq!(error(line), "E493: Backwards range given", ":{line}");
    }
    // Not backwards, the endpoint is the error.
    assert_eq!(error("0,0b"), "E16: Invalid range");
}

#[test]
fn symbolic_ranges_match_nvim() {
    // Accepted: the range is ignored, or overridden by the argument.
    assert_eq!(host("%bf"), BufferCommand::First);
    assert_eq!(host("$bl"), BufferCommand::Last);
    assert_eq!(host(".bf"), BufferCommand::First);
    assert_eq!(host(".bn 2"), BufferCommand::Next(2));
    assert_eq!(host("%bn 2"), BufferCommand::Next(2));
    // The count must be a number, as before.
    assert!(error(".bn").contains("not supported"));
    // Backwards where a symbol is an end is the host's to say.
    let ordered = host(".,1bn");
    assert_eq!(
        ordered,
        BufferCommand::Ordered {
            first: BufferAddress::Current,
            last: n(1),
            command: Box::new(BufferCommand::Next(1)),
        }
    );
    assert!(matches!(host("$,1bf"), BufferCommand::Ordered { .. }));
    // Certainly ordered, not wrapped.
    for line in ["1,.bf", "0,.bf"] {
        assert!(!format!("{:?}", host(line)).contains("Ordered"), ":{line}");
    }
}
