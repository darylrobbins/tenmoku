//! The mouse in a [`TerminalView`]: selecting, scrolling back, and reporting
//! presses, drags and the wheel to a program that asked for them (xterm's
//! mouse modes). What a report writes is worked out by [`encode`], without a
//! window.

use fe_terminal::keys::{self, Key, Mods};
use fe_terminal::{Modes, SelectionKind, Side};
use gpui::{
    App, Bounds, Context, DispatchPhase, Entity, Hitbox, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, Size, Window,
};

use crate::TerminalView;

/// Lines one notch of the wheel scrolls, and the lines of smooth scrolling
/// that make one wheel report.
pub const WHEEL_LINES: f32 = 3.;

/// Coordinates the X10 encoding can carry: a byte, less its offset of 32.
const X10_MAX: usize = 223;

/// A button, as a program hears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

impl Button {
    /// xterm's button number, before modifiers and motion.
    fn code(self) -> u8 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
            Button::WheelUp => 64,
            Button::WheelDown => 65,
        }
    }

    fn is_wheel(self) -> bool {
        matches!(self, Button::WheelUp | Button::WheelDown)
    }
}

/// What the mouse did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Press(Button),
    Release(Button),
    /// Moved with this button held.
    Drag(Button),
    /// Moved with no button held.
    Move,
}

/// A mouse event at viewport cell (`row`, `col`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Event {
    pub action: Action,
    pub row: usize,
    pub col: usize,
    pub mods: Mods,
}

/// Whether the program asked for any mouse reports.
pub fn reporting(modes: &Modes) -> bool {
    modes.mouse_report_click || modes.mouse_drag || modes.mouse_motion
}

/// The bytes `event` reports in `modes`: `CSI < b;x;y M` (or `m` for a
/// release) with SGR mouse mode on, and the X10 `CSI M bxy` otherwise, with
/// shift, alt and ctrl in the button byte. `None` when the program didn't ask
/// for this kind of event, for a wheel release, or for an X10 coordinate past
/// 223.
pub fn encode(event: Event, modes: &Modes) -> Option<Vec<u8>> {
    let wanted = match event.action {
        Action::Press(_) | Action::Release(_) => reporting(modes),
        Action::Drag(_) => modes.mouse_drag || modes.mouse_motion,
        Action::Move => modes.mouse_motion,
    };
    if !wanted {
        return None;
    }
    let button = match event.action {
        Action::Press(button) => button.code(),
        Action::Release(button) if button.is_wheel() => return None,
        // X10 can't say which button was released.
        Action::Release(button) if modes.sgr_mouse => button.code(),
        Action::Release(_) => 3,
        Action::Drag(button) => button.code() + 32,
        Action::Move => 3 + 32,
    };
    let mods = &event.mods;
    let button =
        button + 4 * u8::from(mods.shift) + 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    let (x, y) = (event.col + 1, event.row + 1);
    if modes.sgr_mouse {
        let end = if matches!(event.action, Action::Release(_)) {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{button};{x};{y}{end}").into_bytes());
    }
    if x > X10_MAX || y > X10_MAX {
        return None;
    }
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + button,
        32 + x as u8,
        32 + y as u8,
    ])
}

fn mods(modifiers: &Modifiers) -> Mods {
    Mods {
        shift: modifiers.shift,
        alt: modifiers.alt,
        ctrl: modifiers.control,
    }
}

fn button(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Right => Some(Button::Right),
        MouseButton::Navigate(_) => None,
    }
}

/// Where the grid was laid out in a frame, and the modes it was drawn in:
/// what that frame's listeners map and encode the mouse by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub bounds: Bounds<Pixels>,
    pub cell: Size<Pixels>,
    pub rows: usize,
    pub cols: usize,
    pub modes: Modes,
}

impl Frame {
    /// The cell under `position`, clamped to the grid, and which half of it.
    fn cell_at(&self, position: Point<Pixels>) -> (usize, usize, Side) {
        let at = position - self.bounds.origin;
        let x = f32::from(at.x) / f32::from(self.cell.width).max(1.);
        let y = f32::from(at.y) / f32::from(self.cell.height).max(1.);
        let row = (y.max(0.) as usize).min(self.rows.saturating_sub(1));
        let (col, side) = if x < 0. {
            (0, Side::Left)
        } else if x >= self.cols as f32 {
            (self.cols.saturating_sub(1), Side::Right)
        } else {
            let side = if x.fract() < 0.5 {
                Side::Left
            } else {
                Side::Right
            };
            (x as usize, side)
        };
        (row, col, side)
    }

    /// Which way a drag at `position` scrolls the scrollback, if it's above
    /// or below the grid: -1 back into history, 1 toward the bottom.
    fn drag_scroll(&self, position: Point<Pixels>) -> i32 {
        if position.y < self.bounds.top() {
            -1
        } else if position.y >= self.bounds.bottom() {
            1
        } else {
            0
        }
    }

    fn reporting(&self) -> bool {
        reporting(&self.modes)
    }
}

/// A selecting drag: the left button is held, and the pointer was last here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Drag(pub Point<Pixels>);

/// Where wheel motion is going, which decides the units it adds up in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WheelTo {
    #[default]
    Scrollback,
    Arrows,
    Report,
}

/// Registers this frame's mouse listeners: presses and the wheel over
/// `hitbox`, and moves and releases anywhere, so a drag continues outside it.
pub(crate) fn listen(
    view: &Entity<TerminalView>,
    hitbox: &Hitbox,
    frame: Frame,
    window: &mut Window,
) {
    {
        let (view, hitbox) = (view.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && hitbox.is_hovered(window)
                && view.update(cx, |view, cx| view.mouse_down(event, &frame, window, cx))
            {
                cx.stop_propagation();
            }
        });
    }
    {
        let (view, hitbox) = (view.clone(), hitbox.clone());
        // Captured, so a control that stops the event can't strand a gesture.
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture {
                let hovered = hitbox.is_hovered(window);
                view.update(cx, |view, cx| view.mouse_move(event, hovered, &frame, cx));
            }
        });
    }
    {
        let view = view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                view.update(cx, |view, cx| view.mouse_up(event, &frame, cx));
            }
        });
    }
    {
        let (view, hitbox) = (view.clone(), hitbox.clone());
        window.on_mouse_event(
            move |event: &ScrollWheelEvent, phase, window, cx: &mut App| {
                if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                    view.update(cx, |view, cx| view.scroll_wheel(event, &frame, cx));
                    cx.stop_propagation();
                }
            },
        );
    }
}

impl TerminalView {
    /// Writes the report of `action` at viewport cell `(row, col)` with
    /// `modifiers`, if the program wants it.
    fn report(
        &mut self,
        action: Action,
        (row, col): (usize, usize),
        modifiers: &Modifiers,
        frame: &Frame,
        cx: &mut Context<Self>,
    ) {
        let event = Event {
            action,
            row,
            col,
            mods: mods(modifiers),
        };
        if let Some(bytes) = encode(event, &frame.modes) {
            self.write_report(bytes, cx);
        }
    }

    /// Writes mouse report `bytes`, leaving the selection and how far back
    /// the view is scrolled as they were: unlike typing, it isn't the user
    /// coming back to the prompt.
    fn write_report(&mut self, bytes: Vec<u8>, _cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        if bytes.is_empty() {
            return;
        }
        let offset = terminal.snapshot().display_offset;
        terminal.input(bytes);
        if offset > 0 {
            terminal.scroll(-(offset as i32));
        }
    }

    /// Releases every button the program heard pressed, except `keep`, at
    /// `(row, col)`.
    fn release_reported(
        &mut self,
        keep: Option<Button>,
        (row, col): (usize, usize),
        modifiers: &Modifiers,
        frame: &Frame,
        cx: &mut Context<Self>,
    ) {
        let held = std::mem::take(&mut self.reported_buttons);
        for button in held {
            if Some(button) == keep {
                self.reported_buttons.push(button);
            } else {
                self.report(Action::Release(button), (row, col), modifiers, frame, cx);
            }
        }
    }

    /// Reports the press, or starts or extends a selection with the left
    /// button. Returns whether it did either.
    fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        frame: &Frame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        window.focus(&self.focus, cx);
        let Some(pressed) = button(event.button) else {
            return false;
        };
        let (row, col, side) = frame.cell_at(event.position);
        // Shift is the user's way past a program's reporting, to select.
        if frame.reporting() && !event.modifiers.shift {
            self.reported_buttons.retain(|&held| held != pressed);
            self.reported_buttons.push(pressed);
            self.reported = Some((row, col));
            self.report(
                Action::Press(pressed),
                (row, col),
                &event.modifiers,
                frame,
                cx,
            );
            return true;
        }
        if pressed != Button::Left {
            return false;
        }
        let Some(terminal) = &self.terminal else {
            return false;
        };
        if event.modifiers.shift && terminal.selected_text().is_some() {
            terminal.extend_selection(row, col, side);
        } else {
            let kind = match event.click_count {
                0 | 1 => SelectionKind::Simple,
                2 => SelectionKind::Word,
                _ => SelectionKind::Line,
            };
            terminal.select(kind, row, col, side);
        }
        self.drag = Some(Drag(event.position));
        cx.notify();
        true
    }

    fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        hovered: bool,
        frame: &Frame,
        cx: &mut Context<Self>,
    ) {
        let (row, col, side) = frame.cell_at(event.position);
        let held = event.pressed_button.and_then(button);
        if held.is_none() {
            // The release happened where no listener saw it: the program
            // still hears it.
            self.drag = None;
            self.release_reported(None, (row, col), &event.modifiers, frame, cx);
        } else if !self.reported_buttons.is_empty() {
            // Only the button still down is.
            self.release_reported(held, (row, col), &event.modifiers, frame, cx);
        }
        if let Some(&reported) = self.reported_buttons.last() {
            if event.modifiers.shift {
                // Shift takes the gesture back from the program, which is
                // told the buttons are up.
                let modifiers = Modifiers {
                    shift: false,
                    ..event.modifiers
                };
                self.release_reported(None, (row, col), &modifiers, frame, cx);
            } else if self.reported != Some((row, col)) {
                self.reported = Some((row, col));
                let drag = held.filter(|b| self.reported_buttons.contains(b));
                let drag = drag.unwrap_or(reported);
                self.report(Action::Drag(drag), (row, col), &event.modifiers, frame, cx);
            }
            return;
        }
        if self.drag.is_some() {
            if let Some(terminal) = &self.terminal {
                terminal.extend_selection(row, col, side);
            }
            self.drag = Some(Drag(event.position));
            cx.notify();
            return;
        }
        if hovered
            && frame.modes.mouse_motion
            && !event.modifiers.shift
            && self.reported != Some((row, col))
        {
            self.reported = Some((row, col));
            self.report(Action::Move, (row, col), &event.modifiers, frame, cx);
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, frame: &Frame, cx: &mut Context<Self>) {
        let released = button(event.button);
        if let Some(released) = released.filter(|b| self.reported_buttons.contains(b)) {
            self.reported_buttons.retain(|&held| held != released);
            let (row, col, _) = frame.cell_at(event.position);
            self.report(
                Action::Release(released),
                (row, col),
                &event.modifiers,
                frame,
                cx,
            );
        }
        if event.button == MouseButton::Left {
            self.drag = None;
        }
    }

    /// Scrolls whole lines of the wheel's motion: pixels by the cell height,
    /// a notch [`WHEEL_LINES`]. A program that asked for reports hears a
    /// wheel press per notch; on the alternate screen with alternate scroll
    /// on, it gets arrow keys; otherwise the scrollback moves.
    fn scroll_wheel(&mut self, event: &ScrollWheelEvent, frame: &Frame, cx: &mut Context<Self>) {
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y * WHEEL_LINES,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(frame.cell.height).max(1.),
        };
        let modes = &frame.modes;
        let report = frame.reporting() && !event.modifiers.shift;
        let to = if report {
            WheelTo::Report
        } else if modes.alt_screen && modes.alternate_scroll && !frame.reporting() {
            WheelTo::Arrows
        } else {
            WheelTo::Scrollback
        };
        // What was left over is in another destination's units, or going the
        // other way: drop it.
        if self.wheel_to != to || self.wheel * lines < 0. {
            self.wheel = 0.;
        }
        self.wheel_to = to;
        self.wheel += lines;
        let unit = if report { WHEEL_LINES } else { 1. };
        let steps = (self.wheel / unit).trunc();
        if steps == 0. {
            return;
        }
        self.wheel -= steps * unit;
        let up = steps > 0.;
        let count = steps.abs() as usize;
        if report {
            let (row, col, _) = frame.cell_at(event.position);
            let pressed = if up {
                Button::WheelUp
            } else {
                Button::WheelDown
            };
            let event = Event {
                action: Action::Press(pressed),
                row,
                col,
                mods: mods(&event.modifiers),
            };
            let bytes = encode(event, modes).unwrap_or_default();
            self.write_report(bytes.repeat(count), cx);
        } else if to == WheelTo::Arrows {
            let key = if up { Key::Up } else { Key::Down };
            let bytes = keys::encode(key, Mods::NONE, modes).unwrap_or_default();
            self.write(bytes.repeat(count), cx);
        } else if let Some(terminal) = &self.terminal {
            // Up the wheel is back into history.
            terminal.scroll(-(steps as i32));
            cx.notify();
        }
    }

    /// While a selecting drag is held above or below the grid, scrolls the
    /// scrollback a line toward it and extends the selection to the edge.
    /// Called once a frame, from the element's prepaint; returns which way it
    /// scrolled, if it did.
    pub(crate) fn drag_scroll(&mut self, frame: &Frame) -> i32 {
        let (Some(Drag(position)), Some(terminal)) = (self.drag, &self.terminal) else {
            return 0;
        };
        let direction = frame.drag_scroll(position);
        if direction != 0 {
            terminal.scroll(direction);
            let (row, col, side) = frame.cell_at(position);
            terminal.extend_selection(row, col, side);
        }
        direction
    }
}
