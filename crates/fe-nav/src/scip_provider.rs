//! [`ScipNavigation`]: a [`NavigationProvider`] answering exactly from a
//! [`ScipIndex`], with no language server running.
//!
//! The index was made from the files as they were when it was written, so
//! every site is checked against the text it lands in: the request's
//! occurrence against the request's text, which may have unsaved edits, and
//! every other site against its file as the editor would load it now. A
//! request whose caret isn't on an indexed occurrence of its word is
//! [`NavError::Unavailable`], so the next provider (search) answers; a site
//! whose file changed is dropped.

use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::{Bias, Buffer, BufferId, Text, Version};
use fe_syntax::{OutlineKind, language_name_for_path};
use scip::types::symbol_information::Kind;

use crate::{
    Definition, Definitions, FileReferences, Location, NavError, NavRequest, NavigationProvider,
    Precision, Readiness, Reference, ReferencesSummary, ScipIndex, ScipOccurrence, ScipSite, Tier,
    WorkspaceSymbol,
};

/// The largest file read to check its sites, as references search loads
/// files.
const MAX_LOAD_BYTES: usize = 32 << 20;

/// How much of a long line either side of a reference goes to its preview;
/// [`Reference::preview`] cuts it down further.
const PREVIEW_CONTEXT: usize = 1024;

/// How many symbols workspace symbols looks at between checks for a cancel.
const CANCEL_EVERY: usize = 1024;

/// Answers go-to-definition, references and workspace symbols from a SCIP
/// index of the project at `root`.
///
/// [`Readiness::Ready`] for a language some indexed document is in, and,
/// for workspace symbols (no language), when the index has any global
/// symbol. It never reports [`Readiness::NotYet`]: an index is either
/// loaded or not, and a host registers this provider once it is.
pub struct ScipNavigation {
    index: Arc<ScipIndex>,
    root: PathBuf,
    /// The indexed documents' languages, as fe-syntax names them.
    languages: HashSet<String>,
    /// [`ScipIndex::has_global_symbols`].
    has_globals: bool,
    /// Called with each file a request reads, before it's read.
    #[cfg(test)]
    on_load: Arc<dyn Fn(&Path) + Send + Sync>,
}

impl ScipNavigation {
    /// Its [`NavigationProvider::name`].
    pub const NAME: &'static str = "scip";

    /// A provider answering from `index`, which was made of the project at
    /// `root`.
    pub fn new(index: Arc<ScipIndex>, root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Files are opened under the root without following a symlink, so a
        // root reached through one is resolved first.
        let root = fs::canonicalize(&root).unwrap_or(root);
        let languages = index.languages().map(syntax_name).collect();
        let has_globals = index.has_global_symbols();
        Self {
            index,
            root,
            languages,
            has_globals,
            #[cfg(test)]
            on_load: Arc::new(|_| {}),
        }
    }

    #[cfg(test)]
    fn with_on_load(mut self, on_load: impl Fn(&Path) + Send + Sync + 'static) -> Self {
        self.on_load = Arc::new(on_load);
        self
    }

    /// The indexed occurrence at the request's caret, the word it is, and
    /// the name its symbol's sites are spelt with: only when its bytes in the
    /// request's text are the word at the caret, and that word is the
    /// symbol's name or an alias of it the index records there.
    fn occurrence(&self, req: &NavRequest) -> Result<(ScipOccurrence, String, String), NavError> {
        let unindexed = || NavError::Unavailable("not in the SCIP index".to_owned());
        let word = req.word().ok_or_else(unindexed)?;
        let mut occurrence = (self.index)
            .occurrence_at(&req.rel, &req.text, req.offset)
            .ok_or_else(unindexed)?;
        if !spells(&req.text, &occurrence.range, &word, dollar(req.language)) {
            return Err(unindexed());
        }
        // An alias's own (local) symbol on the span of the global one it
        // names, in the same document: the request is for the global one.
        if scip::symbol::is_local_symbol(&occurrence.symbol)
            && let Some(global) = (self.index.occurrences_on(&occurrence.site, &req.text))
                .into_iter()
                .find(|other| {
                    other.scope == occurrence.scope && !scip::symbol::is_local_symbol(&other.symbol)
                })
        {
            occurrence = global;
        }
        let symbol = &occurrence.symbol;
        let name = self.name_of(symbol).unwrap_or_else(|| word.clone());
        // A definition renamed in place, or a use renamed with no alias the
        // index knows of, is a changed file.
        if word != name
            && (occurrence.definition
                || !self.alias_at(Some(req), &occurrence.site, &req.text, &word, symbol))
        {
            return Err(unindexed());
        }
        Ok((occurrence, word, name))
    }

    /// The kind the index gives the global `symbol`, if it's one an outline
    /// has.
    fn kind(&self, symbol: &str) -> Option<OutlineKind> {
        (self.index.symbol_info(symbol)).and_then(|info| outline_kind(info.kind))
    }

    /// The name the global `symbol`'s sites are spelt with: its display
    /// name, or else its last descriptor's. `None` for a local, whose symbol
    /// is a number: its name is the word at the caret.
    fn name_of(&self, symbol: &str) -> Option<String> {
        if scip::symbol::is_local_symbol(symbol) {
            return None;
        }
        let display_name = (self.index.symbol_info(symbol)).map(|info| &*info.display_name);
        symbol_name(symbol, display_name.unwrap_or(""))
    }

    /// `site`'s bytes in `text`, its file's text, if they spell the global
    /// or local `symbol`: `name`, or, for a site without the Definition role
    /// of a global symbol, an alias the index records there
    /// ([`ScipNavigation::alias_at`]). Otherwise the file changed since it
    /// was indexed. `req` is the request, whose text a site in its own file
    /// is in.
    fn accept(
        &self,
        req: &NavRequest,
        site: &ScipSite,
        text: &Text,
        symbol: &str,
        name: &str,
        is_definition: bool,
    ) -> Option<Range<usize>> {
        let range = site.byte_range(text)?;
        let dollar = self.dollar_at(&site.rel, Some(req));
        if spells(text, &range, name, dollar) {
            return Some(range);
        }
        if is_definition || scip::symbol::is_local_symbol(symbol) {
            return None;
        }
        // The site must be a whole identifier too, not the start of a longer
        // one the alias's use has grown into.
        let spelt = text.slice_to_string(range.clone());
        if !spells(text, &range, &spelt, dollar) {
            return None;
        }
        (self.alias_at(Some(req), site, text, &spelt, symbol)).then_some(range)
    }

    /// Whether the index records `spelt` as an alias of `symbol` at `site`,
    /// a site of it in the file whose text is `text`: another symbol (the
    /// alias's own, as an import alias's local) occurs on exactly that span,
    /// in the same document as `symbol`'s occurrence there, and is defined
    /// in the same file at a site that reads `spelt` too. A global alias
    /// symbol must also be named `spelt` in the index. The occurrence on the
    /// span is the indexer saying the alias is in scope there; the name and
    /// the definition's bytes, that it's still spelt so. `req` is the
    /// request, if any, whose text a site in its own file is in.
    fn alias_at(
        &self,
        req: Option<&NavRequest>,
        site: &ScipSite,
        text: &Text,
        spelt: &str,
        symbol: &str,
    ) -> bool {
        let dollar = self.dollar_at(&site.rel, req);
        let on = self.index.occurrences_on(site, text);
        let scopes: Vec<u32> = (on.iter())
            .filter(|occurrence| *occurrence.symbol == *symbol)
            .map(|occurrence| occurrence.scope)
            .collect();
        (on.iter())
            .filter(|alias| *alias.symbol != *symbol && scopes.contains(&alias.scope))
            .filter(|alias| self.name_of(&alias.symbol).is_none_or(|name| name == spelt))
            .any(|alias| {
                (self.index.definitions_of(alias).iter())
                    .filter(|declared| declared.rel == site.rel)
                    .any(|declared| site_range(declared, text, &[spelt], dollar).is_some())
            })
    }

    /// Whether `$` is an identifier char at a site in `rel`. In the file of
    /// `req`, the request, whose text the site is checked in: by the
    /// request's language, as [`NavRequest::word`] has it. Elsewhere: by the
    /// language the index gives its document, or, when it gives none, the
    /// one its file name has (so an extensionless script indexed as
    /// JavaScript has it).
    fn dollar_at(&self, rel: &Path, req: Option<&NavRequest>) -> bool {
        if let Some(req) = req
            && *rel == *req.rel
        {
            return dollar(req.language);
        }
        match self
            .index
            .language(rel)
            .filter(|language| !language.is_empty())
        {
            Some(language) => dollar(Some(&syntax_name(language))),
            None => dollar(language_name_for_path(rel)),
        }
    }
}

impl fmt::Debug for ScipNavigation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScipNavigation")
            .field("root", &self.root)
            .field("languages", &self.languages)
            .finish_non_exhaustive()
    }
}

impl NavigationProvider for ScipNavigation {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, language: Option<&str>) -> Readiness {
        let ready = match language {
            Some(language) => self.languages.contains(language),
            None => self.has_globals,
        };
        if ready {
            Readiness::Ready
        } else {
            Readiness::Unsupported
        }
    }

    fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError> {
        check(cancel)?;
        let (occurrence, word, name) = self.occurrence(req)?;
        let kind = self.kind(&occurrence.symbol);
        let mut texts = Texts::new(self, Some(req));
        let mut items = Vec::new();
        let mut last: Option<&Path> = None;
        for site in self.index.definitions_of(&occurrence) {
            if last != Some(&*site.rel) {
                check(cancel)?;
                last = Some(&site.rel);
            }
            // A definition is spelt by its symbol's name: never an alias.
            let Some(location) = texts.locate(site, &[name.as_str()]) else {
                continue;
            };
            // A site the index has twice (in two documents for its file) is
            // listed once.
            if (items.iter()).any(|item: &Definition| item.location == location) {
                continue;
            }
            let tier = if location.rel == req.rel {
                Tier::SameFile
            } else {
                Tier::Project
            };
            items.push(Definition {
                location,
                name: name.clone(),
                kind,
                container: None,
                tier,
            });
        }
        check(cancel)?;
        if items.is_empty() {
            return Err(NavError::Unavailable(
                "no definition in the SCIP index matches its file".to_owned(),
            ));
        }
        // The request's own file first; otherwise in the index's order.
        items.sort_by_key(|item| item.tier);
        Ok(Definitions {
            word: Some(word),
            unambiguous: items.len() == 1,
            items,
            partial: false,
        })
    }

    fn references(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        check(cancel)?;
        let (occurrence, word, name) = self.occurrence(req)?;
        let mut sites: Vec<(&ScipSite, bool)> = (self.index.definitions_of(&occurrence).iter())
            .map(|site| (site, true))
            .chain((self.index.references_of(&occurrence).iter()).map(|site| (site, false)))
            .collect();
        // The request's own file first, then by path. Each file's sites are
        // put in order once resolved: documents for one file may count
        // columns in different encodings.
        let own = |site: &ScipSite| *site.rel != *req.rel;
        sites.sort_by(|(a, _), (b, _)| (own(a), &a.rel).cmp(&(own(b), &b.rel)));
        let mut texts = Texts::new(self, Some(req));
        let mut summary = ReferencesSummary {
            word: Some(word),
            ..ReferencesSummary::default()
        };
        let mut delivered = false;
        for file in sites.chunk_by(|(a, _), (b, _)| a.rel == b.rel) {
            if cancel.load(Ordering::Relaxed) {
                return cancelled(summary, delivered);
            }
            summary.files_searched += 1;
            let rel = &file[0].0.rel;
            let Some(text) = texts.get(rel) else {
                continue;
            };
            summary.files_verified += 1;
            let snapshot = (**rel == *req.rel).then_some(req.buffer).flatten();
            let symbol = &occurrence.symbol;
            let mut found: Vec<(Range<usize>, bool)> = (file.iter())
                .filter_map(|&(site, is_definition)| {
                    let range = self.accept(req, site, text, symbol, &name, is_definition)?;
                    Some((range, is_definition))
                })
                .collect();
            found.sort_by_key(|(range, _)| (range.start, range.end));
            // A site the index has twice is listed once, a definition if
            // either says so.
            found.dedup_by(|(range, is_definition), (kept, kept_definition)| {
                let same = range == kept;
                if same {
                    *kept_definition |= *is_definition;
                }
                same
            });
            let refs: Vec<Reference> = (found.into_iter())
                .map(|(range, is_definition)| reference(rel, text, range, snapshot, is_definition))
                .collect();
            // Each file is visited once: don't hold its text until the end.
            texts.forget(rel);
            if refs.is_empty() {
                continue;
            }
            // Reading the file may have taken a while.
            if cancel.load(Ordering::Relaxed) {
                return cancelled(summary, delivered);
            }
            sink(vec![FileReferences {
                rel: rel.to_path_buf(),
                refs,
            }]);
            delivered = true;
        }
        if cancel.load(Ordering::Relaxed) {
            return cancelled(summary, delivered);
        }
        Ok(summary)
    }

    fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        check(cancel)?;
        // As search has it: an empty query matches nothing.
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut candidates: Vec<(WorkspaceSymbol, &ScipSite)> = Vec::new();
        for (step, (symbol, info)) in self.index.symbols().enumerate() {
            if step % CANCEL_EVERY == 0 {
                check(cancel)?;
            }
            let Some(kind) = outline_kind(info.kind) else {
                continue;
            };
            let Some(site) = self.index.definitions(symbol).first() else {
                continue;
            };
            let Some(name) = symbol_name(symbol, &info.display_name) else {
                continue;
            };
            let symbol = WorkspaceSymbol {
                // Resolved once ranked, for the few returned.
                location: Location {
                    rel: site.rel.to_path_buf(),
                    range: 0..0,
                    row: site.start.0,
                    snapshot: None,
                },
                name,
                kind,
                container: None,
                indices: Vec::new(),
            };
            candidates.push((symbol, site));
        }
        // The index's symbols come in no particular order; equal scores keep
        // this one.
        candidates.sort_by(|(a, a_site), (b, b_site)| {
            (&a.name, &a_site.rel, a_site.start).cmp(&(&b.name, &b_site.rel, b_site.start))
        });
        // Each candidate's place in `candidates` rides in its range, so the
        // ranked symbols find their sites again.
        let (symbols, sites): (Vec<WorkspaceSymbol>, Vec<&ScipSite>) = (candidates.into_iter())
            .enumerate()
            .map(|(at, (mut symbol, site))| {
                symbol.location.range = at..at;
                (symbol, site)
            })
            .unzip();
        let ranked = WorkspaceSymbol::rank(query, symbols, limit);
        check(cancel)?;
        // Only the best `limit` are resolved: one whose file changed is
        // left out, not replaced, so a stale index reads no more files.
        let mut texts = Texts::new(self, None);
        let mut found = Vec::new();
        // Names the query doesn't match come last, unhighlighted.
        for mut symbol in ranked.into_iter().take_while(|s| !s.indices.is_empty()) {
            check(cancel)?;
            let site = sites[symbol.location.range.start];
            let Some(location) = texts.locate(site, &[symbol.name.as_str()]) else {
                continue;
            };
            symbol.location = location;
            found.push(symbol);
        }
        check(cancel)?;
        Ok(found)
    }
}

/// The files a request reads, each at most once: the request's own from its
/// text, the rest as the editor loads them.
struct Texts<'a> {
    nav: &'a ScipNavigation,
    req: Option<&'a NavRequest>,
    /// `None` for a file that couldn't be read.
    loaded: HashMap<Arc<Path>, Option<Text>>,
}

impl<'a> Texts<'a> {
    fn new(nav: &'a ScipNavigation, req: Option<&'a NavRequest>) -> Self {
        Self {
            nav,
            req,
            loaded: HashMap::new(),
        }
    }

    /// `rel`'s text, or `None` if it can't be read.
    fn get(&mut self, rel: &Arc<Path>) -> Option<&Text> {
        if let Some(req) = self.req
            && **rel == *req.rel
        {
            return Some(&req.text);
        }
        let nav = self.nav;
        (self.loaded.entry(rel.clone()))
            .or_insert_with(|| {
                #[cfg(test)]
                (nav.on_load)(rel);
                load(&nav.root, rel)
            })
            .as_ref()
    }

    /// Drops `rel`'s loaded text.
    fn forget(&mut self, rel: &Path) {
        self.loaded.remove(rel);
    }

    /// Where `site` is, if its file can be read and the site's bytes there
    /// are one of `names`.
    fn locate(&mut self, site: &ScipSite, names: &[impl AsRef<str>]) -> Option<Location> {
        let snapshot = match self.req {
            Some(req) if *site.rel == *req.rel => req.buffer,
            _ => None,
        };
        let dollar = self.nav.dollar_at(&site.rel, self.req);
        let range = site_range(site, self.get(&site.rel)?, names, dollar)?;
        Some(Location {
            rel: site.rel.to_path_buf(),
            range,
            row: site.start.0,
            snapshot,
        })
    }
}

/// `root/rel` read as the editor's loader reads it, if it's a regular file
/// of at most [`MAX_LOAD_BYTES`]. Opened with [`fe_project::open_file_under`],
/// so a path in the index can't lead out of the project.
fn load(root: &Path, rel: &Path) -> Option<Text> {
    let file = fe_project::open_file_under(root, rel).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_LOAD_BYTES as u64 {
        return None;
    }
    // One byte past the limit tells a file that grew past it.
    let mut raw = Vec::new();
    (file.take(MAX_LOAD_BYTES as u64 + 1))
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() > MAX_LOAD_BYTES {
        return None;
    }
    let buffer = Buffer::from_reader_detecting(raw.as_slice()).ok()?;
    Some(buffer.text().clone())
}

/// `site`'s bytes in `text`, if they spell one of `names` ([`spells`], with
/// `$` an identifier char if `dollar`): otherwise the file changed since it
/// was indexed.
fn site_range(
    site: &ScipSite,
    text: &Text,
    names: &[impl AsRef<str>],
    dollar: bool,
) -> Option<Range<usize>> {
    let range = site.byte_range(text)?;
    (names.iter())
        .any(|name| spells(text, &range, name.as_ref(), dollar))
        .then_some(range)
}

/// Whether `range` of `text` is `name`, whole: a name starting or ending
/// with an identifier char mustn't have one before or after it, or the
/// identifier there is another, longer one (`foo` grown into `foobar`, or
/// into `foó` by a combining mark). Identifier chars are the search's
/// ([`fe_project::is_word_char`]), with `$` where `dollar` says so, as
/// [`NavRequest::word`] has them.
fn spells(text: &Text, range: &Range<usize>, name: &str, dollar: bool) -> bool {
    if range.len() != name.len() || text.slice_to_string(range.clone()) != name {
        return false;
    }
    let before = || {
        let from = text.clip_offset(range.start.saturating_sub(4), Bias::Left);
        text.slice_to_string(from..range.start).chars().next_back()
    };
    let after = || {
        let to = text.clip_offset((range.end + 4).min(text.len()), Bias::Right);
        text.slice_to_string(range.end..to).chars().next()
    };
    let is_word = |ch: char| fe_project::is_word_char(ch) || (dollar && ch == '$');
    let starts_word = name.chars().next().is_some_and(is_word);
    let ends_word = name.chars().next_back().is_some_and(is_word);
    !(starts_word && before().is_some_and(is_word)) && !(ends_word && after().is_some_and(is_word))
}

/// Whether `$` is an identifier char in `language`, by fe-syntax's name: in
/// the JavaScript family, as [`NavRequest::word`] has it.
fn dollar(language: Option<&str>) -> bool {
    matches!(language, Some("javascript" | "typescript" | "tsx"))
}

/// A reference at `range`, bytes in `text`, `rel`'s text.
fn reference(
    rel: &Path,
    text: &Text,
    range: Range<usize>,
    snapshot: Option<(BufferId, Version)>,
    is_definition: bool,
) -> Reference {
    let row = text.offset_to_point(range.start).row;
    let line_start = text.row_start(row);
    let line_end = line_start + text.row_len(row) as usize;
    // No more of a long line than the preview can show.
    let from =
        (text.clip_offset(range.start.saturating_sub(PREVIEW_CONTEXT), Bias::Left)).max(line_start);
    let to = (text.clip_offset(range.end + PREVIEW_CONTEXT, Bias::Right)).min(line_end);
    let line = text.slice_to_string(from..to);
    let (preview, preview_ranges) = Reference::preview(&line, range.start - from..range.end - from);
    Reference {
        location: Location {
            rel: rel.to_path_buf(),
            range,
            row,
            snapshot,
        },
        line_number: u64::from(row) + 1,
        preview,
        preview_ranges,
        verified: true,
        is_definition,
    }
}

/// How a references request ends on a cancel: as the trait has it, an error
/// before any batch went to the sink, the summary after.
fn cancelled(
    mut summary: ReferencesSummary,
    delivered: bool,
) -> Result<ReferencesSummary, NavError> {
    if !delivered {
        return Err(NavError::Cancelled);
    }
    summary.cancelled = true;
    Ok(summary)
}

fn check(cancel: &AtomicBool) -> Result<(), NavError> {
    if cancel.load(Ordering::Relaxed) {
        Err(NavError::Cancelled)
    } else {
        Ok(())
    }
}

/// A SCIP document's language as fe-syntax names it: lowercased, with the
/// names that differ mapped.
fn syntax_name(language: &str) -> String {
    let language = language.to_lowercase();
    match language.as_str() {
        "typescriptreact" => "tsx".to_owned(),
        "javascriptreact" => "javascript".to_owned(),
        "c++" => "cpp".to_owned(),
        "c#" => "csharp".to_owned(),
        "shell" => "bash".to_owned(),
        _ => language,
    }
}

/// The name a global symbol goes by: its display name, or else its last
/// descriptor's.
fn symbol_name(symbol: &str, display_name: &str) -> Option<String> {
    if !display_name.is_empty() {
        return Some(display_name.to_owned());
    }
    let parsed = scip::symbol::parse_symbol(symbol).ok()?;
    let name = &parsed.descriptors.last()?.name;
    (!name.is_empty()).then(|| name.clone())
}

/// The outline kind a SCIP kind names, if any: parameters, literals and the
/// like aren't definitions an outline lists.
fn outline_kind(kind: Kind) -> Option<OutlineKind> {
    Some(match kind {
        Kind::Module | Kind::Namespace | Kind::Package | Kind::PackageObject => OutlineKind::Module,
        Kind::Class | Kind::SingletonClass | Kind::Object => OutlineKind::Class,
        Kind::Struct | Kind::Union => OutlineKind::Struct,
        Kind::Enum => OutlineKind::Enum,
        Kind::Interface | Kind::Protocol => OutlineKind::Interface,
        Kind::Trait | Kind::TypeClass => OutlineKind::Trait,
        Kind::Type
        | Kind::TypeAlias
        | Kind::AssociatedType
        | Kind::TypeFamily
        | Kind::DataFamily => OutlineKind::Type,
        Kind::Function => OutlineKind::Function,
        Kind::Method
        | Kind::AbstractMethod
        | Kind::Constructor
        | Kind::StaticMethod
        | Kind::SingletonMethod
        | Kind::TraitMethod
        | Kind::ProtocolMethod
        | Kind::PureVirtualMethod
        | Kind::TypeClassMethod
        | Kind::MethodSpecification
        | Kind::MethodAlias
        | Kind::Getter
        | Kind::Setter
        | Kind::Accessor => OutlineKind::Method,
        Kind::Constant => OutlineKind::Constant,
        Kind::Macro => OutlineKind::Macro,
        Kind::Field
        | Kind::StaticField
        | Kind::Property
        | Kind::StaticProperty
        | Kind::StaticDataMember => OutlineKind::Field,
        Kind::EnumMember => OutlineKind::Variant,
        Kind::Variable | Kind::StaticVariable => OutlineKind::Variable,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Mutex};

    use protobuf::{EnumOrUnknown, Message};
    use scip::types::{Document, Index, Occurrence, PositionEncoding, SymbolInformation};

    use super::*;

    const FOO: &str = "rust-analyzer cargo demo 0.1.0 foo().";
    const LIB: &str = "pub fn foo() {}\n";
    const MAIN: &str = "fn main() { foo() }\n";

    /// `foo` defined in src/lib.rs and called in src/main.rs, written under
    /// a tempdir, and a provider that sets `cancel` as it reads lib.rs.
    fn project(cancel: Arc<AtomicBool>) -> (tempfile::TempDir, ScipNavigation, Arc<Mutex<usize>>) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), LIB).unwrap();
        fs::write(root.join("src/main.rs"), MAIN).unwrap();
        let occ = |range: &[i32], roles: i32| Occurrence {
            range: range.to_vec(),
            symbol: FOO.to_owned(),
            symbol_roles: roles,
            ..Default::default()
        };
        let doc = |rel: &str, occurrences| Document {
            language: "rust".to_owned(),
            relative_path: rel.to_owned(),
            position_encoding: EnumOrUnknown::new(
                PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
            ),
            occurrences,
            ..Default::default()
        };
        let mut lib = doc("src/lib.rs", vec![occ(&[0, 7, 10], 1)]);
        lib.symbols.push(SymbolInformation {
            symbol: FOO.to_owned(),
            display_name: "foo".to_owned(),
            kind: EnumOrUnknown::new(Kind::Function),
            ..Default::default()
        });
        let main = doc("src/main.rs", vec![occ(&[0, 12, 15], 0)]);
        let bytes = Index {
            documents: vec![lib, main],
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap();
        let index = Arc::new(ScipIndex::from_bytes(&bytes).unwrap());
        let loads = Arc::new(Mutex::new(0));
        let counted = loads.clone();
        let nav = ScipNavigation::new(index, root).with_on_load(move |rel| {
            *counted.lock().unwrap() += 1;
            if rel == Path::new("src/lib.rs") {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        (dir, nav, loads)
    }

    fn request(root: &Path) -> NavRequest {
        NavRequest {
            root: root.to_path_buf(),
            rel: PathBuf::from("src/main.rs"),
            language: Some("rust"),
            text: Text::new(MAIN),
            offset: 13,
            buffer: None,
        }
    }

    #[test]
    fn a_cancel_while_a_file_is_read_sends_nothing_more() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (dir, nav, loads) = project(cancel.clone());
        let mut batches = Vec::new();
        let summary = nav
            .references(&request(dir.path()), &cancel, &mut |batch| {
                batches.push(batch);
            })
            .unwrap();
        // main.rs, from the request's text, went before lib.rs was read.
        assert!(summary.cancelled);
        assert_eq!(*loads.lock().unwrap(), 1);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0][0].rel, Path::new("src/main.rs"));
    }

    #[test]
    fn a_cancel_while_resolving_ends_definitions_and_workspace_symbols() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (dir, nav, _) = project(cancel.clone());
        assert_eq!(
            nav.definitions(&request(dir.path()), &cancel),
            Err(NavError::Cancelled)
        );
        cancel.store(false, Ordering::Relaxed);
        assert_eq!(
            nav.workspace_symbols("foo", 10, &cancel),
            Err(NavError::Cancelled)
        );

        // Uncancelled, both answer.
        let (dir, nav, _) = project(Arc::new(AtomicBool::new(false)));
        let cancel = AtomicBool::new(false);
        assert_eq!(
            nav.definitions(&request(dir.path()), &cancel)
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(nav.workspace_symbols("foo", 10, &cancel).unwrap().len(), 1);
    }
}
