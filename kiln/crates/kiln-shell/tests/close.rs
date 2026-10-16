// SPDX-License-Identifier: LicenseRef-Proprietary

//! Close vetoes: an item that refuses, or asks and is told no, keeps a
//! tab, a pane, a window (by command and by its close button) and a quit
//! from closing anything; an item that agrees lets them go ahead. A close
//! waiting on an answer revalidates before it goes ahead: an item that
//! changed after it agreed, or came while the close waited, is asked
//! again; a second close waits for the first, or joins it if it's the
//! same close.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, PromptButton, PromptLevel, Render,
    SharedString, Styled as _, TestAppContext, VisualTestContext, Window, WindowHandle, div,
};
use kiln_command::Invocation;
use kiln_platform::commands;
use kiln_shell::{
    CloseAnswer, CloseKind, CloseRequest, Closing, Direction, ItemHandle, ItemPane, PaneGroup,
    PaneItem, PaneView, Shell,
    close::{self, Items, LiveItems},
    pane::Peers,
};

/// A pane item that can refuse to close: while `busy` (as a chat streaming
/// a reply is), or `dirty`, it asks with a prompt if `asks`, else stays.
struct Veto {
    title: SharedString,
    /// Shared by the items over one document.
    document: Rc<()>,
    dirty: bool,
    busy: bool,
    asks: bool,
    /// Edits, which change its revision.
    edits: u64,
    focus: FocusHandle,
}

impl Render for Veto {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Veto {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Veto {
    const KIND: &'static str = "test.veto";

    fn tab_title(&self, _: &App) -> SharedString {
        self.title.clone()
    }

    fn is_dirty(&self, _: &App) -> bool {
        self.dirty
    }

    fn shares_document(&self, other: &gpui::AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<Veto>()
            .is_ok_and(|other| Rc::ptr_eq(&other.read(cx).document, &self.document))
    }

    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.dirty && !self.busy {
            return CloseAnswer::Close;
        }
        if !self.asks {
            return CloseAnswer::Keep;
        }
        let buttons = [PromptButton::ok("Close"), PromptButton::cancel("Keep")];
        let answer = window.prompt(PromptLevel::Warning, &self.title, None, &buttons, cx);
        CloseAnswer::Ask(cx.spawn(async move |_, _| answer.await == Ok(0)))
    }

    fn close_revision(&self, _: &App) -> u64 {
        u64::from(self.dirty) | u64::from(self.busy) << 1 | self.edits << 2
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (title, document) = (self.title.clone(), self.document.clone());
        Some(cx.new(|cx| Veto {
            title,
            document,
            dirty: self.dirty,
            busy: false,
            asks: true,
            edits: self.edits,
            focus: cx.focus_handle(),
        }))
    }
}

/// A window: a group of panes in a shell, whose items it gives for its
/// close and quitting.
struct Host {
    group: PaneGroup<ItemPane>,
    peers: Peers<ItemPane>,
    shell: Entity<Shell>,
}

impl Host {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pane = cx.new(ItemPane::new);
        let shell = cx.new(|cx| {
            let mut shell = Shell::new("test_app", window, cx);
            shell.set_center(pane.clone(), cx);
            shell
        });
        let host = cx.weak_entity();
        let items = Rc::new(move |cx: &App| {
            (host.upgrade())
                .map(|host| host.read(cx).group.items(cx))
                .unwrap_or_default()
        });
        close::set_window_items(window, items, cx);
        let mut host = Self {
            group: PaneGroup::new(pane),
            peers: Peers::default(),
            shell,
        };
        host.sync_peers(cx);
        host
    }

    fn sync_peers(&mut self, cx: &mut Context<Self>) {
        *self.peers.borrow_mut() = (self.group.panes().into_iter())
            .map(Entity::downgrade)
            .collect();
        for pane in self.group.panes() {
            let peers = self.peers.clone();
            pane.update(cx, |pane, _| pane.pane_mut().set_peers(peers));
        }
    }

    fn pane(&self, ix: usize) -> Entity<ItemPane> {
        self.group.panes()[ix].clone()
    }

    /// A new pane right of the first.
    fn split(&mut self, cx: &mut Context<Self>) -> Entity<ItemPane> {
        let first = self.pane(0);
        let new = first.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
        self.group.split(&first, new.clone(), Direction::Right);
        self.sync_peers(cx);
        new
    }

    /// Closes `pane` once its items agree: a host's own pane close.
    fn close_pane(&mut self, pane: &Entity<ItemPane>, window: &mut Window, cx: &mut Context<Self>) {
        let now = Items {
            closing: pane.read(cx).pane().items().to_vec(),
            open: self.group.items(cx),
        };
        let (weak, host) = (pane.downgrade(), cx.weak_entity());
        let live: LiveItems = Rc::new(move |cx| {
            Some(Items {
                closing: weak.upgrade()?.read(cx).pane().items().to_vec(),
                open: host.upgrade()?.read(cx).group.items(cx),
            })
        });
        let request =
            CloseRequest::new(CloseKind::Pane).scope(window.window_handle(), Some(now), live);
        let (closed, host) = (pane.clone(), cx.weak_entity());
        let commit = move |_: Option<&mut Window>, cx: &mut App| {
            host.update(cx, |host, cx| host.remove_pane(&closed, cx))
                .ok();
        };
        match close::confirm(request, commit, Some(window), cx) {
            Closing::Now => self.remove_pane(pane, cx),
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    }

    fn remove_pane(&mut self, pane: &Entity<ItemPane>, cx: &mut Context<Self>) {
        self.group.remove(pane);
        self.sync_peers(cx);
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.shell.clone())
    }
}

/// The app: its windows, and how many times it quit.
struct Harness {
    cx: TestAppContext,
    quits: Rc<RefCell<usize>>,
}

impl Harness {
    fn new(cx: &mut TestAppContext) -> Self {
        let quits = Rc::new(RefCell::new(0));
        let counted = quits.clone();
        cx.update(|cx| {
            commands::init(cx, "Test");
            close::init(cx);
            close::set_quit(cx, move |_| *counted.borrow_mut() += 1);
        });
        Self {
            cx: cx.clone(),
            quits,
        }
    }

    fn open(&mut self) -> WindowHandle<Host> {
        let window = self.cx.add_window(Host::new);
        self.cx.run_until_parked();
        window
    }

    fn quits(&self) -> usize {
        *self.quits.borrow()
    }

    fn host(&mut self, window: WindowHandle<Host>) -> Entity<Host> {
        window.root(&mut self.cx).unwrap()
    }

    fn pane(&mut self, window: WindowHandle<Host>, ix: usize) -> Entity<ItemPane> {
        let host = self.host(window);
        host.read_with(&self.cx, |host, _| host.pane(ix))
    }

    /// Adds an item titled `title` to pane `ix` of `window`, shown.
    fn add(&mut self, window: WindowHandle<Host>, ix: usize, title: &str) -> Entity<Veto> {
        let pane = self.pane(window, ix);
        let title = SharedString::from(title.to_owned());
        let item = self.cx.new(|cx| Veto {
            title,
            document: Rc::new(()),
            dirty: false,
            busy: false,
            asks: true,
            edits: 0,
            focus: cx.focus_handle(),
        });
        let added = item.clone();
        pane.update(&mut self.cx, |pane, cx| pane.add_item(Box::new(added), cx));
        self.cx.run_until_parked();
        item
    }

    fn set(&mut self, item: &Entity<Veto>, change: impl FnOnce(&mut Veto)) {
        item.update(&mut self.cx, |item, cx| {
            change(item);
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn titles(&mut self, window: WindowHandle<Host>, ix: usize) -> Vec<String> {
        let pane = self.pane(window, ix);
        pane.read_with(&self.cx, |pane, cx| {
            (pane.pane().items().iter())
                .map(|item| item.tab_title(cx).to_string())
                .collect()
        })
    }

    fn panes(&mut self, window: WindowHandle<Host>) -> usize {
        let host = self.host(window);
        host.read_with(&self.cx, |host, _| host.group.panes().len())
    }

    fn is_open(&mut self, window: WindowHandle<Host>) -> bool {
        let window: AnyWindowHandle = window.into();
        self.cx.update(|cx| cx.windows().contains(&window))
    }

    /// The title of the prompt showing, if any.
    fn prompt(&self) -> Option<String> {
        self.cx.pending_prompt().map(|(title, _)| title)
    }

    fn answer(&mut self, button: &str) {
        self.cx.simulate_prompt_answer(button);
        self.cx.run_until_parked();
    }

    fn close_tab(&mut self, window: WindowHandle<Host>, ix: usize) {
        let pane = self.pane(window, ix);
        window
            .update(&mut self.cx, |_, window, cx| {
                pane.update(cx, |pane, cx| pane.close_active_item(window, cx))
            })
            .unwrap();
        self.cx.run_until_parked();
    }

    fn close_pane(&mut self, window: WindowHandle<Host>, ix: usize) {
        let pane = self.pane(window, ix);
        window
            .update(&mut self.cx, |host, window, cx| {
                host.close_pane(&pane, window, cx)
            })
            .unwrap();
        self.cx.run_until_parked();
    }

    /// Runs `command` from the app, with `window` active.
    fn run(&mut self, window: WindowHandle<Host>, command: &str) {
        window
            .update(&mut self.cx, |_, window, _| window.activate_window())
            .unwrap();
        self.cx.run_until_parked();
        let invocation = Invocation::of(command).unwrap();
        self.cx.update(|cx| commands::dispatch_app(&invocation, cx));
        self.cx.run_until_parked();
    }

    /// The platform's close button: whether the window closed at once.
    fn close_button(&mut self, window: WindowHandle<Host>) -> bool {
        let mut cx = VisualTestContext::from_window(window.into(), &self.cx);
        let closed = cx.simulate_close();
        if closed {
            window
                .update(&mut self.cx, |_, window, _| window.remove_window())
                .ok();
        }
        self.cx.run_until_parked();
        closed
    }
}

/// The ways an item answers, and whether the close goes ahead.
#[derive(Clone, Copy, Debug)]
enum Answer {
    /// Clean: closes without asking.
    Close,
    /// Busy, and asks: the user says close.
    AskYes,
    /// Busy, and asks: the user says keep.
    AskNo,
    /// Busy, and won't.
    Keep,
}

impl Answer {
    const ALL: [Answer; 4] = [Answer::Close, Answer::AskYes, Answer::AskNo, Answer::Keep];

    fn closes(self) -> bool {
        matches!(self, Answer::Close | Answer::AskYes)
    }

    fn set(self, app: &mut Harness, item: &Entity<Veto>) {
        app.set(item, |item| {
            item.busy = !matches!(self, Answer::Close);
            item.asks = matches!(self, Answer::AskYes | Answer::AskNo);
        });
    }

    /// Answers the prompt the item showed, if it asks.
    fn answer(self, app: &mut Harness, title: &str) {
        if let Answer::AskYes | Answer::AskNo = self {
            assert_eq!(app.prompt().as_deref(), Some(title), "{self:?}");
            app.answer(if self.closes() { "Close" } else { "Keep" });
        }
    }
}

#[gpui::test]
fn a_tab_closes_as_its_item_answers(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    let window = app.open();
    for answer in Answer::ALL {
        app.add(window, 0, "other");
        let item = app.add(window, 0, "chat");
        answer.set(&mut app, &item);
        app.close_tab(window, 0);
        answer.answer(&mut app, "chat");
        assert_eq!(app.prompt(), None, "{answer:?}");
        let titles = app.titles(window, 0);
        assert_eq!(
            titles.contains(&"chat".to_owned()),
            !answer.closes(),
            "{answer:?}"
        );
        assert!(titles.contains(&"other".to_owned()), "{answer:?}");
        if !answer.closes() {
            Answer::Close.set(&mut app, &item);
            app.close_tab(window, 0);
        }
        app.close_tab(window, 0);
        assert_eq!(app.titles(window, 0), Vec::<String>::new());
    }
}

#[gpui::test]
fn a_pane_closes_as_its_items_answer(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    let window = app.open();
    for answer in Answer::ALL {
        let host = app.host(window);
        host.update(&mut app.cx, |host, cx| host.split(cx));
        app.add(window, 1, "notes");
        let item = app.add(window, 1, "chat");
        answer.set(&mut app, &item);
        app.close_pane(window, 1);
        answer.answer(&mut app, "chat");
        assert_eq!(app.prompt(), None, "{answer:?}");
        assert_eq!(
            app.panes(window),
            if answer.closes() { 1 } else { 2 },
            "{answer:?}"
        );
        if !answer.closes() {
            Answer::Close.set(&mut app, &item);
            app.close_pane(window, 1);
            assert_eq!(app.panes(window), 1);
        }
    }
}

#[gpui::test]
fn a_window_closes_as_its_items_answer(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    for button in [false, true] {
        for answer in Answer::ALL {
            let window = app.open();
            let item = app.add(window, 0, "chat");
            app.add(window, 0, "notes");
            answer.set(&mut app, &item);
            if button {
                let closed = app.close_button(window);
                assert_eq!(closed, matches!(answer, Answer::Close), "{answer:?}");
            } else {
                app.run(window, "window.close");
            }
            answer.answer(&mut app, "chat");
            assert_eq!(app.prompt(), None, "{answer:?}");
            assert_eq!(app.is_open(window), !answer.closes(), "{answer:?} {button}");
            if app.is_open(window) {
                assert_eq!(app.titles(window, 0), ["chat", "notes"]);
                Answer::Close.set(&mut app, &item);
                app.run(window, "window.close");
                assert!(!app.is_open(window));
            }
        }
    }
}

#[gpui::test]
fn a_quit_asks_window_by_window(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    let (a, b) = (app.open(), app.open());
    let first = app.add(a, 0, "first");
    let second = app.add(b, 0, "second");
    for answer in Answer::ALL {
        Answer::AskYes.set(&mut app, &first);
        answer.set(&mut app, &second);
        let quits = app.quits();
        app.run(a, "app.quit");
        Answer::AskYes.answer(&mut app, "first");
        answer.answer(&mut app, "second");
        assert_eq!(app.prompt(), None, "{answer:?}");
        assert_eq!(
            app.quits(),
            quits + usize::from(answer.closes()),
            "{answer:?}"
        );
        // Nothing closes: the second window refusing leaves both open.
        assert!(app.is_open(a) && app.is_open(b));
        assert_eq!(app.titles(a, 0), ["first"]);
        assert_eq!(app.titles(b, 0), ["second"]);
    }

    // The first window refusing doesn't ask the second.
    Answer::AskNo.set(&mut app, &first);
    Answer::AskYes.set(&mut app, &second);
    let quits = app.quits();
    app.run(a, "app.quit");
    Answer::AskNo.answer(&mut app, "first");
    assert_eq!(app.prompt(), None);
    assert_eq!(app.quits(), quits);
}

#[gpui::test]
fn an_item_that_changes_after_agreeing_is_asked_again(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    for change in ["edited", "busy"] {
        let (a, b) = (app.open(), app.open());
        let first = app.add(a, 0, "first");
        let second = app.add(b, 0, "second");
        Answer::AskYes.set(&mut app, &second);
        let quits = app.quits();

        // The first window's item agrees at once, clean; while the second
        // window asks, it changes.
        app.run(a, "app.quit");
        assert_eq!(app.prompt().as_deref(), Some("second"));
        app.set(&first, |item| match change {
            "edited" => {
                item.dirty = true;
                item.edits += 1;
            }
            _ => item.busy = true,
        });
        app.answer("Close");
        assert_eq!(app.prompt().as_deref(), Some("first"), "{change}");
        assert_eq!(app.quits(), quits, "{change}: not before it agrees again");
        app.answer("Close");
        assert_eq!(app.prompt(), None);
        assert_eq!(app.quits(), quits + 1, "{change}");

        // Told no the second time, nothing closes.
        app.set(&first, |item| item.edits += 1);
        app.run(a, "app.quit");
        app.answer("Close");
        app.set(&second, |item| item.edits += 1);
        app.answer("Close");
        assert_eq!(app.prompt().as_deref(), Some("second"));
        app.answer("Keep");
        assert_eq!(app.prompt(), None);
        assert_eq!(app.quits(), quits + 1, "{change}");
        for window in [a, b] {
            window
                .update(&mut app.cx, |_, window, _| window.remove_window())
                .unwrap();
        }
        app.cx.run_until_parked();
    }
}

#[gpui::test]
fn an_item_that_comes_while_a_close_waits_is_asked(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);

    // A window close, and a quit, waiting on another item.
    for quit in [false, true] {
        let (a, b) = (app.open(), app.open());
        let waiting = app.add(if quit { b } else { a }, 0, "waiting");
        Answer::AskYes.set(&mut app, &waiting);
        let quits = app.quits();
        app.run(a, if quit { "app.quit" } else { "window.close" });
        assert_eq!(app.prompt().as_deref(), Some("waiting"));
        let late = app.add(a, 0, "late");
        Answer::AskYes.set(&mut app, &late);
        app.answer("Close");
        assert_eq!(app.prompt().as_deref(), Some("late"), "quit: {quit}");
        assert!(app.is_open(a));
        app.answer("Close");
        assert_eq!(app.prompt(), None);
        if quit {
            assert_eq!(app.quits(), quits + 1);
            a.update(&mut app.cx, |_, window, _| window.remove_window())
                .unwrap();
        }
        assert!(!app.is_open(a) || quit);
        b.update(&mut app.cx, |_, window, _| window.remove_window())
            .unwrap();
        app.cx.run_until_parked();
    }

    // A pane close, waiting on one of its items.
    let window = app.open();
    let host = app.host(window);
    host.update(&mut app.cx, |host, cx| host.split(cx));
    let waiting = app.add(window, 1, "waiting");
    Answer::AskYes.set(&mut app, &waiting);
    app.close_pane(window, 1);
    assert_eq!(app.prompt().as_deref(), Some("waiting"));
    let late = app.add(window, 1, "late");
    Answer::AskNo.set(&mut app, &late);
    app.answer("Close");
    assert_eq!(app.prompt().as_deref(), Some("late"));
    app.answer("Keep");
    assert_eq!(app.panes(window), 2, "the pane stays, with both");
    assert_eq!(app.titles(window, 1), ["waiting", "late"]);
}

#[gpui::test]
fn a_document_shown_elsewhere_isnt_asked_about(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    let window = app.open();
    let item = app.add(window, 0, "notes");
    app.set(&item, |item| item.dirty = true);
    let host = app.host(window);
    let copy = window
        .update(&mut app.cx, |_, window, cx| {
            item.update(cx, |item, cx| item.split(window, cx))
        })
        .unwrap()
        .unwrap();
    let right = host.update(&mut app.cx, |host, cx| host.split(cx));
    right.update(&mut app.cx, |pane, cx| pane.add_item(Box::new(copy), cx));

    // Its copy stays: nothing asked.
    app.close_tab(window, 0);
    assert_eq!(app.prompt(), None);
    assert_eq!(app.titles(window, 0), Vec::<String>::new());

    // The last view of it asks; a window's close asks once for two views.
    app.close_tab(window, 1);
    assert_eq!(app.prompt().as_deref(), Some("notes"));
    app.answer("Keep");
    let again = window
        .update(&mut app.cx, |_, window, cx| {
            let item = app_item(&right, cx);
            item.update(cx, |item, cx| item.split(window, cx))
        })
        .unwrap()
        .unwrap();
    right.update(&mut app.cx, |pane, cx| pane.add_item(Box::new(again), cx));
    app.run(window, "window.close");
    assert_eq!(app.prompt().as_deref(), Some("notes"));
    app.answer("Close");
    assert_eq!(app.prompt(), None);
    assert!(!app.is_open(window));
}

fn app_item(pane: &Entity<ItemPane>, cx: &App) -> Entity<Veto> {
    pane.read(cx).pane().items()[0].downcast::<Veto>().unwrap()
}

#[gpui::test]
fn a_second_close_waits_or_joins(cx: &mut TestAppContext) {
    let mut app = Harness::new(cx);
    let (a, b) = (app.open(), app.open());
    let first = app.add(a, 0, "first");
    let second = app.add(b, 0, "second");
    Answer::AskYes.set(&mut app, &first);
    Answer::AskYes.set(&mut app, &second);

    // The close button twice: one prompt, one close.
    assert!(!app.close_button(a));
    assert!(!app.close_button(a));
    assert_eq!(app.prompt().as_deref(), Some("first"));
    app.answer("Close");
    assert_eq!(app.prompt(), None, "the second click joined the first");
    assert!(!app.is_open(a));

    // A quit during a tab's close waits for it, then asks.
    let a = app.open();
    let first = app.add(a, 0, "first");
    Answer::AskYes.set(&mut app, &first);
    let pane = app.pane(b, 0);
    let tab = b
        .update(&mut app.cx, |_, window, cx| {
            let now = Items {
                closing: vec![Box::new(second.clone()) as Box<dyn ItemHandle>],
                open: pane.read(cx).pane().items().to_vec(),
            };
            let live: LiveItems = Rc::new(|_| None);
            let request =
                CloseRequest::new(CloseKind::Items).scope(window.window_handle(), Some(now), live);
            close::confirm(request, |_, _| {}, Some(window), cx)
        })
        .unwrap();
    let Closing::Later(tab) = tab else {
        panic!("it asks");
    };
    let closed = Rc::new(Cell::new(None));
    let landed = closed.clone();
    app.cx
        .spawn(async move |_| landed.set(Some(tab.await)))
        .detach();
    app.cx.run_until_parked();
    app.run(a, "app.quit");
    assert_eq!(app.prompt().as_deref(), Some("second"), "only the tab's");
    app.answer("Keep");
    assert_eq!(closed.get(), Some(false));
    assert_eq!(app.prompt().as_deref(), Some("first"), "then the quit's");
    app.answer("Close");
    assert_eq!(app.prompt().as_deref(), Some("second"));
    app.answer("Close");
    assert_eq!(app.quits(), 1);
}
