//! The image viewer's zoom keys, on macOS and Linux: bound in the
//! `ImageViewer` context, and not in the editor's.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, Keymap, KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[&str]) -> ContextStack {
    frames.iter().map(|name| ContextFrame::new(*name)).collect()
}

/// What typing `key` runs in `stack`.
fn run(keymap: &Keymap, key: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(key).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn the_zoom_keys_resolve_in_the_image_viewer_on_each_platform() {
    let viewer = stack(&["workbench", "ImageViewer"]);
    for os in [Os::MacOs, Os::Linux] {
        let secondary = if os == Os::MacOs { "cmd" } else { "ctrl" };
        let keymap = keymap(os);
        for (key, command) in [
            ("=", "image.zoom_in"),
            ("+", "image.zoom_in"),
            ("-", "image.zoom_out"),
            ("0", "image.zoom_fit"),
            ("1", "image.zoom_actual"),
        ] {
            let keys = format!("{secondary}-{key}");
            assert_eq!(
                run(&keymap, &keys, &viewer).as_deref(),
                Some(command),
                "{os:?} {keys}"
            );
        }
    }
}

#[test]
fn the_zoom_keys_are_not_bound_in_the_editor() {
    let editor = stack(&["workbench", "editor"]);
    for os in [Os::MacOs, Os::Linux] {
        let secondary = if os == Os::MacOs { "cmd" } else { "ctrl" };
        let keymap = keymap(os);
        for key in ["=", "+", "-", "0", "1"] {
            let keys = format!("{secondary}-{key}");
            let got = run(&keymap, &keys, &editor);
            assert!(
                !got.as_deref()
                    .is_some_and(|command| command.starts_with("image.")),
                "{os:?} {keys}: {got:?}"
            );
        }
    }
}
