// Adapted from gpui-kit (https://github.com/longbridge/gpui-kit) at commit
// 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, file
// crates/base/src/input/editor/diagnostics.rs.
// Copyright 2024 - 2026 Longbridge. Licensed under the Apache License,
// Version 2.0 (see THIRD_PARTY.md).
//
// Modified for fastedit: entries hold fe-buffer anchors over byte ranges
// rather than `lsp_types::Position` ranges read through a rope, so they move
// with edits; the char-column `position_to_offset` is gone, as are
// `lsp_types` and the sum tree. Diagnostics are kept per server, applied
// only for the server's active identity and rebased from the buffer version
// they were computed on, and cleared by identity.

//! A buffer's diagnostics, from each server that publishes them
//! (docs/decisions/lsp-client.md §4.1, "Delivery to the UI").
//!
//! The set doesn't know what a server or an identity is: the host keys it
//! by its own types. Each server's diagnostics come whole, from one
//! identity (a language server's session and incarnation, say), and
//! replace the ones it had:
//!
//! - [`DiagnosticSet::apply`] takes them only if the identity is the one
//!   the host says is active, and isn't one the set has seen cleared. Their
//!   ranges, bytes of the buffer version they were computed on, are rebased
//!   onto the current text with [`BufferSnapshot::map_offset`] and kept as
//!   anchors, so they move with later edits. A version below the edit
//!   log's floor can't be rebased, and changes nothing.
//! - [`DiagnosticSet::clear`] empties a server's diagnostics if they came
//!   from the identity it names, and remembers that identity as cleared.

use std::{
    any::Any,
    ops::{Deref, Range},
};

use fe_buffer::{Anchor, Bias, BufferSnapshot, Version};

/// How severe a diagnostic is, least first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Hint,
    Information,
    Warning,
    Error,
}

/// How a diagnostic's text may be shown besides its severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticTag {
    /// Unused or unreachable code.
    Unnecessary,
    Deprecated,
}

/// What a diagnostic says. Its range is kept beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// What produced it, as `rustc`.
    pub source: Option<String>,
    pub code: Option<String>,
    pub tags: Vec<DiagnosticTag>,
}

impl Diagnostic {
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            source: None,
            code: None,
            tags: Vec::new(),
        }
    }
}

/// A diagnostic and its byte range in a snapshot's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticEntry<'a> {
    pub range: Range<usize>,
    pub diagnostic: &'a Diagnostic,
}

impl Deref for DiagnosticEntry<'_> {
    type Target = Diagnostic;

    fn deref(&self) -> &Diagnostic {
        self.diagnostic
    }
}

/// Where a range's ends go when text is inserted right at them: inside
/// neither, so a diagnostic doesn't grow over what's typed beside it.
const START: Bias = Bias::Right;
const END: Bias = Bias::Left;

#[derive(Clone, Debug)]
struct Entry {
    range: Range<Anchor>,
    diagnostic: Diagnostic,
}

/// One server's diagnostics.
#[derive(Clone, Debug)]
struct ServerSet<S, I> {
    server: S,
    /// The identity the entries came from, while there are any.
    from: Option<I>,
    /// The last identity seen cleared.
    cleared: Option<I>,
    /// Sorted by start; their anchors are all of `version`.
    entries: Vec<Entry>,
    version: Version,
}

/// The diagnostics of one buffer, per server `S`, each from an identity
/// `I`.
#[derive(Clone, Debug)]
pub struct DiagnosticSet<S, I> {
    servers: Vec<ServerSet<S, I>>,
}

impl<S, I> Default for DiagnosticSet<S, I> {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
        }
    }
}

impl<S: Copy + Eq, I: Copy + Eq> DiagnosticSet<S, I> {
    pub fn new() -> Self {
        Self::default()
    }

    fn server_mut(&mut self, server: S) -> &mut ServerSet<S, I> {
        let at = match self.servers.iter().position(|set| set.server == server) {
            Some(at) => at,
            None => {
                self.servers.push(ServerSet {
                    server,
                    from: None,
                    cleared: None,
                    entries: Vec::new(),
                    version: Version::default(),
                });
                self.servers.len() - 1
            }
        };
        &mut self.servers[at]
    }

    fn server(&self, server: S) -> Option<&ServerSet<S, I>> {
        self.servers.iter().find(|set| set.server == server)
    }

    /// Replaces `server`'s diagnostics with `items`, from `identity`, their
    /// ranges in the text of buffer version `version`, which `snapshot` is
    /// at or after. Returns whether it did: not unless `identity` is
    /// `active` and hasn't been seen cleared, nor if `version` is below the
    /// snapshot's edit-log floor (or after the snapshot).
    pub fn apply(
        &mut self,
        snapshot: &BufferSnapshot,
        server: S,
        identity: I,
        version: Version,
        items: Vec<(Range<usize>, Diagnostic)>,
        active: Option<I>,
    ) -> bool {
        if active != Some(identity)
            || self.server(server).and_then(|set| set.cleared) == Some(identity)
            || version < snapshot.oldest_version()
            || version > snapshot.version()
        {
            return false;
        }
        let mut entries: Vec<(Range<usize>, Diagnostic)> = items
            .into_iter()
            .map(|(range, diagnostic)| {
                let start = snapshot.map_offset(range.start, START, version);
                let end = snapshot.map_offset(range.end, END, version).max(start);
                (start..end, diagnostic)
            })
            .collect();
        entries.sort_by_key(|(range, _)| (range.start, range.end));
        let set = self.server_mut(server);
        set.entries = entries
            .into_iter()
            .map(|(range, diagnostic)| Entry {
                range: snapshot.anchor_at(range.start, START)..snapshot.anchor_at(range.end, END),
                diagnostic,
            })
            .collect();
        set.from = Some(identity);
        set.version = snapshot.version();
        true
    }

    /// `identity` of `server` has ended: empties its diagnostics if they
    /// came from it, and from now on [`apply`](Self::apply) rejects it.
    /// Returns whether it emptied them.
    pub fn clear(&mut self, server: S, identity: I) -> bool {
        let set = self.server_mut(server);
        set.cleared = Some(identity);
        if set.from != Some(identity) {
            return false;
        }
        set.from = None;
        set.entries.clear();
        true
    }

    /// The identity `server`'s diagnostics came from, while it has any.
    pub fn identity(&self, server: S) -> Option<I> {
        self.server(server)?.from
    }

    /// `server`'s diagnostics in `snapshot`, which is at or after the
    /// version the set was last applied or refreshed at, by start.
    pub fn server_entries(&self, snapshot: &BufferSnapshot, server: S) -> Vec<DiagnosticEntry<'_>> {
        self.server(server)
            .map_or_else(Vec::new, |set| resolve(snapshot, set))
    }

    /// Rebases every anchor onto `snapshot`, so resolving them stays cheap
    /// however many edits follow. Diagnostics whose anchors the log has
    /// been compacted past are dropped.
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        for set in &mut self.servers {
            if set.version == snapshot.version() || set.entries.is_empty() {
                continue;
            }
            if set.version < snapshot.oldest_version() {
                set.entries.clear();
                set.from = None;
                continue;
            }
            let anchors: Vec<Anchor> = (set.entries.iter())
                .flat_map(|entry| [entry.range.start, entry.range.end])
                .collect();
            let offsets = snapshot.resolve_many(&anchors);
            for (entry, ends) in set.entries.iter_mut().zip(offsets.chunks(2)) {
                let end = ends[1].max(ends[0]);
                entry.range = snapshot.anchor_at(ends[0], START)..snapshot.anchor_at(end, END);
            }
            set.version = snapshot.version();
        }
    }
}

impl<S, I> DiagnosticSet<S, I> {
    pub fn is_empty(&self) -> bool {
        self.servers.iter().all(|set| set.entries.is_empty())
    }

    pub fn len(&self) -> usize {
        self.servers.iter().map(|set| set.entries.len()).sum()
    }

    /// Every server's diagnostics in `snapshot`, which is at or after the
    /// version the set was last applied or refreshed at, by start.
    pub fn entries(&self, snapshot: &BufferSnapshot) -> Vec<DiagnosticEntry<'_>> {
        let mut entries: Vec<DiagnosticEntry<'_>> = (self.servers.iter())
            .flat_map(|set| resolve(snapshot, set))
            .collect();
        entries.sort_by_key(|entry| (entry.range.start, entry.range.end));
        entries
    }
}

fn resolve<'a, S, I>(
    snapshot: &BufferSnapshot,
    set: &'a ServerSet<S, I>,
) -> Vec<DiagnosticEntry<'a>> {
    if set.entries.is_empty() || set.version < snapshot.oldest_version() {
        return Vec::new();
    }
    let anchors: Vec<Anchor> = (set.entries.iter())
        .flat_map(|entry| [entry.range.start, entry.range.end])
        .collect();
    let offsets = snapshot.resolve_many(&anchors);
    (set.entries.iter().zip(offsets.chunks(2)))
        .map(|(entry, ends)| DiagnosticEntry {
            range: ends[0]..ends[1].max(ends[0]),
            diagnostic: &entry.diagnostic,
        })
        .collect()
}

/// A [`DiagnosticSet`] of any key types, as an
/// [`EditorModel`](crate::EditorModel) holds it.
pub(crate) trait AnyDiagnosticSet: Any {
    fn entries<'a>(&'a self, snapshot: &BufferSnapshot) -> Vec<DiagnosticEntry<'a>>;
    fn refresh(&mut self, snapshot: &BufferSnapshot);
    fn boxed_clone(&self) -> Box<dyn AnyDiagnosticSet>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<S: Copy + Eq + 'static, I: Copy + Eq + 'static> AnyDiagnosticSet for DiagnosticSet<S, I> {
    fn entries<'a>(&'a self, snapshot: &BufferSnapshot) -> Vec<DiagnosticEntry<'a>> {
        DiagnosticSet::entries(self, snapshot)
    }

    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        DiagnosticSet::refresh(self, snapshot);
    }

    fn boxed_clone(&self) -> Box<dyn AnyDiagnosticSet> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
