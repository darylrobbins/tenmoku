//! The hex viewer (T1895) over a small binary file in the demo project,
//! `data.bin`: its header strip (the file's length and the Text button) and
//! its rows of offset, hex and ASCII, with bytes 0x10 to 0x1F selected, so
//! the hex column has the selection and the ASCII column its weaker mirror.

use std::path::Path;

use crate::{
    audit_scenes::{DEFAULT, EditorProbe, Step, Surface, editor_probe, fail, idle},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "hex_viewer",
    steps: |root| {
        let path = root.join("data.bin");
        if let Err(error) = std::fs::write(&path, bytes()) {
            fail(format!("can't write {}: {error}", path.display()));
        }
        vec![open(root), idle(), idle(), select(), idle()]
    },
    probes: PROBES,
    editor_probes: EDITOR_PROBES,
    ..DEFAULT
};

// The pane's centre starts at x 282 and its content at y 96: the header
// strip, then the rows, 20 px each. The inspector card (a later card) takes
// a strip at the bottom, so the probes stay in the header and the upper rows.
const PROBES: &[Probe] = &[
    // Right of the ASCII column, on the rows' ground.
    probe(900., 150., "surface.background"),
];

const EDITOR_PROBES: &[EditorProbe] = &[
    // The header strip, between the length and the Text button.
    editor_probe(600., 104., "popover_background"),
    // Row 1's ASCII column, at a cell's top edge, clear of the glyphs: the
    // mirror of the hex column's selection.
    editor_probe(770., 147., "identifier_highlight"),
];

/// 160 bytes: text, NULs, line breaks and high bytes, so every column has
/// something in it.
fn bytes() -> Vec<u8> {
    let mut bytes = b"\x7FELF\x02\x01\x01\0\0\0\0\0\0\0\0\0Hello, hex view!\r\n".to_vec();
    bytes.extend((0..=255u8).step_by(2));
    bytes.extend_from_slice(b"\xDE\xAD\xBE\xEF tail\n");
    bytes
}

/// Opens `data.bin`, which opens in a hex viewer for its NULs.
fn open(root: &Path) -> Step {
    let path = root.join("data.bin");
    Box::new(move |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    })
}

/// Selects bytes 0x10 to 0x1F in the hex column.
fn select() -> Step {
    Box::new(|workbench, _, cx| {
        let pane = workbench.pane().read(cx);
        let viewer = pane
            .active()
            .and_then(|ix| pane.tabs()[ix].hex_viewer().cloned());
        let Some(viewer) = viewer else {
            return fail("data.bin isn't open in a hex viewer".to_owned());
        };
        viewer.update(cx, |viewer, cx| {
            viewer.select(0x10, 0x1F);
            cx.notify();
        });
    })
}
