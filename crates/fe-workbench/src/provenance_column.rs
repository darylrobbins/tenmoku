//! Edit provenance in the gutter: `editor.toggle_provenance` shows who wrote
//! each row of the active editor in the annotation column left of the line
//! numbers that git blame's annotations use ([`fe_gpui::GutterAnnotations`]).
//!
//! - **Per editor.** On for the active editor only; another tab or split on
//!   its buffer isn't annotated. It goes when the editor's tab closes.
//! - **Entries.** One per run of rows from
//!   [`Provenance::row_authors`](fe_buffer::Provenance::row_authors): `AI ·
//!   <agent>`, `You`, the tool's name (`Format`, `LSP`) or the host's. Rows
//!   with no author are blank. AI rows have heat 1.0, so they are the tinted
//!   ones; the others none.
//! - **Following edits.** Made afresh from the buffer's provenance
//!   [`EDIT_REST`] after the last edit; rows edited since are blank until then.
//! - **Pointer.** Resting on an entry says who wrote it; clicking does nothing.
//! - **One column.** Turning it on turns the editor's blame annotations off,
//!   and turning those on turns it off; neither remembers the other's state.
//!
//! No default keybinding.

use std::{collections::HashMap, rc::Rc, time::Duration};

use fe_buffer::{Author, Version};
use fe_gpui::{EditorView, GutterAnnotation, GutterAnnotations, HoverContent};
use gpui::{App, Context, Entity, EntityId, Global, Subscription, Task, WeakEntity};

use crate::workbench::Workbench;

/// How long after the last edit the column is made afresh: the same rest as
/// blame's (`git_blame::EDIT_REST`, which this module can't name).
const EDIT_REST: Duration = Duration::from_millis(300);

/// An annotated editor.
struct Column {
    editor: WeakEntity<EditorView>,
    /// The version the annotations shown are for.
    shown: Version,
    /// Whether it was one of the window's file editors when last looked at.
    listed: bool,
    /// The rebuild waiting out an edit's rest, for the version asked for.
    resting: Option<(Version, Task<()>)>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
struct State {
    columns: HashMap<EntityId, Column>,
    /// Takes an editor's blame annotations away: set by the blame column once
    /// it has had any.
    blame_off: Option<fn(EntityId, &mut App)>,
}

impl Global for State {}

fn state(cx: &mut App) -> &mut State {
    cx.default_global::<State>()
}

/// Lets turning provenance on take editors' blame annotations away with `off`.
pub(crate) fn yield_to_blame(off: fn(EntityId, &mut App), cx: &mut App) {
    state(cx).blame_off = Some(off);
}

/// Takes editor `id`'s provenance annotations away, if it has them.
pub(crate) fn off(id: EntityId, cx: &mut App) {
    if let Some(column) = state(cx).columns.remove(&id)
        && let Some(editor) = column.editor.upgrade()
    {
        editor.update(cx, |editor, cx| editor.set_gutter_annotations(None, cx));
    }
}

impl Workbench {
    /// `editor.toggle_provenance`: shows who wrote each row of the active
    /// editor, or takes that away.
    pub(super) fn toggle_provenance(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let id = editor.entity_id();
        state(cx)
            .columns
            .retain(|_, column| column.editor.upgrade().is_some());
        if state(cx).columns.contains_key(&id) {
            return off(id, cx);
        }
        if let Some(blame_off) = state(cx).blame_off {
            blame_off(id, cx);
        }
        let subscriptions = vec![
            cx.observe(&editor, move |_, _, cx| follow(id, cx)),
            cx.observe_release(&editor, move |_, _, cx| {
                state(cx).columns.remove(&id);
            }),
            cx.observe_self(move |workbench, cx| look(workbench, id, cx)),
        ];
        let listed = self.file_editors(cx).contains(&editor);
        let column = Column {
            editor: editor.downgrade(),
            shown: editor.read(cx).model().buffer().version(),
            listed,
            resting: None,
            _subscriptions: subscriptions,
        };
        state(cx).columns.insert(id, column);
        show(id, &editor, cx);
    }
}

/// As the window changes: drops editor `id`'s column once its tab has closed.
fn look(workbench: &mut Workbench, id: EntityId, cx: &mut Context<Workbench>) {
    let Some(editor) = (state(cx).columns.get(&id)).and_then(|column| column.editor.upgrade())
    else {
        return;
    };
    let listed = workbench.file_editors(cx).contains(&editor);
    let Some(column) = state(cx).columns.get_mut(&id) else {
        return;
    };
    if column.listed && !listed {
        return off(id, cx);
    }
    column.listed |= listed;
}

/// As editor `id` changes: rebuilds its column once its edits have rested.
fn follow(id: EntityId, cx: &mut Context<Workbench>) {
    let Some(editor) = (state(cx).columns.get(&id)).and_then(|column| column.editor.upgrade())
    else {
        return;
    };
    let version = editor.read(cx).model().buffer().version();
    let Some(column) = state(cx).columns.get_mut(&id) else {
        return;
    };
    let pending = (column.resting.as_ref()).is_some_and(|(asked, _)| *asked == version);
    if column.shown == version || pending {
        return;
    }
    let task = cx.spawn(async move |workbench, cx| {
        cx.background_executor().timer(EDIT_REST).await;
        workbench
            .update(cx, |_, cx| {
                if let Some(column) = state(cx).columns.get_mut(&id) {
                    column.resting = None;
                }
                if let Some(editor) = (state(cx).columns.get(&id)).and_then(|c| c.editor.upgrade())
                {
                    show(id, &editor, cx);
                }
            })
            .ok();
    });
    if let Some(column) = state(cx).columns.get_mut(&id) {
        column.resting = Some((version, task));
    }
}

/// Puts editor `id`'s provenance in its annotation column, as it is now.
fn show(id: EntityId, editor: &Entity<EditorView>, cx: &mut App) {
    let annotations = annotations(editor, cx);
    if let Some(column) = state(cx).columns.get_mut(&id) {
        column.shown = annotations.version;
    }
    editor.update(cx, |editor, cx| {
        editor.set_gutter_annotations(Some(annotations), cx)
    });
}

/// An entry's text for `author`.
fn label(author: &Author) -> String {
    match author {
        Author::User => "You".to_owned(),
        Author::Ai { agent } => format!("AI · {agent}"),
        Author::Tool("lsp") => "LSP".to_owned(),
        Author::Tool("format") => "Format".to_owned(),
        Author::Tool(name) => (*name).to_owned(),
        Author::Host(name) => name.to_string(),
    }
}

/// What resting on an entry of `author` says.
fn written_by(author: &Author) -> String {
    match author {
        Author::User => "Written by you".to_owned(),
        Author::Ai { agent } => format!("Written by {agent}"),
        other => format!("Written by {}", label(other)),
    }
}

/// `editor`'s buffer's authors as annotations at its version now.
fn annotations(editor: &Entity<EditorView>, cx: &App) -> GutterAnnotations {
    let model = editor.read(cx).model();
    let snapshot = model.snapshot();
    let buffer = model.buffer();
    let text = buffer.text();
    let count = text.row_count();
    let start = |row: u32| {
        if row >= count {
            text.len()
        } else {
            text.row_start(row)
        }
    };
    let (mut entries, mut authors) = (Vec::new(), Vec::new());
    for (rows, author) in buffer.provenance().row_authors(text, 0..count) {
        let Some(author) = author else {
            continue;
        };
        entries.push(GutterAnnotation {
            rows: snapshot.anchor_before(start(rows.start))
                ..snapshot.anchor_before(start(rows.end)),
            text: label(&author).into(),
            heat: matches!(author, Author::Ai { .. }).then_some(1.0),
        });
        authors.push(author);
    }
    GutterAnnotations {
        version: snapshot.version(),
        entries,
        hover: Rc::new(move |ix, _| {
            let author = authors.get(ix)?;
            Some(HoverContent {
                text: written_by(author),
                range: None,
            })
        }),
        on_click: Rc::new(|_, _, _, _| {}),
    }
}
