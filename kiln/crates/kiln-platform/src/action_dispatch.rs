// SPDX-License-Identifier: Apache-2.0

//! Dispatching an action the app already holds, such as a button's, as the
//! command it is: a typed command goes through [`dispatch_invocation`], so
//! observers hear of it and its handler can read [`current_invocation`];
//! anything else is dispatched as GPUI would.
//!
//! [`current_invocation`]: crate::command_actions::current_invocation

use gpui::{Action, App, Window};
use kiln_command::{Invocation, Source, Value};

use crate::command_actions::{command_id, dispatch_invocation, typed_command};

/// Dispatches `action` in `window` from its focused element: as an
/// invocation from `source` if it's a registered typed command that
/// [`invocation_for`] can describe, otherwise as a plain GPUI action.
pub fn dispatch_action(action: &dyn Action, source: Source, window: &mut Window, cx: &mut App) {
    if let Some(invocation) = invocation_for(action, cx)
        && dispatch_invocation(invocation.source(source), window, cx).is_ok()
    {
        return;
    }
    window.dispatch_action(action.boxed_clone(), cx);
}

/// The invocation that dispatches `action`: `Some` for a registered typed
/// command without args, the only kind an invocation rebuilds exactly;
/// `None` for an action with args or one that isn't a command.
pub fn invocation_for(action: &dyn Action, cx: &App) -> Option<Invocation> {
    let id = command_id(action.name())?;
    let built = typed_command(&id, cx)?.build(&Value::Null).ok()?;
    built.partial_eq(action).then(|| Invocation::new(id))
}
