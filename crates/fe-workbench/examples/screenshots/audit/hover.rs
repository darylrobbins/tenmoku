//! The hover card at the caret, as rust-analyzer's hover on `HashMap::new`
//! reads: its module path, its signature, a rule and the docs.

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
    // The card, right of its module path and signature: no code block
    // behind them now.
    probe(700., 262., "surface.raised"),
];

/// The hover card at the caret: rust-analyzer's two fences, then the docs.
fn hover() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return fail("no active editor to hover in".to_owned());
        };
        let text = "```rust\nstd::collections::hash::map::HashMap\n```\n\n\
                    ```rust\npub fn new() -> HashMap<K, V, RandomState>\n```\n\n\
                    ---\n\nCreates an empty `HashMap`.\n\n\
                    The hash map is initially created with a capacity of 0, so it will not \
                    allocate until it is first inserted into."
            .to_owned();
        editor.update(cx, |editor, cx| {
            editor.show_popover(HoverContent { text, range: None }, cx)
        });
    })
}
