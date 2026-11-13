//! fastedit's messages, in `l10n/<locale>/*.ftl`: see `kiln_l10n`.
//!
//! - `cli.ftl` holds `--help` and the command line's errors, in the system
//!   locale (the settings aren't read yet).
//! - `app.ftl` holds the updater, speller, theme, grammar install and
//!   shell-command messages.

kiln_l10n::resources! {
    "en-US" => [
        "../l10n/en-US/cli.ftl",
        "../l10n/en-US/app.ftl",
    ],
}
