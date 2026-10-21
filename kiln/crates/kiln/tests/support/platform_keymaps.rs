// SPDX-License-Identifier: LicenseRef-Proprietary

//! A check, shared by the tests that load a whole app's keymap on every
//! platform, that holds however many keys the app and kiln bind.

use std::collections::HashSet;

use kiln::{
    Config,
    core::{CommandCatalog, KeySequence, Os},
};

/// kiln's own bindings whose keys differ off macOS, as
/// [`assert_keymaps_agree`] takes them.
#[allow(dead_code, reason = "the gallery doesn't load kiln's keys")]
pub(crate) const KILN: &[(&str, &str, &str)] = &[(
    "alt-backspace",
    "ctrl-backspace",
    "text_input.delete_word_left",
)];

/// A binding as `(keys, command, context, args, layer)`.
type Identity = (String, String, Option<String>, String, usize);

/// `config`'s keymap loads cleanly on every platform and binds no keys
/// twice in one context of a layer. Off macOS it is macOS's with `cmd` as
/// `ctrl` (`secondary`), binding for binding (keys, command, context, args
/// and layer), but for F10, which opens the menus drawn in the window
/// (macOS's are native), and the bindings `substituted` names as
/// `(macOS keys, other keys, command)`.
pub(crate) fn assert_keymaps_agree(
    config: &Config,
    catalog: &dyn CommandCatalog,
    substituted: &[(&str, &str, &str)],
) {
    let bindings = |os: Os| {
        let (keymap, diagnostics) = config.keymap_for(os, Some(catalog));
        assert_eq!(diagnostics, [], "keymap on {os:?}");
        let mut seen = HashSet::new();
        let mut found: Vec<Identity> = Vec::new();
        for binding in keymap.bindings() {
            let context = binding.context.as_ref().map(ToString::to_string);
            let keys = (binding.layer, binding.keys.clone(), context.clone());
            assert!(
                seen.insert(keys),
                "{} is bound twice on {os:?}",
                binding.keys
            );
            found.push((
                spelled(&binding.keys, os),
                binding.command.to_string(),
                context,
                format!("{:?}", binding.args),
                binding.layer,
            ));
        }
        found
    };
    let parse = |keys: &str, os| spelled(&KeySequence::parse(keys).unwrap(), os);
    let mut macos_only: Vec<_> = substituted
        .iter()
        .map(|&(keys, _, command)| (parse(keys, Os::MacOs), command.to_owned()))
        .collect();
    macos_only.sort();
    let mut others_only: Vec<_> = substituted
        .iter()
        .map(|&(_, keys, command)| (parse(keys, Os::Linux), command.to_owned()))
        .chain([(parse("f10", Os::Linux), "menu_bar.open".to_owned())])
        .collect();
    others_only.sort();

    let macos = bindings(Os::MacOs);
    for os in [Os::Linux, Os::Windows] {
        let other = bindings(os);
        assert_eq!(
            only(&macos, &other),
            macos_only,
            "macOS's alone, against {os:?}"
        );
        assert_eq!(only(&other, &macos), others_only, "{os:?}'s alone");
    }
}

/// `keys` as written, with macOS's `cmd` as `ctrl`, `secondary` elsewhere.
fn spelled(keys: &KeySequence, os: Os) -> String {
    let keys: Vec<String> = keys
        .keystrokes()
        .iter()
        .map(|&keystroke| {
            let mut keystroke = keystroke;
            if os == Os::MacOs && keystroke.modifiers.cmd {
                keystroke.modifiers.cmd = false;
                keystroke.modifiers.ctrl = true;
            }
            keystroke.to_string()
        })
        .collect();
    keys.join(" ")
}

/// The bindings of `of` that `not_in` lacks, one for one, as `(keys,
/// command)`, sorted.
fn only(of: &[Identity], not_in: &[Identity]) -> Vec<(String, String)> {
    let mut left = not_in.to_vec();
    let mut only = Vec::new();
    for binding in of {
        match left.iter().position(|other| other == binding) {
            Some(ix) => {
                left.remove(ix);
            }
            None => only.push((binding.0.clone(), binding.1.clone())),
        }
    }
    only.sort();
    only
}
