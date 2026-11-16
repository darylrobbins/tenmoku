//! The debugger over the line numbers (`docs/design/design-system.md`,
//! Debugger; debugger card D6), in `src/main.rs`: every D6 state, each on
//! a row of its own, as the editor's public API draws them (the workbench's
//! `debug_gutter.rs` hands it the same marks from the store and a session):
//! an enabled breakpoint on `use`, an unverified one on the doc comment, a
//! disabled one on `let`, the stopped frame on a breakpoint on the loop's
//! body, with the current line's band, and a caller frame's band and
//! outlined arrow on `println!`.

use std::{cell::RefCell, rc::Rc};

use fe_editor::decorations::{Decoration, DecorationKind, DecorationSet, Tint};
use fe_gpui::{BreakpointGlyph, BreakpointMark, BreakpointState, EditorView, FrameArrow};
use gpui::{Context, Entity};

use crate::{
    audit_scenes::{DEFAULT, EditorProbe, Step, Surface, editor_probe, fail, idle},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "debug_gutter",
    steps: |_| vec![idle(), show(true), idle(), show(false), check()],
    probes: PROBES,
    editor_probes: EDITOR_PROBES,
    ..DEFAULT
};

/// `src/main.rs`'s rows (`scenes::MAIN_RS`) the states are on.
const ENABLED: u32 = 2;
const UNVERIFIED: u32 = 4;
const DISABLED: u32 = 6;
const STOPPED: u32 = 8;
const CALLER: u32 = 10;

// The line-number column starts at x 282 and the code at 363; the rows
// are 20 px from y 96 (`src/main.rs`'s row 2 at 136).
const PROBES: &[Probe] = &[
    // The enabled breakpoint's dot, at its centre: shinsha's value, its
    // own role.
    probe(302., 146., "status.error"),
    // The stopped frame's arrow, in its body: the accent.
    probe(313.5, 266., "accent.base"),
    // The current line's band's edge, the last 2 px before the code: the
    // accent.
    probe(362., 266., "accent.base"),
    // The caller frame's band's edge: muted text.
    probe(362., 306., "text.muted"),
];

const EDITOR_PROBES: &[EditorProbe] = &[
    // The disabled breakpoint's grey dot, at its centre.
    editor_probe(302., 226., "debug_breakpoint_disabled"),
    // The current line's band, in the loop body's indent, clear of the
    // indent guides.
    editor_probe(378., 266., "debug_current_line"),
    // A caller frame's band, in `println!`'s indent.
    editor_probe(378., 306., "debug_frame_line"),
];

/// The marks: every D6 state.
fn marks() -> Vec<BreakpointMark> {
    let mark = |row, breakpoint, frame| BreakpointMark {
        row,
        breakpoint,
        frame,
        target: None,
    };
    vec![
        mark(ENABLED, Some(BreakpointState::Enabled), None),
        mark(
            UNVERIFIED,
            Some(BreakpointState::Unverified {
                message: Some("no code at this line".into()),
            }),
            None,
        ),
        mark(DISABLED, Some(BreakpointState::Disabled), None),
        mark(
            STOPPED,
            Some(BreakpointState::Enabled),
            Some(FrameArrow::Current),
        ),
        mark(CALLER, None, Some(FrameArrow::Caller)),
    ]
}

/// The active editor, `src/main.rs`.
fn main_rs(
    workbench: &fe_workbench::Workbench,
    cx: &Context<fe_workbench::Workbench>,
) -> Option<Entity<EditorView>> {
    let editor = workbench.active_editor(cx).cloned()?;
    let path = editor.read(cx).model().path()?;
    path.ends_with("src/main.rs").then_some(editor)
}

/// Shows the marks, and with `bands` the two bands as a decoration
/// provider: again a frame later, over anything the store showed since.
fn show(bands: bool) -> Step {
    Box::new(move |workbench, _, cx| {
        let Some(editor) = main_rs(workbench, cx) else {
            return fail("src/main.rs isn't the active editor".to_owned());
        };
        editor.update(cx, |editor, cx| {
            let version = editor.model().snapshot().version();
            if bands {
                let snapshot = editor.model().snapshot().clone();
                let text = snapshot.text();
                let band = |row: u32, tint| {
                    let start = text.row_start(row);
                    let end = start + text.row_len(row) as usize;
                    Decoration::new(start..end, DecorationKind::RowBackground, tint)
                };
                let mut set = DecorationSet::new();
                set.apply(
                    &snapshot,
                    version,
                    vec![
                        band(STOPPED, Tint::DebugCurrent),
                        band(CALLER, Tint::DebugFrame),
                    ],
                );
                (editor.model_mut()).add_decoration_provider(Rc::new(RefCell::new(set)));
            }
            editor.set_breakpoints(version, marks(), cx);
        });
    })
}

/// Fails the scene unless the last frame drew every state and both bands.
fn check() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(editor) = main_rs(workbench, cx) else {
            return fail("src/main.rs isn't the active editor".to_owned());
        };
        let editor = editor.read(cx);
        let Some(drawn) = editor.drawn_gutter() else {
            return fail("the editor drew no gutter".to_owned());
        };
        let glyphs: Vec<(u32, BreakpointGlyph)> = (drawn.breakpoints.iter())
            .map(|drawn| (drawn.row, drawn.glyph))
            .collect();
        let want = [
            (ENABLED, BreakpointGlyph::Dot),
            (UNVERIFIED, BreakpointGlyph::Ring),
            (DISABLED, BreakpointGlyph::Disabled),
            (STOPPED, BreakpointGlyph::Dot),
            (STOPPED, BreakpointGlyph::CurrentArrow),
            (CALLER, BreakpointGlyph::CallerArrow),
        ];
        if glyphs != want {
            return fail(format!("the gutter drew {glyphs:?}, not {want:?}"));
        }
        let bands: Vec<Tint> = (editor.drawn_row_backgrounds().into_iter())
            .map(|(_, tint, _)| tint)
            .filter(|tint| matches!(tint, Tint::DebugCurrent | Tint::DebugFrame))
            .collect();
        if bands != [Tint::DebugCurrent, Tint::DebugFrame] {
            fail(format!("the editor drew the bands {bands:?}"));
        }
    })
}
