// SPDX-License-Identifier: LicenseRef-Proprietary

//! The modal layer: a modal shown focused, hidden on its `DismissEvent`
//! with focus going to the fallback, one modal at a time, a modal left for
//! another element hidden, and a picker shown and dismissed through it.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use kiln_shell::{ModalEvent, ModalId, ModalLayer, ModalView, Picker, PickerDelegate};
use kiln_ui::UiTheme;

const ASK: ModalId = ModalId("ask");
const OTHER: ModalId = ModalId("other");

/// A modal that counts its dismissals.
struct Ask {
    focus: FocusHandle,
    dismissed: usize,
}

impl EventEmitter<DismissEvent> for Ask {}

impl Focusable for Ask {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Ask {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl ModalView for Ask {
    fn dismiss(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.dismissed += 1;
        cx.emit(DismissEvent);
    }
}

/// Its own focusable centre, a second focusable element, and the layer.
struct Host {
    layer: Entity<ModalLayer>,
    centre: FocusHandle,
    elsewhere: FocusHandle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .child(div().track_focus(&self.centre).size_full())
            .child(div().track_focus(&self.elsewhere).size_full())
            .child(self.layer.clone())
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    layer: Entity<ModalLayer>,
    events: Rc<RefCell<Vec<ModalEvent>>>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn ask(&mut self) -> Entity<Ask> {
        self.cx.new(|cx| Ask {
            focus: cx.focus_handle(),
            dismissed: 0,
        })
    }

    fn show<M: ModalView>(&mut self, id: ModalId, modal: &Entity<M>) {
        let (layer, modal) = (self.layer.clone(), modal.clone());
        self.cx.update(|window, cx| {
            layer.update(cx, |layer, cx| layer.show_modal(id, modal, window, cx))
        });
        self.cx.run_until_parked();
    }

    fn modal(&mut self) -> Option<ModalId> {
        self.layer.read_with(self.cx, |layer, _| layer.modal())
    }

    fn focused(&mut self, focus: &FocusHandle) -> bool {
        self.cx.update(|window, _| focus.is_focused(window))
    }

    fn handles(&mut self) -> (FocusHandle, FocusHandle) {
        self.host.read_with(self.cx, |host, _| {
            (host.centre.clone(), host.elsewhere.clone())
        })
    }
}

/// A host whose layer falls back to its centre, focused.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let centre = cx.focus_handle();
        let fallback = centre.clone();
        let layer = cx.new(|_| {
            let mut layer = ModalLayer::new();
            layer.set_focus_fallback(Rc::new(move |_, _| Some(fallback.clone())));
            layer
        });
        cx.subscribe(&layer, move |_, _, event, _| sink.borrow_mut().push(*event))
            .detach();
        Host {
            layer,
            centre,
            elsewhere: cx.focus_handle(),
        }
    });
    let (layer, centre) = host.read_with(cx, |host, _| (host.layer.clone(), host.centre.clone()));
    cx.update(|window, cx| window.focus(&centre, cx));
    cx.run_until_parked();
    Opened {
        host,
        layer,
        events,
        cx,
    }
}

#[gpui::test]
fn a_modal_shows_focused_and_hides_on_dismissal(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (centre, _) = s.handles();
    let ask = s.ask();
    s.show(ASK, &ask);
    assert_eq!(s.modal(), Some(ASK));
    let focus = ask.read_with(s.cx, |ask, _| ask.focus.clone());
    assert!(s.focused(&focus));
    let shown = s.layer.read_with(s.cx, |layer, _| {
        layer.modal_view().map(|view| view.entity_id())
    });
    assert_eq!(shown, Some(ask.entity_id()));

    // Dismissed by itself, as Escape would: hidden, and focus falls back.
    ask.update(s.cx, |_, cx| cx.emit(DismissEvent));
    s.cx.run_until_parked();
    assert_eq!(s.modal(), None);
    assert!(s.focused(&centre));
    assert_eq!(
        *s.events.borrow(),
        [ModalEvent::Shown(ASK), ModalEvent::Dismissed(ASK)]
    );

    // Dismissed by the host: the modal's own dismissal runs.
    s.show(ASK, &ask);
    let layer = s.layer.clone();
    s.cx.update(|window, cx| layer.update(cx, |layer, cx| layer.dismiss_modal(window, cx)));
    s.cx.run_until_parked();
    assert_eq!(s.modal(), None);
    assert_eq!(ask.read_with(s.cx, |ask, _| ask.dismissed), 1);
    assert!(s.focused(&centre));
}

#[gpui::test]
fn one_modal_at_a_time(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (first, second) = (s.ask(), s.ask());
    s.show(ASK, &first);
    s.show(OTHER, &second);
    assert_eq!(s.modal(), Some(OTHER));
    assert_eq!(first.read_with(s.cx, |ask, _| ask.dismissed), 1);
    let focus = second.read_with(s.cx, |ask, _| ask.focus.clone());
    assert!(s.focused(&focus), "the new one has focus");
    // The first's dismissal landing doesn't hide the second.
    s.cx.run_until_parked();
    assert_eq!(s.modal(), Some(OTHER));

    // Shown again, the same modal isn't dismissed.
    s.show(OTHER, &second);
    assert_eq!(second.read_with(s.cx, |ask, _| ask.dismissed), 0);
    assert_eq!(s.modal(), Some(OTHER));
}

#[gpui::test]
fn a_modal_left_for_another_element_hides(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (_, elsewhere) = s.handles();
    let draw = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    };
    // Focus changes are heard as an active window draws.
    s.cx.update(|window, _| window.activate_window());
    let ask = s.ask();
    s.show(ASK, &ask);
    draw(s.cx);
    s.cx.update(|window, cx| window.focus(&elsewhere, cx));
    draw(s.cx);
    assert_eq!(s.modal(), None);
    assert!(s.focused(&elsewhere), "focus stays where it went");
    // Focus had left it, so it needn't be dismissed.
    assert_eq!(ask.read_with(s.cx, |ask, _| ask.dismissed), 0);

    // The window deactivating: focus stays on the modal, which is
    // dismissed, so focus can't stay on a modal no longer shown.
    s.show(ASK, &ask);
    draw(s.cx);
    s.cx.deactivate_window();
    draw(s.cx);
    assert_eq!(s.modal(), None);
    assert_eq!(ask.read_with(s.cx, |ask, _| ask.dismissed), 1);
}

/// Picks one of three words.
struct Words {
    dismissed: Rc<RefCell<usize>>,
}

impl PickerDelegate for Words {
    fn update_matches(&mut self, _: String, _: &mut Context<Picker<Self>>) {}

    fn match_count(&self) -> usize {
        3
    }

    fn match_id(&self, ix: usize) -> SharedString {
        ix.to_string().into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        div().child(ix.to_string()).into_any_element()
    }

    fn confirm(&mut self, _: usize, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {
        *self.dismissed.borrow_mut() += 1;
    }
}

#[gpui::test]
fn a_picker_is_a_modal(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (centre, _) = s.handles();
    let dismissed = Rc::new(RefCell::new(0));
    let delegate = Words {
        dismissed: dismissed.clone(),
    };
    let picker =
        s.cx.new(|cx| Picker::new(delegate, "Pick…", UiTheme::default_dark(), cx));
    let picker_shown = picker.clone();
    s.cx.update(|window, cx| picker_shown.update(cx, |picker, cx| picker.show(window, cx)));
    s.show(ASK, &picker);
    assert_eq!(s.modal(), Some(ASK));

    // A pick dismisses it.
    s.cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(1, window, cx)));
    s.cx.run_until_parked();
    assert_eq!(s.modal(), None);
    assert!(s.focused(&centre));

    // The host's dismissal is the picker's, as Escape's is.
    s.show(ASK, &picker);
    let layer = s.layer.clone();
    s.cx.update(|window, cx| layer.update(cx, |layer, cx| layer.dismiss_modal(window, cx)));
    s.cx.run_until_parked();
    assert_eq!(*dismissed.borrow(), 1);
    assert_eq!(s.modal(), None);
}
