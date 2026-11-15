//! The Problems panel's detail pane (T1587): `src/main.rs`'s error selected
//! and, beside the list (the bottom dock is wide), its code (a link to its
//! page) and source, its message, its row and one either side with
//! `line.clone()` underlined, a suggested fix as a celadon-soft line with a +
//! glyph, then Apply fix, its key and the fix's title. The error is put on the open editor as a language server's
//! would be (`Workbench::set_diagnostics_for_audit`), and the fix as a
//! server's preferred quick fix would show
//! (`Workbench::show_problem_fix_for_audit`): the audit runs no server.

use std::{ops::Range, path::Path};

use fe_editor::diagnostics::{Diagnostic, Severity};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, run},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "problems_detail",
    steps: |root| {
        vec![
            diagnostic(root),
            run("problems.show"),
            run("problems.select_first"),
            fix(),
        ]
    },
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The pane's ground, right of `E0308 · rustc`.
    probe(950., 406., "surface.panel"),
    // The snippet's ground, right of row 10's `}`.
    probe(900., 495., "surface.background"),
    // The fix's line, between its + glyph and its text.
    probe(735., 524., "status.success_soft"),
    // Apply fix, a primary button, left of its label.
    probe(697., 550., "accent.base"),
    // The list beside the pane, below its rows.
    probe(500., 520., "surface.panel"),
    // The editor, below the code.
    probe(400., 300., "surface.background"),
];

/// What's wrong with `line.clone()`, the suggested fix's text for it.
const WRONG: &str = "line.clone()";
const FIXED: &str = "&line";

/// `WRONG`'s bytes in `src/main.rs`.
fn wrong() -> Range<usize> {
    let start = MAIN_RS.find(WRONG).expect("the demo's text");
    start..start + WRONG.len()
}

/// Puts the scene's error on the demo's `src/main.rs`.
fn diagnostic(root: &Path) -> Step {
    let main_rs = root.join("src/main.rs");
    Box::new(move |workbench, _, cx| {
        let tabs = workbench.pane().read(cx).tabs();
        let open = (tabs.iter().filter_map(|tab| tab.editor()))
            .any(|editor| editor.read(cx).model().path() == Some(main_rs.as_path()));
        if !open {
            return fail(format!("{} isn't open", main_rs.display()));
        }
        let error = Diagnostic {
            source: Some("rustc".to_owned()),
            code: Some("E0308".to_owned()),
            code_href: Some("https://doc.rust-lang.org/error_codes/E0308.html".to_owned()),
            ..Diagnostic::new(
                Severity::Error,
                "mismatched types: expected `&str`, found `String`",
            )
        };
        workbench.set_diagnostics_for_audit(&main_rs, vec![(wrong(), error)], cx);
    })
}

/// Shows the suggested fix: the error's row borrowing `line`.
fn fix() -> Step {
    Box::new(|workbench, _, cx| {
        let Range { start, end } = wrong();
        let row_start = MAIN_RS[..start].rfind('\n').map_or(0, |at| at + 1);
        let row_end = MAIN_RS[end..]
            .find('\n')
            .map_or(MAIN_RS.len(), |at| end + at);
        let fixed = format!(
            "{}{FIXED}{}",
            &MAIN_RS[row_start..start],
            &MAIN_RS[end..row_end]
        );
        let title = "Consider borrowing here: `&line`";
        if !workbench.show_problem_fix_for_audit(title, vec![fixed], cx) {
            fail("no problem in an open file is selected".to_owned());
        }
    })
}
