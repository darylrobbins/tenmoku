//! The macOS spell checker binding, against the system's real one.
//!
//! Nothing here is built off macOS but an empty `main`.
//!
//! This binary has no libtest harness (`harness = false`): libtest runs every
//! test on a worker thread, and the binding is also used from the main
//! thread. [`main`] speaks enough of libtest's command line (`--list`,
//! `--exact`, filters) for `cargo test` and nextest, and runs the tests on
//! the process's real main thread.
#![allow(dead_code)]

#[cfg(target_os = "macos")]
#[path = "../src/mac_speller.rs"]
mod imp;

#[cfg(target_os = "macos")]
mod mac_speller {
    use super::imp::MacSpeller;
    use fe_workbench::spell::Speller;
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{MainThreadMarker, NSNotFound, NSString};

    /// The speller, or a printed reason to skip: only when the system itself
    /// lists no English dictionary, asked without the code under test.
    fn speller() -> Option<MacSpeller> {
        // SAFETY: a class method; the tests that use it run one at a time
        // per process under nextest, and only read.
        let languages = unsafe { NSSpellChecker::sharedSpellChecker().availableLanguages() };
        if !languages
            .to_vec()
            .iter()
            .any(|l| l.to_string().starts_with("en"))
        {
            eprintln!("skipped: the system spell server lists no English dictionary");
            return None;
        }
        Some(MacSpeller::new())
    }

    /// The native checker's verdict on one word in one language, with the
    /// legacy call and no binding.
    fn native_misspelled(word: &str, language: &str) -> bool {
        let word = NSString::from_str(word);
        let language = NSString::from_str(language);
        // SAFETY: valid arguments; a null word count is allowed.
        let range = unsafe {
            NSSpellChecker::sharedSpellChecker()
                .checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                    &word,
                    0,
                    Some(&language),
                    false,
                    0,
                    std::ptr::null_mut(),
                )
        };
        range.location != NSNotFound as usize
    }

    /// What `speller` says about a few words and a misspelling: the two
    /// verdicts, and whether the suggestions for "teh" include "the". The
    /// full list is not compared: the system's spell server adds and drops
    /// the less likely guesses ("yeh", "tbh") from one run to the next.
    fn answers(speller: &MacSpeller) -> (bool, bool, bool) {
        (
            speller.is_misspelled("teh", "en_US"),
            speller.is_misspelled("the", "en_US"),
            speller.suggest("teh", "en_US").iter().any(|s| s == "the"),
        )
    }

    pub(crate) fn flags_a_misspelling_and_suggests() {
        let Some(speller) = speller() else { return };
        assert!(speller.is_misspelled("teh", "en_US"));
        assert!(!speller.is_misspelled("the", "en_US"));
        assert!(speller.suggest("teh", "en_US").iter().any(|s| s == "the"));
    }

    pub(crate) fn auto_language_and_unknown_language_do_not_panic() {
        let Some(speller) = speller() else { return };
        // The system identifies the language: English words and a second
        // installed language's are right, a nonsense word is wrong, and no
        // language is named. The second language's word is wrong in English
        // and right in its own language, so a selected-language check
        // couldn't pass.
        assert!(!speller.is_misspelled("the", "auto"));
        assert!(speller.is_misspelled("qzxjv", "auto"));
        let _ = speller.suggest("teh", "auto");
        // SAFETY: as above.
        let available = unsafe { NSSpellChecker::sharedSpellChecker().availableLanguages() };
        let available: Vec<String> = available.to_vec().iter().map(|l| l.to_string()).collect();
        // Installed languages come straight from the native checker, and a
        // fixture is validated with it too, never through the binding.
        let fixtures = [
            ("fr", "maintenant"),
            ("de", "Fenster"),
            ("es", "entonces"),
            ("it", "finestra"),
            ("pt", "obrigado"),
            ("nl", "gezellig"),
        ];
        let primary = |l: &str| l.split(['_', '-']).next().unwrap_or(l).to_owned();
        let others: Vec<String> = available
            .iter()
            .filter(|l| primary(l) != "en")
            .cloned()
            .collect();
        let installed: Vec<_> = fixtures
            .iter()
            .filter(|(tag, _)| others.iter().any(|l| primary(l) == *tag))
            .collect();
        if others.is_empty() {
            eprintln!("skipped the second language: only English is installed");
        } else {
            assert!(
                !installed.is_empty(),
                "installed non-English dictionaries {others:?} have no fixture; add a word wrong in en_US and right in one of them to `fixtures`"
            );
        }
        for (tag, word) in &installed {
            assert!(
                native_misspelled(word, "en_US") && !native_misspelled(word, tag),
                "fixture {word:?} must be wrong in en_US and right in {tag}"
            );
            assert!(!speller.is_misspelled(word, "auto"), "{word}");
        }
        // A language the system lacks finds every word right.
        assert!(!speller.is_misspelled("qzxjv", "xx-NOPE"));
        assert!(speller.suggest("teh", "xx-NOPE").is_empty());
    }

    /// The main thread's answers, a speller built on one thread and used on
    /// another, and one built on another thread all agree.
    pub(crate) fn works_off_the_main_thread() {
        let Some(speller) = speller() else { return };
        assert!(
            MainThreadMarker::new().is_some(),
            "the baseline must run on the main thread"
        );
        let on_main = answers(&speller);
        assert_eq!(on_main, (true, false, true));
        let (background, built_there) = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                assert!(MainThreadMarker::new().is_none());
                answers(&speller)
            });
            let b = scope.spawn(|| answers(&MacSpeller::new()));
            (a.join().unwrap(), b.join().unwrap())
        });
        assert_eq!(on_main, background);
        assert_eq!(on_main, built_there);
    }

    pub(crate) fn distinct_spellers_can_run_concurrently() {
        if speller().is_none() {
            return;
        }
        let expected = &answers(&MacSpeller::new());
        std::thread::scope(|scope| {
            let threads: Vec<_> = (0..8)
                .map(|i| {
                    scope.spawn(move || {
                        let speller = MacSpeller::new();
                        for _ in 0..20 {
                            assert_eq!(&answers(&speller), expected);
                            // Alternating languages changes the checker's
                            // shared state between calls.
                            let _ = speller
                                .is_misspelled("teh", if i % 2 == 0 { "auto" } else { "en_US" });
                        }
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
        });
    }
}

#[cfg(target_os = "macos")]
type Test = (&'static str, fn());

#[cfg(target_os = "macos")]
const TESTS: &[Test] = &[
    (
        "mac_speller::flags_a_misspelling_and_suggests",
        mac_speller::flags_a_misspelling_and_suggests,
    ),
    (
        "mac_speller::auto_language_and_unknown_language_do_not_panic",
        mac_speller::auto_language_and_unknown_language_do_not_panic,
    ),
    (
        "mac_speller::works_off_the_main_thread",
        mac_speller::works_off_the_main_thread,
    ),
    (
        "mac_speller::distinct_spellers_can_run_concurrently",
        mac_speller::distinct_spellers_can_run_concurrently,
    ),
];

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        // Nothing here is `#[ignore]`d.
        if !args.iter().any(|a| a == "--ignored") {
            for (name, _) in TESTS {
                println!("{name}: test");
            }
        }
        return;
    }
    let exact = args.iter().any(|a| a == "--exact");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let mut failed = 0;
    for (name, test) in TESTS {
        let wanted = filters.is_empty()
            || filters.iter().any(|f| {
                if exact {
                    f == name
                } else {
                    name.contains(f.as_str())
                }
            });
        if !wanted {
            continue;
        }
        let passed = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if passed { "ok" } else { "FAILED" });
        failed += usize::from(!passed);
    }
    if failed > 0 {
        std::process::exit(101);
    }
}
