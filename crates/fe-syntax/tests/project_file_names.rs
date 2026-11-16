//! Procfiles, systemd units, CODEOWNERS, Avro schemas, MDX and a few more
//! Ruby tool files open with grammars we already have. `.desktop` files stay
//! unclaimed: the INI grammar puts an ERROR node on `Name[de]=...`.

#[cfg(any(
    feature = "ruby",
    feature = "yaml",
    feature = "bash",
    feature = "ini",
    feature = "gitignore",
    feature = "json",
    feature = "markdown"
))]
use std::path::Path;

#[cfg(any(
    feature = "ruby",
    feature = "yaml",
    feature = "bash",
    feature = "ini",
    feature = "gitignore",
    feature = "json",
    feature = "markdown"
))]
use fe_syntax::language_name_for_path;

#[cfg(any(
    feature = "ruby",
    feature = "yaml",
    feature = "bash",
    feature = "ini",
    feature = "gitignore",
    feature = "json",
    feature = "markdown"
))]
fn language(path: &str) -> Option<&'static str> {
    language_name_for_path(Path::new(path))
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_tool_files_are_ruby() {
    for name in [
        "Dangerfile",
        "dangerfile",
        "Appfile",
        "Matchfile",
        "Pluginfile",
        "Berksfile",
        "Thorfile",
        "Steepfile",
        ".irbrc",
        ".pryrc",
        "x.jbuilder",
        "app/views/x.json.jbuilder",
        "fastlane/Appfile",
        "APP/VIEWS/X.JBUILDER",
    ] {
        assert_eq!(language(name), Some("ruby"), "{name}");
    }
    assert_eq!(language("Dangerfile.bak"), None);
}

#[cfg(feature = "yaml")]
#[test]
fn procfiles_are_yaml() {
    for name in [
        "Procfile",
        "procfile",
        "app/Procfile",
        "Procfile.dev",
        "Procfile.local",
        "PROCFILE.DEV",
        "app/Procfile.dev",
    ] {
        assert_eq!(language(name), Some("yaml"), "{name}");
    }
}

#[cfg(feature = "bash")]
#[test]
fn procfile_look_alikes_keep_their_language() {
    assert_eq!(language("Procfile.sh"), Some("bash"));
}

#[cfg(feature = "ini")]
#[test]
fn systemd_units_are_ini() {
    for name in [
        "a.service",
        "a.timer",
        "a.socket",
        "a.mount",
        "a.path",
        "a.target",
        "A.SERVICE",
        "etc/systemd/system/app.service",
        "setup.cfg",
    ] {
        assert_eq!(language(name), Some("ini"), "{name}");
    }
    assert_eq!(language("notes.target.bak"), None);
    assert_eq!(language("share/applications/app.desktop"), None);
}

#[cfg(feature = "gitignore")]
#[test]
fn codeowners_is_gitignore() {
    for name in [
        "CODEOWNERS",
        "codeowners",
        ".github/CODEOWNERS",
        "docs/CODEOWNERS",
    ] {
        assert_eq!(language(name), Some("gitignore"), "{name}");
    }
    assert_eq!(language("CODEOWNERS.bak"), None);
}

#[cfg(feature = "json")]
#[test]
fn avro_schemas_are_json() {
    for name in ["user.avsc", "schemas/User.AVSC"] {
        assert_eq!(language(name), Some("json"), "{name}");
    }
}

#[cfg(feature = "markdown")]
#[test]
fn mdx_is_markdown() {
    for name in ["page.mdx", "docs/Page.MDX"] {
        assert_eq!(language(name), Some("markdown"), "{name}");
    }
}

#[cfg(any(feature = "yaml", feature = "ini", feature = "gitignore"))]
fn has_error(language_name: &str, text: &str) -> bool {
    use fe_buffer::Buffer;
    use fe_syntax::{Syntax, language_by_name};

    let buffer = Buffer::new(text);
    let language = language_by_name(language_name).expect("compiled in");
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax.tree().expect("parsed").root_node().has_error()
}

#[cfg(feature = "yaml")]
#[test]
fn a_procfile_parses_as_yaml() {
    let text = "web: bundle exec puma -C config/puma.rb\n\
                worker: bundle exec sidekiq -q default\n\
                release: bin/rails db:migrate\n\
                # comment\n";
    assert!(!has_error("yaml", text));
}

#[cfg(feature = "ini")]
#[test]
fn a_systemd_unit_parses_as_ini() {
    let unit = "[Unit]\nDescription=App\n\n[Service]\nExecStart=/usr/bin/app\n\n[Install]\nWantedBy=multi-user.target\n";
    assert!(!has_error("ini", unit));
}

#[cfg(feature = "gitignore")]
#[test]
fn a_codeowners_file_parses_as_gitignore() {
    let text = "# comment\n* @org/team\n/docs/ @alice @bob\n*.js @org/frontend\n";
    assert!(!has_error("gitignore", text));
}
