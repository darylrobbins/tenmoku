//! fe-vim's labelled jump session against `tests/corpus/jump_labels.toml`:
//! fastedit's own spec, after leap, not recorded from nvim, which has no
//! jump. Each case gives the text, the cursor and the view's rows, keys to
//! scroll the view first if needed, the keys, and what they leave: the
//! cursor, the text, the mode and the labels the overlay shows. A picked
//! label is the counted motion, which `tests/jump_oracle.rs` checks against
//! nvim.

use fe_vim::{HeadlessVim, jump::JumpLabelState};
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    case: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    buffer: String,
    cursor: (u32, usize),
    window_rows: u32,
    /// Fed before `keys`, to scroll the view.
    #[serde(default)]
    scroll: String,
    /// `s` and `S` start the jump.
    #[serde(default)]
    jump_on_s: bool,
    keys: String,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    cursor: (u32, usize),
    /// The text, if the keys change it.
    buffer: Option<String>,
    mode: String,
    /// Every label shown, `[row, column, label, state]` in text order;
    /// none if left out (and `live` isn't given).
    #[serde(default)]
    labels: Vec<(u32, usize, String, String)>,
    /// Instead of `labels`, where pages hold too many to list: the live
    /// ones, and how many are concealed.
    live: Option<Vec<(u32, usize, String)>>,
    #[serde(default)]
    concealed: usize,
}

fn state_name(state: JumpLabelState) -> &'static str {
    match state {
        JumpLabelState::Preview => "preview",
        JumpLabelState::Direct => "direct",
        JumpLabelState::Live => "live",
        JumpLabelState::Concealed => "concealed",
    }
}

/// What `case` should leave, against what it left, or `None` if alike.
fn check(case: &Case) -> Option<String> {
    let mut vim = HeadlessVim::new(&case.buffer, case.cursor);
    vim.set_window_rows(case.window_rows);
    vim.set_jump_on_s(case.jump_on_s);
    if let Err(error) = vim.feed(&case.scroll).and_then(|()| vim.feed(&case.keys)) {
        return Some(format!("{}: {error}", case.name));
    }
    let text = vim.model().text();
    let overlay = vim.jump_overlay().unwrap_or_default();
    let shown = overlay.labels.into_iter().map(|label| {
        let point = text.offset_to_point(label.offset);
        let state = state_name(label.state).to_string();
        (point.row, point.column as usize, label.text, state)
    });
    let expect = &case.expect;
    // With `live`, the live labels and a count of the concealed ones.
    let (labels, want_labels, concealed) = match &expect.live {
        Some(live) => {
            let (live_shown, concealed): (Vec<_>, Vec<_>) =
                shown.partition(|label| label.3 == "live");
            let want = live
                .iter()
                .map(|(r, c, l)| (*r, *c, l.clone(), "live".into()));
            (
                live_shown,
                want.collect(),
                (concealed.len(), expect.concealed),
            )
        }
        None => (shown.collect(), expect.labels.clone(), (0, 0)),
    };
    let buffer = expect.buffer.as_deref().unwrap_or(&case.buffer);
    let got = (vim.cursor(), vim.text(), vim.mode(), labels, concealed.0);
    let want = (
        expect.cursor,
        buffer.to_string(),
        expect.mode.as_str(),
        want_labels,
        concealed.1,
    );
    (got != want).then(|| format!("{}:\n  got  {got:?}\n  want {want:?}", case.name))
}

#[test]
fn jump_spec() {
    // Read at run time: a test binary built in another worktree sharing the
    // target would otherwise carry that worktree's path.
    let dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
    let path = dir.join("tests/corpus/jump_labels.toml");
    let file: File = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(
        file.case.len() >= 25,
        "jump_labels: {} cases",
        file.case.len()
    );
    let failures: Vec<String> = file.case.iter().filter_map(check).collect();
    assert!(
        failures.is_empty(),
        "jump_labels: {} of {} cases differ:\n{}",
        failures.len(),
        file.case.len(),
        failures.join("\n")
    );
}
