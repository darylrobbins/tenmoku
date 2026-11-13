//! Dynamic registration (docs/decisions/lsp-client.md §3.6).
//!
//! A successful reply to `client/registerCapability` means the
//! registration is installed, so it accepts only methods whose effect
//! fe-lsp implements:
//!
//! - `workspace/didChangeConfiguration`, which is sent whenever settings
//!   change, whatever is registered (its `dynamicRegistration` is false,
//!   but servers register it anyway);
//! - `workspace/didChangeWatchedFiles`, whose `registerOptions`
//!   (`DidChangeWatchedFilesRegistrationOptions`) name the globs and kinds
//!   of the file changes the server wants (watched_files.rs). Its options
//!   must be present and parse, and each watcher's glob must build
//!   ([`LspGlob`]); a watcher's kind defaults to create | change | delete;
//! - `textDocument/onTypeFormatting`, whose `registerOptions`
//!   (`DocumentOnTypeFormattingRegistrationOptions`) name the trigger
//!   characters and the documents they're for. Its options must be present
//!   and parse, and each filter's glob must build; a null selector is every
//!   document of the server. A trigger is a string's first character.
//!
//! Any other method, or a watched-files registration breaking that rule,
//! refuses the whole request (registration is all or nothing) with
//! `RequestFailed`, naming the methods, and records nothing.
//! `client/unregisterCapability` removes the ids it names, and their
//! watchers; an unknown id is a no-op.
//!
//! Beside those, a session holds its spec's root-scoped static watch
//! ([`StaticWatch`]: `ServerSpec::watch_files` and `watch_excludes`, or
//! the lists their `_from` pointers find in its settings) as a permanent
//! watched-files registration, which no id names and nothing unregisters
//! (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §2.4).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::notification::{DidChangeConfiguration, DidChangeWatchedFiles, Notification};
use lsp_types::request::{OnTypeFormatting, Request};
use lsp_types::{
    DidChangeWatchedFilesRegistrationOptions, DocumentOnTypeFormattingRegistrationOptions,
    FileChangeType, GlobPattern, Registration, RegistrationParams, UnregistrationParams, Url,
    WatchKind,
};

use crate::glob::LspGlob;
use crate::launcher::ServerSpec;

/// The methods a server may register.
const SUPPORTED: [&str; 3] = [
    DidChangeConfiguration::METHOD,
    DidChangeWatchedFiles::METHOD,
    OnTypeFormatting::METHOD,
];

/// An accepted registration's method, with what it registered.
enum Registered {
    Configuration,
    /// Each watcher's glob and the kinds of change it wants.
    WatchedFiles(Vec<(LspGlob, WatchKind)>),
    /// The documents it's for (`None` for all of the server's), and the
    /// characters that ask for it there.
    OnTypeFormatting {
        selector: Option<Vec<Filter>>,
        triggers: Vec<char>,
    },
}

/// One `DocumentFilter`: a document matches if it has every part given.
struct Filter {
    language: Option<String>,
    scheme: Option<String>,
    pattern: Option<LspGlob>,
}

impl Filter {
    fn matches(&self, uri: &Url, language_id: &str) -> bool {
        self.language
            .as_ref()
            .is_none_or(|language| language == language_id)
            && self
                .scheme
                .as_ref()
                .is_none_or(|scheme| scheme == uri.scheme())
            && self
                .pattern
                .as_ref()
                .is_none_or(|pattern| uri.to_file_path().is_ok_and(|path| pattern.matches(&path)))
    }
}

/// The characters of an on-type formatting provider's or registration's
/// `firstTriggerCharacter` and `moreTriggerCharacter`: each string's first,
/// in order, once each.
pub(crate) fn trigger_chars(first: &str, more: Option<&[String]>) -> Vec<char> {
    let mut chars = Vec::new();
    let strings = std::iter::once(first).chain(more.unwrap_or_default().iter().map(String::as_str));
    for ch in strings.filter_map(|string| string.chars().next()) {
        if !chars.contains(&ch) {
            chars.push(ch);
        }
    }
    chars
}

/// A spec's root-scoped static watch: the changes of files under `root`
/// that match one of `files` and neither they nor a folder above them (up
/// to the root) match one of `excludes`, both globs relative to the root.
/// Every kind of change counts.
pub(crate) struct StaticWatch {
    root: PathBuf,
    files: Vec<LspGlob>,
    excludes: Vec<LspGlob>,
}

impl StaticWatch {
    /// The watch of `spec` running at `root` with `settings`, or `None` if
    /// it watches nothing. Each list is the one its `_from` pointer finds
    /// in `settings` when set, else the spec's own. A list that isn't an
    /// array of strings is empty. A files glob that doesn't build is left
    /// out; an exclude that doesn't build leaves no watch at all, rather
    /// than one wider than meant. Each failure is logged.
    pub(crate) fn new(
        spec: &ServerSpec,
        root: &Path,
        settings: &serde_json::Value,
    ) -> Option<Self> {
        let name = &spec.name;
        let files = globs(
            name,
            &spec.watch_files,
            spec.watch_files_from.as_deref(),
            settings,
        );
        let files: Vec<LspGlob> = (files.iter())
            .filter_map(|glob| {
                (compile(glob))
                    .inspect_err(|err| log::warn!("{name}: watch_files: {err}"))
                    .ok()
            })
            .collect();
        if files.is_empty() {
            return None;
        }
        let excludes = globs(
            name,
            &spec.watch_excludes,
            spec.watch_excludes_from.as_deref(),
            settings,
        );
        let excludes = (excludes.iter())
            .map(|glob| compile(glob))
            .collect::<Result<Vec<_>, _>>()
            .inspect_err(|err| log::warn!("{name}: watch_excludes: {err}; watching nothing"))
            .ok()?;
        Some(Self {
            root: root.to_owned(),
            files,
            excludes,
        })
    }

    /// Whether the change of `path` is one it watches. A path with `.` or
    /// `..` in it under the root never is. The globs see the path's parts
    /// joined by `/`, whatever the platform's separator, so `**/dist`
    /// leaves out `src\dist\a.css` on Windows as `src/dist/a.css` here.
    fn watches(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let parts: Option<Vec<_>> = (relative.components())
            .map(|part| match part {
                Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .collect();
        let Some(parts) = parts.filter(|parts| !parts.is_empty()) else {
            return false;
        };
        let under = |count: usize| PathBuf::from(parts[..count].join("/"));
        self.files
            .iter()
            .any(|glob| glob.matches(&under(parts.len())))
            && !(1..=parts.len())
                .any(|count| self.excludes.iter().any(|glob| glob.matches(&under(count))))
    }
}

/// The globs a watch list takes: those at `pointer` in `settings` when it's
/// set, else `own`.
fn globs(
    name: &str,
    own: &[String],
    pointer: Option<&str>,
    settings: &serde_json::Value,
) -> Vec<String> {
    let Some(pointer) = pointer else {
        return own.to_vec();
    };
    let found = settings.pointer(pointer).and_then(|value| {
        (value.as_array()?.iter())
            .map(|glob| glob.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
    });
    found.unwrap_or_else(|| {
        log::warn!("{name}: the settings at {pointer} aren't a list of globs");
        Vec::new()
    })
}

/// A glob relative to a root, matched against paths relative to it. A
/// leading `./` is dropped, as fast-glob does.
fn compile(glob: &str) -> Result<LspGlob, String> {
    let glob = glob.strip_prefix("./").unwrap_or(glob);
    LspGlob::new(&GlobPattern::String(glob.to_owned()))
}

/// A session's accepted registrations, by registration id, and its static
/// watch.
#[derive(Default)]
pub(crate) struct Registrations {
    by_id: BTreeMap<String, Registered>,
    fixed: Option<StaticWatch>,
}

impl Registrations {
    /// None accepted yet, with `watch` as the static watch.
    pub(crate) fn with_watch(watch: Option<StaticWatch>) -> Self {
        Self {
            by_id: BTreeMap::new(),
            fixed: watch,
        }
    }

    /// Replaces the static watch (its settings changed).
    pub(crate) fn set_watch(&mut self, watch: Option<StaticWatch>) {
        self.fixed = watch;
    }

    pub(crate) fn register(&mut self, params: RegistrationParams) -> Result<(), ResponseError> {
        let mut refused = Vec::new();
        let mut accepted = Vec::new();
        for registration in params.registrations {
            if !SUPPORTED.contains(&registration.method.as_str()) {
                refused.push(registration.method);
                continue;
            }
            match registered(&registration) {
                Ok(registered) => accepted.push((registration.id, registered)),
                Err(why) => refused.push(format!("{} ({why})", registration.method)),
            }
        }
        if !refused.is_empty() {
            return Err(ResponseError::new(
                ErrorCode::REQUEST_FAILED,
                format!("Tenmoku can't register {}", refused.join(", ")),
            ));
        }
        self.by_id.extend(accepted);
        Ok(())
    }

    pub(crate) fn unregister(&mut self, params: UnregistrationParams) {
        for unregistration in params.unregisterations {
            self.by_id.remove(&unregistration.id);
        }
    }

    /// Whether a watched-files registration, or the static watch, wants a
    /// change of `typ` to `path`.
    pub(crate) fn watches(&self, path: &Path, typ: FileChangeType) -> bool {
        let kind = match typ {
            FileChangeType::CREATED => WatchKind::Create,
            FileChangeType::CHANGED => WatchKind::Change,
            FileChangeType::DELETED => WatchKind::Delete,
            _ => return false,
        };
        self.fixed.as_ref().is_some_and(|watch| watch.watches(path))
            || (self.by_id.values())
                .flat_map(|registered| match registered {
                    Registered::Configuration | Registered::OnTypeFormatting { .. } => &[][..],
                    Registered::WatchedFiles(watchers) => watchers,
                })
                .any(|(glob, wanted)| wanted.contains(kind) && glob.matches(path))
    }

    /// The characters the on-type formatting registrations for the
    /// document at `uri`, in `language_id`, are asked for by, in the order
    /// registered (by id), once each.
    pub(crate) fn on_type_triggers(&self, uri: &Url, language_id: &str) -> Vec<char> {
        let mut chars = Vec::new();
        for registered in self.by_id.values() {
            let Registered::OnTypeFormatting { selector, triggers } = registered else {
                continue;
            };
            let selected = (selector.as_ref())
                .is_none_or(|filters| filters.iter().any(|f| f.matches(uri, language_id)));
            if selected {
                for &ch in triggers {
                    if !chars.contains(&ch) {
                        chars.push(ch);
                    }
                }
            }
        }
        chars
    }

    /// The registration ids, in order.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> Vec<String> {
        self.by_id.keys().cloned().collect()
    }
}

/// What a registration of a supported method registers.
fn registered(registration: &Registration) -> Result<Registered, String> {
    if registration.method == DidChangeConfiguration::METHOD {
        return Ok(Registered::Configuration);
    }
    let options = registration
        .register_options
        .clone()
        .ok_or("no registerOptions")?;
    if registration.method == OnTypeFormatting::METHOD {
        return on_type_formatting(options);
    }
    let options: DidChangeWatchedFilesRegistrationOptions =
        serde_json::from_value(options).map_err(|err| format!("unreadable options: {err}"))?;
    let watchers = options
        .watchers
        .iter()
        .map(|watcher| {
            let glob = LspGlob::new(&watcher.glob_pattern)?;
            Ok((glob, watcher.kind.unwrap_or(WatchKind::all())))
        })
        .collect::<Result<_, String>>()?;
    Ok(Registered::WatchedFiles(watchers))
}

/// What an on-type formatting registration's `options` register.
fn on_type_formatting(options: serde_json::Value) -> Result<Registered, String> {
    let options: DocumentOnTypeFormattingRegistrationOptions =
        serde_json::from_value(options).map_err(|err| format!("unreadable options: {err}"))?;
    let selector = (options.document_selector)
        .map(|filters| {
            (filters.into_iter())
                .map(|filter| {
                    let pattern = (filter.pattern)
                        .map(|glob| LspGlob::new(&GlobPattern::String(glob)))
                        .transpose()?;
                    Ok(Filter {
                        language: filter.language,
                        scheme: filter.scheme,
                        pattern,
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    let triggers = trigger_chars(
        &options.first_trigger_character,
        options.more_trigger_character.as_deref(),
    );
    Ok(Registered::OnTypeFormatting { selector, triggers })
}
