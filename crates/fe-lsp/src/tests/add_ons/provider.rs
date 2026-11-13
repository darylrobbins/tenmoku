//! Provider routing (§5, §7 Routing): a feature only one server answers
//! goes to the lead if it advertises it, else to the first add-on that
//! does, and to nobody when none does.

use fe_buffer::Buffer;
use lsp_types::{FoldingRangeProviderCapability, OneOf, ServerCapabilities};
use serde_json::json;

use super::{Fixture, answer};
use crate::Feature;
use crate::fake::{Reply, Scripted};
use crate::formatting::FormatOptions;

const FOLDING: &str = "textDocument/foldingRange";
const SYMBOLS: &str = "textDocument/documentSymbol";
const FORMATTING: &str = "textDocument/formatting";
const OPTIONS: FormatOptions = FormatOptions {
    tab_size: 2,
    insert_spaces: true,
};

fn folds(capabilities: &mut ServerCapabilities) {
    capabilities.folding_range_provider = Some(FoldingRangeProviderCapability::Simple(true));
}

fn formats(capabilities: &mut ServerCapabilities) {
    capabilities.document_formatting_provider = Some(OneOf::Left(true));
}

/// One fold, from row `start` to row 2.
fn fold_from(f: &Fixture, name: &str, start: u32) {
    f.fake(name).set_reply(
        FOLDING,
        Scripted::now(Reply::Result(json!([{ "startLine": start, "endLine": 2 }]))),
    );
}

#[test]
fn primary_only_feature_goes_to_add_on_when_primary_lacks_it() {
    // The primary formats but doesn't fold; the add-on folds but doesn't
    // format.
    let mut f = Fixture::css_and_vars_with(formats, folds);
    let mut buffer = Buffer::new("a {\n  b: c;\n}\n");
    f.open_on_both(&mut buffer);
    let (css, vars) = (
        f.placed(&buffer, "css").unwrap(),
        f.placed(&buffer, "vars").unwrap(),
    );
    let (id, version) = (buffer.id(), buffer.version());
    assert_eq!(f.manager.provider_of(id, Feature::Folding), Some(vars));
    assert_eq!(f.manager.provider_of(id, Feature::Formatting), Some(css));
    fold_from(&f, "vars", 0);
    fold_from(&f, "css", 1);

    let found = answer(f.manager.folding_ranges(id, version)).unwrap();
    assert_eq!(
        (found.unwrap().iter())
            .map(|fold| (fold.start_row, fold.end_row))
            .collect::<Vec<_>>(),
        [(0, 2)],
        "the add-on's folds"
    );
    assert_eq!(f.asked("vars", FOLDING).len(), 1);
    assert!(f.asked("css", FOLDING).is_empty());

    // The lead keeps what it provides: formatting goes to it alone.
    f.fake("css")
        .set_reply(FORMATTING, Scripted::now(Reply::Result(json!([]))));
    let edits = answer(f.manager.format(id, version, None, OPTIONS));
    assert_eq!(edits, Ok(Some(Vec::new())));
    assert_eq!(f.asked("css", FORMATTING).len(), 1);
    assert!(f.asked("vars", FORMATTING).is_empty());
}

#[test]
fn nobody_asked_when_neither_has_the_provider() {
    let mut f = Fixture::css_and_vars_with(|_| {}, |_| {});
    let mut buffer = Buffer::new("a {\n  b: c;\n}\n");
    f.open_on_both(&mut buffer);
    let (id, version) = (buffer.id(), buffer.version());
    for feature in [
        Feature::Folding,
        Feature::DocumentSymbols,
        Feature::Formatting,
    ] {
        assert_eq!(f.manager.provider_of(id, feature), None, "{feature:?}");
    }

    assert_eq!(answer(f.manager.folding_ranges(id, version)), Ok(None));
    assert_eq!(answer(f.manager.document_symbols(id, version)), Ok(None));
    let edits = answer(f.manager.format(id, version, None, OPTIONS));
    assert_eq!(edits, Ok(None));
    for name in ["css", "vars"] {
        for method in [FOLDING, SYMBOLS, FORMATTING] {
            assert!(f.asked(name, method).is_empty(), "{name} got {method}");
        }
    }
}
