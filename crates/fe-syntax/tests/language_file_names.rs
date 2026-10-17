#![cfg(all(
    feature = "bash",
    feature = "ruby",
    feature = "make",
    feature = "markdown",
    feature = "rust"
))]

use std::path::Path;

use fe_syntax::language_name_for_path;

#[test]
fn login_shell_ruby_build_and_upper_case_names_are_detected() {
    let detect = |name: &str| language_name_for_path(&Path::new("home").join(name));
    for name in [
        ".zprofile",
        ".zlogin",
        ".zlogout",
        ".bash_login",
        ".bash_logout",
        ".bash_aliases",
        ".envrc",
        "PKGBUILD",
        "APKBUILD",
        "x.ksh",
        "SCRIPT.SH",
    ] {
        assert_eq!(detect(name), Some("bash"), "{name}");
    }
    for name in [
        "x.gemspec",
        "x.rake",
        "config.ru",
        "x.podspec",
        "Podfile",
        "Vagrantfile",
        "Brewfile",
        "Guardfile",
        "Capfile",
        "Fastfile",
        "GEMFILE",
    ] {
        assert_eq!(detect(name), Some("ruby"), "{name}");
    }
    for name in ["makefile", "x.Mak"] {
        assert_eq!(detect(name), Some("make"), "{name}");
    }
    assert_eq!(detect("README.MD"), Some("markdown"));
    assert_eq!(detect("Main.RS"), Some("rust"));
    for name in ["notes.txt", "zprofile.bak", ".zprofilex"] {
        assert_eq!(detect(name), None, "{name}");
    }
}
