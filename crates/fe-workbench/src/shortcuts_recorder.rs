//! The Keyboard Shortcuts tab's recorder, as VS Code's Define Keybinding
//! box: a box in a line's Keybinding cell that takes the keys pressed, to
//! bind the line's command to them or to move its binding there.
//!
//! - Double-click on a line, Enter on the selected one or its pencil button
//!   opens it to change the line's binding; a line without one, its Add
//!   button, or Add Keybinding in a line's menu opens it to add a binding.
//! - While its box has focus it holds every keystroke, the platform
//!   modifier's included ([`HOLDS_ALL_KEYS`]), so `cmd-w` is recorded, not
//!   run. Each is converted as kiln's keymap converts it
//!   ([`keys::keystroke`]); a modifier alone records nothing. A keystroke
//!   within [`CHORD_WINDOW`] of the last extends the keys to a chord of
//!   two; a later one, or one after two, starts them again.
//! - The keys show as menus label them ([`fe_keymap::shortcut_label`]),
//!   with the other commands already bound to them where the binding would
//!   apply, from the tab's conflict marks on the keymap with the binding
//!   in it ([`ShortcutIssue::Conflict`]). That never blocks saving.
//! - A new binding's When is a second field, in the When cell, that starts
//!   as the command's own context ([`fe_keymap::CommandInfo::context`]); a
//!   predicate that doesn't parse shows why under it, and Enter does
//!   nothing until it's fixed. A changed binding keeps its When.
//! - Enter saves, as [`KeymapEdit::Add`] or [`KeymapEdit::Change`] through
//!   the tab's queue of writes to the user's `keymap.toml`; Escape, a click
//!   on another line or a line's menu closes it, writing nothing, as does
//!   focus leaving its box and When field (its line scrolled out of the
//!   list, say).

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use fe_command::{CommandId, ContextPredicate, Value};
use fe_gpui::commands::RunCommand;
use fe_keymap::{
    Entry, KeySequence, Keymap, KeymapEdit, KeymapLayer, Keystroke, Location, ShortcutIssue,
};
use fe_ui::{ActiveTheme as _, TextInput, TextInputEvent};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyContext, KeyDownEvent, MouseButton,
    ParentElement as _, ScrollStrategy, SharedString, Styled as _, Subscription, Window, anchored,
    deferred, div, point, px,
};
use kiln::{
    core::Os,
    platform::keys::{self, HOLDS_ALL_KEYS},
};
use kiln_l10n::{Text, text, tr, verbatim};

use super::{Identity, LINE_HEIGHT, ShortcutsEditor, build_model, input_command};

/// The key context the recorder's box sets, with [`HOLDS_ALL_KEYS`].
pub(crate) const KEY_CONTEXT: &str = "shortcuts_recorder";

/// How soon after a keystroke the next one extends the keys to a chord.
pub(crate) const CHORD_WINDOW: Duration = Duration::from_millis(1500);

/// The most keystrokes a recorded chord has.
const CHORD_LEN: usize = 2;

/// What the box says before a key is pressed.
pub(crate) fn prompt() -> SharedString {
    tr!("workbench-setup-shortcuts-recorder-prompt")
}

/// Where the binding being recorded is, in the keymap a conflict preview
/// is reported on: no file has it.
const PREVIEW_FILE: &str = "<recording>";

/// The binding a recorder changes.
pub(crate) struct Changing {
    pub(crate) keys: KeySequence,
    /// Its context, as its predicate prints; `None` is everywhere.
    pub(crate) when: Option<String>,
    pub(crate) location: Location,
    /// Its args, which a change keeps.
    pub(crate) args: Value,
}

/// A line's open recorder: see the module docs.
pub(crate) struct Recorder {
    /// The command the binding runs.
    pub(crate) command: String,
    /// The binding it changes; `None` adds one.
    pub(crate) changing: Option<Changing>,
    /// The line it's open on, to find it again among new lines.
    pub(crate) anchor: Identity,
    keys: Vec<Keystroke>,
    /// When the last of `keys` was pressed.
    last: Option<Instant>,
    focus: FocusHandle,
    /// A new binding's When.
    when: Option<Entity<TextInput>>,
    /// Why the When doesn't parse.
    when_error: Option<String>,
    /// The other commands bound to the keys where the binding would apply.
    conflict: Option<Text>,
    /// Its When's changes, and its box's and When's blurs.
    _subscriptions: Vec<Subscription>,
}

impl Recorder {
    /// The keys recorded, if any.
    pub(crate) fn keys(&self) -> Option<KeySequence> {
        let written: Vec<String> = self.keys.iter().map(ToString::to_string).collect();
        KeySequence::parse_for(&written.join(" "), Os::current()).ok()
    }

    /// The keys as menus label them, or `""` before any.
    pub(crate) fn label(&self) -> String {
        self.keys()
            .map_or_else(String::new, |keys| fe_keymap::shortcut_label(&keys))
    }

    /// What the conflict preview says, if the keys clash.
    #[cfg(test)]
    pub(crate) fn conflict(&self) -> Option<SharedString> {
        self.conflict.as_ref().map(Text::resolve)
    }

    /// Why the When doesn't parse, if it doesn't.
    #[cfg(test)]
    pub(crate) fn when_error(&self) -> Option<&str> {
        self.when_error.as_deref()
    }

    /// A new binding's When field.
    #[cfg(test)]
    pub(crate) fn when_input(&self) -> Option<&Entity<TextInput>> {
        self.when.as_ref()
    }

    /// The box's focus.
    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus
    }

    /// Records `keystroke`, pressed at `now`: see the module docs. Whether
    /// it was recorded.
    fn record(&mut self, keystroke: &gpui::Keystroke, now: Instant) -> bool {
        let modifier = matches!(
            keystroke.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function"
        );
        if modifier {
            return false;
        }
        let Some(keystroke) = keys::keystroke(keystroke) else {
            return false;
        };
        let extends = self.keys.len() < CHORD_LEN
            && (self.last).is_some_and(|last| now.saturating_duration_since(last) <= CHORD_WINDOW);
        if !extends {
            self.keys.clear();
        }
        self.keys.push(keystroke);
        self.last = Some(now);
        true
    }

    /// The context the binding would have: a changed one's, or the When
    /// field's; `Err` with why if the field's doesn't parse.
    fn context(&self, cx: &App) -> Result<Option<String>, String> {
        let Some(when) = &self.when else {
            return Ok(self
                .changing
                .as_ref()
                .and_then(|changing| changing.when.clone()));
        };
        let text = when.read(cx).text().trim();
        if text.is_empty() {
            return Ok(None);
        }
        match ContextPredicate::parse(text) {
            Ok(_) => Ok(Some(text.to_owned())),
            Err(error) => Err(error.to_string()),
        }
    }

    /// The edit Enter makes: `Err` if it can't save yet, with no keys or a
    /// When that doesn't parse; `None` if the keys are the binding's own.
    fn edit(&self, cx: &App) -> Result<Option<KeymapEdit>, ()> {
        let keys = self.keys().ok_or(())?;
        let context = self.context(cx).map_err(|_| ())?;
        Ok(match &self.changing {
            Some(changing) if changing.keys == keys => None,
            Some(changing) => Some(KeymapEdit::Change {
                old_keys: changing.keys.to_string(),
                context: changing.when.clone(),
                new_keys: keys.to_string(),
            }),
            None => Some(KeymapEdit::Add {
                keys: keys.to_string(),
                context,
                command: self.command.clone(),
                args: None,
            }),
        })
    }
}

impl ShortcutsEditor {
    /// Opens the recorder on line `ix`: to change its binding, or with
    /// `add`, or on a line without one, to add a binding of its command. A
    /// binding a later entry hides isn't changed.
    pub(crate) fn start_recording(
        &mut self,
        ix: usize,
        add: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((row, binding)) = self.line(ix) else {
            return;
        };
        let binding = binding.filter(|_| !add);
        if binding.is_some_and(|binding| self.hidden_note(binding).is_some()) {
            return;
        }
        let Some(anchor) = self.identity(ix) else {
            return;
        };
        let command = row.command.clone();
        let changing = binding.map(|binding| Changing {
            keys: binding.keys.clone(),
            when: binding.when.clone(),
            location: binding.location.clone(),
            args: (self.keymap.bindings().iter())
                .find(|ours| ours.keys == binding.keys && ours.location == binding.location)
                .map(|ours| ours.args.clone())
                .unwrap_or_default(),
        });
        let focus = cx.focus_handle();
        let mut subscriptions = vec![cx.on_blur(&focus, window, |this, window, cx| {
            this.recorder_blurred(window, cx)
        })];
        let when = match changing {
            Some(_) => None,
            None => {
                let ui = fe_ui::ui_theme(cx);
                let context = row.context.clone().unwrap_or_default();
                let input = cx.new(|cx| {
                    let mut input =
                        TextInput::new(tr!("workbench-setup-shortcuts-column-when"), &ui, cx)
                            .bordered();
                    input.set_text(context, cx);
                    input
                });
                subscriptions.push(cx.subscribe(&input, |this, _, event, cx| {
                    if *event == TextInputEvent::Changed {
                        this.refresh_recorder(cx);
                        cx.notify();
                    }
                }));
                let input_focus = input.focus_handle(cx);
                subscriptions.push(cx.on_blur(&input_focus, window, |this, window, cx| {
                    this.recorder_blurred(window, cx)
                }));
                Some(input)
            }
        };
        let recorder = Recorder {
            command,
            changing,
            anchor,
            keys: Vec::new(),
            last: None,
            focus,
            when,
            when_error: None,
            conflict: None,
            _subscriptions: subscriptions,
        };
        window.focus(&recorder.focus, cx);
        self.recorder = Some(recorder);
        self.refresh_recorder(cx);
        self.menu = None;
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Closes the recorder, writing nothing, and focuses the list.
    pub(crate) fn cancel_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recorder.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Saves what the recorder has, closing it, unless it can't yet: see
    /// [`Recorder::edit`].
    pub(crate) fn save_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = (self.recorder.as_ref()).and_then(|recorder| recorder.edit(cx).ok())
        else {
            return;
        };
        self.cancel_recording(window, cx);
        if let Some(edit) = edit {
            self.edit(edit, cx);
        }
    }

    /// The open recorder.
    #[cfg(test)]
    pub(crate) fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
    }

    /// Closes the recorder, writing nothing, once neither its box nor its
    /// When field has focus: focus went elsewhere, or its line scrolled
    /// out of the list, taking the box that holds the keys with it.
    fn recorder_blurred(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        if recorder.focus.is_focused(window) || self.when_focused(window, cx) {
            return;
        }
        self.recorder = None;
        cx.notify();
    }

    /// The line the recorder is open on, if it's still listed.
    pub(crate) fn recorder_line(&self) -> Option<usize> {
        let anchor = &self.recorder.as_ref()?.anchor;
        (0..self.lines.len()).position(|ix| self.identity(ix).as_ref() == Some(anchor))
    }

    /// The recorder's When error and conflict preview, for its keys and
    /// context as they are now, on the keymap the rows show.
    pub(crate) fn refresh_recorder(&mut self, cx: &App) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        let context = recorder.context(cx);
        let conflict = match (recorder.keys(), &context) {
            (Some(keys), Ok(context)) => {
                self.preview_conflict(recorder, keys, context.as_deref(), cx)
            }
            _ => None,
        };
        if let Some(recorder) = &mut self.recorder {
            recorder.when_error = context.err();
            recorder.conflict = conflict;
        }
    }

    /// The other commands bound to `keys` where the recorder's binding
    /// would apply in `context`, as the tab would mark it: on the keymap
    /// the rows show, without the binding it changes and with the new one
    /// last, marked as the rows are. Not the longer keys it would start.
    fn preview_conflict(
        &self,
        recorder: &Recorder,
        keys: KeySequence,
        context: Option<&str>,
        cx: &App,
    ) -> Option<Text> {
        let command = CommandId::parse(&recorder.command).ok()?;
        let context = context.map(ContextPredicate::parse).transpose().ok()?;
        let changed = |keys: &KeySequence, location: &Location| {
            (recorder.changing.as_ref())
                .is_some_and(|changing| changing.keys == *keys && changing.location == *location)
        };
        let mut entries: Vec<Entry> = (self.keymap.bindings().iter())
            .filter(|binding| !changed(&binding.keys, &binding.location))
            .map(|binding| Entry {
                keys: binding.keys.clone(),
                context: binding.context.clone(),
                command: Some(binding.command.clone()),
                args: binding.args.clone(),
                location: binding.location.clone(),
            })
            .collect();
        let at = Location {
            file: PathBuf::from(PREVIEW_FILE),
            line: 0,
            column: 0,
        };
        entries.push(Entry {
            keys,
            context,
            command: Some(command),
            args: (recorder.changing.as_ref())
                .map_or_else(Value::default, |changing| changing.args.clone()),
            location: at.clone(),
        });
        let layer = KeymapLayer {
            path: PathBuf::from(PREVIEW_FILE),
            entries,
        };
        let model = build_model(&Keymap::from_layers([layer]), cx);
        let binding = (model.rows().iter())
            .filter(|row| row.command == recorder.command)
            .flat_map(|row| &row.bindings)
            .find(|binding| binding.location == at)?;
        let mut also: Vec<&str> = Vec::new();
        for issue in &binding.issues {
            if let ShortcutIssue::Conflict { command, .. } = issue
                && !also.contains(&command.as_str())
            {
                also.push(command);
            }
        }
        let commands = also.join(verbatim!(", "));
        (!also.is_empty())
            .then(|| text!("workbench-setup-shortcuts-also-bound", commands = commands))
    }

    /// The box's keys: every one is held from the keymap and recorded,
    /// but Enter and Escape alone, which save and cancel.
    fn on_recorder_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let keystroke = &event.keystroke;
        if !keystroke.modifiers.modified() {
            match keystroke.key.as_str() {
                "enter" => return self.save_recording(window, cx),
                "escape" => return self.cancel_recording(window, cx),
                _ => {}
            }
        }
        if event.is_held {
            return;
        }
        let now = cx.background_executor().now();
        let recorded =
            (self.recorder.as_mut()).is_some_and(|recorder| recorder.record(keystroke, now));
        if recorded {
            self.refresh_recorder(cx);
            cx.notify();
        }
    }

    /// Whether the recorder's When field has focus.
    pub(crate) fn when_focused(&self, window: &Window, cx: &App) -> bool {
        (self.recorder.as_ref())
            .and_then(|recorder| recorder.when.as_ref())
            .is_some_and(|when| when.focus_handle(cx).is_focused(window))
    }

    /// The When field's editing commands, as the search's.
    fn on_when_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let Some(when) = (self.recorder.as_ref()).and_then(|recorder| recorder.when.clone()) else {
            return cx.propagate();
        };
        if !input_command(&when, run, cx) {
            cx.propagate();
        }
    }

    /// The recorder's box, for the Keybinding cell, with the conflict
    /// preview under it.
    pub(crate) fn render_recorder_keys(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let recorder = self.recorder.as_ref()?;
        let ui = fe_ui::ui_theme(cx);
        let colors = cx.theme().colors;
        let label = recorder.label();
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        context.add(HOLDS_ALL_KEYS);
        let shown = match label.is_empty() {
            true => div().text_color(ui.muted_text).child(prompt()),
            false => div().text_color(ui.text).child(label),
        };
        let preview = recorder.conflict.as_ref().map(Text::resolve).map(|note| {
            deferred(
                anchored().offset(point(px(0.), LINE_HEIGHT)).child(
                    div()
                        .debug_selector(|| "shortcuts-recorder-conflict".into())
                        .occlude()
                        .px_2()
                        .py_1()
                        .bg(colors.surface.panel)
                        .border_1()
                        .border_color(colors.status.warning)
                        .rounded_sm()
                        .text_color(colors.text.primary)
                        .whitespace_nowrap()
                        .child(note),
                ),
            )
            .with_priority(1)
        });
        Some(
            div()
                .id("shortcuts-recorder")
                .debug_selector(|| "shortcuts-recorder".into())
                .key_context(context)
                .track_focus(&recorder.focus)
                .on_key_down(cx.listener(Self::on_recorder_key))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        if let Some(recorder) = &this.recorder {
                            window.focus(&recorder.focus, cx);
                        }
                    }),
                )
                .flex_1()
                .min_w_0()
                .h(px(20.))
                .px_1()
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .border_1()
                .border_color(colors.border.focused)
                .rounded_sm()
                .child(shown)
                .children(preview)
                .into_any_element(),
        )
    }

    /// The recorder's When field, for a new binding's When cell, with why
    /// it doesn't parse under it.
    pub(crate) fn render_recorder_when(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let recorder = self.recorder.as_ref()?;
        let when = recorder.when.clone()?;
        let colors = cx.theme().colors;
        let error = recorder.when_error.clone().map(|error| {
            deferred(
                anchored().offset(point(px(0.), LINE_HEIGHT)).child(
                    div()
                        .debug_selector(|| "shortcuts-recorder-when-error".into())
                        .occlude()
                        .px_2()
                        .py_1()
                        .bg(colors.surface.panel)
                        .border_1()
                        .border_color(colors.status.error)
                        .rounded_sm()
                        .text_color(colors.status.error)
                        .whitespace_nowrap()
                        .child(error),
                ),
            )
            .with_priority(1)
        });
        Some(
            div()
                .debug_selector(|| "shortcuts-recorder-when".into())
                .on_action(cx.listener(Self::on_when_command))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex_1()
                .min_w_0()
                .child(when)
                .children(error)
                .into_any_element(),
        )
    }
}
