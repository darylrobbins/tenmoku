//! Expand and shrink selection: every selection grows to the syntax node
//! enclosing it, and shrinking steps back through the sets it grew from.

use fe_buffer::{ResolvedSelection, Version};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, EditorHost, add},
};

/// The selections before each expand, and what identifies the state the
/// last one left: any other change of the text or the selections drops it.
#[derive(Clone, Debug)]
pub(crate) struct ExpandHistory {
    version: Version,
    /// The selections the last expand or shrink set.
    current: Vec<ResolvedSelection>,
    /// The selections before each expand, oldest first.
    steps: Vec<Vec<ResolvedSelection>>,
}

impl ExpandHistory {
    /// The history as of `version`, for a buffer whose text came back to
    /// what it was when the history was made.
    pub(crate) fn at_version(self, version: Version) -> Self {
        Self { version, ..self }
    }
}

/// Registers the commands. Neither edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "expand_selection",
        "Expand Selection",
        false,
        |model, host| {
            Ok(if model.expand_selection(host) {
                Effect::Changed
            } else {
                Effect::Nothing
            })
        },
    );
    add(
        registry,
        "shrink_selection",
        "Shrink Selection",
        false,
        |model, _| {
            Ok(if model.shrink_selection() {
                Effect::Changed
            } else {
                Effect::Nothing
            })
        },
    );
}

impl EditorModel {
    /// Grows each selection to the next enclosing node `host` knows of (one
    /// with none stays), merging any that now meet. Returns whether any grew.
    pub fn expand_selection(&mut self, host: &dyn EditorHost) -> bool {
        self.sync_display();
        let mut steps = self.live_expand_steps();
        let before = self.selections();
        let grown: Vec<_> = before
            .iter()
            .map(|selection| ResolvedSelection {
                range: host
                    .larger_syntax_node(selection.range.clone())
                    .filter(|range| {
                        range.start <= selection.range.start && selection.range.end <= range.end
                    })
                    .unwrap_or_else(|| selection.range.clone()),
                reversed: selection.reversed,
            })
            .collect();
        if grown == before {
            self.keep_expand_steps(steps);
            return false;
        }
        self.set_selections(grown);
        steps.push(before);
        self.keep_expand_steps(steps);
        true
    }

    /// Restores the selections before the last expand, if nothing has changed
    /// since. Returns whether it did.
    pub fn shrink_selection(&mut self) -> bool {
        self.sync_display();
        let mut steps = self.live_expand_steps();
        let Some(previous) = steps.pop() else {
            return false;
        };
        self.set_selections(previous);
        self.keep_expand_steps(steps);
        true
    }

    /// The history, if it still holds: the text and the selections are as
    /// the last expand or shrink left them.
    pub(crate) fn live_expand_history(&self) -> Option<ExpandHistory> {
        self.expand_history
            .as_ref()
            .filter(|history| {
                history.version == self.snapshot.version() && history.current == self.selections()
            })
            .cloned()
    }

    /// The history's steps, taken out of the model; empty if the text or the
    /// selections changed since the history was last set.
    fn live_expand_steps(&mut self) -> Vec<Vec<ResolvedSelection>> {
        match self.expand_history.take() {
            Some(history)
                if history.version == self.snapshot.version()
                    && history.current == self.selections() =>
            {
                history.steps
            }
            _ => Vec::new(),
        }
    }

    /// Keeps `steps` as the history of the selections as they now stand.
    fn keep_expand_steps(&mut self, steps: Vec<Vec<ResolvedSelection>>) {
        self.expand_history = (!steps.is_empty()).then(|| ExpandHistory {
            version: self.snapshot.version(),
            current: self.selections(),
            steps,
        });
    }
}
