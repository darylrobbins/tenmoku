#![cfg(all(
    feature = "ruby",
    feature = "lua",
    feature = "php",
    feature = "make",
    feature = "python"
))]

use std::path::Path;

use fe_syntax::{detect_language_name, language_name_for_shebang};

#[test]
fn ruby_lua_php_and_make_shebangs_pick_their_language() {
    for (line, language) in [
        ("#!/usr/bin/env ruby\n", "ruby"),
        ("#!/usr/bin/ruby3.2 -w\n", "ruby"),
        ("#!/usr/bin/env lua\n", "lua"),
        ("#!/usr/bin/env lua5.4\n", "lua"),
        ("#!/usr/bin/php\n", "php"),
        ("#!/usr/bin/env php8.2\n", "php"),
        ("#!/usr/bin/make -f\n", "make"),
    ] {
        assert_eq!(language_name_for_shebang(line), Some(language), "{line}");
    }
}

#[test]
fn an_extensionless_ruby_script_is_detected_from_its_shebang() {
    let text = "#!/usr/bin/env ruby\nputs 1\n";
    assert_eq!(
        detect_language_name(Some(Path::new("bin/rails")), text),
        Some("ruby")
    );
}

#[test]
fn near_miss_interpreters_stay_undetected() {
    for line in [
        "#!/usr/bin/env rubyx\n",
        "#!/usr/bin/env lua-5\n",
        "#!/usr/bin/makepkg\n",
    ] {
        assert_eq!(language_name_for_shebang(line), None, "{line}");
    }
    assert_eq!(
        language_name_for_shebang("#!/usr/bin/env python3\n"),
        Some("python")
    );
}
