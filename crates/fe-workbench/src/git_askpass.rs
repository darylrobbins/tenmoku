//! Credential prompts for fetch, pull and push (`git_remote`): what git or
//! ssh asks for that no credential helper or ssh-agent key answers, a
//! username, password or key passphrase, asked in a masked field over the
//! window the command started in.
//!
//! The app sets the helper git's prompts run, its own executable in its
//! `--askpass` mode, with [`set_git_askpass`]; without one (tests, headless)
//! [`askpass`] gives none and commands never prompt, failing as before.
//! fe-git's `askpass` module says how a prompt reaches the app, over a unix
//! socket of the command's own, and why there's a script between. Windows
//! isn't covered: fe-git ignores the askpass there.
//!
//! One prompt shows at a time, across windows: others wait their turn,
//! first asked first, each in its own window. Enter sends what was typed to
//! git; Escape, focus going elsewhere in the window, or another modal
//! cancels, and the command fails as it would have without a prompt. The
//! window going inactive (the user fetching a password from another app)
//! hides the prompt, and it shows again, as typed, when the window's
//! active again. A window closed meanwhile cancels its prompts, showing or
//! waiting. A prompt git or ssh no longer waits for (its helper stopped,
//! or the command ended) is dropped, or closes if it shows.
//!
//! What's typed is sent to git and dropped: it's never logged, shown, or
//! written anywhere.

use std::{
    cell::Cell,
    collections::VecDeque,
    ffi::OsString,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use fe_git::{Askpass, AskpassRequest};
use futures::StreamExt as _;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, Styled as _, Subscription, WeakEntity, Window, WindowId, div,
};
use kiln_l10n::tr;
use kiln_shell::ModalId;

use crate::Workbench;

/// The modal layer's id for the prompt.
pub(crate) const GIT_ASKPASS: ModalId = ModalId("git_askpass");

/// The helper git's prompts run, and where each command's socket goes.
#[derive(Clone)]
struct Helper {
    command: Vec<OsString>,
    dir: PathBuf,
}

impl Global for Helper {}

/// Has fetches, pulls and pushes ask the user for credentials git can't
/// find, through `helper` (a program and its arguments, as fe-git's
/// `Askpass::new` takes it: the app's executable and `--askpass`), each
/// command's private directory made in `dir`.
pub fn set_git_askpass(cx: &mut App, helper: Vec<OsString>, dir: PathBuf) {
    cx.set_global(Helper {
        command: helper,
        dir,
    });
}

/// What the app's `--askpass` mode runs: `args` are the socket's path and
/// the prompt, after `--askpass`. Its exit code.
pub fn git_askpass_helper(args: impl IntoIterator<Item = OsString>) -> i32 {
    fe_git::askpass::helper_main(args)
}

/// How often a command waiting on an answer checks it still wants one.
const WITHDRAWN_CHECK: Duration = Duration::from_millis(100);

/// A prompt git's waiting on, and where its answer goes.
struct Asked {
    /// Which, of all asked, for its withdrawal to name.
    id: u64,
    request: AskpassRequest,
    reply: mpsc::Sender<Option<String>>,
}

/// What a command's askpass tells the main thread.
enum Event {
    Asked(Asked),
    /// The prompt asked as this no longer wants an answer.
    Withdrawn(u64),
}

/// Where a command's prompts show: its window and its workbench.
#[derive(Clone)]
struct Origin {
    window: AnyWindowHandle,
    workbench: WeakEntity<Workbench>,
}

/// The prompt showing, or hidden while its window's inactive, and those
/// waiting for it to go, first asked first.
#[derive(Default)]
struct Prompts {
    current: Option<Entity<CredentialPrompt>>,
    waiting: VecDeque<(Origin, Asked)>,
}

impl Global for Prompts {}

/// An askpass for a remote command started in `workbench`'s `window`, its
/// prompts shown there; none without [`set_git_askpass`]. It asks from the
/// command's thread, which waits for the answer.
pub(crate) fn askpass(
    workbench: WeakEntity<Workbench>,
    window: AnyWindowHandle,
    cx: &mut App,
) -> Option<Askpass> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let helper = cx.try_global::<Helper>()?.clone();
    let origin = Origin { window, workbench };
    let (send, mut events) = futures::channel::mpsc::unbounded::<Event>();
    // Its prompts, waiting or showing, go with its window, and any asked
    // after it's gone are cancelled as they come.
    let gone = Rc::new(Cell::new(false));
    let closed = cx.on_window_closed({
        let gone = gone.clone();
        move |cx, closed| {
            if closed == window.window_id() {
                gone.set(true);
                cancel_in(closed, cx);
            }
        }
    });
    // Ends once the command's done with the askpass.
    cx.spawn(async move |cx| {
        let _closed = closed;
        while let Some(event) = events.next().await {
            let origin = origin.clone();
            cx.update(|cx| match event {
                // Dropping it cancels.
                Event::Asked(_) if gone.get() => {}
                Event::Asked(asked) => {
                    (cx.default_global::<Prompts>().waiting).push_back((origin, asked));
                    show_next(cx);
                }
                Event::Withdrawn(id) => withdraw(id, cx),
            });
        }
    })
    .detach();
    Some(Askpass::new(
        helper.command,
        helper.dir,
        move |request, withdrawn| {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let (reply, answer) = mpsc::channel();
            let request = request.clone();
            let asked = Asked { id, request, reply };
            send.unbounded_send(Event::Asked(asked)).ok()?;
            // After it's asked, so the main thread hears it first.
            let events = send.clone();
            withdrawn.on_withdrawn(move || {
                events.unbounded_send(Event::Withdrawn(id)).ok();
            });
            // The reply dropped unanswered (the window or the app gone) is
            // a cancel.
            loop {
                match answer.recv_timeout(WITHDRAWN_CHECK) {
                    Ok(answer) => return answer,
                    Err(mpsc::RecvTimeoutError::Timeout) if !withdrawn.is_withdrawn() => {}
                    Err(_) => return None,
                }
            }
        },
    ))
}

/// The prompt asked as `id` no longer wants an answer: it's dropped if it
/// waits, and closes if it shows.
fn withdraw(id: u64, cx: &mut App) {
    let prompts = cx.default_global::<Prompts>();
    prompts.waiting.retain(|(_, asked)| asked.id != id);
    let current = (prompts.current.clone()).filter(|prompt| prompt.read(cx).id == id);
    if let Some(prompt) = current {
        prompt.update(cx, |prompt, cx| prompt.answer(None, cx));
    }
}

/// Window `closed` is gone: its prompts are cancelled, waiting or showing.
fn cancel_in(closed: WindowId, cx: &mut App) {
    let prompts = cx.default_global::<Prompts>();
    // Dropping the reply cancels.
    (prompts.waiting).retain(|(origin, _)| origin.window.window_id() != closed);
    let current = (prompts.current.clone()).filter(|prompt| prompt.read(cx).window == closed);
    if let Some(prompt) = current {
        prompt.update(cx, |prompt, cx| prompt.answer(None, cx));
    }
}

/// Shows the first prompt waiting, unless one's showing: in its window, or,
/// that window gone, it's cancelled and the next is tried.
fn show_next(cx: &mut App) {
    loop {
        let prompts = cx.default_global::<Prompts>();
        if prompts.current.is_some() {
            return;
        }
        let Some((origin, asked)) = prompts.waiting.pop_front() else {
            return;
        };
        let window = origin.window;
        // Dropping the reply, if the window's gone, cancels.
        let shown = cx.update_window(window, |_, window, cx| {
            let workbench = origin.workbench.upgrade()?;
            let prompt = cx.new(|cx| CredentialPrompt::new(asked, &workbench, window, cx));
            cx.default_global::<Prompts>().current = Some(prompt.clone());
            show(&prompt, window, cx);
            Some(())
        });
        if let Ok(Some(())) = shown {
            return;
        }
    }
}

/// The current prompt is done with: the next waiting shows, once the
/// update that answered it is over.
fn finished(prompt: &Entity<CredentialPrompt>, cx: &mut App) {
    let prompts = cx.default_global::<Prompts>();
    if prompts.current.as_ref() == Some(prompt) {
        prompts.current = None;
    }
    cx.defer(show_next);
}

/// Shows `prompt` in its workbench's modal layer, unless it's showing or
/// answered.
fn show(prompt: &Entity<CredentialPrompt>, window: &mut Window, cx: &mut App) {
    let (pending, workbench) = prompt.read_with(cx, |prompt, _| {
        (prompt.reply.is_some(), prompt.workbench.upgrade())
    });
    let Some(workbench) = workbench.filter(|_| pending) else {
        return;
    };
    let layer = workbench.read(cx).shell().read(cx).modals().clone();
    let shown = layer.read(cx).modal_view().map(|view| view.entity_id());
    if shown != Some(prompt.entity_id()) {
        let prompt = prompt.clone();
        layer.update(cx, |layer, cx| {
            layer.show_modal(GIT_ASKPASS, prompt, window, cx)
        });
    }
}

/// The masked field a prompt is answered in, over the window, naming the
/// remote and saying what git or ssh asked.
pub(crate) struct CredentialPrompt {
    id: u64,
    /// The window it shows in.
    window: WindowId,
    request: AskpassRequest,
    /// Taken once it's answered or cancelled.
    reply: Option<mpsc::Sender<Option<String>>>,
    field: Entity<fe_ui::TextField>,
    workbench: WeakEntity<Workbench>,
    _subscriptions: [Subscription; 3],
}

impl EventEmitter<DismissEvent> for CredentialPrompt {}

impl Focusable for CredentialPrompt {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl kiln_shell::ModalView for CredentialPrompt {
    /// The layer dismisses it as the window goes inactive, focus staying
    /// in the field, and as another modal replaces it, which the field's
    /// blur cancels: nothing's answered here, and it stays to show again.
    fn dismiss(&mut self, _: &mut Window, _: &mut Context<Self>) {}
}

impl CredentialPrompt {
    fn new(
        asked: Asked,
        workbench: &Entity<Workbench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt = asked.request.prompt.trim().to_owned();
        let field = cx.new(|cx| {
            fe_ui::TextField::new(window, cx)
                .placeholder(kiln_l10n::Text::verbatim(prompt.clone()))
                .name(prompt)
                .masked(true)
        });
        let events = cx.subscribe_in(&field, window, |this, field, event, window, cx| {
            match event {
                fe_ui::TextFieldEvent::Submitted => {
                    let answer = field.read(cx).value().to_owned();
                    this.answer(Some(answer), cx);
                }
                // Focus gone elsewhere in the window. Still in the field,
                // the window went inactive, and the prompt stays.
                fe_ui::TextFieldEvent::Blurred
                    if !field.focus_handle(cx).is_focused(window) && this.reply.is_some() =>
                {
                    this.answer(None, cx);
                }
                _ => {}
            }
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.reply.is_some() {
                // The layer reads it as it shows it.
                let prompt = cx.entity();
                window.defer(cx, move |window, cx| show(&prompt, window, cx));
            }
        });
        let closed = cx.observe_release(workbench, |this, _, cx| this.answer(None, cx));
        Self {
            id: asked.id,
            window: window.window_handle().window_id(),
            request: asked.request,
            reply: Some(asked.reply),
            field,
            workbench: workbench.downgrade(),
            _subscriptions: [events, activation, closed],
        }
    }

    /// Sends `answer`, or with `None` cancels, and goes.
    fn answer(&mut self, answer: Option<String>, cx: &mut Context<Self>) {
        let Some(reply) = self.reply.take() else {
            return;
        };
        reply.send(answer).ok();
        // Nothing typed is kept.
        self.field.update(cx, |field, cx| field.set_value("", cx));
        cx.emit(DismissEvent);
        finished(&cx.entity(), cx);
    }

    /// Escape cancels.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.answer(None, cx);
            cx.stop_propagation();
        }
    }
}

impl Render for CredentialPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let remote = self.request.remote.as_str();
        let frame = fe_ui::PaletteFrame::new(&ui)
            .name(tr!("workbench-git-askpass-title", remote = remote))
            .input(div().px_3().py_2().child(self.field.clone()))
            .status(tr!("workbench-git-askpass-status", remote = remote));
        div()
            .on_key_down(cx.listener(Self::on_key_down))
            .absolute()
            .inset_0()
            .child(frame)
    }
}

#[cfg(test)]
impl CredentialPrompt {
    /// What it says: its name, its status, and the field's placeholder.
    pub(crate) fn shown_text(&self, cx: &App) -> [String; 3] {
        let remote = self.request.remote.as_str();
        [
            tr!("workbench-git-askpass-title", remote = remote).to_string(),
            tr!("workbench-git-askpass-status", remote = remote).to_string(),
            self.field.read(cx).placeholder_text().to_string(),
        ]
    }

    pub(crate) fn is_masked(&self, cx: &App) -> bool {
        self.field.read(cx).is_masked()
    }

    /// What's typed in it.
    pub(crate) fn typed(&self, cx: &App) -> String {
        self.field.read(cx).value().to_owned()
    }
}

/// The prompt showing or hidden, if any, and how many wait behind it.
#[cfg(test)]
pub(crate) fn prompts(cx: &mut App) -> (Option<Entity<CredentialPrompt>>, usize) {
    let prompts = cx.default_global::<Prompts>();
    (prompts.current.clone(), prompts.waiting.len())
}
