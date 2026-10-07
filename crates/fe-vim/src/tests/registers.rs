use fe_editor::commands::HeadlessHost;

use super::registers;
use crate::{HeadlessVim, Register, RegisterError, Registers, Write};

fn chars(text: &str) -> Register {
    Register::new(text, false)
}

fn lines(text: &str) -> Register {
    Register::new(text, true)
}

const SMALL: Write = Write::Delete {
    small: true,
    numbered: false,
};
const LINES: Write = Write::Delete {
    small: false,
    numbered: true,
};

#[test]
fn slots_and_rules() {
    let host = &mut HeadlessHost::new();
    let mut registers = Registers::new();
    let get = |registers: &Registers, name| registers.get(name).cloned();

    // A yank writes 0 and `"` follows it; a named one leaves 0 alone.
    registers
        .write(None, Write::Yank, chars("y0"), host)
        .unwrap();
    assert_eq!(get(&registers, '0'), Some(chars("y0")));
    assert_eq!(get(&registers, '"'), Some(chars("y0")));
    registers
        .write(Some('b'), Write::Yank, chars("yb"), host)
        .unwrap();
    assert_eq!(get(&registers, '0'), Some(chars("y0")));
    assert_eq!(get(&registers, '"'), Some(chars("yb")));
    // `""` is no register named.
    registers
        .write(Some('"'), Write::Yank, chars("yq"), host)
        .unwrap();
    assert_eq!(get(&registers, '0'), Some(chars("yq")));

    // A delete within a row writes `-` and leaves 1.
    registers.write(None, SMALL, chars("small"), host).unwrap();
    assert_eq!(get(&registers, '-'), Some(chars("small")));
    assert_eq!(get(&registers, '1'), None);
    assert_eq!(get(&registers, '"'), Some(chars("small")));

    // A linewise or multi-row delete shifts 1..9, dropping the old 9.
    for row in 1..=10 {
        registers
            .write(None, LINES, lines(&format!("{row}\n")), host)
            .unwrap();
    }
    for slot in 1..=9u8 {
        let text = format!("{}\n", 11 - slot);
        assert_eq!(get(&registers, (b'0' + slot) as char), Some(lines(&text)));
    }
    assert_eq!(get(&registers, '"'), Some(lines("10\n")));
    assert_eq!(get(&registers, '-'), Some(chars("small")));

    // A named delete writes its register, not `-`, and (as nvim does) still
    // shifts the numbered ones when it's linewise or spans rows.
    registers
        .write(Some('c'), SMALL, chars("named"), host)
        .unwrap();
    assert_eq!(get(&registers, 'c'), Some(chars("named")));
    assert_eq!(get(&registers, '-'), Some(chars("small")));
    assert_eq!(get(&registers, '1'), Some(lines("10\n")));
    registers
        .write(Some('c'), LINES, lines("c\n"), host)
        .unwrap();
    assert_eq!(get(&registers, '1'), Some(lines("c\n")));
    assert_eq!(get(&registers, '2'), Some(lines("10\n")));
    assert_eq!(get(&registers, '"'), Some(lines("c\n")));

    // A delete over `%` writes 1 even within a row, and `-` too.
    let percent = Write::Delete {
        small: true,
        numbered: true,
    };
    registers.write(None, percent, chars("(a)"), host).unwrap();
    assert_eq!(get(&registers, '1'), Some(chars("(a)")));
    assert_eq!(get(&registers, '-'), Some(chars("(a)")));
    assert_eq!(get(&registers, '2'), Some(lines("c\n")));

    // A-Z append to a-z: whole lines if either part is.
    registers
        .write(Some('a'), Write::Yank, chars("x"), host)
        .unwrap();
    registers
        .write(Some('A'), Write::Yank, chars("y"), host)
        .unwrap();
    assert_eq!(get(&registers, 'a'), Some(chars("xy")));
    assert_eq!(get(&registers, 'A'), Some(chars("xy")));
    assert_eq!(get(&registers, '"'), Some(chars("xy")));
    registers
        .write(Some('A'), Write::Yank, lines("z\n"), host)
        .unwrap();
    assert_eq!(get(&registers, 'a'), Some(lines("xy\nz\n")));
    registers
        .write(Some('a'), Write::Yank, lines("l\n"), host)
        .unwrap();
    registers.write(Some('A'), SMALL, chars("c"), host).unwrap();
    assert_eq!(get(&registers, 'a'), Some(lines("l\nc\n")));
    registers
        .write(Some('Z'), Write::Yank, chars("new"), host)
        .unwrap();
    assert_eq!(get(&registers, 'z'), Some(chars("new")));

    // `_` writes nothing and leaves `"` alone.
    let before = registers.clone();
    registers
        .write(Some('_'), LINES, lines("gone\n"), host)
        .unwrap();
    registers
        .write(Some('_'), Write::Yank, chars("gone"), host)
        .unwrap();
    assert_eq!(registers, before);
    assert_eq!(registers.read(Some('_'), host), Ok(None));

    // A name that isn't a register is refused, changing nothing.
    for name in ['!', '%', 'é', '/', ' '] {
        assert_eq!(Registers::check(name), Err(RegisterError(name)));
        assert_eq!(
            registers.write(Some(name), Write::Yank, chars("bad"), host),
            Err(RegisterError(name))
        );
        assert_eq!(registers.read(Some(name), host), Err(RegisterError(name)));
    }
    assert_eq!(registers, before);
    assert_eq!(host.clipboard, None);
}

#[test]
fn clipboard_through_the_host() {
    // `"+yy` writes the line and its newline to the host.
    let mut vim = HeadlessVim::new("one\ntwo", (0, 1));
    vim.feed("\"+yy").unwrap();
    assert_eq!(vim.host().clipboard.as_deref(), Some("one\n"));
    assert_eq!(registers(&vim), [], "only the clipboard holds it");
    // `"*` reads the same clipboard: ending in a newline, it's lines, put
    // below.
    vim.feed("j\"*p").unwrap();
    assert_eq!(vim.text(), "one\ntwo\none");
    assert_eq!(vim.cursor(), (2, 0));
    // `"` follows the clipboard written last.
    vim.feed("p").unwrap();
    assert_eq!(vim.text(), "one\ntwo\none\none");

    // Text without a newline is put charwise, after the caret.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.host_mut().clipboard = Some("XY".into());
    vim.feed("\"*p").unwrap();
    assert_eq!(vim.text(), "aXYbc");
    assert_eq!(vim.cursor(), (0, 2));
    vim.feed("\"*yl").unwrap();
    assert_eq!(vim.host().clipboard.as_deref(), Some("Y"));
    vim.feed("\"+P").unwrap();
    assert_eq!(vim.text(), "aXYYbc");

    // Lines a host wrote without their line break still put as lines.
    let mut vim = HeadlessVim::new("a\nb", (1, 0));
    let host = &mut HeadlessHost::new();
    vim.registers_mut()
        .write(Some('q'), Write::Yank, lines("é"), host)
        .unwrap();
    vim.feed("\"qp").unwrap();
    assert_eq!(vim.text(), "a\nb\né");

    // The clipboard's \r\n and \r are line breaks: lines put below the last
    // row add no empty row, and carets land where the buffer's text puts
    // them.
    let mut vim = HeadlessVim::new("a\nb", (1, 0));
    vim.host_mut().clipboard = Some("X\r\n".into());
    vim.feed("\"+p").unwrap();
    assert_eq!(vim.text(), "a\nb\nX");
    assert_eq!(vim.cursor(), (2, 0));
    let mut vim = HeadlessVim::new("ab\ncd", (0, 0));
    let carets = [0, 3].map(|offset| fe_buffer::ResolvedSelection {
        range: offset..offset,
        reversed: false,
    });
    vim.model_mut().set_selections(carets);
    vim.host_mut().clipboard = Some("x\ry".into());
    vim.feed("\"*P").unwrap();
    assert_eq!(vim.text(), "x\nyab\nx\nycd");
    let heads: Vec<usize> = vim.model().selections().iter().map(|s| s.head()).collect();
    assert_eq!(heads, [0, 6], "each at the start of its multi-row text");

    // An empty clipboard puts nothing.
    let mut vim = HeadlessVim::new("abc", (0, 1));
    vim.feed("\"+p").unwrap();
    vim.host_mut().clipboard = Some(String::new());
    vim.feed("\"*P").unwrap();
    assert_eq!(vim.text(), "abc");
    assert_eq!(vim.cursor(), (0, 1));
    assert!(!vim.model().is_dirty());
}
