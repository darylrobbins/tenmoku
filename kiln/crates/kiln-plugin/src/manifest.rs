// SPDX-License-Identifier: Apache-2.0

//! Parsing and validating a plugin's manifest.
//!
//! A plugin is an extension whose `extension.toml` has a
//! `[contributes.plugin]` table. The loader checks that table against
//! [`plugin_kind`]'s schema; [`PluginManifest::from_extension`] reads it
//! from the checked [`Manifest`].

use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};

use kiln_command::{Capability, CapabilityError, Grants};
use kiln_core::{AppKind, extension::Manifest};
use kiln_l10n::tr;
use serde_json::json;
use toml::Value;

use crate::limits::{
    BACKGROUND_MEMORY_FLOOR, BACKGROUND_WALL_FLOOR, BackgroundLimit, BackgroundLimits, Ceilings,
    DATA_QUOTA_CEILING,
};

/// The `contributes` key that makes an extension a plugin.
pub const PLUGIN_KIND: &str = "plugin";

/// The `[contributes.plugin]` kind: `wasm`, `capabilities`, an optional
/// `signature` and an optional `[contributes.plugin.background]` table. Register it for the user's extensions layer only.
pub fn plugin_kind() -> AppKind {
    AppKind {
        name: PLUGIN_KIND,
        schema: json!({
            "type": "object",
            "properties": {
                "wasm": { "type": "string" },
                "capabilities": { "type": "array", "items": { "type": "string" } },
                "signature": { "type": "string" },
                "background": {
                    "type": "object",
                    "properties": {
                        "memory": { "type": "string" },
                        "wall": { "type": "string" },
                        "data": { "type": "string" },
                    },
                    "additionalProperties": false,
                },
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
    UnknownBackgroundKey {
        id: String,
        key: String,
    },
    Background {
        id: String,
        key: &'static str,
        reason: BackgroundError,
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
            Self::UnknownBackgroundKey { id, key } => {
                tr!(
                    "kiln-plugin-manifest-background-unknown-key",
                    id = id,
                    key = key
                )
            }
            Self::Background { id, key, reason } => tr!(
                "kiln-plugin-manifest-background",
                id = id,
                key = *key,
                reason = reason.to_string()
            ),
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

/// Why a `[contributes.plugin.background]` value is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundError {
    NotAString,
    Malformed,
    TooSmall,
    TooLarge,
}

/// In the active locale.
impl std::fmt::Display for BackgroundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!(match self {
            Self::NotAString => "kiln-plugin-background-not-a-string",
            Self::Malformed => "kiln-plugin-background-malformed",
            Self::TooSmall => "kiln-plugin-background-too-small",
            Self::TooLarge => "kiln-plugin-background-too-large",
        }))
    }
}

/// `text` as a whole number of `units`, each a suffix and its size: `512MiB`, `30m`.
fn with_unit(text: &str, units: &[(&str, u64)]) -> Result<u64, BackgroundError> {
    let text = text.trim();
    let (suffix, size) = units
        .iter()
        .filter(|(suffix, _)| text.ends_with(suffix))
        // `MiB` before `B`, were there one.
        .max_by_key(|(suffix, _)| suffix.len())
        .ok_or(BackgroundError::Malformed)?;
    let digits = text[..text.len() - suffix.len()].trim_end();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(BackgroundError::Malformed);
    }
    let count: u64 = digits.parse().map_err(|_| BackgroundError::TooLarge)?;
    count.checked_mul(*size).ok_or(BackgroundError::TooLarge)
}

const SIZE_UNITS: &[(&str, u64)] = &[("KiB", 1 << 10), ("MiB", 1 << 20), ("GiB", 1 << 30)];
const TIME_UNITS: &[(&str, u64)] = &[("s", 1), ("m", 60)];

/// A size such as `256MiB`, in bytes, within `min..=max`. A value past `max` isn't an error
/// here for `memory` and `wall`: the host's ceilings clamp it ([`Ceilings::clamp`]).
fn size_value(
    id: &str,
    key: &'static str,
    value: &Value,
    min: u64,
    max: u64,
) -> Result<u64, PluginManifestError> {
    let fail = |reason| PluginManifestError::Background {
        id: id.to_owned(),
        key,
        reason,
    };
    let Value::String(text) = value else {
        return Err(fail(BackgroundError::NotAString));
    };
    let bytes = with_unit(text, SIZE_UNITS).map_err(fail)?;
    if bytes < min {
        return Err(fail(BackgroundError::TooSmall));
    }
    if bytes > max {
        return Err(fail(BackgroundError::TooLarge));
    }
    Ok(bytes)
}

/// Why a plugin's `wasm` path isn't a file inside its extension's directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmPathError {
    Empty,
    Backslash,
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
            Self::Backslash => "kiln-plugin-wasm-path-backslash",
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
    /// What `[contributes.plugin.background]` declares, over the defaults. As declared: the
    /// host's [`Ceilings`] clamp it where it is used.
    pub background: BackgroundLimits,
    /// Notes on capabilities that were accepted but are better written
    /// another way, such as `process.spawn:rg.exe`.
    pub warnings: Vec<String>,
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
        let mut warnings = Vec::new();
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
                    warnings.extend(capability.warning());
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
        let mut background = BackgroundLimits::default();
        match entry.get("background") {
            None => {}
            Some(Value::Table(table)) => {
                if let Some(key) = table
                    .keys()
                    .find(|key| !matches!(key.as_str(), "memory" | "wall" | "data"))
                {
                    return Err(PluginManifestError::UnknownBackgroundKey {
                        id: id.clone(),
                        key: key.clone(),
                    });
                }
                // A value past the ceiling isn't refused: the host clamps it.
                if let Some(value) = table.get("memory") {
                    let bytes = size_value(id, "memory", value, BACKGROUND_MEMORY_FLOOR, u64::MAX)?;
                    background.memory_bytes = bytes;
                }
                if let Some(value) = table.get("wall") {
                    let fail = |reason| PluginManifestError::Background {
                        id: id.clone(),
                        key: "wall",
                        reason,
                    };
                    let Value::String(text) = value else {
                        return Err(fail(BackgroundError::NotAString));
                    };
                    let secs = with_unit(text, TIME_UNITS).map_err(fail)?;
                    if secs < BACKGROUND_WALL_FLOOR.as_secs() {
                        return Err(fail(BackgroundError::TooSmall));
                    }
                    background.wall = Duration::from_secs(secs);
                }
                if let Some(value) = table.get("data") {
                    background.data_bytes =
                        Some(size_value(id, "data", value, 0, DATA_QUOTA_CEILING)?);
                }
            }
            Some(_) => {
                return Err(PluginManifestError::Background {
                    id: id.clone(),
                    key: "background",
                    reason: BackgroundError::NotAString,
                });
            }
        }
        Ok(Some(Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version,
            wasm,
            capabilities,
            signature,
            background,
            warnings,
        }))
    }

    /// The background limits jobs run with on a computer with `ceilings`, and a warning for each
    /// declared value clamped to them. Not an error: the plugin still loads.
    pub fn background_within(&self, ceilings: &Ceilings) -> (BackgroundLimits, Vec<String>) {
        let (limits, clamped) = ceilings.clamp(self.background);
        let warnings = clamped
            .iter()
            .map(|clamped| match clamped.limit {
                BackgroundLimit::Memory => tr!(
                    "kiln-plugin-background-memory-clamped",
                    id = self.id.as_str(),
                    declared = clamped.declared / (1 << 20),
                    ceiling = clamped.ceiling / (1 << 20)
                )
                .to_string(),
                BackgroundLimit::Wall => tr!(
                    "kiln-plugin-background-wall-clamped",
                    id = self.id.as_str(),
                    declared = clamped.declared / 60,
                    ceiling = clamped.ceiling / 60
                )
                .to_string(),
            })
            .collect();
        (limits, warnings)
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
    if entry.contains('\\') {
        return Err(WasmPathError::Backslash);
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
