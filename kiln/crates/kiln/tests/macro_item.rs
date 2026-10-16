// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::item]`: a struct implementing `kiln::Item` and `kiln::View`
//! is a pane item with no other wiring. `open` shows the item of a state,
//! its tab if one has an equal state, else a new one; its title is
//! `Item::title`'s, following a model it watches on the next draw; it
//! restores from a saved layout through its typed state, its kind
//! `<crate>.<snake name>` unless `kind = ".."`; `active_state` gives the
//! active item's state if it's of the type asked; and its own commands
//! are attached; and its `may_close`, `close_revision` and `split` reach
//! the pane item, defaulting to the dirty state's, `None` for a split.

use gpui::{
    App, Entity, FocusHandle, IntoElement, ParentElement as _, SharedString, TestAppContext,
    Window, div,
};
use kiln::{
    Cx, Item, View,
    command::Value,
    prelude::*,
    serde::{Deserialize, Serialize},
    shell::{CloseAnswer, PaneItem, PaneView as _},
    testing::TestApp,
};

/// What a note's title starts with: a model its title reads.
#[derive(Default)]
struct Prefix(String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
struct NoteState {
    name: String,
}

fn note(name: &str) -> NoteState {
    NoteState { name: name.into() }
}

/// A note, its kind the default: `macro_item.note`.
#[kiln::item]
struct Note {
    name: String,
    prefix: Entity<Prefix>,
    focus: FocusHandle,
}

impl Item for Note {
    type State = NoteState;

    fn new(state: NoteState, _: &mut Window, cx: &mut App) -> Option<Self> {
        Some(Self {
            name: state.name,
            prefix: kiln::model::entity::<Prefix>(cx),
            focus: cx.focus_handle(),
        })
    }

    fn state(&self) -> NoteState {
        note(&self.name)
    }

    fn title(&self, cx: &App) -> SharedString {
        format!("{}{}", self.prefix.read(cx).0, self.name).into()
    }

    fn focus(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl View for Note {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
        // Redraws, and so retitles its tab, as the prefix changes.
        let prefix = cx.watch::<Prefix>().0.clone();
        div().child(format!("{prefix}{}", self.name))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
struct ThreadState {
    thread: u32,
}

/// A conversation, keeping an existing kind; thread 0 doesn't exist.
#[kiln::item(kind = "chat.conversation")]
struct Conversation {
    thread: u32,
    sent: usize,
    focus: FocusHandle,
}

impl Item for Conversation {
    type State = ThreadState;

    fn new(state: ThreadState, _: &mut Window, cx: &mut App) -> Option<Self> {
        (state.thread != 0).then(|| Self {
            thread: state.thread,
            sent: 0,
            focus: cx.focus_handle(),
        })
    }

    fn state(&self) -> ThreadState {
        ThreadState {
            thread: self.thread,
        }
    }

    fn title(&self, _: &App) -> SharedString {
        format!("Thread {}", self.thread).into()
    }

    fn focus(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl View for Conversation {
    fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
        div().child(format!("{} sent", self.sent))
    }
}

#[kiln::command(context = "conversation")]
impl Conversation {
    /// Send Message
    #[command(key = "f6")]
    fn send(&mut self, cx: &mut Cx<Self>) {
        self.sent += 1;
        cx.notify();
    }
}

/// The app, with no item registered by hand.
fn builder() -> kiln::AppBuilder {
    kiln::app("dev.kiln.MacroItem", "Kiln Macro Item").restore_layout(false)
}

/// Opens the item of `state` in `app`'s workspace, then draws.
fn open<T: Item + PaneItem>(app: &mut TestApp, state: T::State) -> Option<Entity<T>> {
    let workspace = app.workspace();
    let opened = app.cx().update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.open::<T>(state, window, cx))
    });
    app.draw();
    opened
}

/// The active pane's tab titles, and the active one's index.
fn tabs(app: &mut TestApp) -> (Vec<String>, Option<usize>) {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let pane = workspace.read(cx).active_pane().read(cx).pane();
        let titles = (pane.items().iter()).map(|item| item.tab_title(cx).to_string());
        (titles.collect(), pane.active())
    })
}

fn active_state<T: Item + PaneItem>(app: &mut TestApp) -> Option<T::State> {
    let workspace = app.workspace();
    app.cx().read(|cx| workspace.read(cx).active_state::<T>(cx))
}

/// The window's title, as the shell shows it.
fn window_title(app: &mut TestApp) -> String {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let shell = workspace.read(cx).shell();
        shell.read(cx).title().to_string()
    })
}

#[gpui::test]
fn open_makes_a_tab_and_an_equal_state_activates_it_instead(cx: &mut TestAppContext) {
    let mut app = TestApp::open(builder(), cx);
    let a = open::<Note>(&mut app, note("a")).expect("a opens");
    assert_eq!(tabs(&mut app), (vec!["a".into()], Some(0)));
    let b = open::<Note>(&mut app, note("b")).expect("b opens");
    assert_eq!(tabs(&mut app), (vec!["a".into(), "b".into()], Some(1)));
    assert_ne!(a, b);
    assert_eq!(app.active_item::<Note>(), Some(b));

    // An equal state: a's tab, activated and focused, none added.
    let again = open::<Note>(&mut app, note("a")).expect("a shows");
    assert_eq!(again, a);
    assert_eq!(tabs(&mut app), (vec!["a".into(), "b".into()], Some(0)));
    assert_eq!(app.active_item::<Note>(), Some(a.clone()));
    let focused = (app.cx()).update(|window, cx| a.read(cx).focus.is_focused(window));
    assert!(focused, "the tab it activates is focused");

    // `new` giving `None` opens nothing.
    assert_eq!(
        open::<Conversation>(&mut app, ThreadState { thread: 0 }),
        None
    );
    assert_eq!(tabs(&mut app), (vec!["a".into(), "b".into()], Some(0)));
}

#[gpui::test]
fn the_title_is_item_titles_and_follows_a_model_change_on_the_next_draw(cx: &mut TestAppContext) {
    let mut app = TestApp::open(builder(), cx);
    open::<Note>(&mut app, note("a"));
    assert_eq!(tabs(&mut app).0, ["a"]);
    assert_eq!(window_title(&mut app), "a");

    app.cx().update(|_, cx| {
        kiln::model::update(cx, |prefix: &mut Prefix, cx| {
            prefix.0 = "Draft: ".into();
            cx.notify();
        })
    });
    app.draw();
    assert_eq!(tabs(&mut app).0, ["Draft: a"]);
    assert_eq!(
        window_title(&mut app),
        "Draft: a",
        "the item redrew, and the window was retitled"
    );
}

/// Each saved item's kind in `entry`, as `Workspace::entry` saved it.
fn saved_kinds(entry: &Value) -> Vec<String> {
    fn walk(value: &Value, kinds: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let (Some(Value::String(kind)), true) =
                    (map.get("kind"), map.contains_key("state"))
                {
                    kinds.push(kind.clone());
                }
                map.values().for_each(|value| walk(value, kinds));
            }
            Value::Array(values) => values.iter().for_each(|value| walk(value, kinds)),
            _ => {}
        }
    }
    let mut kinds = Vec::new();
    walk(&entry["panes"], &mut kinds);
    kinds
}

#[gpui::test]
fn an_item_restores_from_a_saved_layout_through_its_state_with_no_register_call(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    assert_eq!(<Note as PaneItem>::KIND, "macro_item.note");
    assert_eq!(<Conversation as PaneItem>::KIND, "chat.conversation");

    let mut app = TestApp::open(builder(), first);
    open::<Note>(&mut app, note("a"));
    open::<Conversation>(&mut app, ThreadState { thread: 7 });
    open::<Note>(&mut app, note("b"));
    let workspace = app.workspace();
    let saved = app.cx().read(|cx| workspace.read(cx).entry(cx));
    assert_eq!(
        saved_kinds(&saved),
        ["macro_item.note", "chat.conversation", "macro_item.note"]
    );

    let restore = saved.clone();
    let mut app = TestApp::open(
        builder().workspace(move |workspace, window, cx| {
            assert!(workspace.restore(&restore, window, cx), "it has panes");
        }),
        second,
    );
    assert_eq!(
        tabs(&mut app),
        (vec!["a".into(), "Thread 7".into(), "b".into()], Some(2))
    );
    assert_eq!(active_state::<Note>(&mut app), Some(note("b")));

    // Restored items are found again by state.
    open::<Conversation>(&mut app, ThreadState { thread: 7 });
    assert_eq!(tabs(&mut app).1, Some(1));
}

#[gpui::test]
fn active_state_is_the_active_items_if_its_a_t_and_its_commands_are_attached(
    cx: &mut TestAppContext,
) {
    let mut app = TestApp::open(builder(), cx);
    assert_eq!(active_state::<Note>(&mut app), None, "no item");

    open::<Note>(&mut app, note("a"));
    assert_eq!(active_state::<Note>(&mut app), Some(note("a")));
    assert_eq!(active_state::<Conversation>(&mut app), None);

    let thread = ThreadState { thread: 3 };
    let conversation = open::<Conversation>(&mut app, thread.clone()).expect("it opens");
    assert_eq!(active_state::<Conversation>(&mut app), Some(thread));
    assert_eq!(active_state::<Note>(&mut app), None);

    // Its `#[kiln::command(context = "conversation")]` impl holds in it.
    app.press("f6");
    assert_eq!(app.cx().read(|cx| conversation.read(cx).sent), 1);

    // And not in a note.
    open::<Note>(&mut app, note("a"));
    app.press("f6");
    assert_eq!(app.cx().read(|cx| conversation.read(cx).sent), 1);
}

/// A draft: dirty, yet it closes unasked; its revision is its length; and
/// it splits into a copy.
#[kiln::item]
struct Draft {
    text: String,
    focus: FocusHandle,
}

impl Item for Draft {
    type State = String;

    fn new(text: String, _: &mut Window, cx: &mut App) -> Option<Self> {
        Some(Self {
            text,
            focus: cx.focus_handle(),
        })
    }

    fn state(&self) -> String {
        self.text.clone()
    }

    fn title(&self, _: &App) -> SharedString {
        "Draft".into()
    }

    fn focus(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }

    fn is_dirty(&self, _: &App) -> bool {
        true
    }

    fn may_close(&mut self, _: &mut Window, _: &mut App) -> CloseAnswer {
        CloseAnswer::Close
    }

    fn close_revision(&self, _: &App) -> u64 {
        self.text.len() as u64
    }

    fn split(&mut self, _: &mut Window, cx: &mut App) -> Option<Self> {
        Some(Self {
            text: self.text.clone(),
            focus: cx.focus_handle(),
        })
    }
}

impl View for Draft {
    fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
        div().child(self.text.clone())
    }
}

#[gpui::test]
fn the_close_and_split_hooks_reach_the_pane_item_and_default_to_the_old_behaviour(
    cx: &mut TestAppContext,
) {
    let mut app = TestApp::open(builder(), cx);
    let note = open::<Note>(&mut app, note("a")).expect("it opens");
    let draft = open::<Draft>(&mut app, "abc".into()).expect("it opens");
    let (answer, revision, split) = app.cx().update(|window, cx| {
        note.update(cx, |note, cx| {
            let answer = PaneItem::may_close(note, window, cx);
            (
                answer,
                PaneItem::close_revision(note, cx),
                PaneItem::split(note, window, cx),
            )
        })
    });
    assert!(matches!(answer, CloseAnswer::Close), "a clean item closes");
    assert_eq!(revision, 0);
    assert_eq!(split, None, "no split by default");

    let (answer, revision, split) = app.cx().update(|window, cx| {
        draft.update(cx, |draft, cx| {
            let answer = PaneItem::may_close(draft, window, cx);
            (
                answer,
                PaneItem::close_revision(draft, cx),
                PaneItem::split(draft, window, cx),
            )
        })
    });
    assert!(matches!(answer, CloseAnswer::Close), "its own answer");
    assert_eq!(revision, 3);
    let split = split.expect("a copy");
    assert_ne!(split, draft);
    assert_eq!(
        split.read_with(app.cx(), |split, _| split.text.clone()),
        "abc"
    );
}
