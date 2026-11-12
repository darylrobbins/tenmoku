//! The user's own snippets, from `<config_dir>/snippets/` in VS Code's
//! snippet file format ([`fe_config::user_snippets`]), offered as
//! completions as extensions' are ([`crate::extension_snippets`]), detail
//! "User snippet", before an extension's that matches as well, and listed
//! by Insert Snippet ([`crate::snippet_picker`]).
//!
//! - **Read** when first asked for, and read again whenever they're asked
//!   for after the directory's snippet files changed on disk
//!   ([`DirStamp`]): a file written, created, replaced or deleted takes
//!   effect at the next completion request or Insert Snippet, with no
//!   restart. Asking only stats the directory's snippet files.
//! - **Checked** as extensions' are: a body that doesn't parse
//!   ([`snippet_body_error`]), like an entry with no prefix or no body, is
//!   dropped with a warning naming its file and name, logged once per read;
//!   the rest load.
//! - **Shared** by every window, as the config directory is.

use std::{path::PathBuf, rc::Rc};

use fe_config::user_snippets::{DirStamp, UserSnippet, load_snippets, snippets_dir};
use gpui::{App, Global};

use crate::{
    extension_snippets::{Offer, SnippetText, user_snippet},
    snippet_body_error,
};

/// The user's snippets as last read, and the directory's stamp then.
#[derive(Default)]
struct Loaded {
    dir: Option<PathBuf>,
    stamp: DirStamp,
    set: UserSnippetSet,
}

/// One read of the user's snippets: each with its text, and their
/// completions' offers, all from the same files.
#[derive(Clone, Default)]
pub(crate) struct UserSnippetSet {
    pub(crate) snippets: Rc<[UserSnippet]>,
    /// Each of `snippets`' text, by index, shared with its offers.
    pub(crate) texts: Rc<[Rc<SnippetText>]>,
    /// One for each prefix, built once a read.
    pub(crate) offers: Rc<[Offer]>,
}

impl Global for Loaded {}

/// `<config_dir>/snippets`, if there's a config directory.
pub(crate) fn user_snippets_dir(cx: &App) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(over) = cx.try_global::<crate::open::ConfigDirOverride>() {
        return over.0.as_deref().map(snippets_dir);
    }
    // A test with no override has no user config, not the developer's.
    #[cfg(test)]
    return None;
    #[cfg(not(test))]
    {
        let _ = cx;
        fe_config::config_dir().map(|dir| snippets_dir(&dir))
    }
}

/// The user's snippets as their files are now: those last read, unless
/// the files changed since, when they're read again. Everything in it is
/// from one read.
pub(crate) fn user_snippet_set(cx: &mut App) -> UserSnippetSet {
    current(cx).set.clone()
}

/// [`user_snippet_set`]'s offers for completions: one for each prefix,
/// their text shared, built when they were read.
pub(crate) fn user_offers(cx: &mut App) -> Rc<[Offer]> {
    current(cx).set.offers.clone()
}

fn current(cx: &mut App) -> &Loaded {
    let dir = user_snippets_dir(cx);
    // Taken before reading, so a write while reading reads again next time.
    let stamp = dir.as_deref().map(DirStamp::of).unwrap_or_default();
    let loaded = cx.default_global::<Loaded>();
    if loaded.dir == dir && loaded.stamp == stamp {
        return loaded;
    }
    let read = match &dir {
        Some(dir) => load_snippets(dir, &snippet_body_error),
        None => Default::default(),
    };
    for diagnostic in &read.diagnostics {
        if diagnostic.is_error() {
            log::error!("user snippets: {diagnostic}");
        } else {
            log::warn!("user snippets: {diagnostic}");
        }
    }
    let texts: Rc<[Rc<SnippetText>]> = (read.snippets.iter())
        .map(|snippet| SnippetText::new(&snippet.body, snippet.description.as_deref()))
        .collect();
    let detail = user_snippet();
    let offers = (read.snippets.iter().zip(texts.iter()))
        .flat_map(|(snippet, text)| {
            let (prefixes, languages) = (&snippet.prefixes, &snippet.languages);
            Offer::each_prefix(prefixes, text.clone(), languages, &detail, true)
        })
        .collect();
    *loaded = Loaded {
        dir,
        stamp,
        set: UserSnippetSet {
            snippets: read.snippets.into(),
            texts,
            offers,
        },
    };
    loaded
}
