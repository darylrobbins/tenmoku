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
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_net::l10n::RESOURCES).check();
}

#[test]
fn counts_are_numbers_and_english_keeps_its_wording() {
    use fe_net::GrammarFetchError;
    for max_bytes in [1_u64, 2, 16 << 20] {
        assert_eq!(
            GrammarFetchError::TooLarge { max_bytes }.to_string(),
            format!("the download is over the limit of {max_bytes} bytes")
        );
    }
    assert_eq!(
        GrammarFetchError::TooManyRedirects.to_string(),
        "the download redirected more than 3 times"
    );
}
