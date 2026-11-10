// SPDX-License-Identifier: Apache-2.0

//! kiln's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists, other crates' public;
//! then, in en-US, each message as the text it replaced, byte for byte.

use kiln_l10n::{set_locales, tr};

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &kiln::l10n::RESOURCES).check();
}

/// Each message, formatted as kiln formats it, against the text kiln
/// wrote before it was a message.
#[test]
fn en_us_messages_are_unchanged() {
    set_locales(&[]);
    let messages = [
        (
            tr!(
                "kiln-theme-wrong-appearance",
                id = "ocean",
                appearance = "light",
                slot = "dark"
            ),
            "theme ocean is light, so it can't be the dark theme",
        ),
        (
            tr!(
                "kiln-theme-wrong-appearance",
                id = "ocean",
                appearance = "dark",
                slot = "light"
            ),
            "theme ocean is dark, so it can't be the light theme",
        ),
        (
            tr!("kiln-theme-not-installed", id = "ocean"),
            "theme ocean isn't installed",
        ),
        (
            tr!(
                "kiln-keys-bound-twice",
                keys = "cmd-k",
                other = "notes.a",
                command = "notes.b"
            ),
            "cmd-k is bound to both notes.a and notes.b",
        ),
        (
            tr!("kiln-command-takes-no-args", command = "notes.a"),
            "notes.a takes no args",
        ),
        (
            tr!("kiln-menu-item-unknown-command", command = "notes.a"),
            "unknown command notes.a; the item is left out",
        ),
        (
            tr!(
                "kiln-menu-item-bad-args",
                command = "notes.a",
                reason = "it takes none"
            ),
            "bad args for notes.a: it takes none; the item is left out",
        ),
        (
            tr!("kiln-menu-repeated", menu = "\"File\""),
            "menu \"File\" is repeated",
        ),
        (
            tr!(
                "kiln-menu-id-repeated",
                id = "\"notes.a\"",
                menu = "\"File\""
            ),
            "id \"notes.a\" is repeated in menu \"File\"",
        ),
        (
            tr!(
                "kiln-extension-theme-id-taken",
                id = "ocean",
                extension = "acme.themes"
            ),
            "`ocean` is the ID of another of acme.themes's themes, which loads instead",
        ),
        (tr!("kiln-extension-notice-unnamed-app"), "The app"),
        (
            tr!(
                "kiln-extension-workspace-untrusted",
                root = "/projects/notes"
            ),
            "/projects/notes isn't trusted",
        ),
        (tr!("kiln-extension-file-missing"), "no such file"),
        (
            tr!(
                "kiln-extension-file-unreadable",
                reason = "permission denied"
            ),
            "can't be read: permission denied",
        ),
        (
            tr!(
                "kiln-extension-file-leads-outside",
                target = "/etc/passwd",
                extension = "acme.themes"
            ),
            "leads to /etc/passwd, outside acme.themes's directory, so it isn't read",
        ),
        (
            tr!("kiln-extension-cant-unbind", keys = "cmd-k"),
            "an extension can't unbind keys: `command = \"\"` for cmd-k is ignored",
        ),
        (
            tr!(
                "kiln-extension-cant-remove-item",
                id = "\"notes.a\"",
                menu = "\"File\""
            ),
            "an extension can't remove menu items: the removal of \"notes.a\" from \"File\" is ignored",
        ),
        (
            tr!(
                "kiln-extension-cant-replace-item",
                id = "\"notes.a\"",
                menu = "\"File\""
            ),
            "\"File\" already has an item \"notes.a\"; an extension can't replace it, so this one is ignored",
        ),
    ];
    for (message, english) in messages {
        assert_eq!(message, english);
    }
}
