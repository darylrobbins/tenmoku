//! Language servers an extension contributes reach the workbench through
//! fastedit's `lsp` contribution kind, after `kiln::init`, and a table of
//! the wrong shape is a positioned error in the config's diagnostics.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{fs, path::Path};

use fe_config::SettingsStore;
use gpui::TestAppContext;
use kiln::{
    Config,
    core::Severity,
    platform::config::{batch_channel, diagnostics},
    ui::ThemeMode,
};

fn install(extensions: &Path, dir: &str, id: &str, lsp: &str) {
    let dir = extensions.join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.0\"\n\n{lsp}"
        ),
    )
    .unwrap();
}

#[gpui::test]
fn extension_lsp_servers_reach_the_workbench(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    install(
        &extensions,
        "zig",
        "acme.zig",
        "[contributes.lsp.zls]\ncommand = \"zls\"\nargs = [\"--stdio\"]\n\
         languages = [\"zig\", \"zon\"]\n",
    );
    install(
        &extensions,
        "odin",
        "acme.odin",
        "[contributes.lsp.ols]\ncommand = \"ols\"\nargs = \"--stdio\"\n",
    );
    let user = state.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (_sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, extensions.clone());
    let at_init = cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    // They start only once the user consents (extension_consent.rs).
    for pending in cx.update(|cx| fe_workbench::pending_extension_consents(cx)) {
        cx.update(|cx| fe_workbench::grant_extension_server(cx, &pending))
            .unwrap();
    }
    cx.run_until_parked();

    let specs = cx.update(|cx| fe_workbench::extension_servers(cx));
    let specs: Vec<_> = specs
        .iter()
        .map(|spec| {
            (
                spec.name.as_str(),
                spec.command.to_str().unwrap(),
                spec.args.clone(),
                spec.languages.clone(),
            )
        })
        .collect();
    assert_eq!(
        specs,
        [(
            "zls",
            "zls",
            vec!["--stdio".to_owned()],
            vec!["zig".to_owned(), "zon".to_owned()]
        )]
    );

    // The odin extension's `args` isn't an array: it's refused, with an
    // error at the value.
    let odin = extensions.join("odin/extension.toml");
    let diagnostics = cx.update(|cx| diagnostics(cx).to_vec());
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.file.clone(), d.key.clone(), d.line, d.column))
        .collect();
    let expected = (odin, Some("contributes.lsp.ols.args".to_owned()), 11, 8);
    assert_eq!(errors, [expected], "{diagnostics:#?}");
    assert_eq!(at_init, diagnostics);
}
