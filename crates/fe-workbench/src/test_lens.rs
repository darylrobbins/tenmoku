//! A Run Test lens above each test, as rust-analyzer's in VS Code: a
//! [`LensProvider`] per editor, giving `Run Test` over each test function
//! and `Run Tests` over each test module or class, whose click runs
//! `tests.run_row` ([`crate::test_commands`]) for that row, wherever the
//! caret is.
//!
//! The provider is asked each frame for a snapshot but has no tree, so it
//! shows what [`sync`] read off the editor's parse: the rows, anchored to
//! the text, so an edit moves them at once and a new parse corrects them.
//! The editors that get one are the tabs' showing a file inside a trusted
//! root, in a language with runnables, while `editor.code_lens` is on.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use fe_buffer::{Anchor, BufferSnapshot, Version};
use fe_command::{Invocation, Value};
use fe_gpui::{EditorView, Lens, LensItem, LensProvider};
use gpui::{App, Context, Entity, EntityId, Subscription, WeakEntity};

use crate::{
    Workbench,
    test_runnables::{RunnableKind, TEST_LANGUAGES, runnables},
};

/// What a click on a test's lens runs.
const RUN_ROW: &str = "tests.run_row";

/// One lens row: where it sits, and what it says.
type Row = (Anchor, &'static str);

/// What a provider shows: the rows of the parse at `version`.
#[derive(Default)]
struct Shown {
    version: Option<Version>,
    rows: Vec<Row>,
}

impl LensProvider for Shown {
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens> {
        let text = snapshot.text();
        (self.rows.iter())
            .map(|(anchor, title)| {
                let row = text.offset_to_point(snapshot.resolve(anchor)).row;
                Lens {
                    row,
                    items: vec![LensItem {
                        title: (*title).into(),
                        invocation: click(row),
                    }],
                }
            })
            .collect()
    }
}

/// The click on the lens over `row`.
fn click(row: u32) -> Invocation {
    Invocation::of(RUN_ROW)
        .expect("tests.run_row is registered")
        .args(Value::Object(
            [("row".to_owned(), row.into())].into_iter().collect(),
        ))
}

/// An editor's provider.
struct Slot {
    editor: WeakEntity<EditorView>,
    shown: Rc<RefCell<Shown>>,
    provider: Rc<RefCell<dyn LensProvider>>,
    /// Whether the editor has the provider added.
    added: bool,
    _observed: Subscription,
}

/// A window's providers, by editor.
type Slots = Rc<RefCell<HashMap<EntityId, Slot>>>;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|_: &mut Workbench, window, cx| {
        if window.is_none() {
            return;
        }
        let slots = Slots::default();
        cx.observe_self({
            let slots = slots.clone();
            move |workbench, cx| sync(workbench, &slots, cx)
        })
        .detach();
    })
    .detach();
}

/// Brings the providers up to date with the tabs' editors, the root's trust,
/// the setting and each parse. Quiet when nothing changed.
fn sync(workbench: &mut Workbench, again: &Slots, cx: &mut Context<Workbench>) {
    let root = workbench.root().map(std::path::Path::to_owned);
    let on = workbench.lsp.code_lens_on.get() && workbench.root_trusted();
    let editors = match (&root, on) {
        (Some(_), true) => workbench.open_editors(cx),
        _ => Vec::new(),
    };
    let Ok(mut slots) = again.try_borrow_mut() else {
        return;
    };
    let open: Vec<EntityId> = editors.iter().map(Entity::entity_id).collect();
    let gone: Vec<EntityId> = (slots.keys().copied())
        .filter(|id| !open.contains(id))
        .collect();
    for id in gone {
        let slot = slots.remove(&id).expect("just listed");
        if let Some(editor) = slot.editor.upgrade() {
            editor.update(cx, |view, cx| {
                view.remove_lens_provider(&slot.provider);
                cx.notify();
            });
        }
    }
    for editor in editors {
        let slot = slots.entry(editor.entity_id()).or_insert_with(|| {
            let shown = Rc::new(RefCell::new(Shown::default()));
            // The editor changed: its parse may be new.
            let observed = cx.observe(&editor, {
                let slots = again.clone();
                move |workbench, _, cx| sync(workbench, &slots, cx)
            });
            Slot {
                editor: editor.downgrade(),
                provider: shown.clone(),
                shown,
                added: false,
                _observed: observed,
            }
        });
        let inside = {
            let view = editor.read(cx);
            let path = view.model().path();
            matches!((path, &root), (Some(path), Some(root)) if path.starts_with(root))
        };
        let wanted = inside && read_rows(&editor, &slot.shown, cx).is_some();
        if wanted != slot.added {
            slot.added = wanted;
            let provider = slot.provider.clone();
            editor.update(cx, |view, cx| {
                if wanted {
                    view.add_lens_provider(provider);
                } else {
                    view.remove_lens_provider(&provider);
                }
                cx.notify();
            });
        }
    }
}

/// Reads `editor`'s parse into `shown` if it is new, and whether the editor
/// is in a language with runnables.
fn read_rows(
    editor: &Entity<EditorView>,
    shown: &Rc<RefCell<Shown>>,
    cx: &mut Context<Workbench>,
) -> Option<()> {
    let view = editor.read(cx);
    let syntax = view.syntax()?;
    let language = syntax.language().name();
    if !TEST_LANGUAGES.contains(&language) {
        return None;
    }
    let snapshot = view.model().snapshot();
    let current =
        syntax.version() == snapshot.version() && !syntax.needs_parse() && !syntax.failed();
    if !current || shown.borrow().version == Some(snapshot.version()) {
        return Some(());
    }
    let text = snapshot.text();
    let rows = (runnables(language, syntax, text).into_iter())
        .filter_map(|runnable| {
            let title = match runnable.kind {
                RunnableKind::Test => "Run Test",
                RunnableKind::Module => "Run Tests",
                RunnableKind::File => return None,
            };
            let row = runnable.rows.start.min(text.row_count().saturating_sub(1));
            Some((snapshot.anchor_before(text.row_start(row)), title))
        })
        .collect();
    *shown.borrow_mut() = Shown {
        version: Some(snapshot.version()),
        rows,
    };
    editor.update(cx, |_, cx| cx.notify());
    Some(())
}
