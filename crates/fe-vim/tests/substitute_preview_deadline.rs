//! The `:s` preview gives up cleanly, changing nothing, once its time limit
//! has passed: the template's validation as the line is read, its `~`
//! measured and expanded, its compilation and its application each count
//! work units against the preview's deadline. The deadline is expired at a
//! counted point of the work (`set_work_limit`), not by lifting the time
//! limit and waiting.

use fe_vim::HeadlessVim;
use fe_vim::search::{set_work_limit, work_done};

/// How long the templates are: under the preview's size cap.
const LEN: u64 = 1000;

/// Previews `line` on `text` with a deadline of `limit` work units; the
/// preview, the units counted, and checks that nothing else changed.
fn preview_within(
    text: &str,
    line: &str,
    limit: Option<u64>,
) -> (Option<fe_vim::SubstitutePreview>, u64) {
    preview_after(text, "", line, limit)
}

/// [`preview_within`] after running `seed`, a `:` line, which sets the last
/// substitute's replacement.
fn preview_after(
    text: &str,
    seed: &str,
    line: &str,
    limit: Option<u64>,
) -> (Option<fe_vim::SubstitutePreview>, u64) {
    let mut vim = HeadlessVim::new(text, (0, 0));
    if !seed.is_empty() {
        vim.feed(&format!(":{seed}<CR>")).unwrap();
    }
    let text = vim.text();
    vim.feed(":").unwrap();
    let registers = format!("{:?}", vim.registers().last_search());
    set_work_limit(limit);
    let before = work_done();
    let preview = vim
        .state()
        .substitute_preview(vim.model(), vim.registers(), line);
    let counted = work_done() - before;
    set_work_limit(None);
    assert_eq!(vim.text(), text);
    assert_eq!(format!("{:?}", vim.registers().last_search()), registers);
    assert!(!vim.state().failed());
    (preview, counted)
}

/// The units a template of `LEN` bytes (that matches nothing) takes through
/// each stage, in order: validated as the line is read, read again, `~`
/// measured, `~` expanded, compiled.
#[test]
fn template_work_stops_at_each_stage() {
    let line = format!("s/zzz/{}/", "a".repeat(LEN as usize));
    let (none, whole) = preview_within("one\n", &line, None);
    assert!(none.is_none());
    assert_eq!(whole, 5 * LEN);
    for stage in 0..5 {
        // Half way through the stage.
        let limit = stage * LEN + LEN / 2;
        let (preview, counted) = preview_within("one\n", &line, Some(limit));
        assert!(preview.is_none(), "stage {stage}");
        assert!(
            (limit..=limit + 2).contains(&counted),
            "stage {stage}: {counted} units counted past {limit}"
        );
    }
}

#[test]
fn tilde_expansion_counts_what_it_writes() {
    // `~` is the last substitute's replacement: not set here, so empty. Its
    // own bytes count (here none, so compiling it takes nothing).
    let line = format!("s/zzz/{}/", "~".repeat(LEN as usize));
    let (_, whole) = preview_within("one\n", &line, None);
    assert_eq!(whole, 4 * LEN);
    let (preview, counted) = preview_within("one\n", &line, Some(3 * LEN + 5));
    assert!(preview.is_none());
    assert!(counted <= 3 * LEN + 8, "{counted}");
}

#[test]
fn application_stops_within_a_bound_of_the_deadline() {
    let text = "a ".repeat(500);
    let line = "s/a/xy/g";
    let (full, whole) = preview_within(&text, line, None);
    let full = full.expect("a full preview");
    assert_eq!(full.matches.len(), 500);
    // The template takes 5 × 2 units; each match a piece and two bytes.
    let setup = 10;
    for matches in [0, 1, 7, 250, 499] {
        let limit = setup + 3 * matches + 1;
        let (preview, counted) = preview_within(&text, line, Some(limit));
        assert!(counted <= limit + 3, "{counted} past {limit}");
        assert!(counted < whole);
        // Gives up (nothing), or keeps what it got before: never more than
        // the matches it had time for, and the same ones.
        if let Some(preview) = preview {
            assert!(preview.matches.len() as u64 <= matches);
            assert_eq!(preview.matches[..], full.matches[..preview.matches.len()]);
        }
    }
}

#[test]
fn an_unhurried_preview_is_unchanged() {
    let (preview, _) = preview_within("a a\n", "s/a/b/g", None);
    assert_eq!(preview.unwrap().matches.len(), 2);
}

#[test]
fn a_nonempty_previous_replacement_counts_as_it_is_copied() {
    // `~` copies the last substitute's replacement: `COPIED` bytes each.
    const COPIED: u64 = 50;
    const TILDES: u64 = 20;
    let seed = format!("s/one/{}/", "x".repeat(COPIED as usize));
    let line = format!("s/zzz/{}/", "~".repeat(TILDES as usize));
    let run = |limit| preview_after("one\n", &seed, &line, limit);
    let (none, whole) = run(None);
    assert!(none.is_none());
    // Read twice and measured a unit a `~`, expanded a unit and the copy a
    // `~`, then the expanded replacement compiled a unit a byte.
    let expanding = 3 * TILDES;
    let compiling = expanding + TILDES * (1 + COPIED);
    assert_eq!(whole, compiling + TILDES * COPIED);
    for (limit, step) in [
        (expanding + TILDES * (1 + COPIED) / 2, 1 + COPIED),
        (compiling + TILDES * COPIED / 2, 1),
    ] {
        let (preview, counted) = run(Some(limit));
        assert!(preview.is_none(), "limit {limit}");
        assert!(
            (limit..=limit + step).contains(&counted),
            "{counted} units counted past {limit}"
        );
    }
}
