// SPDX-License-Identifier: Apache-2.0

//! The literal lint (kiln-l10n): no hard-coded user-facing text in this
//! crate's non-test source outside `l10n-allow.toml`.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_syntax::l10n::RESOURCES).check();
}

#[test]
fn english_keeps_its_wording_for_counts_and_clashes() {
    use fe_syntax::{Clash, GrammarError};
    let too_large = GrammarError::TooLarge {
        file: "g.wasm".into(),
        bytes: 1,
    };
    assert_eq!(
        too_large.to_string(),
        "g.wasm: 1 bytes; a grammar's files may have at most 20971520"
    );
    let clash = |clash| GrammarError::Clash {
        language: "foo".into(),
        built_in: "Rust",
        clash,
    };
    assert_eq!(
        clash(Clash::Name).to_string(),
        "foo: its name is the built-in Rust's"
    );
    assert_eq!(
        clash(Clash::Extension("rs".into())).to_string(),
        "foo: the extension \"rs\" is the built-in Rust's"
    );
    assert_eq!(
        clash(Clash::FileName("Makefile".into())).to_string(),
        "foo: the file name \"Makefile\" is the built-in Rust's"
    );
    assert_eq!(
        clash(Clash::Interpreter("sh".into())).to_string(),
        "foo: the interpreter \"sh\" is the built-in Rust's"
    );
}
