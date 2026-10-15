// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::Cx`: a view's reads of models, settings, its workspace and
//! services. A view that watches a model or the settings redraws when they
//! change, observing each once however often it watches, until it's
//! released; one that reads a model without watching it doesn't.
//!
//! The views read are drawn cached (`AnyView::cached`), as GPUI redraws a
//! window that read an entity when it changes, but not a cached view in it
//! unless the view is notified.

use std::{
    any::type_name,
    cell::Cell,
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StyleRefinement, Styled as _, Subscription, TestAppContext, VisualTestContext, WeakEntity,
    Window, div,
};
use kiln::{
    Config, Cx, GlobalSettings, model,
    platform::config::{BatchSender, batch_channel},
    serde::{Deserialize, Serialize},
    shell::PaneItem,
    testing::TestApp,
    with_cx,
};

/// A model: a count.
#[derive(Default)]
struct Count(usize);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(crate = "kiln::serde", default)]
struct NotesSettings {
    font_size: f64,
}

impl Default for NotesSettings {
    fn default() -> Self {
        Self { font_size: 13.0 }
    }
}

/// How a [`Reader`] reads what it shows.
#[derive(Clone, Copy)]
enum Read {
    /// `cx.watch::<Count>()`.
    Watch,
    /// `cx.model::<Count>()`.
    Model,
    /// `cx.setting::<NotesSettings>()`.
    Setting,
}

/// What a [`Reader`] did: its renders and the value it last read.
#[derive(Clone, Default)]
struct Seen {
    renders: Rc<Cell<usize>>,
    value: Rc<Cell<f64>>,
}

struct Reader {
    read: Read,
    seen: Seen,
}

impl Render for Reader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let read = self.read;
        let seen = self.seen.clone();
        with_cx(window, cx, |cx| {
            seen.renders.set(seen.renders.get() + 1);
            let value = match read {
                Read::Watch => cx.watch::<Count>().0 as f64,
                Read::Model => cx.model::<Count>().0 as f64,
                Read::Setting => cx.setting::<NotesSettings>().font_size,
            };
            seen.value.set(value);
            div().size_full().child(value.to_string())
        })
    }
}

/// The window's root: the reader drawn cached, while there is one.
struct Host {
    reader: Option<Entity<Reader>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let reader = (self.reader.clone())
            .map(|reader| AnyView::from(reader).cached(StyleRefinement::default().size_full()));
        div().size_full().children(reader)
    }
}

/// A window of a host of a reader reading as `read`, drawn, and how many
/// times the reader has been notified.
struct Opened<'a> {
    host: Entity<Host>,
    reader: WeakEntity<Reader>,
    seen: Seen,
    notified: Rc<Cell<usize>>,
    _notifications: Subscription,
    cx: &'a mut VisualTestContext,
}

fn open(read: Read, cx: &mut TestAppContext) -> Opened<'_> {
    let seen = Seen::default();
    let made = seen.clone();
    let (host, cx) = cx.add_window_view(|_, cx| Host {
        reader: Some(cx.new(|_| Reader { read, seen: made })),
    });
    let reader = host.read_with(cx, |host, _| host.reader.clone().unwrap());
    let notified = Rc::new(Cell::new(0));
    let notifications = cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&reader, move |_, _| notified.set(notified.get() + 1))
    });
    draw(cx);
    Opened {
        host,
        reader: reader.downgrade(),
        seen,
        notified,
        _notifications: notifications,
        cx,
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

impl Opened<'_> {
    /// Adds one to the count through a `Cx` of the host's, then draws.
    fn count(&mut self) {
        let host = self.host.clone();
        self.cx.update(|window, cx| {
            host.update(cx, |_, cx| {
                with_cx(window, cx, |cx| {
                    cx.update_model(|count: &mut Count, _| count.0 += 1)
                })
            })
        });
        draw(self.cx);
    }

    /// Redraws the reader for a reason of its own.
    fn notify(&mut self) {
        let reader = self.reader.upgrade().expect("the reader");
        self.cx
            .update(|_, cx| reader.update(cx, |_, cx| cx.notify()));
        draw(self.cx);
    }

    fn renders(&self) -> usize {
        self.seen.renders.get()
    }

    fn value(&self) -> f64 {
        self.seen.value.get()
    }

    fn watchers<T: 'static>(&mut self) -> usize {
        self.cx.update(|_, cx| model::watchers::<T>(cx))
    }
}

#[gpui::test]
fn a_watched_model_redraws_its_view_which_observes_it_once_until_released(cx: &mut TestAppContext) {
    let mut opened = open(Read::Watch, cx);
    assert_eq!((opened.renders(), opened.value()), (1, 0.0));
    assert_eq!(opened.watchers::<Count>(), 1);

    opened.count();
    assert_eq!((opened.renders(), opened.value()), (2, 1.0), "it redraws");
    assert_eq!(opened.notified.get(), 1);

    for _ in 0..10 {
        opened.notify();
    }
    assert_eq!(opened.renders(), 12, "it watched on each of 10 renders");
    assert_eq!(opened.watchers::<Count>(), 1, "and observes once");
    opened.notified.set(0);
    opened.count();
    assert_eq!((opened.renders(), opened.value()), (13, 2.0));
    assert_eq!(opened.notified.get(), 1, "one observer notifies it once");

    let host = opened.host.clone();
    opened.cx.update(|_, cx| {
        host.update(cx, |host, cx| {
            host.reader = None;
            cx.notify();
        })
    });
    draw(opened.cx);
    assert!(opened.reader.upgrade().is_none(), "the reader is released");
    assert_eq!(opened.watchers::<Count>(), 0, "with its observer");
    opened.count();
    assert_eq!(opened.renders(), 13);
    assert_eq!(opened.cx.update(|_, cx| model::read::<Count>(cx).0), 3);
}

#[gpui::test]
fn a_model_read_without_watching_doesnt_redraw_its_view(cx: &mut TestAppContext) {
    let mut opened = open(Read::Model, cx);
    assert_eq!((opened.renders(), opened.value()), (1, 0.0));

    opened.count();
    opened.count();
    assert_eq!(opened.renders(), 1, "the count changing doesn't redraw it");
    assert_eq!(opened.notified.get(), 0);
    assert_eq!(opened.watchers::<Count>(), 0);

    opened.notify();
    assert_eq!(
        (opened.renders(), opened.value()),
        (2, 2.0),
        "redrawn, it reads the count now"
    );
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-cx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Writes `text` to `path` and sends a batch naming it, as the watcher
/// would.
fn save(path: &Path, text: &str, sender: &BatchSender) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
}

#[gpui::test]
fn a_view_reading_the_settings_redraws_after_they_reload(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("settings.toml");
    fs::write(&user, "font_size = 15\n").unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .settings::<NotesSettings>(Some(user.clone()), None)
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);

    let mut opened = open(Read::Setting, cx);
    assert_eq!(opened.value(), 15.0, "the settings loaded");
    assert_eq!(opened.watchers::<GlobalSettings<NotesSettings>>(), 1);

    save(&user, "font_size = 17\n", &sender);
    draw(opened.cx);
    assert_eq!(opened.value(), 17.0, "it redraws after the reload");
    assert_eq!(opened.notified.get(), 1, "notified by its watch");
    assert_eq!(opened.renders(), 2);

    opened.notify();
    assert_eq!(opened.watchers::<GlobalSettings<NotesSettings>>(), 1);
}

#[gpui::test]
fn without_a_store_the_settings_are_the_defaults(cx: &mut TestAppContext) {
    let opened = open(Read::Setting, cx);
    assert_eq!(opened.value(), 13.0);
}

/// A service the app provides.
struct Greeting(&'static str);

/// A service nothing provides.
struct Missing;

/// An item that notes its window's workspace and the greeting.
struct Probe {
    focus: FocusHandle,
    workspace: Rc<Cell<Option<EntityId>>>,
    greeting: Rc<Cell<&'static str>>,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (workspace, greeting) = with_cx(window, cx, |cx| {
            (cx.workspace().entity_id(), cx.service::<Greeting>().0)
        });
        self.workspace.set(Some(workspace));
        self.greeting.set(greeting);
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Probe {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Probe {
    const KIND: &'static str = "cx.probe";

    fn tab_title(&self, _: &App) -> SharedString {
        "Probe".into()
    }
}

/// What `run` panicked with.
fn panic_message(run: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(run)).expect_err("it panics");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => (*payload.downcast::<&str>().unwrap()).to_owned(),
    }
}

#[gpui::test]
fn a_view_in_an_app_window_reaches_its_workspace_and_the_services(cx: &mut TestAppContext) {
    let workspace = Rc::new(Cell::new(None));
    let greeting = Rc::new(Cell::new(""));
    let (noted, greeted) = (workspace.clone(), greeting.clone());
    let made: Rc<Cell<Option<Entity<Probe>>>> = Rc::default();
    let probe = made.clone();
    cx.update(|cx| kiln::provide(cx, Greeting("hello")));
    let builder = kiln::app("dev.kiln.Cx", "Kiln Cx")
        .restore_layout(false)
        .workspace(move |workspace, window, cx| {
            let item = cx.new(|cx| Probe {
                focus: cx.focus_handle(),
                workspace: noted.clone(),
                greeting: greeted.clone(),
            });
            probe.set(Some(item.clone()));
            workspace.add_item(Box::new(item), window, cx);
        });
    let mut app = TestApp::open(builder, cx);
    let expected = app.workspace().entity_id();
    assert_eq!(
        workspace.get(),
        Some(expected),
        "cx.workspace() is the window's"
    );
    assert_eq!(greeting.get(), "hello", "cx.service() is what was provided");

    let probe = made.take().expect("the probe");
    let message = app.cx().update(|window, cx| {
        probe.update(cx, |_, cx| {
            with_cx(window, cx, |cx| {
                panic_message(|| {
                    cx.service::<Missing>();
                })
            })
        })
    });
    assert!(
        message.contains(type_name::<Missing>()),
        "the panic names the type: {message}"
    );
}

/// A view that counts its clicks.
struct Clicked(usize);

impl Render for Clicked {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn cx_is_the_views_context_and_window_and_its_handlers_have_one(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Clicked(0));
    let handle = cx.update(|window, _| window.window_handle());
    let handler = cx.update(|window, cx| {
        view.update(cx, |_, cx| {
            with_cx(window, cx, |cx: &mut Cx<Clicked>| {
                // Context's calls, through Deref and DerefMut.
                assert_eq!(cx.entity_id(), view.entity_id());
                cx.notify();
                // Window's, on its field, and Context's on its.
                assert_eq!(cx.window.window_handle(), handle);
                assert_eq!(cx.cx.entity_id(), view.entity_id());
                cx.handler::<(), _>(move |clicked, cx| {
                    clicked.0 += 1;
                    assert_eq!(cx.window.window_handle(), handle);
                    cx.update_model(|count: &mut Count, _| count.0 += 1);
                })
            })
        })
    });
    cx.update(|window, cx| handler(&(), window, cx));
    cx.update(|window, cx| handler(&(), window, cx));
    assert_eq!(view.read_with(cx, |view, _| view.0), 2);
    assert_eq!(cx.update(|_, cx| model::read::<Count>(cx).0), 2);
}
