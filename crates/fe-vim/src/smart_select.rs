//! `vim.expand_selection` and `vim.shrink_selection`: the editor's structural
//! selection (`EditorModel::expand_selection`, with its own history) driving
//! vim's visual mode. The model's selection is the caret or the visual
//! extent's mirror; what the model sets is adopted as a charwise extent, or
//! normal mode at the caret the first expand started from.

use fe_command::Dispatched;
use fe_command::{Actor, ContextPredicate, Effect, Registry};

use crate::{VimCommands, VimTarget, visual};

pub(crate) fn register(registry: &mut Registry<VimCommands>, context: ContextPredicate) {
    registry
        .register(
            kiln_l10n::verbatim!("vim.expand_selection"),
            &crate::l10n::command_title("vim.expand_selection"),
            |target: &mut VimTarget<'_>, _: &Actor| {
                let VimTarget {
                    model, host, state, ..
                } = target;
                state.sync_visual(model);
                Ok(Dispatched::new(if model.expand_selection(&**host) {
                    visual::adopt(model, state, model.primary_selection());
                    Effect::Changed
                } else {
                    Effect::Nothing
                }))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.expand_selection"))
        .context(context.clone())
        .mutates_buffer(false);
    registry
        .register(
            kiln_l10n::verbatim!("vim.shrink_selection"),
            &crate::l10n::command_title("vim.shrink_selection"),
            |target: &mut VimTarget<'_>, _: &Actor| {
                let VimTarget { model, state, .. } = target;
                state.sync_visual(model);
                Ok(Dispatched::new(if model.shrink_selection() {
                    visual::adopt(model, state, model.primary_selection());
                    Effect::Changed
                } else {
                    Effect::Nothing
                }))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.shrink_selection"))
        .context(context)
        .mutates_buffer(false);
}
