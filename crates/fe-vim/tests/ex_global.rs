//! What `Ex::parse` makes of `:g[lobal][!]`, `:v[global]` and
//! `:norm[al][!]`, their abbreviations, delimiters and errors, and what
//! running them says where the nvim oracle can't look (messages, keys nvim
//! reads as typed, recursion).

use fe_command::{Actor, Invocation, Source};
use fe_editor::commands::{HeadlessHost, origin};
use fe_vim::{Ex, ExCommand, ExError, HeadlessVim, KeyError, LineSearch, Register, VimKey, Write};

fn command(line: &str) -> ExCommand {
    Ex::parse(line)
        .unwrap_or_else(|error| panic!("{line}: {error}"))
        .command
}

fn error(line: &str) -> ExError {
    Ex::parse(line).expect_err(line)
}

fn global(invert: bool, pattern: &str, command: &str) -> ExCommand {
    ExCommand::Global {
        invert,
        pattern: LineSearch::Pattern {
            pattern: pattern.into(),
            forward: true,
        },
        command: command.into(),
    }
}

#[test]
fn ex_parses_global() {
    for name in ["g", "gl", "glo", "glob", "globa", "global"] {
        assert_eq!(command(&format!("{name}/x/d")), global(false, "x", "d"));
        assert_eq!(command(&format!("{name}!/x/d")), global(true, "x", "d"));
    }
    for name in ["v", "vg", "vgl", "vglo", "vglob", "vgloba", "vglobal"] {
        assert_eq!(command(&format!("{name}/x/d")), global(true, "x", "d"));
    }
    // Any delimiter but a letter, after blanks; `!` is one after a blank.
    assert_eq!(command("g#a#s/a/b/"), global(false, "a", "s/a/b/"));
    assert_eq!(command("g|a|d"), global(false, "a", "d"));
    assert_eq!(command("g\"a\"d"), global(false, "a", "d"));
    assert_eq!(command("g1x1d"), global(false, "x", "d"));
    assert_eq!(command("g  ,a, d"), global(false, "a", " d"));
    assert_eq!(command("g !x!d"), global(false, "x", "d"));
    // An escaped delimiter stays in the pattern; a collection holds one.
    assert_eq!(command("g/a\\/b/d"), global(false, "a\\/b", "d"));
    assert_eq!(command("g/[/]/d"), global(false, "[/]", "d"));
    // No command, no closing delimiter, an empty pattern.
    assert_eq!(command("g/x/"), global(false, "x", ""));
    assert_eq!(command("g/x"), global(false, "x", ""));
    assert_eq!(command("g//normal x"), global(false, "", "normal x"));
    // The rest of the line is the command, `|` and `"` too.
    assert_eq!(command("g/x/s/a/b/|d"), global(false, "x", "s/a/b/|d"));
    assert_eq!(command("g/x/\"no"), global(false, "x", "\"no"));
    let last = |forward| ExCommand::Global {
        invert: false,
        pattern: LineSearch::LastSearch { forward },
        command: "d".into(),
    };
    assert_eq!(command("g\\/d"), last(true));
    assert_eq!(command("g\\?d"), last(false));
    assert_eq!(
        command("v\\&m0"),
        ExCommand::Global {
            invert: true,
            pattern: LineSearch::LastSubstitute,
            command: "m0".into(),
        }
    );
    let ex = Ex::parse("2,4g/x/d").unwrap();
    assert_eq!(ex.range.len(), 2);

    assert_eq!(error("g"), ExError::NoGlobalPattern);
    assert_eq!(error("g!"), ExError::NoGlobalPattern);
    assert_eq!(error("g  "), ExError::NoGlobalPattern);
    assert_eq!(error("g x"), ExError::LetterDelimiter);
    assert_eq!(error("g!axad"), ExError::LetterDelimiter);
    assert_eq!(error("g\\x\\d"), ExError::BackslashDelimiter);
    assert_eq!(error("g\\"), ExError::BackslashDelimiter);
    assert_eq!(error("v!/x/d"), ExError::NoBang);
    assert_eq!(error("gx/x/d"), ExError::NotACommand("gx/x/d".into()));
    assert_eq!(error("vx/x/d"), ExError::NotACommand("vx/x/d".into()));
    // A `:g` inside a `:g`, with a range or not.
    for line in [
        "g/x/g/y/d",
        "g/x/v/y/d",
        "g/x/ .g/y/d",
        "v/x/:2,3gl!/y/d",
        "g/x/g",
    ] {
        assert_eq!(error(line), ExError::Recursive, "{line}");
    }
    assert_eq!(
        error("g").to_string(),
        "E148: Regular expression missing from global"
    );
    assert_eq!(
        error("g/x/.g/y/").to_string(),
        "E147: Cannot do :global recursive with a range"
    );
    assert_eq!(
        error("g\\x\\").to_string(),
        "E10: \\ should be followed by /, ? or &"
    );
}

#[test]
fn ex_parses_normal() {
    let normal = |keys: &str| ExCommand::Normal {
        keys: fe_vim::parse_keys(keys).unwrap(),
    };
    for name in ["norm", "norma", "normal"] {
        assert_eq!(command(&format!("{name} dw")), normal("dw"), "{name}");
        assert_eq!(command(&format!("{name}! dw")), normal("dw"), "{name}!");
    }
    // Blanks before the keys are skipped; the rest are keys, in vim
    // notation, `"` and `|` among them.
    assert_eq!(command("normal   A;"), normal("A;"));
    assert_eq!(command("normal!A;"), normal("A;"));
    assert_eq!(command("norm x  "), normal("x  "));
    assert_eq!(command("norm \"ayy|x"), normal("\"ayy|x"));
    assert_eq!(
        command("%norm ix<Esc>.<lt>"),
        ExCommand::Normal {
            keys: vec![
                VimKey::Char('i'),
                VimKey::Char('x'),
                VimKey::Esc,
                VimKey::Char('.'),
                VimKey::Char('<'),
            ],
        }
    );
    assert_eq!(command("normal :d<CR>"), normal(":d<CR>"));
    assert_eq!(Ex::parse("2,3norm! >>").unwrap().range.len(), 2);

    assert_eq!(error("normal"), ExError::ArgumentRequired);
    assert_eq!(error("norm!"), ExError::ArgumentRequired);
    assert_eq!(error("normal \t"), ExError::ArgumentRequired);
    assert_eq!(error("normal").to_string(), "E471: Argument required");
    assert_eq!(
        error("normal i<Nope>"),
        ExError::Keys(KeyError("Nope".into()))
    );
    // `:nor` is another command.
    assert_eq!(error("nor x"), ExError::NotACommand("nor x".into()));
    assert_eq!(error("normalx"), ExError::NotACommand("normalx".into()));
}

fn vim(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    vim
}

#[test]
fn global_says_what_it_matched_without_failing() {
    let vim = vim("x1\nx2\ny", (2, 0), ":g/z/d<CR>");
    assert_eq!(vim.state().message(), Some("Pattern not found: z"));
    assert!(!vim.state().failed());
    assert_eq!(vim.text(), "x1\nx2\ny");
    // The pattern is the last search even so.
    assert_eq!(vim.registers().last_search(), Some(("z", true)));

    let vim = self::vim("x1\nx2\ny", (2, 0), ":v/./d<CR>");
    assert_eq!(
        vim.state().message(),
        Some("Pattern found in every line: .")
    );
    assert!(!vim.state().failed());

    // A `:s` finding nothing on a row isn't an error there.
    let vim = self::vim("x1\nx2\ny", (2, 0), ":g/x/s/2/Q/<CR>");
    assert_eq!(vim.text(), "x1\nxQ\ny");
    assert!(!vim.state().failed());
}

#[test]
fn global_stops_at_a_command_that_fails() {
    let vim = vim("x1\nx2\ny", (2, 0), ":g/x/zz<CR>");
    assert_eq!(
        vim.state().message(),
        Some("E492: Not an editor command: zz")
    );
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (0, 0));

    // A `:g` reached through `:normal` inside a `:g` fails there, on each
    // row, and the `:normal` doesn't.
    let vim = self::vim("x1\nx2\ny", (2, 0), ":g/x/normal :g/1/d<lt>CR><CR>");
    assert_eq!(vim.text(), "x1\nx2\ny");
    assert_eq!(
        vim.state().message(),
        Some("E147: Cannot do :global recursive with a range")
    );
    assert!(!vim.state().failed());

    // E35 with no last search, and nothing marked.
    let vim = self::vim("x1\nx2\ny", (2, 0), ":g//d<CR>");
    assert_eq!(
        vim.state().message(),
        Some("E35: No previous regular expression")
    );
    assert_eq!(vim.text(), "x1\nx2\ny");
}

#[test]
fn normal_runs_keys_nvim_reads_as_typed() {
    // A command line it opens runs at `<CR>`, and at `<Esc>`, as a
    // register's does.
    assert_eq!(
        vim("a\nb\nc", (0, 0), ":normal :2d<lt>CR><CR>").text(),
        "a\nc"
    );
    assert_eq!(
        vim("a\nb\nc", (0, 0), ":normal :3d<lt>Esc><CR>").text(),
        "a\nb"
    );
    // `<Esc>` ends an insert session and the keys go on.
    let vim = vim("abc", (0, 1), ":normal ix<lt>Esc>x<CR>");
    assert_eq!(vim.text(), "abc");
    assert_eq!(vim.mode(), "normal");
    // Keys that name no key leave a message.
    let vim = self::vim("abc", (0, 1), ":normal x<lt>Nope><CR>");
    assert_eq!(vim.text(), "abc");
    assert_eq!(vim.state().message(), Some("unknown key <Nope>"));
}

#[test]
fn normal_keeps_a_recording_and_a_replay_as_typed() {
    // Recording `:normal x` gets the line, not the `x` it runs.
    let mut vim = vim("abcdef", (0, 0), "qa:normal x<CR>q");
    assert_eq!(vim.text(), "bcdef");
    let recorded = vim.registers().get('a').unwrap().text.clone();
    assert_eq!(recorded.matches('x').count(), 1, "{recorded:?}");
    // Played, it runs the line once and the replay goes on after it.
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "cdef");
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    write(&mut vim, 'b', ":normal x\nx");
    vim.feed("@b").unwrap();
    assert_eq!(vim.text(), "cdef");
    assert!(vim.state().recording().is_none());
}

#[test]
fn normal_inside_itself_stops() {
    let mut vim = HeadlessVim::new("abc", (0, 0));
    write(&mut vim, 'a', ":normal @a\n");
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "abc");
    assert_eq!(
        vim.state().message(),
        Some("E192: Recursive use of :normal too deep")
    );
}

/// Writes `text` into register `name`, charwise.
fn write(vim: &mut HeadlessVim, name: char, text: &str) {
    let register = Register::new(text, false);
    vim.registers_mut()
        .write(Some(name), Write::Yank, register, &mut HeadlessHost::new())
        .unwrap();
}

#[test]
fn normal_types_as_its_actor() {
    let mut vim = HeadlessVim::new("ab\ncd", (0, 0));
    vim.feed(":").unwrap();
    let ai = Actor::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": "normal ixy<CR>z<BS>w" }))
        .actor(ai.clone())
        .source(Source::Keyboard)
        .context(fe_vim::context(vim.state()));
    vim.dispatch(&submit).unwrap();
    assert_eq!(vim.text(), "xy\nwab\ncd");
    // The user has nothing to undo; the actor undoes it all in one step.
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "xy\nwab\ncd");
    let (undone, _) = vim.model_mut().run_as(origin(&ai), |model| model.undo());
    assert!(undone.unwrap().is_some());
    assert_eq!(vim.text(), "ab\ncd");
}
