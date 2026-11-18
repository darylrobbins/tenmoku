//! The PDF notice (T1955) over a small PDF in the demo project,
//! `paper.pdf`: its name and size, the line saying Tenmoku can't show PDFs
//! yet, and the Open in Default App and Open as Hex buttons, centred in the
//! editor background.

use std::path::Path;

use crate::{
    audit_scenes::{DEFAULT, EditorProbe, Step, Surface, editor_probe, fail, idle},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "pdf_notice",
    steps: |root| {
        let path = root.join("paper.pdf");
        if let Err(error) = std::fs::write(&path, bytes()) {
            fail(format!("can't write {}: {error}", path.display()));
        }
        vec![open(root), idle(), idle()]
    },
    probes: PROBES,
    editor_probes: EDITOR_PROBES,
    ..DEFAULT
};

// The notice fills the pane in the editor's background; its content is
// centred, so a point near the top of the pane is bare ground.
const PROBES: &[Probe] = &[probe(300., 450., "surface.background")];

const EDITOR_PROBES: &[EditorProbe] = &[editor_probe(900., 150., "background")];

/// A PDF's header and a few binary bytes.
fn bytes() -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    bytes.extend((0..3000).map(|i| (i % 7) as u8));
    bytes
}

/// Opens `paper.pdf`, which opens in the notice.
fn open(root: &Path) -> Step {
    let path = root.join("paper.pdf");
    Box::new(move |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    })
}
