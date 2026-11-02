//! matchit's `%` and `g%` through the keyword groups of the buffer's
//! language (`feature/matchit.rs`). nvim's oracle corpus can't set a
//! filetype or load matchit, so each case is written out from nvim 0.12.4:
//! `nvim --clean --headless --cmd 'packadd matchit' --cmd 'filetype plugin
//! on' -l probe.lua`, `vim.bo.filetype` set per case and the keys fed with
//! `:normal`. The fallbacks with no language (and rust, c) are nvim's
//! built-in `%`, probed with `:normal!` (nvim loads matchit by default).

use fe_vim::HeadlessVim;

fn open(language: Option<&'static str>, rows: &[&str], cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(&rows.join("\n"), cursor);
    vim.set_language(language);
    vim
}

/// Where the cursor is after each of `keys`, fed one after another.
fn path(
    language: Option<&'static str>,
    rows: &[&str],
    cursor: (u32, usize),
    keys: &[&str],
) -> Vec<(u32, usize)> {
    let mut vim = open(language, rows, cursor);
    keys.iter()
        .map(|keys| {
            vim.feed(keys).unwrap();
            vim.cursor()
        })
        .collect()
}

/// The text after `keys`, and the unnamed register.
fn edited(language: Option<&'static str>, row: &str, col: usize, keys: &str) -> (String, String) {
    let mut vim = open(language, &[row], (0, col));
    vim.feed(keys).unwrap();
    let register = vim.registers().get('"').map(|r| r.text.clone());
    (vim.text(), register.unwrap_or_default())
}

const LUA_IF: [&str; 7] = [
    "if a then",
    "  x()",
    "elseif b then",
    "  y()",
    "else",
    "  z = 'end'",
    "end",
];

#[test]
fn matchit_cycles_keyword_groups() {
    let lua = Some("lua");
    // `%` through if/elseif/else/end, the `end` in the string counting,
    // and from it back to the `if`; `g%` the other way.
    let five = ["%"; 5];
    assert_eq!(
        path(lua, &LUA_IF, (0, 0), &five),
        [(2, 0), (4, 0), (5, 7), (0, 0), (2, 0)]
    );
    let back = ["g%"; 5];
    assert_eq!(
        path(lua, &LUA_IF, (0, 0), &back),
        [(5, 7), (4, 0), (2, 0), (0, 0), (5, 7)]
    );
    // Anywhere on the keyword.
    assert_eq!(path(lua, &LUA_IF, (0, 1), &["%"]), [(2, 0)]);
    // Nesting counts: from `local` the first keyword is `function`.
    let nested = ["local f = function() if a then return end end"];
    assert_eq!(
        path(lua, &nested, (0, 0), &["%", "%", "%"]),
        [(0, 42), (0, 10), (0, 42)]
    );
    let rows = ["if a then", "if b then", "end", "end"];
    assert_eq!(
        path(lua, &rows, (0, 0), &["%", "%", "g%"]),
        [(3, 0), (0, 0), (3, 0)]
    );
    // From `then` (no keyword) the first one ahead, `end`, back to `if`.
    assert_eq!(path(lua, &["if a then y end"], (0, 5), &["%"]), [(0, 0)]);
    // A caret on blanks before the keyword.
    assert_eq!(
        path(lua, &["x = f(a) if b then end"], (0, 8), &["%"]),
        [(0, 19)]
    );
    // Unbalanced: `elseif` goes on to `end`, which has no opener.
    let unbalanced = ["elseif a then y end"];
    assert_eq!(
        path(lua, &unbalanced, (0, 0), &["%", "%"]),
        [(0, 16), (0, 16)]
    );
    // `do` opens (`for` isn't a keyword); `repeat` to `until`.
    assert_eq!(
        path(lua, &["for i=1,2 do x end"], (0, 0), &["%", "%"]),
        [(0, 15), (0, 10)]
    );
    assert_eq!(
        path(lua, &["repeat x until y"], (0, 0), &["%", "%"]),
        [(0, 9), (0, 0)]
    );
    // Whole words: `ifx` and `aéif` aren't `if`; a byte column after `é`.
    assert_eq!(path(lua, &["a = ifx end"], (0, 0), &["%"]), [(0, 0)]);
    assert_eq!(path(lua, &["aéif x end"], (0, 0), &["%"]), [(0, 0)]);
    assert_eq!(path(lua, &["if é then end"], (0, 0), &["%"]), [(0, 11)]);
    // A comment's keyword counts too (no syntax skips it).
    assert_eq!(path(lua, &["a=1 -- if", "b end"], (0, 0), &["%"]), [(1, 2)]);
    // No keyword on the row: nothing on other rows is looked at.
    assert_eq!(
        path(lua, &["if x then", "y", "end"], (1, 0), &["%"]),
        [(1, 0)]
    );

    // bash: only where a command starts, at the row's start or after `;`.
    let bash = Some("bash");
    assert_eq!(
        path(bash, &["echo if; if x; then :; fi"], (0, 0), &["%", "%"]),
        [(0, 23), (0, 9)]
    );
    let chain = ["if a; then b; elif c; then d; else e; fi"];
    assert_eq!(
        path(bash, &chain, (0, 0), &["%", "%", "%", "%", "g%", "g%"]),
        [(0, 14), (0, 30), (0, 38), (0, 0), (0, 38), (0, 30)]
    );
    // `then if` isn't a command's start, so the first `fi` closes.
    let inner = ["if a; then if b; then c; fi; fi"];
    assert_eq!(path(bash, &inner, (0, 0), &["%"]), [(0, 25)]);
    let case = ["case $x in", "  a) echo;;", "esac"];
    assert_eq!(
        path(bash, &case, (0, 0), &["%", "%", "g%"]),
        [(2, 0), (0, 0), (2, 0)]
    );
    let done = ["while true; do", "  x", "done"];
    assert_eq!(
        path(bash, &done, (0, 0), &["%", "%", "g%"]),
        [(2, 0), (0, 0), (2, 0)]
    );
    // Indented, by spaces or a tab: it lands on the keyword.
    assert_eq!(
        path(bash, &["  if a", "  then", "  fi"], (0, 0), &["%", "%"]),
        [(2, 2), (0, 2)]
    );
    assert_eq!(
        path(bash, &["\tif a", "\tfi"], (0, 0), &["%", "%"]),
        [(1, 1), (0, 1)]
    );

    // make: conditionals at the row's start after spaces, landing on its
    // first column; `else ifdef` is one middle; `define`/`endef`; nmake's
    // `!if`.
    let make = Some("make");
    let cond = ["ifeq ($(A),b)", "x = 1", "else", "x = 2", "endif"];
    assert_eq!(
        path(make, &cond, (0, 0), &["%", "%", "%", "g%"]),
        [(2, 0), (4, 0), (0, 0), (4, 0)]
    );
    assert_eq!(path(make, &cond, (0, 2), &["%"]), [(2, 0)]);
    let spaced = ["  ifdef A", "x = 1", "  else ifndef B", "x = 2", "  endif"];
    assert_eq!(
        path(make, &spaced, (0, 4), &["%", "%", "%"]),
        [(2, 0), (4, 0), (0, 0)]
    );
    assert_eq!(
        path(make, &["  ifdef A", "  endif"], (0, 1), &["%"]),
        [(1, 0)]
    );
    let chained = ["ifdef A", "else ifdef B", "endif"];
    assert_eq!(path(make, &chained, (0, 0), &["%", "%"]), [(1, 0), (2, 0)]);
    let define = ["define X", "a", "endef"];
    assert_eq!(path(make, &define, (0, 0), &["%", "%"]), [(2, 0), (0, 0)]);
    assert_eq!(
        path(make, &["x define y", "endef"], (0, 0), &["%"]),
        [(1, 0)]
    );
    let bang = ["!ifdef A", "a", "! else", "!endif"];
    assert_eq!(
        path(make, &bang, (0, 0), &["%", "%", "%"]),
        [(2, 0), (3, 0), (0, 0)]
    );
    // A tab before it, or a word: not a conditional, so `%` takes the
    // bracket.
    assert_eq!(
        path(make, &["\tifeq (a,b)", "endif"], (0, 0), &["%"]),
        [(0, 10)]
    );
    assert_eq!(
        path(make, &["x ifeq (a,b)", "endif"], (0, 0), &["%"]),
        [(0, 11)]
    );
}

#[test]
fn matchit_falls_back() {
    // With no language, or one without a table, `%` takes the bracket.
    let row = ["if a then f(x) end"];
    for language in [None, Some("rust"), Some("c")] {
        assert_eq!(
            path(language, &row, (0, 0), &["%"]),
            [(0, 13)],
            "{language:?}"
        );
    }
    assert_eq!(path(Some("lua"), &row, (0, 0), &["%"]), [(0, 15)]);
    assert_eq!(
        path(Some("rust"), &["if a { x } end"], (0, 0), &["%"]),
        [(0, 9)]
    );
    // C comments and `#if` rows, as before.
    let c = ["#if A", "x /* if */ (y)", "#else", "#endif"];
    assert_eq!(
        path(Some("c"), &c, (0, 0), &["%", "%", "%"]),
        [(2, 0), (3, 0), (0, 0)]
    );
    assert_eq!(path(Some("c"), &c, (1, 2), &["%"]), [(1, 9)]);
    assert_eq!(path(Some("c"), &c, (1, 9), &["%"]), [(1, 2)]);
    assert_eq!(path(Some("c"), &c, (1, 4), &["%"]), [(1, 13)]);
    // A bracket before the row's first keyword keeps bracket `%`, in a
    // language with a table too.
    let lua = Some("lua");
    assert_eq!(path(lua, &["x = (a) if b then"], (0, 0), &["%"]), [(0, 6)]);
    assert_eq!(
        path(lua, &["x = f(a) if b then end"], (0, 6), &["%"]),
        [(0, 5)]
    );
    // `{count}%` is the row that far down, `{count}g%` too, as matchit runs
    // it.
    let rows = ["a", "b", "c", "if x then", "end"];
    assert_eq!(path(lua, &rows, (4, 0), &["60%"]), [(2, 0)]);
    assert_eq!(path(lua, &rows, (4, 0), &["60g%"]), [(2, 0)]);
    // `g%` with no table fails and changes nothing, `dg%` too.
    for language in [None, Some("rust")] {
        let mut vim = open(language, &["if a then (b) end"], (0, 0));
        vim.feed("g%").unwrap();
        assert!(vim.state().failed(), "{language:?}");
        assert_eq!(vim.cursor(), (0, 0));
        vim.feed("dg%").unwrap();
        assert_eq!(vim.text(), "if a then (b) end");
        assert_eq!(vim.cursor(), (0, 0));
    }
    // With a table, `g%` on a bracket is `%`.
    assert_eq!(
        path(lua, &["f(a) if x then end"], (0, 0), &["g%"]),
        [(0, 3)]
    );
    assert_eq!(
        path(lua, &["f(a) if x then end"], (0, 3), &["g%"]),
        [(0, 1)]
    );
    // And on a C conditional it goes the other way.
    let rows = ["#if A", "#else", "#endif"];
    assert_eq!(path(lua, &rows, (0, 0), &["g%", "g%"]), [(2, 0), (1, 0)]);
}

#[test]
fn matchit_operators_and_jumps() {
    let lua = Some("lua");
    let row = "x if a then y end z";
    let taken = "if a then y end".to_string();
    // `d%` from `if` takes the whole `end`, charwise and inclusive.
    assert_eq!(edited(lua, row, 2, "d%"), ("x  z".into(), taken.clone()));
    assert_eq!(edited(lua, row, 2, "dg%"), ("x  z".into(), taken.clone()));
    // Back from `end`'s `n`: to the `if`'s first char, the `n` included.
    let back = ("x d z".to_string(), "if a then y en".to_string());
    assert_eq!(edited(lua, row, 15, "d%"), back);
    assert_eq!(edited(lua, row, 15, "dg%"), back);
    // `y%` yanks the same, leaving the cursor.
    let mut vim = open(lua, &[row], (0, 2));
    vim.feed("y%").unwrap();
    assert_eq!(vim.registers().get('"').unwrap().text, taken);
    assert_eq!((vim.text().as_str(), vim.cursor()), (row, (0, 2)));
    // `v%` lands on `end`'s first char.
    assert_eq!(edited(lua, row, 2, "v%d").0, "x nd z");
    // `.` repeats `d%`.
    let mut vim = open(lua, &[row, row], (0, 2));
    vim.feed("d%j0w.").unwrap();
    assert_eq!(vim.text(), "x  z\nx  z");
    // Whole rows: a `d%` from column 0 to a row's end takes them.
    let bash = Some("bash");
    let mut vim = open(bash, &["for i in 1; do", "done"], (0, 0));
    vim.feed("d%").unwrap();
    assert_eq!(vim.text(), "");

    // `%` and `g%` set `''` where they leave from when they land.
    let mut vim = open(lua, &LUA_IF, (0, 0));
    vim.feed("%").unwrap();
    assert_eq!(vim.mark('\''), Some((0, 0)));
    vim.feed("g%").unwrap();
    assert_eq!((vim.cursor(), vim.mark('\'')), ((0, 0), Some((2, 0))));
    // A failing one leaves it (matchit, finding the `end`, would set it).
    let mut vim = open(lua, &["abc", "end"], (0, 0));
    vim.feed("G").unwrap();
    assert_eq!(vim.mark('\''), Some((0, 0)));
    for keys in ["%", "g%"] {
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        assert_eq!((vim.cursor(), vim.mark('\'')), ((1, 0), Some((0, 0))));
    }
    // No keyword on the row: `%` fails and leaves it, as matchit does.
    let mut vim = open(lua, &["abc", "if a then", "end"], (0, 0));
    vim.feed("Gg%gg").unwrap();
    assert_eq!(vim.mark('\''), Some((1, 0)));
    vim.feed("%").unwrap();
    assert_eq!((vim.cursor(), vim.mark('\'')), ((0, 0), Some((1, 0))));
}
