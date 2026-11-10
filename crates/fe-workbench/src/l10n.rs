//! fe-workbench's catalogue files (`docs/decisions/xte5e7hqsw-i18n-design.md`
//! § 3). Each area's card adds its own `l10n/en-US/<area>.ftl` to the list.

kiln_l10n::resources! {
    dynamic_prefixes = ["command-"];
    "en-US" => [
        "../l10n/en-US/commands.ftl",
        "../l10n/en-US/git.ftl",
        "../l10n/en-US/navigation.ftl",
        "../l10n/en-US/find.ftl",
        "../l10n/en-US/setup.ftl",
        "../l10n/en-US/chrome.ftl",
        "../l10n/en-US/run.ftl",
    ],
}
