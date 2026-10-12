//! `ZZ` and `ZQ` (`:help ZZ`), a feature module; see [`crate::feature`].
//!
//! In normal mode `ZZ` runs what `:x` runs and `ZQ` what `:q!` runs, so the
//! host sees the same [`HostCommand`](crate::HostCommand)s. `Z` followed by
//! any other key cancels.

use fe_command::{Actor, ContextPredicate, Registry};

use crate::{
    Pending, Step, VimCommands, VimKey, VimTarget, ex, feature::Feature, reducer::invocation,
};

pub(crate) const FEATURE: Feature = Feature {
    big_z: Some(big_z),
    register,
    ..Feature::NONE
};

fn big_z(_: &Pending, key: VimKey) -> Option<Step> {
    match key {
        VimKey::Char('Z') => Some(invocation("vim.write_quit", ())),
        VimKey::Char('Q') => Some(invocation("vim.quit_discard", ())),
        _ => None,
    }
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    for (id, name, line, description) in [
        (
            "vim.write_quit",
            "Vim Write Quit",
            "x",
            "Runs `:x`, as ZZ does: the host saves a modified buffer and closes the window.",
        ),
        (
            "vim.quit_discard",
            "Vim Quit Discard",
            "q!",
            "Runs `:q!`, as ZQ does: the host closes the window, discarding changes.",
        ),
    ] {
        registry
            .register(
                id,
                name,
                move |target: &mut VimTarget<'_>, actor: &Actor| ex::run(target, actor, line),
            )
            .expect("vim command IDs are valid and distinct")
            .description(description)
            .context(normal.clone())
            .mutates_buffer(false);
    }
}
