use fe_terminal::keys::{Key, Mods, encode, encode_paste};
use fe_terminal::{Modes, indexed_rgb};

fn enc(key: Key, mods: Mods) -> Vec<u8> {
    encode(key, mods, &Modes::default()).unwrap()
}

#[test]
fn encodes_xterm() {
    let app_cursor = Modes {
        app_cursor: true,
        ..Modes::default()
    };
    let ctrl_shift = Mods {
        shift: true,
        ctrl: true,
        ..Mods::NONE
    };

    // Control characters.
    assert_eq!(enc(Key::Char('a'), Mods::CTRL), [0x01]);
    assert_eq!(enc(Key::Char('Z'), ctrl_shift), [0x1a]);
    assert_eq!(enc(Key::Char(' '), Mods::CTRL), [0x00]);
    assert_eq!(enc(Key::Char('@'), Mods::CTRL), [0x00]);
    assert_eq!(enc(Key::Char('['), Mods::CTRL), [0x1b]);
    assert_eq!(enc(Key::Char('\\'), Mods::CTRL), [0x1c]);
    assert_eq!(enc(Key::Char(']'), Mods::CTRL), [0x1d]);
    assert_eq!(enc(Key::Char('^'), Mods::CTRL), [0x1e]);
    assert_eq!(enc(Key::Char('_'), Mods::CTRL), [0x1f]);

    // Plain and Alt characters.
    assert_eq!(enc(Key::Char('x'), Mods::NONE), b"x");
    assert_eq!(enc(Key::Char('é'), Mods::NONE), "é".as_bytes());
    assert_eq!(enc(Key::Char('x'), Mods::ALT), [0x1b, 0x78]);
    assert_eq!(
        enc(
            Key::Char('c'),
            Mods {
                alt: true,
                ctrl: true,
                ..Mods::NONE
            }
        ),
        [0x1b, 0x03]
    );

    // Editing keys.
    assert_eq!(enc(Key::Backspace, Mods::NONE), [0x7f]);
    assert_eq!(enc(Key::Enter, Mods::NONE), b"\r");
    assert_eq!(enc(Key::Tab, Mods::NONE), b"\t");
    assert_eq!(enc(Key::Tab, Mods::SHIFT), b"\x1b[Z");
    assert_eq!(enc(Key::Escape, Mods::NONE), [0x1b]);

    // Cursor keys: CSI normally, SS3 in application cursor mode, the
    // modifier parameter either way.
    assert_eq!(enc(Key::Up, Mods::NONE), b"\x1b[A");
    assert_eq!(encode(Key::Up, Mods::NONE, &app_cursor).unwrap(), b"\x1bOA");
    assert_eq!(enc(Key::Down, Mods::NONE), b"\x1b[B");
    assert_eq!(enc(Key::Left, Mods::NONE), b"\x1b[D");
    assert_eq!(enc(Key::Home, Mods::NONE), b"\x1b[H");
    assert_eq!(
        encode(Key::End, Mods::NONE, &app_cursor).unwrap(),
        b"\x1bOF"
    );
    assert_eq!(enc(Key::Right, Mods::CTRL), b"\x1b[1;5C");
    assert_eq!(
        encode(Key::Right, Mods::CTRL, &app_cursor).unwrap(),
        b"\x1b[1;5C"
    );
    assert_eq!(enc(Key::Left, Mods::ALT), b"\x1b[1;3D");
    assert_eq!(enc(Key::Up, Mods::SHIFT), b"\x1b[1;2A");

    // Page, insert, delete and function keys.
    assert_eq!(enc(Key::PageUp, Mods::NONE), b"\x1b[5~");
    assert_eq!(enc(Key::PageDown, Mods::SHIFT), b"\x1b[6;2~");
    assert_eq!(enc(Key::Insert, Mods::NONE), b"\x1b[2~");
    assert_eq!(enc(Key::Delete, Mods::NONE), b"\x1b[3~");
    assert_eq!(enc(Key::F(1), Mods::NONE), b"\x1bOP");
    assert_eq!(enc(Key::F(4), Mods::CTRL), b"\x1b[1;5S");
    assert_eq!(enc(Key::F(5), Mods::NONE), b"\x1b[15~");
    assert_eq!(enc(Key::F(12), Mods::SHIFT), b"\x1b[24;2~");
    assert_eq!(encode(Key::F(13), Mods::NONE, &Modes::default()), None);

    // Paste.
    assert_eq!(encode_paste("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
    assert_eq!(encode_paste("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
    assert_eq!(
        encode_paste("x\x1b[20\x1b[201~1~y", true),
        b"\x1b[200~xy\x1b[201~"
    );
    assert_eq!(encode_paste("a\nb\r\nc", false), b"a\rb\rc");

    // The 256-colour palette.
    assert_eq!(indexed_rgb(16), (0, 0, 0));
    assert_eq!(indexed_rgb(17), (0, 0, 95));
    assert_eq!(indexed_rgb(196), (255, 0, 0));
    assert_eq!(indexed_rgb(231), (255, 255, 255));
    assert_eq!(indexed_rgb(232), (8, 8, 8));
    assert_eq!(indexed_rgb(255), (238, 238, 238));
}
