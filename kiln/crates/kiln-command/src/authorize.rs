// SPDX-License-Identifier: LicenseRef-Proprietary

//! Authorizing plugin-origin dispatches. `Actor::Host("<extension id>")` is
//! attribution: before a plugin's invocation runs, the registry checks it
//! against the plugin's [`Grants`] (the plugin record,
//! `docs/decisions/vmtnmkay62-extensions-and-plugins.md` §7). Default-deny:
//! with no [`PluginPolicy`] installed, or for a command that declares no
//! [`PluginAccess`], a plugin is refused.

use std::{
    collections::VecDeque,
    fs, io,
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

use crate::{Capability, CommandId, Grants};

/// What a command needs of a plugin that invokes it, declared with
/// [`Registry::allow_plugins`](crate::Registry::allow_plugins).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginAccess {
    /// The capabilities the command's effects need, beyond `commands`.
    pub capabilities: Vec<Capability>,
    /// The names of its args that are workspace paths. Each must be given,
    /// as an absolute path inside a workspace root: one left out could
    /// default to anywhere.
    pub path_args: Vec<&'static str>,
}

/// What the host knows of the plugins: their grants and the workspace they
/// may reach.
pub trait PluginPolicy {
    /// The plugin's grants, or `None` if it isn't installed.
    fn grants(&self, plugin: &str) -> Option<Grants>;
    /// The folders open in the workspace.
    fn workspace_roots(&self) -> Vec<PathBuf>;
}

/// Why a plugin's invocation was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PluginDenied {
    #[error("plugin {plugin} can't run {command}: no plugin policy is installed")]
    NoPolicy { plugin: String, command: CommandId },
    #[error("plugin {plugin} can't run {command}: it isn't installed")]
    UnknownPlugin { plugin: String, command: CommandId },
    #[error("plugin {plugin} can't run {command}: the command isn't open to plugins")]
    Undeclared { plugin: String, command: CommandId },
    #[error("plugin {plugin} can't run {command}: it needs {capability}")]
    MissingCapability {
        plugin: String,
        command: CommandId,
        capability: Capability,
    },
    #[error("plugin {plugin} can't run {command}: {arg} {path:?} is outside the workspace")]
    OutsideWorkspace {
        plugin: String,
        command: CommandId,
        arg: &'static str,
        path: String,
    },
    #[error("plugin {plugin} can't run {command}: it needs a workspace path for {arg}")]
    MissingPath {
        plugin: String,
        command: CommandId,
        arg: &'static str,
    },
}

/// Checks that `plugin` may run `command` with `args`.
pub(crate) fn authorize(
    policy: Option<&dyn PluginPolicy>,
    plugin: &str,
    command: &CommandId,
    access: Option<&PluginAccess>,
    args: &Value,
) -> Result<(), PluginDenied> {
    let plugin_owned = || plugin.to_string();
    let Some(policy) = policy else {
        return Err(PluginDenied::NoPolicy {
            plugin: plugin_owned(),
            command: command.clone(),
        });
    };
    let Some(grants) = policy.grants(plugin) else {
        return Err(PluginDenied::UnknownPlugin {
            plugin: plugin_owned(),
            command: command.clone(),
        });
    };
    let Some(access) = access else {
        return Err(PluginDenied::Undeclared {
            plugin: plugin_owned(),
            command: command.clone(),
        });
    };
    let mut needed = std::iter::once(&Capability::Commands).chain(&access.capabilities);
    if let Some(capability) = needed.find(|needed| !grants.allows(needed)) {
        return Err(PluginDenied::MissingCapability {
            plugin: plugin_owned(),
            command: command.clone(),
            capability: capability.clone(),
        });
    }
    if access.path_args.is_empty() {
        return Ok(());
    }
    let roots: Vec<PathBuf> = policy
        .workspace_roots()
        .iter()
        .filter_map(|root| resolve(root))
        .collect();
    for &arg in &access.path_args {
        let outside = |path: String| PluginDenied::OutsideWorkspace {
            plugin: plugin_owned(),
            command: command.clone(),
            arg,
            path,
        };
        match args.get(arg) {
            None | Some(Value::Null) => {
                return Err(PluginDenied::MissingPath {
                    plugin: plugin_owned(),
                    command: command.clone(),
                    arg,
                });
            }
            Some(Value::String(path)) => {
                let inside = resolve(Path::new(path))
                    .is_some_and(|path| roots.iter().any(|root| path.starts_with(root)));
                if !inside {
                    return Err(outside(path.clone()));
                }
            }
            // Not a path this can check, so not one a plugin may pass.
            Some(other) => return Err(outside(other.to_string())),
        }
    }
    Ok(())
}

/// The most symlinks [`resolve`] follows in one path, as Linux's
/// `MAXSYMLINKS`: more is a loop, or as good as one.
const MAX_SYMLINKS: u32 = 40;

/// Where `path` leads, symlinks followed one component at a time as the
/// OS would, so `..` after a symlink leaves where it leads, and a symlink
/// whose target doesn't exist yet still counts as its target. A component
/// that doesn't exist is taken as named, as creating it would make it.
/// `None` for a relative path, which says nothing of
/// where it is, for a symlink loop, and for anything that can't be read,
/// so a path that can't be checked is refused.
fn resolve(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let owned = |component: Component<'_>| PathBuf::from(component.as_os_str());
    let mut rest: VecDeque<PathBuf> = path.components().map(owned).collect();
    let mut resolved = PathBuf::new();
    let mut symlinks = 0;
    while let Some(component) = rest.pop_front() {
        match component.components().next() {
            None | Some(Component::CurDir) => {}
            Some(Component::ParentDir) => {
                resolved.pop();
            }
            // An absolute symlink target starts over from its root.
            Some(Component::RootDir | Component::Prefix(_)) => resolved.push(&component),
            Some(Component::Normal(_)) => {
                let next = resolved.join(&component);
                match fs::symlink_metadata(&next) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        symlinks += 1;
                        if symlinks > MAX_SYMLINKS {
                            return None;
                        }
                        // Its target's components come next, from the
                        // symlink's directory.
                        let target = fs::read_link(&next).ok()?;
                        for component in target.components().rev() {
                            rest.push_front(owned(component));
                        }
                    }
                    Ok(_) => resolved = next,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => resolved = next,
                    Err(_) => return None,
                }
            }
        }
    }
    Some(resolved)
}
