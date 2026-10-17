// SPDX-License-Identifier: LicenseRef-Proprietary

//! Declarative extensions: a directory of data files (themes, keymaps,
//! menus, and kinds the app defines) described by an `extension.toml`
//! manifest. [`load_manifest`] reads and checks one, against the
//! [`ExtensionApi`] the app offers, with every problem a positioned
//! [`Diagnostic`] like a config file's.
//!
//! ```toml
//! id = "acme.solarized"
//! name = "Solarized"
//! version = "1.2.0"
//! license = "MIT"
//!
//! [engine]
//! notes = "1.3"
//!
//! [contributes]
//! themes = ["themes/solarized-dark.toml"]
//! ```

use std::{
    collections::BTreeMap,
    fmt, io,
    path::{Component, Path, PathBuf},
};

use kiln_command::CommandId;
use serde_json::{Value as Json, json};
use toml::{
    Value,
    de::{DeTable, DeValue},
};

use crate::{
    diagnostic::{Diagnostic, LineIndex, Severity},
    layers::{Layer, load_layer},
};

/// The manifest's file name, in the extension's directory.
pub const MANIFEST_FILE: &str = "extension.toml";

/// The most files one extension may contribute, all kinds together.
pub const MAX_CONTRIBUTED_FILES: usize = 256;

/// An extension API version, `MAJOR.MINOR`. An app at 1.3 serves
/// extensions that need 1.0 to 1.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A contribution kind the app defines, such as a language server: its
/// `contributes.<name>` table is checked against `schema` and kept as
/// written in [`Manifest::app`].
#[derive(Clone, Debug, PartialEq)]
pub struct AppKind {
    pub name: &'static str,
    pub schema: Json,
}

/// What the app accepts from extensions.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionApi {
    /// The app's [`crate::AppId::short`], the key manifests name in
    /// `[engine]`.
    pub app: String,
    pub version: ApiVersion,
    /// Command namespaces an extension ID may not start with, such as the
    /// app's own `editor` and `app`.
    pub reserved: Vec<String>,
    pub kinds: Vec<AppKind>,
    /// `extensions.allowed_licenses`: when set, only extensions whose
    /// licence these SPDX IDs satisfy load.
    pub allowed_licenses: Option<Vec<String>>,
}

/// An extension's licence: its SPDX expression and what kind of licence it
/// comes to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct License {
    pub expr: String,
    pub class: LicenseClass,
}

/// What kind of licence an SPDX expression comes to, least permissive
/// first: `OR` takes its most permissive branch, `AND` its least.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LicenseClass {
    /// `LicenseRef-*`, `NOASSERTION`, and any ID not known here.
    Unknown,
    /// GPL, LGPL, AGPL, MPL, EPL, CDDL, EUPL and OSL.
    Copyleft,
    /// The licences kiln's own dependencies may have.
    Permissive,
}

impl License {
    /// Parses an SPDX expression (IDs joined by `AND` and `OR`, with
    /// `WITH` exceptions, a `+` suffix and parentheses) and classifies it.
    pub fn parse(expr: &str) -> Result<Self, String> {
        let tree = spdx::parse(expr)?;
        Ok(Self {
            expr: expr.to_owned(),
            class: tree.class(),
        })
    }

    /// Whether licences with these SPDX IDs (each may be `<id> WITH
    /// <exception>`) cover this one: each `AND` term needs one, an `OR` any
    /// of its branches. Matched ignoring case.
    pub fn allowed_by(&self, allowed: &[String]) -> bool {
        spdx::parse(&self.expr).is_ok_and(|tree| tree.allowed_by(allowed))
    }
}

/// A checked manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// The extension's directory, made absolute.
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
    pub version: (u32, u32, u32),
    pub description: Option<String>,
    pub license: License,
    /// The extension API version it needs.
    pub engine: ApiVersion,
    /// Contributed files, each under `dir`.
    pub themes: Vec<PathBuf>,
    pub keymaps: Vec<PathBuf>,
    pub menus: Vec<PathBuf>,
    /// The app kinds' `contributes.<kind>` values, by kind, as written.
    pub app: BTreeMap<String, Value>,
}

/// The file kinds every app takes.
const FILE_KINDS: [&str; 3] = ["themes", "keymaps", "menus"];

/// Reads and checks `<dir>/extension.toml`. The manifest is `None` when
/// any diagnostic is an error; warnings (unknown keys and contribution
/// kinds, which are ignored) leave it loaded.
pub fn load_manifest(dir: &Path, api: &ExtensionApi) -> (Option<Manifest>, Vec<Diagnostic>) {
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_owned());
    let path = dir.join(MANIFEST_FILE);
    let layer = load_layer(&path);
    if layer.has_errors() {
        return (None, layer.diagnostics);
    }
    if layer.source.is_empty() && !path.try_exists().unwrap_or(true) {
        let message = format!("no {MANIFEST_FILE} in the extension's directory");
        let diagnostic = layer.diagnostic_at(Severity::Error, None, 0, message);
        return (None, vec![diagnostic]);
    }
    let mut check = Check {
        layer: &layer,
        diagnostics: layer.check_schema(&manifest_schema(api)),
    };
    let manifest = check.manifest(&dir, api);
    let diagnostics = check.diagnostics;
    let manifest = manifest.filter(|_| !diagnostics.iter().any(Diagnostic::is_error));
    (manifest, diagnostics)
}

/// The manifest's schema, for types and unknown keys. Required keys and
/// value formats are checked by hand.
fn manifest_schema(api: &ExtensionApi) -> Json {
    let files = json!({ "type": "array" });
    let mut contributes: serde_json::Map<String, Json> = FILE_KINDS
        .iter()
        .map(|kind| ((*kind).to_owned(), files.clone()))
        .collect();
    let mut defs = serde_json::Map::new();
    for kind in &api.kinds {
        let mut schema = kind.schema.clone();
        // Its definitions move to the root, where `$ref`s resolve, under
        // `<kind>/` so two kinds' names can't collide.
        let own = schema
            .as_object_mut()
            .and_then(|schema| schema.remove("$defs"));
        let prefix = format!("{}/", kind.name);
        rescope_refs(&mut schema, &prefix);
        if let Some(Json::Object(own)) = own {
            for (name, mut def) in own {
                rescope_refs(&mut def, &prefix);
                defs.insert(format!("{prefix}{name}"), def);
            }
        }
        contributes.insert(kind.name.to_owned(), schema);
    }
    let string = json!({ "type": "string" });
    json!({
        "type": "object",
        "properties": {
            "id": string,
            "name": { "type": "string", "minLength": 1 },
            "version": string,
            "description": string,
            "license": string,
            // Its key is an app's name, so it lists no properties.
            "engine": { "type": "object" },
            "contributes": { "type": "object", "properties": contributes },
            // Refused by hand, with its own message.
            "plugin": {},
        },
        "$defs": defs,
    })
}

/// Points each `#/$defs/<name>` reference in `schema` at `#/$defs/<prefix><name>`.
fn rescope_refs(schema: &mut Json, prefix: &str) {
    match schema {
        Json::Object(object) => {
            for (key, value) in object.iter_mut() {
                if key == "$ref"
                    && let Json::String(reference) = value
                    && let Some(name) = reference.strip_prefix("#/$defs/")
                {
                    *reference = format!("#/$defs/{prefix}{name}");
                } else {
                    rescope_refs(value, prefix);
                }
            }
        }
        Json::Array(items) => items.iter_mut().for_each(|item| rescope_refs(item, prefix)),
        _ => {}
    }
}

struct Check<'a> {
    layer: &'a Layer,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Check<'a> {
    fn manifest(&mut self, dir: &Path, api: &ExtensionApi) -> Option<Manifest> {
        let table = &self.layer.table;
        if table.contains_key("plugin") {
            self.at_key(
                &["plugin"],
                "code plugins aren't supported by this app".into(),
            );
        }
        let id = self.required_str("id").and_then(|id| self.id(id, api));
        let name = self.required_str("name").map(str::to_owned);
        let version = self.required_str("version").and_then(|version| {
            let parsed = parse_numbers::<3>(version).map(|[a, b, c]| (a, b, c));
            if parsed.is_none() {
                self.at_value(
                    &["version"],
                    format!(
                        "{version:?} isn't a version: expected MAJOR.MINOR.PATCH, such as 1.0.0"
                    ),
                );
            }
            parsed
        });
        let description = table
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let license = self
            .required_str("license")
            .and_then(|expr| self.license(expr, id.as_deref(), api));
        let engine = self.engine(api);
        let files = self.files(dir);
        let app = self.app_kinds(api);
        Some(Manifest {
            dir: dir.to_owned(),
            id: id?,
            name: name?,
            version: version?,
            description,
            license: license?,
            engine: engine?,
            themes: files.get("themes")?.clone(),
            keymaps: files.get("keymaps")?.clone(),
            menus: files.get("menus")?.clone(),
            app,
        })
    }

    fn at_key(&mut self, path: &[&str], message: String) {
        let path = owned(path);
        self.diagnostics
            .push(self.layer.key_diagnostic(Severity::Error, &path, message));
    }

    fn at_value(&mut self, path: &[&str], message: String) {
        let path = owned(path);
        self.diagnostics
            .push(self.layer.value_diagnostic(Severity::Error, &path, message));
    }

    /// The string at top-level `key`; an error if it's missing. One of
    /// another type was reported by the schema.
    fn required_str(&mut self, key: &str) -> Option<&'a str> {
        let layer = self.layer;
        match layer.table.get(key) {
            None => {
                self.at_key(&[key], format!("missing `{key}`"));
                None
            }
            Some(value) => value.as_str(),
        }
    }

    fn id(&mut self, id: &str, api: &ExtensionApi) -> Option<String> {
        let parsed = match CommandId::parse(id) {
            Ok(parsed) => parsed,
            Err(error) => {
                let reason = match error.reason {
                    "needs a namespace, as in `editor.move_left`" => {
                        "needs two or more segments, as in `acme.solarized`"
                    }
                    reason => reason,
                };
                self.at_value(&["id"], format!("{id:?} isn't an extension ID: {reason}"));
                return None;
            }
        };
        let namespace = parsed.namespace();
        if api.reserved.iter().any(|reserved| reserved == namespace) {
            self.at_value(
                &["id"],
                format!(
                    "{id:?} starts with `{namespace}`, which is reserved for the app's own commands"
                ),
            );
            return None;
        }
        Some(id.to_owned())
    }

    fn license(&mut self, expr: &str, id: Option<&str>, api: &ExtensionApi) -> Option<License> {
        let license = match License::parse(expr) {
            Ok(license) => license,
            Err(reason) => {
                self.at_value(
                    &["license"],
                    format!("{expr:?} isn't an SPDX licence expression: {reason}"),
                );
                return None;
            }
        };
        if let Some(allowed) = &api.allowed_licenses
            && !license.allowed_by(allowed)
        {
            let id = id.or_else(|| self.layer.table.get("id")?.as_str());
            self.at_value(
                &["license"],
                format!(
                    "{}: licence {expr} isn't in extensions.allowed_licenses",
                    id.unwrap_or("this extension")
                ),
            );
            return None;
        }
        Some(license)
    }

    fn engine(&mut self, api: &ExtensionApi) -> Option<ApiVersion> {
        let layer = self.layer;
        let example = format!("as in `[engine] {} = \"{}\"`", api.app, api.version);
        let engine = match layer.table.get("engine") {
            None => {
                self.at_key(
                    &["engine"],
                    format!("missing [engine]: the app and extension API version this extension needs, {example}"),
                );
                return None;
            }
            Some(Value::Table(engine)) => engine,
            // Reported by the schema.
            Some(_) => return None,
        };
        let Some(value) = engine.get(&api.app) else {
            match engine.keys().next() {
                Some(other) => self.at_key(
                    &["engine", other],
                    format!("this extension is for {other}, not this app ({})", api.app),
                ),
                None => self.at_key(&["engine"], format!("[engine] names no app, {example}")),
            }
            return None;
        };
        let key = ["engine", api.app.as_str()];
        let Some(written) = value.as_str() else {
            self.at_value(
                &key,
                format!(
                    "expected a string, such as \"{}\", found {}",
                    api.version,
                    value.type_str()
                ),
            );
            return None;
        };
        let Some([major, minor]) = parse_numbers::<2>(written) else {
            self.at_value(
                &key,
                format!(
                    "{written:?} isn't an API version: expected MAJOR.MINOR, such as {}",
                    api.version
                ),
            );
            return None;
        };
        let needed = ApiVersion { major, minor };
        if needed.major != api.version.major || needed.minor > api.version.minor {
            self.at_value(
                &key,
                format!("needs extension API {needed}; this app has {}", api.version),
            );
            return None;
        }
        Some(needed)
    }

    /// The contributed files by kind, each checked to be a file under
    /// `dir`. A kind missing from the map had errors.
    fn files(&mut self, dir: &Path) -> BTreeMap<&'static str, Vec<PathBuf>> {
        let layer = self.layer;
        let contributes = match layer.table.get("contributes") {
            Some(Value::Table(contributes)) => Some(contributes),
            _ => None,
        };
        let arrays: Vec<(&'static str, Option<&Vec<Value>>)> = FILE_KINDS
            .iter()
            .filter_map(
                |&kind| match contributes.and_then(|table| table.get(kind)) {
                    None => Some((kind, None)),
                    Some(Value::Array(entries)) => Some((kind, Some(entries))),
                    // Reported by the schema.
                    Some(_) => None,
                },
            )
            .collect();
        let mut files = BTreeMap::new();
        // Counted first, so a huge array is refused without checking each entry.
        let count: usize = arrays
            .iter()
            .filter_map(|(_, entries)| *entries)
            .map(Vec::len)
            .sum();
        if count > MAX_CONTRIBUTED_FILES {
            self.at_key(
                &["contributes"],
                format!("contributes {count} files; an extension may contribute at most {MAX_CONTRIBUTED_FILES}"),
            );
            return files;
        }
        let canonical_dir = dir.canonicalize().ok();
        let mut spans = None;
        for (kind, entries) in arrays {
            let mut paths = Some(Vec::new());
            for (index, entry) in entries.into_iter().flatten().enumerate() {
                let resolved = match entry.as_str() {
                    Some(entry) => contributed_file(dir, canonical_dir.as_deref(), entry),
                    None => Err(format!("expected a path, found {}", entry.type_str())),
                };
                match resolved {
                    Ok(path) => {
                        if let Some(paths) = &mut paths {
                            paths.push(path);
                        }
                    }
                    Err(message) => {
                        let spans = spans.get_or_insert_with(|| ArraySpans::new(layer));
                        self.diagnostics
                            .push(spans.diagnostic(layer, kind, index, message));
                        paths = None;
                    }
                }
            }
            if let Some(paths) = paths {
                files.insert(kind, paths);
            }
        }
        files
    }

    fn app_kinds(&mut self, api: &ExtensionApi) -> BTreeMap<String, Value> {
        let Some(Value::Table(contributes)) = self.layer.table.get("contributes") else {
            return BTreeMap::new();
        };
        api.kinds
            .iter()
            .filter_map(|kind| {
                let value = contributes.get(kind.name)?;
                Some((kind.name.to_owned(), value.clone()))
            })
            .collect()
    }
}

fn owned(path: &[&str]) -> Vec<String> {
    path.iter().map(|part| (*part).to_owned()).collect()
}

/// `N` `.`-separated decimal numbers, digits only.
fn parse_numbers<const N: usize>(text: &str) -> Option<[u32; N]> {
    let mut numbers = [0; N];
    let mut parts = text.split('.');
    for number in &mut numbers {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

/// `entry` resolved under `dir`: relative, not climbing out with `..`,
/// and a file there, symlinks followed, still inside `dir`. The path is
/// `dir` joined with where the entry leads, symlinks and `..` resolved,
/// so it names the file that was checked.
fn contributed_file(
    dir: &Path,
    canonical_dir: Option<&Path>,
    entry: &str,
) -> Result<PathBuf, String> {
    let relative = Path::new(entry);
    if entry.is_empty() {
        return Err("the path is empty".into());
    }
    if relative.has_root() || relative.is_absolute() {
        return Err(format!(
            "{entry:?} must be relative to the extension's directory"
        ));
    }
    // Written to climb out, whatever the links inside lead to.
    let mut depth = 0usize;
    for component in relative.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(format!("{entry:?} is outside the extension's directory")),
        }
    }
    // Resolved as written: `link/..` is the link's parent, not `dir`.
    let canonical = match dir.join(relative).canonicalize() {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!("{entry:?} doesn't exist"));
        }
        Err(error) => return Err(format!("{entry:?} can't be read: {error}")),
    };
    let Some(inside) = canonical_dir.and_then(|root| canonical.strip_prefix(root).ok()) else {
        return Err(format!(
            "{entry:?} leads to {}, outside the extension's directory",
            canonical.display()
        ));
    };
    if !canonical.is_file() {
        return Err(format!("{entry:?} isn't a file"));
    }
    Ok(dir.join(inside))
}

/// Where each entry of the `contributes` file arrays is in the source,
/// which [`Layer`] doesn't index, and the source's lines, built once for
/// all of them.
struct ArraySpans {
    spans: BTreeMap<(String, usize), usize>,
    lines: LineIndex,
}

impl ArraySpans {
    fn new(layer: &Layer) -> Self {
        let mut spans = BTreeMap::new();
        let lines = LineIndex::new(&layer.source);
        let Ok(document) = DeTable::parse(&layer.source) else {
            return Self { spans, lines };
        };
        let contributes = document
            .get_ref()
            .iter()
            .find(|(key, _)| key.get_ref() == "contributes");
        if let Some((_, contributes)) = contributes
            && let DeValue::Table(contributes) = contributes.get_ref()
        {
            for (kind, value) in contributes.iter() {
                if let DeValue::Array(entries) = value.get_ref() {
                    for (index, entry) in entries.iter().enumerate() {
                        spans.insert((kind.get_ref().to_string(), index), entry.span().start);
                    }
                }
            }
        }
        Self { spans, lines }
    }

    /// An error at entry `index` of `contributes.<kind>`, else at the key.
    fn diagnostic(&self, layer: &Layer, kind: &str, index: usize, message: String) -> Diagnostic {
        let offset = self
            .spans
            .get(&(kind.to_owned(), index))
            .copied()
            .unwrap_or_else(|| {
                layer
                    .key_offset(&owned(&["contributes", kind]))
                    .unwrap_or_default()
            });
        let (line, column) = self.lines.line_column(&layer.source, offset);
        Diagnostic {
            severity: Severity::Error,
            file: layer.path.clone(),
            key: Some(format!("contributes.{kind}")),
            line,
            column,
            message,
        }
    }
}

/// The extensions in one directory, a directory each, as last loaded:
/// see [`ExtensionSet::discover`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionSet {
    root: PathBuf,
    api: ExtensionApi,
    extensions: Vec<Manifest>,
    diagnostics: Vec<Diagnostic>,
}

impl ExtensionSet {
    /// Loads every directory directly under `root` that has an
    /// [`MANIFEST_FILE`], with [`load_manifest`], in ID order. A missing
    /// `root` is no extensions and no diagnostics. Of two directories with
    /// one ID, the first by directory name loads, and the other has an
    /// error naming both.
    pub fn discover(root: &Path, api: &ExtensionApi) -> ExtensionSet {
        let mut set = ExtensionSet {
            root: std::path::absolute(root).unwrap_or_else(|_| root.to_owned()),
            api: api.clone(),
            extensions: Vec::new(),
            diagnostics: Vec::new(),
        };
        set.rescan();
        set
    }

    /// Reads the directory again: an extension whose manifest now has
    /// errors, or is missing or unreadable, keeps its last good version,
    /// reporting them, and one whose directory is gone drops out. Returns whether anything changed, the
    /// diagnostics included.
    pub fn rescan(&mut self) -> bool {
        let mut diagnostics = Vec::new();
        let dirs = match extension_dirs(&self.root) {
            Ok(mut dirs) => {
                // A directory that held an extension keeps it until the
                // directory is gone: a manifest removed or unreadable is
                // an error, and its last good version stays.
                for last in &self.extensions {
                    if !dirs.contains(&last.dir) && still_a_dir(&last.dir) {
                        dirs.push(last.dir.clone());
                    }
                }
                dirs.sort();
                dirs
            }
            Err(error) => {
                // Unreadable for now: what was loaded stays.
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    file: self.root.clone(),
                    key: None,
                    line: 1,
                    column: 1,
                    message: format!("can't read the extensions directory: {error}"),
                });
                let changed = diagnostics != self.diagnostics;
                self.diagnostics = diagnostics;
                return changed;
            }
        };
        let mut extensions: Vec<Manifest> = Vec::new();
        for dir in dirs {
            let (loaded, found) = load_manifest(&dir, &self.api);
            let has_errors = found.iter().any(Diagnostic::is_error);
            diagnostics.extend(found);
            let manifest = match loaded {
                Some(manifest) => manifest,
                None if has_errors => {
                    let last_good = self.extensions.iter().find(|last| last.dir == dir);
                    let Some(last_good) = last_good else {
                        continue;
                    };
                    last_good.clone()
                }
                None => continue,
            };
            if let Some(first) = extensions.iter().find(|first| first.id == manifest.id) {
                let message = format!(
                    "`{}` is also the ID of the extension in {}, which loads instead",
                    manifest.id,
                    first.dir.display()
                );
                let layer = load_layer(&dir.join(MANIFEST_FILE));
                let path = owned(&["id"]);
                diagnostics.push(layer.value_diagnostic(Severity::Error, &path, message));
                continue;
            }
            extensions.push(manifest);
        }
        extensions.sort_by(|a, b| a.id.cmp(&b.id));
        let changed = extensions != self.extensions || diagnostics != self.diagnostics;
        self.extensions = extensions;
        self.diagnostics = diagnostics;
        changed
    }

    /// The directory the extensions are in, made absolute.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The extensions loaded, in ID order.
    pub fn extensions(&self) -> &[Manifest] {
        &self.extensions
    }

    /// Their manifests' problems, in directory order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// A notice for each extension loaded whose licence is copyleft or
    /// unknown that the user hasn't had at its version, recorded in
    /// `<state_dir>/`[`ACKNOWLEDGED_FILE`] so it's given once: the app
    /// ships none of these, so the user should know they installed one. A
    /// permissive one has none. The file is read and written under a lock
    /// on `<file>.lock`, so of two processes discovering the same
    /// extension only one has its notice, and neither drops the other's
    /// entries. Call it when the notices will reach the user; a failure
    /// to record them is logged, and they're still returned.
    pub fn license_notices(&self, state_dir: &Path) -> Vec<LicenseNotice> {
        let path = state_dir.join(ACKNOWLEDGED_FILE);
        let claimed = crate::trust::locked(&path, || {
            let mut acknowledged = read_acknowledged(&path);
            let notices: Vec<LicenseNotice> = self
                .unpermissive()
                .filter(|notice| acknowledged.insert(notice.key()))
                .collect();
            if !notices.is_empty() {
                let entries: Value = acknowledged.into_iter().collect::<Vec<_>>().into();
                let text = format!("{ACKNOWLEDGED_HEADER}acknowledged = {entries}\n");
                if let Err(error) = crate::trust::write(&path, text.as_bytes()) {
                    log::warn!("can't write {}: {error}", path.display());
                }
            }
            notices
        });
        claimed.unwrap_or_else(|error| {
            log::warn!("can't lock {}: {error}", path.display());
            self.pending_license_notices(state_dir)
        })
    }

    /// The notices [`ExtensionSet::license_notices`] would give, without
    /// recording them: for when there's nowhere to show them yet.
    pub fn pending_license_notices(&self, state_dir: &Path) -> Vec<LicenseNotice> {
        let acknowledged = read_acknowledged(&state_dir.join(ACKNOWLEDGED_FILE));
        self.unpermissive()
            .filter(|notice| !acknowledged.contains(&notice.key()))
            .collect()
    }

    /// A notice for each extension loaded whose licence isn't permissive.
    fn unpermissive(&self) -> impl Iterator<Item = LicenseNotice> + '_ {
        self.extensions
            .iter()
            .filter(|manifest| manifest.license.class != LicenseClass::Permissive)
            .map(|manifest| LicenseNotice {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version,
                license: manifest.license.clone(),
            })
    }
}

/// Whether `dir` may still be a directory: it is one, or what it is can't
/// be read. Gone, or something else now, it isn't.
fn still_a_dir(dir: &Path) -> bool {
    match std::fs::metadata(dir) {
        Ok(meta) => meta.is_dir(),
        Err(error) => !matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        ),
    }
}

/// The directories directly under `root` with a manifest, or one that
/// can't be read, by name; none if `root` is missing.
fn extension_dirs(root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let dir = entry?.path();
        let manifest = dir.join(MANIFEST_FILE).symlink_metadata();
        let has_manifest = match manifest {
            Ok(_) => true,
            Err(error) => error.kind() != io::ErrorKind::NotFound,
        };
        if dir.is_dir() && has_manifest {
            dirs.push(dir);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// The file in the state directory recording which extensions' licence
/// notices the user has had ([`ExtensionSet::license_notices`]), each as
/// `id@version`.
pub const ACKNOWLEDGED_FILE: &str = "extensions-acknowledged.toml";

const ACKNOWLEDGED_HEADER: &str = "\
# The extensions you installed, as id@version, whose licence you've been
# told of. Remove an entry to be told again.

";

/// The `id@version` entries in `path`; none if it's missing or unreadable.
fn read_acknowledged(path: &Path) -> std::collections::BTreeSet<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Default::default(),
        Err(error) => {
            log::warn!("can't read {}: {error}", path.display());
            return Default::default();
        }
    };
    let table: toml::Table = toml::from_str(&text).unwrap_or_default();
    let entries = table.get("acknowledged").and_then(Value::as_array);
    entries
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.as_str()?.to_owned()))
        .collect()
}

/// That an extension the user installed isn't permissively licensed:
/// [`ExtensionSet::license_notices`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LicenseNotice {
    pub id: String,
    pub name: String,
    pub version: (u32, u32, u32),
    pub license: License,
}

impl LicenseNotice {
    /// What to tell the user, in the app named `app`: "Solarized
    /// (acme.solarized) is licensed GPL-3.0-only. Notes doesn't ship it;
    /// you installed it."
    pub fn message(&self, app: &str) -> String {
        format!(
            "{} ({}) is licensed {}. {app} doesn't ship it; you installed it.",
            self.name, self.id, self.license.expr
        )
    }

    /// Its entry in [`ACKNOWLEDGED_FILE`]: `id@version`.
    fn key(&self) -> String {
        let (major, minor, patch) = self.version;
        format!("{}@{major}.{minor}.{patch}", self.id)
    }
}

/// A small SPDX licence expression parser, after the grammar in SPDX 2.3
/// annex D. Expressions are short: their length and nesting are capped, and
/// `AND` and `OR` chains are flat, so nothing recurses deeply.
mod spdx {
    use super::LicenseClass;

    /// The longest expression read, in bytes.
    const MAX_LEN: usize = 1024;

    /// The deepest parentheses nest.
    const MAX_DEPTH: usize = 16;

    /// Licences kiln's own dependencies may have (deny.toml's
    /// `[licenses] allow`).
    const PERMISSIVE: &[&str] = &[
        "MIT",
        "MIT-0",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "ISC",
        "Zlib",
        "0BSD",
        "BSL-1.0",
        "Unlicense",
        "Unicode-3.0",
        "bzip2-1.0.6",
        "CC0-1.0",
    ];

    const COPYLEFT: &[&str] = &[
        "GPL-", "LGPL-", "AGPL-", "MPL-", "EPL-", "CDDL-", "EUPL-", "OSL-",
    ];

    pub(super) enum Expr {
        License {
            id: String,
            plus: bool,
            exception: Option<String>,
        },
        And(Vec<Expr>),
        Or(Vec<Expr>),
    }

    impl Expr {
        pub(super) fn class(&self) -> LicenseClass {
            match self {
                Expr::License { id, .. } => {
                    if PERMISSIVE
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(id))
                    {
                        LicenseClass::Permissive
                    } else if COPYLEFT
                        .iter()
                        .any(|prefix| starts_with_ignoring_case(id, prefix))
                    {
                        LicenseClass::Copyleft
                    } else {
                        LicenseClass::Unknown
                    }
                }
                Expr::And(terms) => terms
                    .iter()
                    .map(Expr::class)
                    .min()
                    .unwrap_or(LicenseClass::Unknown),
                Expr::Or(terms) => terms
                    .iter()
                    .map(Expr::class)
                    .max()
                    .unwrap_or(LicenseClass::Unknown),
            }
        }

        pub(super) fn allowed_by(&self, allowed: &[String]) -> bool {
            match self {
                Expr::License {
                    id,
                    plus,
                    exception,
                } => {
                    let plus = if *plus { "+" } else { "" };
                    let written = match exception {
                        None => format!("{id}{plus}"),
                        Some(exception) => format!("{id}{plus} WITH {exception}"),
                    };
                    allowed.iter().any(|allowed| {
                        let allowed = allowed.split_whitespace().collect::<Vec<_>>().join(" ");
                        allowed.eq_ignore_ascii_case(&written)
                    })
                }
                Expr::And(terms) => terms.iter().all(|term| term.allowed_by(allowed)),
                Expr::Or(terms) => terms.iter().any(|term| term.allowed_by(allowed)),
            }
        }
    }

    fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
        text.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Token<'a> {
        Open,
        Close,
        /// A run of ID characters, and whether `+` follows it directly.
        Word(&'a str, bool),
    }

    const OPERATORS: [&str; 3] = ["AND", "OR", "WITH"];

    fn tokens(text: &str) -> Result<Vec<Token<'_>>, String> {
        let mut tokens = Vec::new();
        let mut rest = text;
        loop {
            rest = rest.trim_start();
            let Some(ch) = rest.chars().next() else {
                return Ok(tokens);
            };
            let (token, len) = match ch {
                '(' => (Token::Open, 1),
                ')' => (Token::Close, 1),
                '+' => return Err("`+` must follow a licence ID directly".into()),
                ch if is_word_char(ch) => {
                    let len = rest.find(|ch| !is_word_char(ch)).unwrap_or(rest.len());
                    let plus = rest[len..].starts_with('+');
                    let end = len + usize::from(plus);
                    // `GPL-2.0+OR MIT`: what follows is set apart from the
                    // `+` as from an ID, by a space or a parenthesis.
                    if plus && rest[end..].starts_with(|ch| is_word_char(ch) || ch == '+') {
                        return Err(format!(
                            "`{}` must be followed by a space, a parenthesis or the end",
                            &rest[..end]
                        ));
                    }
                    (Token::Word(&rest[..len], plus), end)
                }
                ch => return Err(format!("{ch:?} can't be in a licence expression")),
            };
            tokens.push(token);
            rest = &rest[len..];
        }
    }

    fn is_word_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':')
    }

    /// SPDX's `idstring`: letters, digits, `-` and `.`.
    fn is_idstring(text: &str) -> bool {
        !text.is_empty()
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    }

    /// Why `word` isn't a licence ID or reference, if it isn't.
    fn license_problem(word: &str, plus: bool) -> Option<String> {
        let reference = |text: &str| {
            starts_with_ignoring_case(text, "LicenseRef-")
                && is_idstring(&text["LicenseRef-".len()..])
        };
        let valid = match word.split_once(':') {
            Some((document, license)) => {
                starts_with_ignoring_case(document, "DocumentRef-")
                    && is_idstring(&document["DocumentRef-".len()..])
                    && reference(license)
            }
            None if starts_with_ignoring_case(word, "LicenseRef-") => reference(word),
            None => is_idstring(word),
        };
        if !valid {
            return Some(format!("`{word}` isn't a licence ID"));
        }
        let is_reference = word.contains(':') || starts_with_ignoring_case(word, "LicenseRef-");
        (plus && is_reference).then(|| format!("`{word}+`: a licence reference takes no `+`"))
    }

    struct Parser<'a> {
        tokens: Vec<Token<'a>>,
        at: usize,
        depth: usize,
    }

    pub(super) fn parse(text: &str) -> Result<Expr, String> {
        if text.len() > MAX_LEN {
            return Err(format!("it's longer than {MAX_LEN} bytes"));
        }
        let mut parser = Parser {
            tokens: tokens(text)?,
            at: 0,
            depth: 0,
        };
        if parser.tokens.is_empty() {
            return Err("it's empty".into());
        }
        let expr = parser.or()?;
        match parser.next() {
            None => Ok(expr),
            token => Err(format!("unexpected {}", describe(token))),
        }
    }

    fn describe(token: Option<Token<'_>>) -> String {
        match token {
            None => "end".into(),
            Some(Token::Open) => "`(`".into(),
            Some(Token::Close) => "`)`".into(),
            Some(Token::Word(word, false)) => format!("`{word}`"),
            Some(Token::Word(word, true)) => format!("`{word}+`"),
        }
    }

    impl<'a> Parser<'a> {
        fn next(&mut self) -> Option<Token<'a>> {
            let token = self.tokens.get(self.at).copied();
            self.at += 1;
            token
        }

        /// Takes the next token if it's the operator `name`.
        fn operator(&mut self, name: &str) -> bool {
            let found = self.tokens.get(self.at) == Some(&Token::Word(name, false));
            if found {
                self.at += 1;
            }
            found
        }

        fn or(&mut self) -> Result<Expr, String> {
            let mut terms = vec![self.and()?];
            while self.operator("OR") {
                terms.push(self.and()?);
            }
            Ok(match terms.len() {
                1 => terms.remove(0),
                _ => Expr::Or(terms),
            })
        }

        fn and(&mut self) -> Result<Expr, String> {
            let mut terms = vec![self.term()?];
            while self.operator("AND") {
                terms.push(self.term()?);
            }
            Ok(match terms.len() {
                1 => terms.remove(0),
                _ => Expr::And(terms),
            })
        }

        fn term(&mut self) -> Result<Expr, String> {
            match self.next() {
                Some(Token::Open) => {
                    self.depth += 1;
                    if self.depth > MAX_DEPTH {
                        return Err(format!("parentheses nest more than {MAX_DEPTH} deep"));
                    }
                    let expr = self.or()?;
                    self.depth -= 1;
                    match self.next() {
                        Some(Token::Close) => Ok(expr),
                        token => Err(format!("expected `)`, found {}", describe(token))),
                    }
                }
                Some(Token::Word(word, plus)) if !OPERATORS.contains(&word) => {
                    if let Some(problem) = license_problem(word, plus) {
                        return Err(problem);
                    }
                    let exception = if self.operator("WITH") {
                        match self.next() {
                            Some(Token::Word(word, false))
                                if !OPERATORS.contains(&word) && is_idstring(word) =>
                            {
                                Some(word.to_owned())
                            }
                            token => {
                                return Err(format!(
                                    "expected an exception ID after WITH, found {}",
                                    describe(token)
                                ));
                            }
                        }
                    } else {
                        None
                    };
                    Ok(Expr::License {
                        id: word.to_owned(),
                        plus,
                        exception,
                    })
                }
                token => Err(format!("expected a licence ID, found {}", describe(token))),
            }
        }
    }
}
