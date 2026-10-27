//! Focus Editor Group 1-4: `cmd-1` .. `cmd-4` on macOS and `ctrl-1` ..
//! `ctrl-4` on Linux, in the workbench and editor contexts; the image
//! viewer's `secondary-1` still wins there.

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

fn run(keymap: &Keymap, key: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(key).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn the_group_keys_resolve_in_the_workbench_and_editor() {
    for os in [Os::MacOs, Os::Linux] {
        let secondary = if os == Os::MacOs { "cmd" } else { "ctrl" };
        let keymap = keymap(os);
        for context in [&["workbench"][..], &["workbench", "editor"]] {
            let stack = stack(context);
            for n in 1..=4 {
                let keys = format!("{secondary}-{n}");
                assert_eq!(
                    run(&keymap, &keys, &stack).as_deref(),
                    Some(format!("pane.focus_group{n}").as_str()),
                    "{os:?} {keys} in {context:?}"
                );
            }
        }
    }
}

#[test]
fn the_image_viewer_keeps_its_zoom_key() {
    for os in [Os::MacOs, Os::Linux] {
        let secondary = if os == Os::MacOs { "cmd" } else { "ctrl" };
        let keymap = keymap(os);
        let keys = format!("{secondary}-1");
        let viewer = stack(&["workbench", "ImageViewer"]);
        assert_eq!(
            run(&keymap, &keys, &viewer).as_deref(),
            Some("image.zoom_actual"),
            "{os:?}"
        );
        let editor = stack(&["workbench", "editor"]);
        let command = run(&keymap, &keys, &editor).unwrap();
        assert!(!command.starts_with("image."), "{os:?} {command}");
    }
}
