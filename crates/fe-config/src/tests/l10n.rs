//! The generated catalogue files: `settings-template.ftl` and the fake
//! locale's are goldens (`settings.ftl` is checked beside the schema's, in
//! `settings::schema_matches_golden`), and every generated message formats
//! back to its source text byte for byte.

use std::{fs, path::PathBuf};

use kiln_l10n::{Args, Catalogue, LanguageIdentifier};

use crate::l10n::generate::{fake_marker, fake_template_ftl, has_path, settings_ftl, template_ftl};
use crate::l10n::template::{Line, Template};
use crate::l10n::{KeyKind, setting_key, variant_attribute};
use crate::settings_model::{SettingKind, rows};
use crate::{USER_SETTINGS_TEMPLATE, settings_schema, user_settings_template};

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
}

/// Checks `generated` against the file at `rel`, rewriting it first under
/// `UPDATE_GOLDEN`.
pub(super) fn golden(rel: &str, generated: &str) {
    // Read at run time: a shared target can hold a test binary built for
    // another checkout.
    let path = manifest_dir().join(rel);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, generated).unwrap();
    }
    let on_disk = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == generated,
        "{rel} is out of date: rerun with UPDATE_GOLDEN=1 and review the diff"
    );
}

/// An en-US catalogue of `source` alone.
fn catalogue(source: &str) -> Catalogue {
    let en: LanguageIdentifier = "en-US".parse().unwrap();
    let catalogue = Catalogue::from_sources(
        [(
            "en-US".to_owned(),
            "generated".to_owned(),
            source.to_owned(),
        )],
        &[en],
    );
    assert_eq!(catalogue.errors(), [] as [String; 0]);
    catalogue
}

#[test]
fn template_ftl_matches_golden() {
    let schema = settings_schema();
    let ftl = template_ftl(USER_SETTINGS_TEMPLATE, &schema)
        .unwrap_or_else(|errors| panic!("{}", errors.join("\n")));
    golden("l10n/en-US/settings-template.ftl", &ftl);
    golden(
        "src/tests/fixtures/qps-ploc/settings-template.ftl",
        &fake_template_ftl(USER_SETTINGS_TEMPLATE),
    );
}

#[test]
fn template_messages_format_to_their_blocks() {
    let template = Template::parse(USER_SETTINGS_TEMPLATE);
    let ftl = template_ftl(USER_SETTINGS_TEMPLATE, &settings_schema()).unwrap();
    let catalogue = catalogue(&ftl);
    assert!(template.blocks.len() > 90, "{}", template.blocks.len());
    for block in &template.blocks {
        let formatted = catalogue.try_format(&block.key, &Args::new()).unwrap();
        assert_eq!(formatted.errors, [] as [String; 0], "{}", block.key);
        assert_eq!(formatted.text, template.text(block), "{}", block.key);
    }
}

#[test]
fn settings_messages_format_to_the_schema() {
    let schema = settings_schema();
    let catalogue = catalogue(&settings_ftl(&schema).unwrap());
    let walk = rows(&schema);
    let args = Args::new();
    let mut checked = 0;
    for entry in &walk.entries {
        let key = setting_key(KeyKind::Description, &entry.key_path());
        if !entry.description.is_empty() {
            assert_eq!(catalogue.tr_or(&key, ""), entry.description, "{key}");
            checked += 1;
        }
        if let SettingKind::Enum(variants) = &entry.kind {
            for variant in variants.iter().filter(|v| !v.description.is_empty()) {
                let name = variant_attribute(&variant.value);
                assert_eq!(
                    catalogue.attribute(&key, &name, &args).as_deref(),
                    Some(variant.description.as_str()),
                    "{key}.{name}"
                );
                checked += 1;
            }
        }
    }
    for group in &walk.groups {
        let key = setting_key(KeyKind::Description, &group.key.join("."));
        if !group.description.is_empty() {
            assert_eq!(catalogue.tr_or(&key, ""), group.description, "{key}");
        }
        for name in &group.key {
            let key = setting_key(KeyKind::Group, name);
            assert!(catalogue.has_message(&key), "{key}");
        }
    }
    assert!(checked > 150, "{checked} descriptions");
    assert_eq!(
        catalogue.tr_or("settings-group-status_bar", ""),
        "Status Bar"
    );
    assert_eq!(catalogue.tr_or("settings-group-lsp", ""), "LSP");
}

#[test]
fn keys_join_the_path_with_dashes() {
    assert_eq!(
        setting_key(KeyKind::Description, "workbench.preview_tabs"),
        "settings-workbench-preview_tabs"
    );
    assert_eq!(
        setting_key(KeyKind::Template, "workbench.preview_tabs"),
        "settings-template-workbench-preview_tabs"
    );
    assert_eq!(
        setting_key(KeyKind::Group, "zen_mode"),
        "settings-group-zen_mode"
    );
}

#[test]
fn blocks_are_keyed_by_their_toml_context() {
    let template = Template::parse(USER_SETTINGS_TEMPLATE);
    let keys: Vec<&str> = template.blocks.iter().map(|b| b.key.as_str()).collect();
    for key in [
        "settings-template-intro",
        "settings-template-editor-font_family",
        "settings-template-editor-font_size",
        "settings-template-vim-remap",
        "settings-template-vim-which_key_group",
        "settings-template-workbench-zen_mode-hide_tabs",
        "settings-template-extensions-disabled",
        "settings-template-extensions-plugins_preview",
        "settings-template-formatters-typescript",
    ] {
        assert!(keys.contains(&key), "{key} in {keys:?}");
    }
    assert_eq!(keys[0], "settings-template-intro");
    // The intro's own `#` line is inside its block; the `#` after
    // `# disabled = []` and before `# [formatters.typescript]` aren't.
    let intro = &template.blocks[0];
    assert_eq!(
        template.text(intro),
        "Tenmoku settings.\n\nThis file reloads when you save it. A project's \
         `.tenmoku/settings.toml`\noverrides it, key by key. Uncomment a line to \
         change that setting."
    );
    let plugins = template
        .blocks
        .iter()
        .find(|b| b.key == "settings-template-extensions-plugins_preview")
        .unwrap();
    assert_eq!(template.kinds[plugins.start - 1], Line::Blank);
    assert!(template.text(plugins).starts_with("Run code plugins"));
    let formatters = template
        .blocks
        .iter()
        .find(|b| b.key == "settings-template-formatters-typescript")
        .unwrap();
    assert!(template.text(formatters).starts_with("External formatters"));
    assert!(template.text(formatters).ends_with("the others apply."));
    assert!(
        template
            .text(formatters)
            .contains("\n  command    the program"),
        "the table's indent is kept"
    );
}

#[test]
fn commented_toml_is_never_a_block() {
    let template = Template::parse(USER_SETTINGS_TEMPLATE);
    let commented: Vec<&str> = (template.lines.iter().zip(&template.kinds))
        .filter(|(_, kind)| matches!(kind, Line::Commented(_)))
        .map(|(line, _)| *line)
        .collect();
    for line in [
        "# font_size = 13.0",
        "# [[vim.remap]]",
        "# mode = \"n\"",
        "# from = \"<leader>w\"",
        "# to = \":w<CR>\"",
        "# [[vim.which_key_group]]",
        "# [formatters.typescript]",
        "# [formatters.rust]",
        "# [formatters.python]",
        "# [formatters.go]",
        "# timeout_ms = 5000",
    ] {
        assert!(commented.contains(&line), "{line}");
    }
    for block in &template.blocks {
        for kind in &template.kinds[block.start..block.end] {
            assert!(matches!(kind, Line::Prose | Line::Blank), "{}", block.key);
        }
    }
}

#[test]
fn template_keys_name_the_schema() {
    let schema = settings_schema();
    for path in [
        "editor.font_size",
        "vim.remap",
        "vim.remap.mode",
        "vim.which_key_group",
        "formatters.typescript",
        "formatters.go.timeout_ms",
        "workbench.zen_mode",
    ] {
        assert!(has_path(&schema, path), "{path}");
    }
    assert!(!has_path(&schema, "editor.no_such_key"));
    assert!(!has_path(&schema, "nothing"));
}

#[test]
fn en_us_renders_the_template_itself() {
    // No test of this binary changes the process's locale.
    assert_eq!(kiln_l10n::locales()[0].to_string(), "en-US");
    assert_eq!(user_settings_template(), USER_SETTINGS_TEMPLATE);
    let template = Template::parse(USER_SETTINGS_TEMPLATE);
    assert_eq!(template.render(|_| None), USER_SETTINGS_TEMPLATE);
}

#[test]
fn a_translated_block_wraps_at_eighty_columns() {
    let template = Template::parse("# Intro.\n\n[editor]\n# Size.\n# font_size = 13.0\n");
    let rendered = template.render(|key| {
        (key == "settings-template-editor-font_size").then(|| {
            format!(
                "{} one two three four five six seven eight nine ten eleven twelve thirteen\n\n  indented",
                fake_marker(key)
            )
        })
    });
    assert_eq!(
        rendered,
        "# Intro.\n\n[editor]\n\
         # ⟦settings-template-editor-font_size⟧ one two three four five six seven eight\n\
         # nine ten eleven twelve thirteen\n\
         #\n\
         #   indented\n\
         # font_size = 13.0\n"
    );
    for line in rendered.lines() {
        assert!(line.chars().count() <= 80, "{line}");
    }
}

#[test]
fn template_problems_are_errors() {
    let schema = settings_schema();
    let errors = |text: &str| template_ftl(text, &schema).unwrap_err();
    assert!(errors("#bad\n")[0].contains("starts `# `"));
    assert!(errors("[editor]\n# One.\n# nothing = 1\n")[0].contains("which the schema hasn't"));
    assert!(
        errors("[editor]\n# One.\n# font_size = 1\n# Two.\n# font_size = 2\n")[0]
            .contains("a second block")
    );
    assert!(errors("[editor]\n# One.\n# bad-key = 1\n")[0].contains("no key"));
}

#[test]
fn project_warnings_are_their_constants_in_en_us() {
    use crate::settings::*;
    for (constant, key) in [
        (UNTRUSTED_LSP, "config-project-lsp-untrusted"),
        (PROJECT_THEME, "config-project-theme-ignored"),
        (PROJECT_FORMATTERS, "config-project-formatters-ignored"),
        (PROJECT_EXTENSIONS, "config-project-extensions-ignored"),
        (PROJECT_CHAT, "config-project-chat-ignored"),
        (
            PROJECT_TERMINAL_SHELL,
            "config-project-terminal-shell-ignored",
        ),
        (PROJECT_VIM_USER_ONLY, "config-project-vim-user-only"),
        (PROJECT_ICON_THEME, "config-project-icon-theme-ignored"),
    ] {
        assert_eq!(kiln_l10n::tr_or(key, ""), constant, "{key}");
        assert_eq!(project_warning(constant), constant, "{key}");
    }
}
