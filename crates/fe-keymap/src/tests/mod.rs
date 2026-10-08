mod click;
mod keymap;
mod keystroke;
mod load;
mod matcher;

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{
    CommandTarget, ContextFrame, ContextPredicate, ContextStack, Dispatched, Effect, Registry,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{KeySequence, Keymap, KeymapLayer, Keystroke};

pub(super) fn keystroke(source: &str) -> Keystroke {
    Keystroke::parse(source).unwrap()
}

pub(super) fn keys(source: &str) -> KeySequence {
    KeySequence::parse(source).unwrap()
}

/// A layer parsed from `source` as if read from `name`, which must be clean.
pub(super) fn layer(name: &str, source: &str) -> KeymapLayer {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(name), source, None);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    layer.unwrap()
}

/// The keymap from a defaults file and a user file.
pub(super) fn keymap(defaults: &str, user: &str) -> Keymap {
    Keymap::from_layers([layer("default.toml", defaults), layer("user.toml", user)])
}

/// A stack of frames with these names.
pub(super) fn stack(names: &[&str]) -> ContextStack {
    names.iter().map(|&name| ContextFrame::new(name)).collect()
}

/// Writes `text` to `name` in `dir`, returning its path.
pub(super) fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

pub(super) struct Target;

impl CommandTarget for Target {
    type Tx = ();
    type Target<'a> = ();
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "only deserialized")]
struct MoveArgs {
    motion: String,
    #[serde(default)]
    extend: bool,
}

/// A registry with `editor.fold_all`, `editor.save`, `editor.move`, which
/// takes args and applies in `editor && !read_only`, and
/// `editor.wide_move`, which takes the same args and applies only with all
/// of 13 flags.
pub(super) fn registry() -> Registry<Target> {
    let mut registry = Registry::new();
    let ok = |_: &mut (), _: &_| Ok(Dispatched::new(Effect::Nothing));
    registry
        .register("editor.fold_all", "Fold all", ok)
        .unwrap();
    registry.register("editor.save", "Save", ok).unwrap();
    registry
        .register_with_args("editor.move", "Move", |_, _, _: MoveArgs| {
            Ok(Dispatched::new(Effect::Changed))
        })
        .unwrap()
        .context(ContextPredicate::parse("editor && !read_only").unwrap());
    // Args are checked whatever the context, however hard to satisfy.
    let wide = ('a'..='m')
        .map(String::from)
        .collect::<Vec<_>>()
        .join(" && ");
    registry
        .register_with_args("editor.wide_move", "Move", |_, _, _: MoveArgs| {
            Ok(Dispatched::new(Effect::Changed))
        })
        .unwrap()
        .context(ContextPredicate::parse(&wide).unwrap());
    registry
}
