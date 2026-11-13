// SPDX-License-Identifier: Apache-2.0

//! Basic controls: [`Button`], [`Checkbox`] and [`Switch`], and what the
//! controls share: the control radius, [`DISABLED_OPACITY`] and the
//! [`focus_ring`]. Each is a GPUI element built like a `div`: it takes
//! GPUI's styling and interaction methods (`.w_full()`, `.id()`, `.on_mouse_down(..)`, `.when(..)`), with
//! the caller's styles applied over the control's own, and its handlers
//! take GPUI's arguments, so `cx.listener(..)` fits them. Values are the
//! caller's: a checkbox shows `checked` and reports a click with
//! `on_change`, leaving the caller to store it. Colours are the active
//! theme's tokens (`cx.theme()`), read when the control is drawn.

use std::rc::Rc;

use gpui::{
    Action, AnyElement, App, AppContext as _, Bounds, ClickEvent, CursorStyle, Div, Element,
    ElementId, FocusHandle, GlobalElementId, Hsla, InspectorElementId, InteractiveElement,
    Interactivity, IntoElement, LayoutId, MouseButton, ParentElement as _, Pixels, Refineable as _,
    RenderOnce, Role, SharedString, Stateful, StatefulInteractiveElement, StyleRefinement, Styled,
    Window, accesskit, div, prelude::FluentBuilder as _, px, svg,
};

use kiln_command::Source;
use kiln_l10n::Text;
use kiln_platform::action_dispatch::dispatch_action;

use crate::{
    ActiveTheme as _, FOCUS_RING, Icon, States, Tokens, Tooltip, height, node_with, radius,
    ui_theme,
};

type ChangeHandler<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;
type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A disabled control's opacity: half of its enabled look, as macOS draws
/// one, rather than a look of its own.
pub const DISABLED_OPACITY: f32 = 0.5;

/// A toggle's on-state fill: the accent at this alpha, a tint rather than
/// a solid fill.
pub const TOGGLE_ON_ALPHA: u8 = 0x26;

/// `color` at `alpha` (0–255), whatever its own alpha.
pub(crate) fn with_alpha(color: Hsla, alpha: u8) -> Hsla {
    Hsla {
        a: f32::from(alpha) / 255.,
        ..color
    }
}

/// The focus ring around a control `radius`-rounded with a `border`-wide
/// border: [`FOCUS_RING`]'s 2 px of `accent.base`, 1 px out from its outer
/// edge, following its corners. It's positioned absolutely, so add it as a
/// child of the control, which must be `relative()` and must not clip its
/// children (an absolute child is placed from inside the border, which is
/// why the ring needs its width).
pub fn focus_ring(radius: Pixels, border: Pixels, c: &Tokens<Hsla>) -> Div {
    let out = FOCUS_RING.width + FOCUS_RING.offset;
    div()
        .absolute()
        .top(-(out + border))
        .left(-(out + border))
        .right(-(out + border))
        .bottom(-(out + border))
        .rounded(radius + out)
        .border(FOCUS_RING.width)
        .border_color(focus_ring_color(c))
}

/// The focus ring's colour: the accent.
pub(crate) fn focus_ring_color(c: &Tokens<Hsla>) -> Hsla {
    c.accent.base
}

/// A tooltip naming a control, with its key after the name if it has one.
pub(crate) fn name_tooltip(
    name: SharedString,
    shortcut: Option<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| {
        let tooltip = Tooltip::new(name.clone(), shortcut.clone(), &ui_theme(cx));
        cx.new(|_| tooltip).into()
    }
}

/// A control's element: its `div`, and whether the control is disabled,
/// which its accessibility node reports (GPUI's `div` has no setter for
/// AccessKit's disabled flag, so this element adds it). Everything else is
/// the `div`'s: role, label, toggled state, layout and painting.
pub(crate) struct Control {
    element: Stateful<Div>,
    disabled: bool,
}

impl Control {
    pub(crate) fn new(element: Stateful<Div>, disabled: bool) -> Self {
        Self { element, disabled }
    }

    /// Its accessibility node, as GPUI would build it once a screen reader
    /// is listening: the role, then what the element writes.
    #[cfg(test)]
    pub(crate) fn a11y_node(&self) -> Option<accesskit::Node> {
        let mut node = accesskit::Node::new(Element::a11y_role(self)?);
        Element::write_a11y_info(self, &mut node);
        Some(node)
    }
}

impl IntoElement for Control {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Control {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.element)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Element::source_location(&self.element)
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        Element::a11y_role(&self.element)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        Element::write_a11y_info(&self.element, node);
        if self.disabled {
            node.set_disabled();
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.element, prepaint, builder);
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        Element::request_layout(&mut self.element, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        Element::prepaint(
            &mut self.element,
            id,
            inspector_id,
            bounds,
            state,
            window,
            cx,
        )
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        Element::paint(
            &mut self.element,
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

/// A control's keyboard focus: the focus handle it tracks, in the tab
/// order, and whether it draws the focus ring now. An enabled control
/// takes `given`, or else a handle of its own kept with its element (`id`)
/// across frames; a disabled one takes none, so the tab order skips it.
/// The ring shows while it has focus and the last input was the keyboard,
/// as macOS shows focus: tabbing to a button rings it, clicking it doesn't.
/// A focused control runs its click handler on Enter or Space (GPUI's
/// keyboard click).
pub(crate) fn key_focus(
    id: Option<ElementId>,
    given: Option<FocusHandle>,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> (Option<FocusHandle>, bool) {
    if disabled {
        return (None, false);
    }
    let handle = given.or_else(|| {
        let key = ElementId::from((id?, "focus"));
        let state = window.use_keyed_state(key, cx, |_, cx| cx.focus_handle());
        Some(state.read(cx).clone())
    });
    let Some(handle) = handle.map(|handle| handle.tab_stop(true)) else {
        return (None, false);
    };
    let ring = handle.is_focused(window) && window.last_input_was_keyboard();
    (Some(handle), ring)
}

/// `base` tracking `focus`, with the focus ring (for a `radius`-rounded
/// control with a `border`-wide border) while `ring`. A click doesn't move
/// focus to it, as a click on a macOS button doesn't: focus stays where it
/// was (in the editor, say), and the keyboard is what moves focus to it.
pub(crate) fn with_key_focus(
    base: Stateful<Div>,
    (focus, ring): (Option<FocusHandle>, bool),
    radius: Pixels,
    border: Pixels,
    c: &Tokens<Hsla>,
) -> Stateful<Div> {
    base.when_some(focus, |base, focus| {
        // Runs before GPUI's own focus-on-mouse-down for this element,
        // which a prevented default skips.
        base.track_focus(&focus)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
    })
    .when(ring, |base| {
        base.child(focus_ring(radius, border, c).debug_selector(|| "focus-ring".into()))
    })
}

/// Asserts, in a debug build, that an icon-only control has an accessible
/// name: a screen reader would have nothing to read for it.
pub(crate) fn assert_named(name: Option<&SharedString>, what: &str) {
    debug_assert!(
        name.is_some_and(|name| !name.trim().is_empty()),
        "an icon-only button needs a name: {what}(..).name(..)"
    );
}

/// A control's `base` with its `role` and `states`, and its `name` if it
/// has one ([`node_with`]). Without one, the role and states alone, so a
/// label the caller gave through `aria_label` stands: GPUI keeps it where
/// the control can't read it.
fn named(
    base: Stateful<Div>,
    window: &Window,
    cx: &mut App,
    role: Role,
    name: Option<SharedString>,
    states: States,
) -> Stateful<Div> {
    match name {
        Some(name) => node_with(base, window, cx, role, name, states),
        None => base
            .role(role)
            .when_some(states.toggled, |base, on| base.aria_toggled(on.into())),
    }
}

/// Applies `own` (the control's look) to `base`, then the styles the
/// caller set on it, so the caller's win.
pub(crate) fn styled(
    mut base: Stateful<Div>,
    own: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let caller = std::mem::take(base.style());
    let mut base = own(base);
    base.style().refine(&caller);
    base
}

/// A button's look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The main action: filled with the accent colour, its text in
    /// `text.on_accent`.
    Primary,
    /// Outlined in `border.control`, on whatever is behind it.
    #[default]
    Secondary,
    /// No background until hovered: for toolbars.
    Ghost,
    /// A destructive action: filled with the error colour.
    Danger,
}

/// A button's size: its height and its text's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    /// 22 px.
    Small,
    /// 26 px, the controls' height.
    #[default]
    Medium,
    /// 32 px.
    Large,
}

impl ButtonSize {
    pub fn height(self) -> Pixels {
        match self {
            ButtonSize::Small => height::CONTROL_SMALL,
            ButtonSize::Medium => height::CONTROL,
            ButtonSize::Large => px(32.),
        }
    }

    pub(crate) fn text(self) -> Pixels {
        match self {
            ButtonSize::Small => px(11.),
            ButtonSize::Medium => px(12.),
            ButtonSize::Large => px(13.),
        }
    }
}

/// How a button is drawn: its colours, corner radius and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonLook {
    pub background: Hsla,
    pub text: Hsla,
    pub border: Hsla,
    /// The background while hovered; a disabled button has no hover.
    pub hover: Hsla,
    /// The background while pressed.
    pub pressed: Hsla,
    pub radius: Pixels,
    /// 1, or [`DISABLED_OPACITY`] for a disabled button.
    pub opacity: f32,
}

/// A button with a label, an icon, or both.
///
/// Its label is its accessible name. A button with only an icon must be
/// given one with [`Button::name`]: drawing one without is a debug
/// assertion. An icon-only button shows its name, and its key if
/// [`Button::shortcut`] gave one, in a tooltip; a labelled button shows a
/// tooltip only for a key.
///
/// ```ignore
/// Button::new("save", "Save")
///     .primary()
///     .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
///     .w_full()
/// ```
#[derive(IntoElement)]
pub struct Button {
    base: Stateful<Div>,
    label: Option<SharedString>,
    name: Option<SharedString>,
    shortcut: Option<SharedString>,
    icon: Option<Icon>,
    after_icon: Option<AnyElement>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    focus: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// A button labelled `label`: a catalogue message (`text!`), or
    /// verbatim text. A button is built at each render, so this resolves it
    /// in the locale active then.
    ///
    /// ```
    /// use kiln_l10n::{Text, text};
    /// use kiln_ui::Button;
    ///
    /// let _ = Button::new("save", text!("kiln-ui-cancel"));
    /// let _ = Button::new("open", Text::verbatim("Cargo.toml"));
    /// ```
    ///
    /// A string isn't a label, however it is stored, so it can't skip the
    /// catalogue (`docs/decisions/xte5e7hqsw-i18n-design.md` § 8, layer 1):
    ///
    /// ```compile_fail,E0277
    /// let _ = kiln_ui::Button::new("save", "Save");
    /// ```
    ///
    /// ```compile_fail,E0277
    /// let _ = kiln_ui::Button::new("save", gpui::SharedString::from("Save"));
    /// ```
    pub fn new(id: impl Into<ElementId>, label: impl Into<Text>) -> Self {
        Self::bare(id).label(label.into().resolve())
    }

    /// A button showing only `icon`. Give it a [`Button::name`].
    pub fn icon_only(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self::bare(id).icon(icon)
    }

    fn bare(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            label: None,
            name: None,
            shortcut: None,
            icon: None,
            after_icon: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            disabled: false,
            focus: None,
            on_click: None,
        }
    }

    /// An element placed after the icon in the button's children, and
    /// before the label: with the row reversed, between the label and the
    /// icon. It never shrinks.
    pub fn after_icon(mut self, element: impl IntoElement) -> Self {
        self.after_icon = Some(element.into_any_element());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Its accessible name, read by a screen reader and shown in an
    /// icon-only button's tooltip: the label unless this sets one.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The key that runs it, as a label such as `⌘S`: shown in its tooltip
    /// and told to a screen reader.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Its accessible name: [`Button::name`]'s, else its label.
    pub fn accessible_name(&self) -> Option<&SharedString> {
        self.name.as_ref().or(self.label.as_ref())
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Small)
    }

    pub fn large(self) -> Self {
        self.size(ButtonSize::Large)
    }

    /// A disabled button is drawn at half its enabled look's opacity, has
    /// no hover, takes no focus and runs nothing, and a screen reader hears
    /// that it's unavailable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The focus handle it tracks, for a caller that moves focus to it;
    /// without one it keeps its own. Either way it's in the tab order,
    /// Enter or Space clicks it while focused, and it shows the focus ring
    /// while focused from the keyboard.
    pub fn focus_handle(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Clicking dispatches `action` from the focused element, as a key
    /// bound to it would: a typed command goes as an invocation from the
    /// mouse, so observers hear of it (see
    /// [`kiln_platform::action_dispatch`]). Replaces any `on_click`.
    pub fn command(self, action: impl Action) -> Self {
        self.on_click(move |_, window, cx| {
            dispatch_action(&action, Source::Mouse, window, cx);
        })
    }
}

impl ButtonVariant {
    /// The variant's look, disabled or not: a disabled button keeps its
    /// colours, at [`DISABLED_OPACITY`].
    pub fn look(self, disabled: bool, c: &Tokens<Hsla>) -> ButtonLook {
        let clear = gpui::transparent_black();
        let (background, text, border, hover, pressed) = match self {
            ButtonVariant::Primary => (
                c.accent.base,
                c.text.on_accent,
                c.accent.base,
                c.accent.hover,
                c.accent.hover,
            ),
            ButtonVariant::Secondary => (
                clear,
                c.text.primary,
                c.border.control,
                c.state.hover,
                c.state.pressed,
            ),
            ButtonVariant::Ghost => (clear, c.text.primary, clear, c.state.hover, c.state.pressed),
            ButtonVariant::Danger => {
                let hover = c.status.error.opacity(0.85);
                (
                    c.status.error,
                    c.text.on_accent,
                    c.status.error,
                    hover,
                    hover,
                )
            }
        };
        ButtonLook {
            background,
            text,
            border,
            hover,
            pressed,
            radius: radius::CONTROL,
            opacity: if disabled { DISABLED_OPACITY } else { 1. },
        }
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window, cx)
    }
}

impl Button {
    /// The element it renders as.
    pub(crate) fn element(mut self, window: &mut Window, cx: &mut App) -> Control {
        let name = self.accessible_name().cloned();
        assert_named(name.as_ref(), "Button::icon_only");
        let c = cx.theme().colors;
        let look = self.variant.look(self.disabled, &c);
        let (size, disabled) = (self.size, self.disabled);
        let icon_only = self.label.is_none();
        let tooltip = name
            .clone()
            .filter(|_| icon_only || self.shortcut.is_some())
            .map(|name| name_tooltip(name, self.shortcut.clone()));
        let id = self.base.interactivity().element_id.clone();
        let focus = key_focus(id, self.focus.take(), disabled, window, cx);
        let base = styled(self.base, |base| {
            base.relative()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .gap_1()
                .h(size.height())
                .when(icon_only, |base| base.w(size.height()))
                .when(!icon_only, |base| base.px_3())
                .rounded(look.radius)
                .border_1()
                .border_color(look.border)
                .bg(look.background)
                .text_color(look.text)
                .text_size(size.text())
                .opacity(look.opacity)
                .when(!disabled, |base| {
                    base.cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(look.hover))
                        .active(move |style| style.bg(look.pressed))
                })
        })
        .when_some(self.shortcut, |base, key| base.aria_keyshortcuts(key))
        .when_some(tooltip, |base, tooltip| base.tooltip(tooltip))
        .children(self.icon.map(|icon| {
            svg()
                .path(icon.path())
                .size(px(14.))
                .flex_none()
                .text_color(look.text)
        }))
        .children(
            self.after_icon
                .map(|element| div().flex_none().child(element)),
        )
        // A button the caller lets shrink ends its label in an ellipsis;
        // its icon keeps its size.
        .children(
            self.label
                .map(|label| div().min_w_0().truncate().child(label)),
        )
        .when_some(self.on_click.filter(|_| !disabled), |base, on_click| {
            base.on_click(move |event, window, cx| on_click(event, window, cx))
        });
        let base = with_key_focus(base, focus, look.radius, px(1.), &c);
        Control::new(
            named(base, window, cx, Role::Button, name, States::default()),
            disabled,
        )
    }
}

/// A box that's checked or not, with an optional label; clicking either
/// reports the new value to `on_change`. Its label is its accessible name,
/// or [`Checkbox::name`]'s for one without.
#[derive(IntoElement)]
pub struct Checkbox {
    base: Stateful<Div>,
    checked: bool,
    label: Option<SharedString>,
    name: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler<bool>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            checked: false,
            label: None,
            name: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Its accessible name, for a screen reader: the label unless this
    /// sets one.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// A disabled checkbox is drawn at half its enabled look's opacity and
    /// reports nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the value a click asks for: the opposite of `checked`.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Checkbox {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Checkbox {}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window, cx)
    }
}

impl Checkbox {
    /// The element it renders as.
    pub(crate) fn element(self, window: &mut Window, cx: &mut App) -> Control {
        let c = cx.theme().colors;
        let checked = self.checked;
        let box_ = div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(14.))
            .rounded(radius::CONTROL)
            .border_1()
            .border_color(if checked {
                c.accent.base
            } else {
                c.border.control
            })
            .when(checked, |box_| {
                box_.bg(c.accent.base).child(
                    svg()
                        .path(Icon::Check.path())
                        .size(px(10.))
                        .text_color(c.text.on_accent),
                )
            });
        ToggleRow {
            base: self.base,
            role: Role::CheckBox,
            label: self.label,
            name: self.name,
            value: checked,
            disabled: self.disabled,
            on_change: self.on_change,
        }
        .render(box_, &c, window, cx)
    }
}

/// A sliding switch, on or off, with an optional label; clicking either
/// reports the new value to `on_change`. Its label is its accessible name,
/// or [`Switch::name`]'s for one without.
#[derive(IntoElement)]
pub struct Switch {
    base: Stateful<Div>,
    on: bool,
    label: Option<SharedString>,
    name: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler<bool>>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            on: false,
            label: None,
            name: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Its accessible name, for a screen reader: the label unless this
    /// sets one.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// A disabled switch is drawn at half its enabled look's opacity and
    /// reports nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the value a click asks for: the opposite of `on`.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Switch {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Switch {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Switch {}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window, cx)
    }
}

impl Switch {
    /// The element it renders as.
    pub(crate) fn element(self, window: &mut Window, cx: &mut App) -> Control {
        let c = cx.theme().colors;
        let on = self.on;
        let (track, thumb) = if on {
            (c.accent.base, c.text.on_accent)
        } else {
            (c.border.control, c.text.muted)
        };
        let track = div()
            .flex()
            .items_center()
            .w(px(28.))
            .h(px(16.))
            .p(px(2.))
            .rounded_full()
            .bg(track)
            .when(on, |track| track.justify_end())
            .child(div().size(px(12.)).rounded_full().bg(thumb));
        ToggleRow {
            base: self.base,
            role: Role::Switch,
            label: self.label,
            name: self.name,
            value: on,
            disabled: self.disabled,
            on_change: self.on_change,
        }
        .render(track, &c, window, cx)
    }
}

/// A checkbox's or switch's row: the control, then its label, the whole
/// row clickable.
struct ToggleRow {
    base: Stateful<Div>,
    role: Role,
    label: Option<SharedString>,
    name: Option<SharedString>,
    value: bool,
    disabled: bool,
    on_change: Option<ChangeHandler<bool>>,
}

impl ToggleRow {
    fn render(
        mut self,
        control: impl IntoElement,
        c: &Tokens<Hsla>,
        window: &mut Window,
        cx: &mut App,
    ) -> Control {
        let (value, disabled) = (self.value, self.disabled);
        let name = self.name.or_else(|| self.label.clone());
        let id = self.base.interactivity().element_id.clone();
        let focus = key_focus(id, None, disabled, window, cx);
        let base = styled(self.base, |base| {
            base.relative()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .rounded(radius::CONTROL)
                .text_color(c.text.primary)
                .when(disabled, |base| base.opacity(DISABLED_OPACITY))
                .when(!disabled, |base| base.cursor(CursorStyle::PointingHand))
        })
        .child(control)
        .children(self.label)
        .when_some(self.on_change.filter(|_| !disabled), |base, on_change| {
            base.on_click(move |_, window, cx| on_change(&!value, window, cx))
        });
        let base = with_key_focus(base, focus, radius::CONTROL, px(0.), c);
        let states = States {
            toggled: Some(value),
            ..States::default()
        };
        Control::new(named(base, window, cx, self.role, name, states), disabled)
    }
}
