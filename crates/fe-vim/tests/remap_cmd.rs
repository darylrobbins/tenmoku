//! A remap's `<Cmd>…<CR>` right sides (`docs/decisions/vim-remaps.md` §6),
//! through `HeadlessVim`: an ex line runs without the command line, and
//! `fe:{command id}` leaves the command for the host.

use std::sync::Arc;

use fe_command::{Invocation, Source};
use fe_vim::{CommandLineKind, HeadlessVim, Remap, RemapCommand, RemapMode, RemapSet};

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;

fn noremap(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

fn vim(text: &str, cursor: (u32, usize), remaps: &[Remap]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let (set, issues) = RemapSet::new(remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

/// [`vim`] at the start of `text`.
fn vim_at(text: &str, remaps: &[Remap]) -> HeadlessVim {
    vim(text, (0, 0), remaps)
}

/// The messages of the issues `to` has as a map of `X`'s right side.
fn issues(to: &str) -> Vec<String> {
    let (_, issues) = RemapSet::new(&[noremap(&[N], "X", to)], "\\");
    issues.into_iter().map(|issue| issue.message).collect()
}

/// The `:` history, oldest first.
fn ex_history(vim: &HeadlessVim) -> Vec<String> {
    let history = vim.registers().history(CommandLineKind::Ex);
    history.map(str::to_owned).collect()
}

#[test]
fn remap_cmd_runs_ex_lines() {
    let maps = [
        noremap(&[N, X], "S", "<Cmd>s/a/b/<CR>"),
        noremap(&[N], "J", "<Cmd>s/a/b/<CR>j"),
        noremap(&[N], "L", "<Cmd>s/a/<lt>b<Bar>/<CR>"),
        noremap(&[N], "Z", "<Cmd>s/zzz/b/<CR>j"),
    ];

    // In normal mode: the line runs, and no command line opens, nor goes
    // into the history or the `:` register.
    let mut normal = vim("aaa\naaa", (0, 2), &maps);
    normal.feed("S").unwrap();
    assert_eq!(normal.text(), "baa\naaa");
    assert_eq!(normal.mode(), "normal");
    assert_eq!(normal.state().command_line_prefill(), "");
    assert_eq!(ex_history(&normal), Vec::<String>::new());
    assert!(normal.registers().get(':').is_none());
    // `@:` has no line to repeat.
    normal.feed("j@:").unwrap();
    assert_eq!(normal.text(), "baa\naaa");
    // A typed line still goes into the history.
    normal.feed(":s/a/c/<CR>").unwrap();
    assert_eq!(normal.text(), "baa\ncaa");
    assert_eq!(ex_history(&normal), ["s/a/c/"]);

    // The keys after it run, in order.
    let mut after = vim("aaa\naaa", (0, 0), &maps);
    after.feed("J").unwrap();
    assert_eq!(
        (after.text().as_str(), after.cursor()),
        ("baa\naaa", (1, 0))
    );

    // The text is read as keys: `<lt>` and `<Bar>` are characters.
    let mut notation = vim("aaa", (0, 0), &maps);
    notation.feed("L").unwrap();
    assert_eq!(notation.text(), "<b|aa");

    // A line that fails leaves its message and drops the rest of the map.
    let mut failing = vim("aaa\naaa", (0, 0), &maps);
    failing.feed("Z").unwrap();
    assert_eq!(failing.cursor(), (0, 0));
    assert_eq!(
        failing.state().message(),
        Some("E486: Pattern not found: zzz")
    );

    // In a visual mode the selection stays around it, as `:` wouldn't
    // leave it; then the keys after the map go on extending it.
    let mut visual = vim("aaa aaa\naaa", (0, 4), &maps);
    visual.feed("vlS").unwrap();
    assert_eq!(visual.text(), "baa aaa\naaa");
    assert_eq!(visual.mode(), "visual");
    assert_eq!(visual.visual_anchor(), Some((0, 4)));
    assert_eq!(ex_history(&visual), Vec::<String>::new());
    visual.feed("ly").unwrap();
    assert_eq!(visual.mode(), "normal");
    assert_eq!(visual.registers().get('"').unwrap().text, "aaa");

    // An ex command that itself changes the mode still does: `:normal`.
    let mut moded = vim("one", (0, 0), &[noremap(&[N], "V", "<Cmd>normal! v<CR>l")]);
    moded.feed("V").unwrap();
    assert_eq!(moded.mode(), "visual");
    assert_eq!(moded.visual_anchor(), Some((0, 0)));
    assert_eq!(moded.cursor(), (0, 1));

    // A recursive map whose right side starts with a `<Cmd>` and then its
    // left side runs again, until the line fails.
    let looping = Remap {
        recursive: true,
        ..noremap(&[N], "Q", "<Cmd>s/a/b/<CR>Q")
    };
    let mut recursive = vim("aaa", (0, 0), &[looping]);
    recursive.feed("Q").unwrap();
    assert_eq!(recursive.text(), "bbb");
    assert_eq!(
        recursive.state().message(),
        Some("E486: Pattern not found: a")
    );

    // Through `:normal` too, in insert mode as typed: the line runs, and
    // the text after it is typed where it leaves the caret: at the row's
    // first non-blank, as `:s` puts it.
    let insert = [noremap(&[N], "I", "A<Cmd>s/a/b/<CR>x<Esc>")];
    let mut typed = vim("aaa", (0, 0), &insert);
    typed.feed("I").unwrap();
    let mut normal = vim("aaa", (0, 0), &insert);
    normal.feed(":normal I<CR>").unwrap();
    assert_eq!(typed.text(), "xbaa");
    assert_eq!(normal.text(), typed.text());
    assert_eq!(normal.mode(), "normal");

    // An unclosed `<Cmd>`, a key that isn't text in one, and one in a left
    // side are issues.
    assert_eq!(issues("<Cmd>s/a/b/"), ["<Cmd> without a <CR> to end it"]);
    assert_eq!(issues("x<cmd>w"), ["<Cmd> without a <CR> to end it"]);
    assert_eq!(
        issues("<Cmd>s/a/<Esc>/<CR>"),
        ["<Cmd> holds a key that isn't text before its <CR>"]
    );
    let (_, from_issues) = RemapSet::new(&[noremap(&[N], "<Cmd>", "x")], "\\");
    assert_eq!(from_issues.len(), 1);
    assert_eq!(issues("<CMD>w<cr>x"), Vec::<String>::new());
}

#[test]
fn remap_cmd_leaves_fastedit_commands() {
    let save = || {
        let invocation = Invocation::of("editor.save").unwrap();
        RemapCommand(invocation.source(Source::Keyboard))
    };
    let maps = [
        noremap(&[N], "W", "<Cmd>fe:editor.save<CR>"),
        noremap(&[N], "J", "<Cmd>fe:editor.save<CR>j"),
        noremap(&[N], "I", "i<Cmd>fe:editor.save<CR>x<Esc>"),
        noremap(&[N], "D", "3<Cmd>fe:editor.save<CR>x"),
    ];

    // Nothing runs in fe-vim: the command waits for the host, once.
    let mut vim = vim("one\ntwo", (0, 0), &maps);
    vim.feed("W").unwrap();
    assert_eq!(vim.text(), "one\ntwo");
    assert_eq!(vim.mode(), "normal");
    assert!(!vim.state().failed());
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));
    assert_eq!(vim.state_mut().take_remap_command(), None);

    // The keys after it go on.
    vim.feed("J").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));

    // In insert mode too, where it isn't typed.
    vim.feed("I").unwrap();
    assert_eq!(vim.text(), "one\nxtwo");
    assert_eq!(vim.mode(), "normal");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));

    // It drops a count before it, as a command does.
    vim.feed("D").unwrap();
    assert_eq!(vim.text(), "one\ntwo");
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));

    // A `:normal` stops at one: the keys before it run, it doesn't, and
    // nothing after it does.
    vim.feed("gg:normal jJk<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(
        vim.state().message(),
        Some("fe: commands don't run inside :normal")
    );
    assert_eq!(vim.state_mut().take_remap_command(), None);
    // A `:normal` that runs one that stops stops too, over its range, and
    // so does a map that ran it.
    let nested = [
        noremap(&[N], "W", "<Cmd>fe:editor.save<CR>"),
        noremap(&[N], "N", "<Cmd>normal xW<CR>"),
        noremap(&[N], "M", "<Cmd>normal N<CR>x"),
    ];
    let mut ranged = vim_at("abc\ndef\nghi", &nested);
    ranged.feed(":%normal N<CR>").unwrap();
    assert_eq!(ranged.text(), "bc\ndef\nghi");
    assert_eq!(
        ranged.state().message(),
        Some("fe: commands don't run inside :normal")
    );
    assert_eq!(ranged.state_mut().take_remap_command(), None);
    let mut mapped = vim_at("abc", &nested);
    mapped.feed("M").unwrap();
    assert_eq!(mapped.text(), "bc");
    assert_eq!(
        mapped.state().message(),
        Some("fe: commands don't run inside :normal")
    );

    // One left before the `:normal`, not taken yet, stays for the host.
    vim.feed("Wgg:normal j<CR>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    assert_eq!(vim.state_mut().take_remap_command(), Some(save()));
    assert_eq!(vim.state_mut().take_remap_command(), None);

    // It answers no `:s` confirmation: it ends it, and the keys after it
    // aren't answers.
    let confirm = [noremap(
        &[N],
        "C",
        "<Cmd>s/o/0/gc<CR><Cmd>fe:editor.save<CR>",
    )];
    let mut confirming = vim_at("one", &confirm);
    confirming.feed("C").unwrap();
    assert_eq!(confirming.state_mut().take_remap_command(), Some(save()));
    confirming.feed("y").unwrap();
    assert_eq!(confirming.text(), "one");

    // Nor picks a jump label: it ends the labels.
    let labelled = [noremap(&[N], "L", "gsab<Cmd>fe:editor.save<CR>")];
    let mut jumping = vim_at("ab ab ab ab", &labelled);
    jumping.feed("gsab").unwrap();
    assert!(jumping.jump_overlay().is_some(), "labels wait");
    jumping.feed("<Esc>L").unwrap();
    assert_eq!(jumping.state_mut().take_remap_command(), Some(save()));
    assert!(jumping.jump_overlay().is_none());
    assert_eq!(jumping.state().pending().kind(), None);

    // An ID that isn't a valid command ID is an issue.
    for bad in ["fe:save", "fe:Editor.save", "fe:", "fe:editor..save"] {
        let messages = issues(&format!("<Cmd>{bad}<CR>"));
        assert_eq!(messages.len(), 1, "{bad}: {messages:?}");
        assert!(
            messages[0].starts_with(&format!("<Cmd>{bad}<CR>: ")),
            "{messages:?}"
        );
    }
    // Valid, if no command has it: the host says so when it runs.
    assert_eq!(issues("<Cmd>fe:nowhere.unknown<CR>"), Vec::<String>::new());
}
