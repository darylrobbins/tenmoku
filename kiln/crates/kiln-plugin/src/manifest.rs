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
use kiln_l10n::tr;
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
    WorkspaceCannotInstallPlugins {
        id: String,
    },
    MoreThanOnePlugin {
        id: String,
    },
    MissingWasm {
        id: String,
    },
    CapabilitiesNotStrings {
        id: String,
    },
    SignatureNotString {
        id: String,
    },
    Capability {
        id: String,
        #[source]
        source: CapabilityError,
    },
    WasmPath {
        id: String,
        path: String,
        reason: WasmPathError,
    },
}

/// In the active locale.
impl std::fmt::Display for PluginManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::WorkspaceCannotInstallPlugins { id } => {
                tr!("kiln-plugin-manifest-workspace", id = id)
            }
            Self::MoreThanOnePlugin { id } => tr!("kiln-plugin-manifest-more-than-one", id = id),
            Self::MissingWasm { id } => tr!("kiln-plugin-manifest-missing-wasm", id = id),
            Self::CapabilitiesNotStrings { id } => {
                tr!("kiln-plugin-manifest-capabilities-not-strings", id = id)
            }
            Self::SignatureNotString { id } => {
                tr!("kiln-plugin-manifest-signature-not-string", id = id)
            }
            Self::Capability { id, source } => tr!(
                "kiln-plugin-manifest-capability",
                id = id,
                reason = source.to_string()
            ),
            Self::WasmPath { id, path, reason } => tr!(
                "kiln-plugin-manifest-wasm-path",
                id = id,
                path = format!("{path:?}"),
                reason = reason.to_string()
            ),
        })
    }
}

/// Why a plugin's `wasm` path isn't a file inside its extension's directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmPathError {
    Empty,
    Absolute,
    Outside,
    DirUnreadable,
    Missing,
    NotAFile,
}

/// In the active locale.
impl std::fmt::Display for WasmPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!(match self {
            Self::Empty => "kiln-plugin-wasm-path-empty",
            Self::Absolute => "kiln-plugin-wasm-path-absolute",
            Self::Outside => "kiln-plugin-wasm-path-outside",
            Self::DirUnreadable => "kiln-plugin-wasm-path-dir-unreadable",
            Self::Missing => "kiln-plugin-wasm-path-missing",
            Self::NotAFile => "kiln-plugin-wasm-path-not-a-file",
        }))
    }
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
fn wasm_file(dir: &Path, entry: &str) -> Result<PathBuf, WasmPathError> {
    let relative = Path::new(entry);
    if entry.is_empty() {
        return Err(WasmPathError::Empty);
    }
    if relative.has_root() || relative.is_absolute() {
        return Err(WasmPathError::Absolute);
    }
    // Written to climb out, whatever the links inside lead to.
    let mut depth = 0usize;
    for component in relative.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(WasmPathError::Outside),
        }
    }
    // Without Windows' `\\?\`, as paths are shown and compared elsewhere.
    let canonical_dir =
        kiln_core::platform::canonicalize(dir).map_err(|_| WasmPathError::DirUnreadable)?;
    let canonical = kiln_core::platform::canonicalize(&dir.join(relative))
        .map_err(|_| WasmPathError::Missing)?;
    if !canonical.starts_with(&canonical_dir) {
        return Err(WasmPathError::Outside);
    }
    if !canonical.is_file() {
        return Err(WasmPathError::NotAFile);
    }
    Ok(canonical)
}
