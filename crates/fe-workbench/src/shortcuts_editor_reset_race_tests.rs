//! Reset Keybinding's refusal to edit keymap.toml when it can't tell what
//! a gated entry does to a kiln default, held at the click on the text the
//! write edits: a gated unbind saved after the menu opened is refused.
//! On the parent module's workbench, config directory and keymap store.

use super::*;

const PALETTE: &str = "command_palette.toggle";

/// Opens Show All Commands' menu over a file that holds only a rebinding
/// of it, so Reset is offered, and then saves a gated unbind of its kiln
/// key behind the tab's back, as another editor would. Returns the file's
/// text after that, and clicks Reset.
fn click_reset_after_gated_unbind(cx: &mut TestAppContext) -> (Dirs, String, Vec<String>) {
    let (other, palette_keys) = match Os::current() {
        Os::MacOs => ("linux", "ctrl-shift-p"),
        _ => ("macos", "cmd-shift-p"),
    };
    let dirs = Dirs::new();
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"alt-9\"\ncommand = \"{PALETTE}\"\n"
    ));
    // A store kept with this platform's default layers alone, not kiln's
    // setup: the one whose Reset can be refused.
    let shipped = Shipped {
        extension: None,
        extension_elsewhere: None,
        platforms: false,
    };
    let (workbench, cx) = workbench_with(cx, &dirs, shipped);
    let editor = open_shortcuts(&workbench, cx);
    set_search(&editor, "show all commands", cx);
    right_click(&format!("shortcuts-row-{PALETTE}"), cx);
    let items = menu_items(&editor, cx).unwrap();
    assert!(items.contains(&MenuItem::ResetKeybinding), "{items:?}");
    let disabled = editor.read_with(cx, |editor, _| {
        editor.disabled_menu_items().map(<[_]>::to_vec)
    });
    assert_eq!(disabled, Some(vec![]), "offered on the file as read");

    // Saved after the menu opened; the tab hasn't reloaded.
    let gated = format!(
        "[[bindings]]\nkeys = \"alt-9\"\ncommand = \"{PALETTE}\"\n\n\
         [[bindings]]\nkeys = \"{palette_keys}\"\ncommand = \"\"\nplatform = \"{other}\"\n"
    );
    dirs.write_keymap(&gated);
    click_menu_item(MenuItem::ResetKeybinding, cx);
    let toasts = (toasts(&workbench, cx).into_iter())
        .filter_map(|(_, _, detail)| detail)
        .collect();
    (dirs, gated, toasts)
}

#[gpui::test]
fn reset_rechecks_refusal_on_fresh_source(cx: &mut TestAppContext) {
    let (_dirs, _gated, toasts) = click_reset_after_gated_unbind(cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert!(toasts[0].contains("edit keymap.toml"), "{toasts:?}");
}

#[gpui::test]
fn reset_refusal_leaves_file_untouched(cx: &mut TestAppContext) {
    let (dirs, gated, _toasts) = click_reset_after_gated_unbind(cx);
    assert_eq!(dirs.read_keymap(), gated);
}
