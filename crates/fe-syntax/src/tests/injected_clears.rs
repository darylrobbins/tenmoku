//! `@none` clears its bytes' highlights in an injected layer too, rather
//! than leaving them to the host's.

#[cfg(all(feature = "php", feature = "scala", feature = "kotlin"))]
use fe_buffer::Buffer;

#[cfg(all(feature = "php", feature = "scala", feature = "kotlin"))]
use super::parsed;
#[cfg(all(feature = "php", feature = "scala", feature = "kotlin"))]
use crate::language_by_name;

/// Scala's and Kotlin's code interpolated in a string is highlighted the
/// same in a PHP nowdoc, which colours its text as a string, as on its own:
/// `$x` is cleared, plain rather than a string.
#[test]
#[cfg(all(feature = "php", feature = "scala", feature = "kotlin"))]
fn injected_interpolations_stay_plain() {
    for (language, code) in [
        ("scala", "val m = s\"a $x b ${y.size} c\""),
        ("kotlin", "val m = \"a $x b ${y.size} c\""),
    ] {
        let label = language.to_uppercase();
        let nowdoc = format!("<?php\n$s = <<<'{label}'\n{code}\n{label};\n");
        // The highlight of each probe's first byte.
        let probes = ["x b", " b ", "y.", "size"];
        let highlights = |host: &str, text: &str| {
            let syntax = parsed(language_by_name(host).unwrap(), &Buffer::new(text));
            let spans = syntax.highlights_compiling(0..text.len());
            probes.map(|probe| {
                let at = text.find(probe).unwrap();
                let span = spans.iter().find(|span| span.range.contains(&at));
                span.map(|span| span.highlight.name())
            })
        };
        let own = highlights(language, code);
        assert_eq!(own[..2], [None, Some("string")], "{language}");
        assert_eq!(highlights("php", &nowdoc), own, "{language}");
    }
}
