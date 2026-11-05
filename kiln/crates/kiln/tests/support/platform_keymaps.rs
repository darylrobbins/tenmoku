// SPDX-License-Identifier: Apache-2.0

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
/// `ctrl` (`secondary`), binding for binding (see [`compare`]), but for F10
/// and the bindings `substituted` names as `(macOS keys, other keys,
/// command)`.
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
    let substituted: Vec<_> = substituted
        .iter()
        .map(|&(macos, other, command)| (parse(macos, Os::MacOs), parse(other, Os::Linux), command))
        .collect();

    let macos = bindings(Os::MacOs);
    for os in [Os::Linux, Os::Windows] {
        if let Err(why) = compare(&macos, &bindings(os), &substituted) {
            panic!("macOS against {os:?}: {why}");
        }
    }
}

/// The one binding that exists off macOS only: F10 opens the menu bar
/// drawn in the window, anywhere, in the app's own layer.
fn f10() -> Identity {
    (
        "f10".to_owned(),
        "menu_bar.open".to_owned(),
        None,
        "Null".to_owned(),
        0,
    )
}

/// `other` is `macos` binding for binding (keys, command, context, args and
/// layer), but for [`f10`] and the `substituted` bindings, given as `(macOS
/// keys, other keys, command)`. A substituted binding differs in its keys
/// alone: it is renamed to the macOS keys, then compared like the rest.
fn compare(
    macos: &[Identity],
    other: &[Identity],
    substituted: &[(String, String, &str)],
) -> Result<(), String> {
    let mut other = other.to_vec();
    for (macos_keys, other_keys, command) in substituted {
        let mut named = other
            .iter_mut()
            .filter(|binding| binding.0 == *other_keys && binding.1 == *command);
        let Some(binding) = named.next() else {
            return Err(format!("no `{command}` on {other_keys}"));
        };
        binding.0.clone_from(macos_keys);
    }
    let macos_only = only(macos, &other);
    if !macos_only.is_empty() {
        return Err(format!("macOS's alone: {macos_only:?}"));
    }
    let other_only = only(&other, macos);
    if other_only != [f10()] {
        return Err(format!("the other's alone: {other_only:?}, not [F10]"));
    }
    Ok(())
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

/// The bindings of `of` that `not_in` lacks, one for one, sorted.
fn only(of: &[Identity], not_in: &[Identity]) -> Vec<Identity> {
    let mut left = not_in.to_vec();
    let mut only = Vec::new();
    for binding in of {
        match left.iter().position(|other| other == binding) {
            Some(ix) => {
                left.remove(ix);
            }
            None => only.push(binding.clone()),
        }
    }
    only.sort();
    only
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(keys: &str, command: &str, context: Option<&str>) -> Identity {
        (
            keys.to_owned(),
            command.to_owned(),
            context.map(str::to_owned),
            "Null".to_owned(),
            0,
        )
    }

    fn subs() -> Vec<(String, String, &'static str)> {
        vec![("alt-backspace".into(), "ctrl-backspace".into(), "w.del")]
    }

    fn pair() -> (Vec<Identity>, Vec<Identity>) {
        (
            vec![
                id("a", "x", None),
                id("alt-backspace", "w.del", Some("input")),
            ],
            vec![
                id("a", "x", None),
                id("ctrl-backspace", "w.del", Some("input")),
                f10(),
            ],
        )
    }

    #[test]
    fn the_permitted_differences_pass() {
        let (macos, other) = pair();
        assert_eq!(compare(&macos, &other, &subs()), Ok(()));
    }

    #[test]
    fn a_substituted_binding_in_another_context_fails() {
        let (macos, mut other) = pair();
        other[1].2 = Some("picker".into());
        assert!(compare(&macos, &other, &subs()).is_err());
    }

    #[test]
    fn a_substituted_binding_with_other_args_or_layer_fails() {
        let (macos, mut other) = pair();
        other[1].3 = "Some(1)".into();
        assert!(compare(&macos, &other, &subs()).is_err());
        let (macos, mut other) = pair();
        other[1].4 = 1;
        assert!(compare(&macos, &other, &subs()).is_err());
    }

    #[test]
    fn f10_restricted_to_a_context_or_moved_fails() {
        for mutate in [
            (|b: &mut Identity| b.2 = Some("picker".into())) as fn(&mut Identity),
            |b| b.0 = "f9".into(),
            |b| b.3 = "Some(1)".into(),
            |b| b.4 = 1,
        ] {
            let (macos, mut other) = pair();
            mutate(&mut other[2]);
            assert!(compare(&macos, &other, &subs()).is_err());
        }
    }

    #[test]
    fn a_missing_f10_or_substitute_fails() {
        let (macos, mut other) = pair();
        other.pop();
        assert!(compare(&macos, &other, &subs()).is_err());
        let (macos, mut other) = pair();
        other.remove(1);
        assert!(compare(&macos, &other, &subs()).is_err());
    }
}
