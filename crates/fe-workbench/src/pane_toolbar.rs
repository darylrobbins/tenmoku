//! What a pane's toolbar shows: breadcrumbs, the active item's path under
//! the workbench's root (kiln-shell's [`kiln_shell::crumbs::path_crumbs`]),
//! the item's own crumbs, and the symbols enclosing its caret, each in its
//! kind's syntax colour ([`symbol_color`]).
//!
//! The symbols come from the outline [`crate::OutlinePanel`] already keeps
//! for the active editor: the workbench works the chain out as it follows
//! that editor, only when the caret, the editor or the outline's
//! generation moved, and pushes it into the active pane
//! ([`Toolbar::symbols`]). An inactive pane keeps the chain it showed last.
//! A click on a symbol crumb opens Structure's quick outline
//! ([`QUICK_OUTLINE`]).
//!
//! `view.toggle_breadcrumbs` hides or shows every pane's toolbar; it's a
//! typed command on the workbench.

use std::{ffi::OsString, path::PathBuf};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation, Source, Value};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_syntax::{Outline, OutlineKind};
use gpui::{App, EntityId, Hsla, SharedString, Window};

/// Which of back and forward can run, which the title bar's buttons show;
/// both are off until there's a navigation history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NavButtons {
    pub(crate) back: bool,
    pub(crate) forward: bool,
}

/// A pane's toolbar state, set by the workbench.
#[derive(Clone, Debug)]
pub(crate) struct Toolbar {
    pub(crate) visible: bool,
    /// The workbench's root, which path crumbs are relative to.
    pub(crate) root: Option<PathBuf>,
    pub(crate) symbols: SymbolChain,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self {
            visible: true,
            root: None,
            symbols: SymbolChain::default(),
        }
    }
}

/// The outline items enclosing an editor's caret, at `caret`, outermost
/// first, as `(index in the outline's items, name, kind)`, from the outline of
/// `generation`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SymbolChain {
    pub(crate) editor: Option<EntityId>,
    pub(crate) caret: usize,
    pub(crate) generation: u64,
    pub(crate) items: Vec<(u32, SharedString, OutlineKind)>,
}

/// What a click on a crumb, the tab list or the "…" button asks the
/// workbench to list, in the toolbar picker.
#[derive(Clone, Debug)]
pub(crate) enum ToolbarRequest {
    /// A directory's entries, with the one named `select` selected.
    Entries {
        dir: PathBuf,
        select: Option<OsString>,
    },
    /// The pane commands.
    PaneCommands,
    /// The pane's tabs.
    Tabs,
}

pub(crate) use kiln_shell::crumbs::path_crumbs;

/// What a click on a symbol crumb opens: Structure's quick outline, Go to
/// Symbol in File (`␣o` in the design).
pub(crate) const QUICK_OUTLINE: &str = "nav.document_symbols";

/// The items enclosing `offset`, outermost first: the innermost by binary
/// search, then up its parents.
pub(crate) fn symbol_chain(
    outline: &Outline,
    offset: usize,
) -> Vec<(u32, SharedString, OutlineKind)> {
    let mut chain = Vec::new();
    let mut ix = outline.item_at(offset);
    while let Some(i) = ix {
        let item = &outline.items[i];
        chain.push((i as u32, SharedString::from(item.name.clone()), item.kind));
        ix = item.parent.map(|parent| parent as usize);
    }
    chain.reverse();
    chain
}

/// The colour a symbol crumb of `kind` is drawn in, as its name is in
/// `editor`'s code; `None` if the theme gives that highlight none.
pub(crate) fn symbol_color(kind: OutlineKind, editor: &EditorView) -> Option<Hsla> {
    let highlight = crate::outline_panel::symbol_highlight(kind);
    fe_syntax::Highlight::from_capture(highlight)
        .and_then(|highlight| editor.theme().syntax_color(highlight))
}

/// The head of `editor`'s primary selection: the caret.
pub(crate) fn caret(editor: &EditorView) -> usize {
    let primary = editor.model().primary_selection();
    if primary.reversed {
        primary.range.start
    } else {
        primary.range.end
    }
}

/// The first single key that runs `command` in the workbench's editor, as
/// this platform prints it, read from the keymap now so it follows
/// reloads. A chord isn't shown.
pub(crate) fn shortcut(command: &str, cx: &App) -> Option<String> {
    let command = CommandId::parse(command).ok()?;
    let stack = editor_context();
    let keymap = fe_gpui::current_keymap(cx);
    fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack)
        .iter()
        .find(|keys| keys.keystrokes().len() == 1)
        .map(fe_keymap::shortcut_label)
}

/// The keys the pane menu shows for `command`: of those the active keymap
/// runs it with in `stack`, the focused element's, the one bound for the
/// innermost context there, then with the fewest modifiers held, chords
/// too, as this platform prints them. Read from the keymap now, so it
/// follows reloads; `None` if no key runs it there.
pub(crate) fn menu_shortcut(command: &str, stack: &ContextStack, cx: &App) -> Option<String> {
    let command = CommandId::parse(command).ok()?;
    let keymap = fe_gpui::current_keymap(cx);
    let found = fe_keymap::shortcuts(&keymap, &command, &Value::Null, stack);
    let bindings = keymap.bindings_for(&command);
    let depth = |keys: &fe_keymap::KeySequence| {
        (bindings.iter())
            .filter(|binding| &binding.keys == keys && binding.args.is_null())
            .filter_map(|binding| binding.depth(stack))
            .max()
            .unwrap_or(0)
    };
    let held = |keys: &fe_keymap::KeySequence| {
        (keys.keystrokes().iter())
            .map(|keystroke| {
                let held = keystroke.modifiers;
                [held.cmd, held.ctrl, held.alt, held.shift, held.function]
                    .into_iter()
                    .filter(|&on| on)
                    .count()
            })
            .sum::<usize>()
    };
    // The first of the best, so the keymap's order breaks ties.
    (found.iter().enumerate())
        .min_by_key(|(ix, keys)| (std::cmp::Reverse(depth(keys)), held(keys), *ix))
        .map(|(_, keys)| fe_keymap::shortcut_label(keys))
}

/// The context the pane menu reads keys in when the pane didn't say:
/// the workbench's editor, as [`shortcut`] reads them.
pub(crate) fn editor_context() -> ContextStack {
    [crate::KEY_CONTEXT, fe_gpui::KEY_CONTEXT]
        .into_iter()
        .map(ContextFrame::new)
        .collect()
}

/// Dispatches `command` as a key would, to the focused element and up to
/// the workbench, from `source`.
pub(crate) fn dispatch(command: &str, source: Source, window: &mut Window, cx: &mut App) {
    let Ok(invocation) = Invocation::of(command) else {
        return;
    };
    let context = [ContextFrame::new(crate::KEY_CONTEXT)]
        .into_iter()
        .collect();
    let invocation = invocation.source(source).context(context);
    window.dispatch_action(Box::new(RunCommand(invocation)), cx);
}
