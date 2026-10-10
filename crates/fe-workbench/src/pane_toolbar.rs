//! What a pane's toolbar shows: back and forward, then breadcrumbs, the
//! active tab's path under the workbench's root and the symbols enclosing
//! its caret.
//!
//! The symbols come from the outline [`crate::OutlinePanel`] already keeps
//! for the active editor: the workbench works the chain out as it follows
//! that editor, only when the caret, the editor or the outline's
//! generation moved, and pushes it into the active pane
//! ([`Toolbar::symbols`]). An inactive pane keeps the chain it showed last.
//!
//! `view.toggle_breadcrumbs` hides or shows every pane's toolbar; it's in
//! [`init`]'s registry, in the workbench's [`crate::KEY_CONTEXT`], and its
//! handler only says it ran: the workbench does it.

use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use fe_command::{
    Actor, CommandId, CommandTarget, ContextFrame, ContextPredicate, ContextStack, Dispatched,
    Effect, Invocation, Registry, Source, Value,
};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_syntax::Outline;
use gpui::{App, EntityId, Global, SharedString, WeakEntity, Window};

/// Which of back and forward can run. The workbench pushes it into each
/// pane; both are off until there's a navigation history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NavButtons {
    pub(crate) back: bool,
    pub(crate) forward: bool,
}

/// A pane's toolbar state, set by the workbench.
#[derive(Clone, Debug)]
pub(crate) struct Toolbar {
    pub(crate) visible: bool,
    pub(crate) nav: NavButtons,
    /// The workbench's root, which path crumbs are relative to.
    pub(crate) root: Option<PathBuf>,
    pub(crate) symbols: SymbolChain,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self {
            visible: true,
            nav: NavButtons::default(),
            root: None,
            symbols: SymbolChain::default(),
        }
    }
}

/// The outline items enclosing an editor's caret, at `caret`, outermost
/// first, as `(index in the outline's items, name)`, from the outline of
/// `generation`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SymbolChain {
    pub(crate) editor: Option<EntityId>,
    pub(crate) caret: usize,
    pub(crate) generation: u64,
    pub(crate) items: Vec<(u32, SharedString)>,
}

/// What a click on a crumb or the "…" button asks the workbench to list,
/// in the toolbar picker.
#[derive(Clone, Debug)]
pub(crate) enum ToolbarRequest {
    /// A directory's entries, with the one named `select` selected.
    Entries {
        dir: PathBuf,
        select: Option<OsString>,
    },
    /// The siblings of `item`, of the outline of `generation`, whose chain
    /// of names from the top level is `names`.
    Symbols {
        editor: WeakEntity<EditorView>,
        generation: u64,
        item: u32,
        names: Vec<SharedString>,
    },
    /// The pane commands.
    PaneCommands,
}

/// One path crumb: a directory, or the file last.
pub(crate) struct PathCrumb {
    pub(crate) label: SharedString,
    /// The directory it lists when clicked: its own, or the file's.
    pub(crate) dir: Option<PathBuf>,
    pub(crate) file: bool,
}

/// `path`'s crumbs: a directory per component under `root`, then the file;
/// outside `root`, its parent directory's name, then the file. A buffer
/// with no path is `untitled`.
pub(crate) fn path_crumbs(path: Option<&Path>, root: Option<&Path>) -> Vec<PathCrumb> {
    let Some(path) = path else {
        return vec![PathCrumb {
            label: "untitled".into(),
            dir: None,
            file: true,
        }];
    };
    let parent = path.parent().map(Path::to_owned);
    let (mut dir, dirs): (PathBuf, Vec<_>) =
        match root.and_then(|root| Some((root, path.strip_prefix(root).ok()?))) {
            Some((root, relative)) => {
                let mut components: Vec<_> = relative.components().collect();
                components.pop();
                (root.to_owned(), components)
            }
            None => match parent.as_deref().and_then(Path::file_name) {
                Some(name) => {
                    let above = parent.as_deref().and_then(Path::parent);
                    let above = above.map_or_else(PathBuf::new, Path::to_owned);
                    (above, vec![Component::Normal(name)])
                }
                None => (PathBuf::new(), Vec::new()),
            },
        };
    let mut crumbs: Vec<_> = dirs
        .into_iter()
        .map(|component| {
            dir.push(component);
            PathCrumb {
                label: component.as_os_str().to_string_lossy().into_owned().into(),
                dir: Some(dir.clone()),
                file: false,
            }
        })
        .collect();
    crumbs.push(PathCrumb {
        label: crate::pane::file_name(Some(path)).into(),
        dir: parent,
        file: true,
    });
    crumbs
}

/// The items enclosing `offset`, outermost first: the innermost by binary
/// search, then up its parents.
pub(crate) fn symbol_chain(outline: &Outline, offset: usize) -> Vec<(u32, SharedString)> {
    let mut chain = Vec::new();
    let mut ix = outline.item_at(offset);
    while let Some(i) = ix {
        let item = &outline.items[i];
        chain.push((i as u32, SharedString::from(item.name.clone())));
        ix = item.parent.map(|parent| parent as usize);
    }
    chain.reverse();
    chain
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
    let stack: ContextStack = [crate::KEY_CONTEXT, fe_gpui::KEY_CONTEXT]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let keymap = fe_gpui::current_keymap(cx);
    fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack)
        .iter()
        .find(|keys| keys.keystrokes().len() == 1)
        .map(fe_keymap::shortcut_label)
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

/// The [`CommandTarget`] of the toolbar's command: whether it ran.
pub(crate) struct ToolbarCommands;

impl CommandTarget for ToolbarCommands {
    type Tx = ();
    type Target<'a> = bool;
}

struct Commands(Rc<Registry<ToolbarCommands>>);

impl Global for Commands {}

/// Registers `view.toggle_breadcrumbs`.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<ToolbarCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            "view.toggle_breadcrumbs",
            "Toggle Breadcrumbs",
            |ran: &mut bool, _: &Actor| {
                *ran = true;
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("the toolbar command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// Whether `run` is `view.toggle_breadcrumbs`, in a context where it
/// applies.
pub(crate) fn toggles(run: &RunCommand, cx: &App) -> bool {
    let Some(commands) = cx.try_global::<Commands>() else {
        return false;
    };
    let registry = commands.0.clone();
    if registry.get(&run.0.command).is_none() {
        return false;
    }
    let mut ran = false;
    if let Err(error) = registry.dispatch(&mut ran, &run.0) {
        log::warn!("{error}");
    }
    ran
}
