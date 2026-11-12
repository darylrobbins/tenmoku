//! The workbench's commands a plugin may invoke, and what each needs of it
//! beyond `commands`.
//!
//! The workbench's commands are kiln typed commands, which check no
//! plugin's grants, so a plugin reaches none of them directly. This
//! module's [`registry`] has a plugin-facing entry for each one a plugin
//! may run, opened with [`Registry::allow_plugins`]: the bridge
//! (`plugin_bridge.rs`) checks a plugin's invocation against it, the
//! [`CommandHost`](kiln_plugin::commands::CommandHost) as its policy, and
//! only then forwards it to the typed command, as a key would run it.
//! Every other workbench command refuses plugins, as does a plugin's own
//! command ([`is_plugin_command`]).

use fe_command::{
    Actor, Capability, CommandError, CommandId, CommandTarget, ContextPredicate, Dispatched,
    Effect, FsScope, Invocation, PluginAccess, PluginDenied, PluginPolicy, Registry,
};
use gpui::{App, Focusable as _, Window};
use kiln_l10n::tr;

use crate::Workbench;
use serde::Deserialize;

/// Opens a workspace file at a row and column: the typed command's id,
/// which a plugin reaches through this module.
pub(crate) const OPEN_PATH: &str = "workbench.open_path";

/// `workbench.open_path`'s args.
#[derive(Debug, Deserialize, kiln::schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(crate = "kiln::schemars")]
pub(crate) struct OpenPathArgs {
    /// The file, an absolute path: for a plugin, one inside a workspace
    /// root, where it leads once symlinks and `..` are followed.
    pub(crate) path: String,
    /// The 0-based row to put the caret on, clamped to the text; the
    /// caret is left alone without one.
    #[serde(default)]
    pub(crate) row: Option<u32>,
    /// The 0-based column on `row`, in chars, clamped to the row's end; its
    /// start without one.
    #[serde(default)]
    pub(crate) column: Option<u32>,
}

/// The workbench commands open to plugins: their ids, the capabilities
/// they need beyond `commands`, and their path args. Each registers with
/// its `command-<id>` message as its title ([`command_title`]).
const PLUGIN_CALLABLE: [(&str, &[Capability], &[&str]); 7] = [
    (
        OPEN_PATH,
        &[Capability::FsRead(FsScope::Workspace)],
        &["path"],
    ),
    ("nav.back", &[], &[]),
    ("nav.forward", &[], &[]),
    ("diagnostics.next", &[], &[]),
    ("diagnostics.previous", &[], &[]),
    ("project_panel.reveal_active", &[], &[]),
    ("problems.show", &[], &[]),
];

/// The title command `id` registers with: its `command-<id>` message,
/// which `label()` resolves again in the active locale wherever it's shown.
fn command_title(id: &str) -> gpui::SharedString {
    kiln_l10n::message(&fe_command::l10n::command_key(id))
}

/// The [`CommandTarget`] of the plugin-facing entries: the window to run
/// the typed command in.
pub(crate) struct WorkbenchCommands;

impl CommandTarget for WorkbenchCommands {
    type Tx = ();
    type Target<'a> = Forward<'a>;
}

/// A plugin's invocation, authorized, to run as its typed command in
/// `window`.
pub(crate) struct Forward<'a> {
    pub(crate) invocation: Invocation,
    pub(crate) window: &'a mut Window,
    pub(crate) cx: &'a mut App,
}

impl Forward<'_> {
    /// Dispatches the invocation as its typed command, as a key would: from
    /// the window's focus, in its key contexts. With nothing focused, or
    /// focus on an element no longer drawn, from which the dispatch would
    /// reach none of the workbench's handlers, focus moves first to where
    /// the workbench puts it ([`Workbench::focus_handle`]). Fails, running
    /// nothing, if no handler of the command is on the focus's path then.
    ///
    /// [`Workbench::focus_handle`]: gpui::Focusable::focus_handle
    fn run(&mut self) -> Result<Dispatched<()>, CommandError> {
        let command = &self.invocation.command;
        let typed = kiln::platform::command_actions::typed_command(command, self.cx)
            .ok_or_else(|| CommandError::new(tr!("workbench-workbench-plugin-not-a-command")))?;
        let action = typed
            .build(&self.invocation.args)
            .map_err(CommandError::new)?;
        if !self.drawn_focus(&*action) {
            let workbench = self.window.root::<Workbench>().flatten();
            if let Some(workbench) = workbench {
                let focus = workbench.read(self.cx).focus_handle(self.cx);
                self.window.focus(&focus, self.cx);
            }
        }
        let context = kiln::platform::keys::context_stack(&self.window.context_stack(), self.cx);
        let reached = self.drawn_focus(&*action)
            && self.window.is_action_available(&*action, self.cx)
            && ContextPredicate::identifier(crate::KEY_CONTEXT).eval(&context);
        if !reached {
            return Err(CommandError::new(tr!(
                "workbench-workbench-plugin-no-focus"
            )));
        }
        let invocation = self.invocation.clone().context(context);
        kiln::dispatch_invocation(invocation, self.window, self.cx)
            .map_err(|error| CommandError::new(error.to_string()))?;
        Ok(Dispatched::new(Effect::Nothing))
    }

    /// Whether the window's focus is on an element drawn in its last frame,
    /// for `action`.
    fn drawn_focus(&mut self, action: &dyn gpui::Action) -> bool {
        (self.window.focused(self.cx)).is_some_and(|focus| {
            kiln::platform::focus::is_drawn_for(&focus, action, self.window, self.cx)
        })
    }
}

/// The plugin-facing entries of the [`PLUGIN_CALLABLE`] commands, each open
/// to plugins with what it needs. Without a policy it refuses every
/// plugin.
pub(crate) fn registry() -> Registry<WorkbenchCommands> {
    let mut registry = Registry::new();
    for (id, capabilities, path_args) in PLUGIN_CALLABLE {
        let title = &command_title(id);
        if id == OPEN_PATH {
            registry.register_with_args(
                id,
                title,
                |forward: &mut Forward<'_>, _: &Actor, _: OpenPathArgs| forward.run(),
            )
        } else {
            registry.register(id, title, |forward: &mut Forward<'_>, _: &Actor| {
                forward.run()
            })
        }
        .expect("plugin-callable workbench command ids are valid and distinct");
        let access = PluginAccess {
            capabilities: capabilities.to_vec(),
            path_args: path_args.to_vec(),
        };
        registry
            .allow_plugins(id, access)
            .expect("plugin-callable workbench commands are registered");
    }
    registry
}

/// Whether `command` is a plugin's own, `<extension id>.<name>`, of an
/// extension `policy` has loaded. No plugin can run one, its own or
/// another's, so that no plugin's grants are used through another's.
pub(crate) fn is_plugin_command(policy: &dyn PluginPolicy, command: &CommandId) -> bool {
    let id = command.as_str();
    (id.match_indices('.'))
        .map(|(dot, _)| &id[..dot])
        .any(|owner| policy.grants(owner).is_some())
}

/// The refusal of `plugin`'s invocation of `command`, which isn't open to
/// plugins.
pub(crate) fn undeclared(plugin: &str, command: &CommandId) -> PluginDenied {
    PluginDenied::Undeclared {
        plugin: plugin.to_owned(),
        command: command.clone(),
    }
}

#[cfg(test)]
#[path = "plugin_access_tests.rs"]
mod plugin_access_tests;
