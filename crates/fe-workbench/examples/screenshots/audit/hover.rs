//! A hover popover at the caret, with a code block and prose, as
//! rust-analyzer's hover on `HashMap::new` reads.

use fe_gpui::HoverContent;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, select},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "hover",
    steps: |_| vec![select("HashMap::new", 0), hover()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The popover, right of its last line.
    probe(700., 385., "surface.raised"),
    // Its code block, right of the signature.
    probe(700., 305., "surface.panel"),
];

/// A hover popover at the caret, with a signature and a sentence.
fn hover() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return fail("no active editor to hover in".to_owned());
        };
        let text = "```rust\nstd::collections::hash::map::HashMap\n\npub fn new() -> \
                    HashMap<K, V, RandomState>\n```\n\n---\n\nCreates an empty `HashMap`.\n\n\
                    The hash map is initially created with a capacity of 0, so it will not \
                    allocate until it is first inserted into."
            .to_owned();
        editor.update(cx, |editor, cx| {
            editor.show_popover(HoverContent { text, range: None }, cx)
        });
    })
}
