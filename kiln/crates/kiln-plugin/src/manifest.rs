// SPDX-License-Identifier: Apache-2.0

//! Parsing and validating a plugin's manifest.
//!
//! A plugin is an extension whose `extension.toml` has a
//! `[contributes.plugin]` table. The loader checks that table against
//! [`plugin_kind`]'s schema; [`PluginManifest::from_extension`] reads it
//! from the checked [`Manifest`].

use std::path::{Component, Path, PathBuf};

use kiln_command::{Capability, CapabilityError, Grants};
use kiln_core::{AppKind, extension::Manifest};
use serde_json::json;
use toml::Value;

/// The `contributes` key that makes an extension a plugin.
pub const PLUGIN_KIND: &str = "plugin";

/// The `[contributes.plugin]` kind: `wasm`, `capabilities` and an optional
/// `signature`. Register it for the user's extensions layer only.
pub fn plugin_kind() -> AppKind {
    AppKind {
        name: PLUGIN_KIND,
        schema: json!({
            "type": "object",
            "properties": {
                "wasm": { "type": "string" },
                "capabilities": { "type": "array", "items": { "type": "string" } },
                "signature": { "type": "string" },
            },
            "required": ["wasm"],
            "additionalProperties": false,
        }),
    }
}

/// Where an extension was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionOrigin {
    /// The user's own extensions directory: the only place plugins install from.
    User,
    /// A workspace's extensions, which can never install or grant a plugin.
    Workspace,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginManifestError {
    #[error("a workspace extension cannot install a plugin ({id})")]
    WorkspaceCannotInstallPlugins { id: String },
    #[error("{id}: [contributes.plugin] must be a single table")]
    MoreThanOnePlugin { id: String },
    #[error("{id}: [contributes.plugin] needs a `wasm` string")]
    MissingWasm { id: String },
    #[error("{id}: `capabilities` must be an array of strings")]
    CapabilitiesNotStrings { id: String },
    #[error("{id}: `signature` must be a string")]
    SignatureNotString { id: String },
    #[error("{id}: {source}")]
    Capability {
        id: String,
        #[source]
        source: CapabilityError,
    },
    #[error("{id}: wasm path {path:?}: {reason}")]
    WasmPath {
        id: String,
        path: String,
        reason: &'static str,
    },
}

/// A plugin's checked manifest entry.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: (u32, u32, u32),
    /// The component, an absolute path to a file inside the extension's
    /// directory.
    pub wasm: PathBuf,
    /// What the manifest asks for; granting is decided elsewhere.
    pub capabilities: Grants,
    /// A reserved slot: accepted and kept, never verified.
    pub signature: Option<String>,
}

impl PluginManifest {
    /// The plugin entry of `manifest`, or `None` for an extension that
    /// contributes none. A `Workspace` extension with one is an error.
    pub fn from_extension(
        manifest: &Manifest,
        origin: ExtensionOrigin,
    ) -> Result<Option<Self>, PluginManifestError> {
        let id = &manifest.id;
        let Some(entry) = manifest.app.get(PLUGIN_KIND) else {
            return Ok(None);
        };
        if origin == ExtensionOrigin::Workspace {
            return Err(PluginManifestError::WorkspaceCannotInstallPlugins { id: id.clone() });
        }
        let Value::Table(entry) = entry else {
            return Err(PluginManifestError::MoreThanOnePlugin { id: id.clone() });
        };
        let Some(Value::String(wasm)) = entry.get("wasm") else {
            return Err(PluginManifestError::MissingWasm { id: id.clone() });
        };
        let wasm = wasm_file(&manifest.dir, wasm).map_err(|reason| {
            let path = wasm.clone();
            PluginManifestError::WasmPath {
                id: id.clone(),
                path,
                reason,
            }
        })?;
        let mut capabilities = Grants::new();
        match entry.get("capabilities") {
            None => {}
            Some(Value::Array(items)) => {
                for item in items {
                    let Value::String(text) = item else {
                        return Err(PluginManifestError::CapabilitiesNotStrings { id: id.clone() });
                    };
                    let capability = Capability::parse(text).map_err(|source| {
                        PluginManifestError::Capability {
                            id: id.clone(),
                            source,
                        }
                    })?;
                    capabilities.insert(capability);
                }
            }
            Some(_) => {
                return Err(PluginManifestError::CapabilitiesNotStrings { id: id.clone() });
            }
        }
        let signature = match entry.get("signature") {
            None => None,
            Some(Value::String(signature)) => Some(signature.clone()),
            Some(_) => return Err(PluginManifestError::SignatureNotString { id: id.clone() }),
        };
        Ok(Some(Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version,
            wasm,
            capabilities,
            signature,
        }))
    }

    /// Whether the signature was checked. Always false: the slot is
    /// reserved and nothing verifies it yet.
    pub fn signature_verified(&self) -> bool {
        false
    }
}

/// `entry` as a file inside `dir`, symlinks followed.
fn wasm_file(dir: &Path, entry: &str) -> Result<PathBuf, &'static str> {
    let relative = Path::new(entry);
    if entry.is_empty() {
        return Err("the path is empty");
    }
    if relative.has_root() || relative.is_absolute() {
        return Err("must be relative to the extension's directory");
    }
    // Written to climb out, whatever the links inside lead to.
    let mut depth = 0usize;
    for component in relative.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err("is outside the extension's directory"),
        }
    }
    // Without Windows' `\\?\`, as paths are shown and compared elsewhere.
    let canonical_dir = kiln_core::platform::canonicalize(dir)
        .map_err(|_| "the extension's directory is unreadable")?;
    let canonical =
        kiln_core::platform::canonicalize(&dir.join(relative)).map_err(|_| "no such file")?;
    if !canonical.starts_with(&canonical_dir) {
        return Err("is outside the extension's directory");
    }
    if !canonical.is_file() {
        return Err("is not a file");
    }
    Ok(canonical)
}
