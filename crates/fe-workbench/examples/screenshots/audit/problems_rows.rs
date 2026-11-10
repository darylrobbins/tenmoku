//! The Problems panel with rows: one problem of each severity in
//! `src/main.rs`, the error with a related location, and one in
//! `README.md`, put on the open editors as a language server's would be
//! (`Workbench::set_diagnostics_for_audit`), so the editor shows their
//! squiggles too.

use std::{ops::Range, path::Path};

use fe_editor::diagnostics::{Diagnostic, RelatedInfo, Severity};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, run},
    scenes::MAIN_RS,
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "problems_rows",
    steps: |root| vec![diagnostics(root), run("problems.show")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The panel, below its rows.
    probe(174., 400., "surface.panel"),
    // The editor, below the code its squiggles mark.
    probe(400., 400., "surface.background"),
];

/// The `nth` (from 0) `text` in `src/main.rs`, as bytes.
fn find(text: &str, nth: usize) -> Range<usize> {
    let (start, _) = (MAIN_RS.match_indices(text).nth(nth)).expect("the demo's text");
    start..start + text.len()
}

/// `severity`'s `message` from `source`, with `code` if any.
fn diagnostic(severity: Severity, message: &str, source: &str, code: Option<&str>) -> Diagnostic {
    Diagnostic {
        source: Some(source.to_owned()),
        code: code.map(str::to_owned),
        ..Diagnostic::new(severity, message)
    }
}

/// Puts the scene's problems on the demo's two open files.
fn diagnostics(root: &Path) -> Step {
    let (main_rs, readme) = (root.join("src/main.rs"), root.join("README.md"));
    Box::new(move |workbench, _, cx| {
        let tabs = workbench.pane().read(cx).tabs();
        for path in [&main_rs, &readme] {
            let open = (tabs.iter().filter_map(|tab| tab.editor()))
                .any(|editor| editor.read(cx).model().path() == Some(path.as_path()));
            if !open {
                return fail(format!("{} isn't open", path.display()));
            }
        }
        let assigned = find("line.clone()", 0);
        let error = Diagnostic {
            related: vec![RelatedInfo {
                path: None,
                row: 6,
                range: Some(find("HashMap<String, usize>", 0)),
                message: "expected due to this".to_owned(),
            }],
            ..diagnostic(
                Severity::Error,
                "mismatched types: expected `&str`, found `String`",
                "rustc",
                Some("E0308"),
            )
        };
        let main_problems = vec![
            (assigned, error),
            (
                find("mut counts", 0),
                diagnostic(
                    Severity::Warning,
                    "variable does not need to be mutable",
                    "rustc",
                    Some("unused_mut"),
                ),
            ),
            (
                find("line.split_whitespace().count()", 0),
                diagnostic(
                    Severity::Information,
                    "this counts the words of every line again for each entry",
                    "clippy",
                    None,
                ),
            ),
            (
                find("{counts:?}", 0),
                diagnostic(
                    Severity::Hint,
                    "consider sorting the counts before printing them",
                    "rust-analyzer",
                    None,
                ),
            ),
        ];
        workbench.set_diagnostics_for_audit(&main_rs, main_problems, cx);
        let readme_problems = vec![(
            10..15,
            diagnostic(
                Severity::Warning,
                "\"small\" is a weasel word",
                "vale",
                Some("write-good.Weasel"),
            ),
        )];
        workbench.set_diagnostics_for_audit(&readme, readme_problems, cx);
    })
}
