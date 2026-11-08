// SPDX-License-Identifier: Apache-2.0

//! kiln-shell's catalogue (kiln-l10n). Its command titles are keys derived
//! from their ids, `command-<id>`, which kiln-command's resolver reads.

kiln_l10n::resources! {
    dynamic_prefixes = ["command-"];
    "en-US" => ["../l10n/en-US/kiln-shell.ftl"],
}
