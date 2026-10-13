//! A delete or change on a buffer that a linewise delete emptied writes no
//! register, as in nvim: the buffer has no rows left, so there's nothing to
//! take. A yank still writes, and the buffer stops being emptied once text
//! goes back in. Each expectation was probed with `nvim --clean --headless`.

use fe_vim::HeadlessVim;

/// The non-empty registers among `" 0-9 - a b`, as (name, text).
fn registers(vim: &HeadlessVim) -> Vec<(char, String)> {
    "\"0123456789-ab"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            (!register.text.is_empty()).then(|| (name, register.text.clone()))
        })
        .collect()
}

/// Feeds `keys` to a vim on `buffer` at (0, 0) and checks the text and the
/// registers.
fn check(buffer: &str, keys: &str, text: &str, want: &[(char, &str)]) {
    let mut vim = HeadlessVim::new(buffer, (0, 0));
    vim.feed(keys).unwrap();
    let want: Vec<(char, String)> = want
        .iter()
        .map(|(name, text)| (*name, text.to_string()))
        .collect();
    assert_eq!(
        (vim.text(), registers(&vim)),
        (text.to_string(), want),
        "{keys:?} on {buffer:?}"
    );
}

#[test]
fn deletes_and_changes_on_an_emptied_buffer_write_no_register() {
    let a = [('"', "a\n"), ('1', "a\n")];
    check("a", "dddd", "", &a);
    check("a", "dd.", "", &a);
    check("a", "ddVd", "", &a);
    check("a", "dd\"bdd", "", &a);
    check("a", "ddccx<Esc>", "x", &a);
    check("a", "ddSx<Esc>", "x", &a);
    check("a", "ddciwx<Esc>", "x", &a);
    check("a", "ddVcx<Esc>", "x", &a);
    check("a", "dd<C-v>cx<Esc>", "x", &a);
    // Edits that change nothing leave it emptied.
    check("a", "ddi<Esc>dd", "", &a);
    check("a", "dd>>dd", "", &a);
    check("a", "ddcc<Esc>dd", "", &a);
    // Any linewise delete of every row empties it.
    check("a\nb", "dGdd", "", &[('"', "a\nb\n"), ('1', "a\nb\n")]);
    check("a\nb", "Vjddd", "", &[('"', "a\nb\n"), ('1', "a\nb\n")]);
    check("", "dd\"add", "", &[('"', "\n"), ('1', "\n")]);
}

#[test]
fn yanks_and_deletes_once_text_is_back_still_write() {
    // A yank writes.
    check("a", "ddyy", "", &[('"', "\n"), ('0', "\n"), ('1', "a\n")]);
    // A charwise delete that leaves no text doesn't empty the buffer.
    check("a", "xdd", "", &[('"', "\n"), ('1', "\n"), ('-', "a")]);
    check(
        "a\nb",
        "vj$ddd",
        "",
        &[('"', "\n"), ('1', "\n"), ('2', "a\nb")],
    );
    // Nor does a linewise change, which keeps its row.
    check(
        "a",
        "cc<Esc>dd",
        "",
        &[('"', "\n"), ('1', "\n"), ('2', "a\n")],
    );
    // Text put back in ends it, typed or undone.
    check(
        "a",
        "ddix<Esc>dd",
        "",
        &[('"', "x\n"), ('1', "x\n"), ('2', "a\n")],
    );
    check(
        "a",
        "ddudd",
        "",
        &[('"', "a\n"), ('1', "a\n"), ('2', "a\n")],
    );
}
