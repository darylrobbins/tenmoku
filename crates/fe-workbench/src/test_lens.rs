//! A Run Test lens above each test, as rust-analyzer's in VS Code: a
//! [`LensProvider`] per editor, giving `▷ Run test` over each test function
//! and `▷ Run tests` over each test module or class
//! (`docs/design/design-system.md#editor`; the editor sets lens rows in UI
//! small sans), whose click runs `tests.run_row` ([`crate::test_commands`])
//! for that row, wherever the caret is.
//!
//! The provider is asked each frame for a snapshot but has no tree, so it
//! shows what [`sync`] read off the editor's parse: the rows, anchored to
//! the text, so an edit moves them at once and a new parse corrects them.
//! What it read is of a version and a language: a parse in another language
//! of the same text (the editor's language changed) is read again, and an
//! editor in no language with runnables shows none.
//! The editors that get one are the tabs' showing a file inside a trusted
//! root, in a language with runnables, while `editor.code_lens` is on.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use fe_buffer::{Anchor, BufferSnapshot, Version};
use fe_command::{Invocation, Value};
use fe_gpui::{EditorView, Lens, LensItem, LensProvider};
use gpui::{App, Context, Entity, EntityId, SharedString, Subscription, WeakEntity};
use kiln_l10n::tr;

use crate::{
    Workbench,
    test_runnables::{RunnableKind, TEST_LANGUAGES, runnables},
};

/// What a click on a test's lens runs.
const RUN_ROW: &str = "tests.run_row";

/// One lens row: where it sits, and what it runs.
type Row = (Anchor, RunnableKind);

/// What a provider shows: the rows of the parse at `read`'s version, in its
/// language.
#[derive(Default)]
struct Shown {
    read: Option<(Version, &'static str)>,
    rows: Vec<Row>,
}

/// What the lens over a runnable of `kind` says.
fn title(kind: RunnableKind) -> SharedString {
    match kind {
        RunnableKind::Module => tr!("workbench-lens-run-tests"),
        RunnableKind::Test | RunnableKind::File => tr!("workbench-lens-run-test"),
    }
}

impl LensProvider for Shown {
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens> {
        let text = snapshot.text();
        (self.rows.iter())
            .map(|&(anchor, kind)| {
                let row = text.offset_to_point(snapshot.resolve(&anchor)).row;
                Lens {
                    row,
                    items: vec![LensItem {
                        title: title(kind),
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
/// is in a language with runnables. One that isn't, or has no parse, empties
/// `shown`, so no rows of a language it was in show if it gets one again.
fn read_rows(
    editor: &Entity<EditorView>,
    shown: &Rc<RefCell<Shown>>,
    cx: &mut Context<Workbench>,
) -> Option<()> {
    let view = editor.read(cx);
    let syntax = view.syntax();
    let language = syntax
        .map(|syntax| syntax.language().name())
        .filter(|language| TEST_LANGUAGES.contains(language));
    let (Some(syntax), Some(language)) = (syntax, language) else {
        *shown.borrow_mut() = Shown::default();
        return None;
    };
    let snapshot = view.model().snapshot();
    let current =
        syntax.version() == snapshot.version() && !syntax.needs_parse() && !syntax.failed();
    let read = (snapshot.version(), language);
    if !current || shown.borrow().read == Some(read) {
        return Some(());
    }
    let text = snapshot.text();
    let rows = (runnables(language, syntax, text).into_iter())
        .filter(|runnable| runnable.kind != RunnableKind::File)
        .map(|runnable| {
            let row = runnable.rows.start.min(text.row_count().saturating_sub(1));
            (snapshot.anchor_before(text.row_start(row)), runnable.kind)
        })
        .collect();
    *shown.borrow_mut() = Shown {
        read: Some(read),
        rows,
    };
    editor.update(cx, |_, cx| cx.notify());
    Some(())
}
