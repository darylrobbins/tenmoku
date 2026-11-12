//! The spelling core: the tokenizer, the speller seam and the dictionary.

use std::{
    fs,
    sync::{Barrier, Mutex},
    thread,
};

use crate::spell::{
    DICTIONARY_FILE, Dictionary, MAX_SUGGESTIONS, NoSpeller, Speller, check_text,
    set_test_config_dir, suggest, tokenize,
};

/// Knows the words it was given, in any case, and remembers what it was asked.
#[derive(Default)]
struct FakeSpeller {
    known: Vec<&'static str>,
    suggestions: Vec<&'static str>,
    asked: Mutex<Vec<(String, String)>>,
}

impl FakeSpeller {
    fn knowing(known: &[&'static str]) -> Self {
        Self {
            known: known.to_vec(),
            ..Self::default()
        }
    }

    fn suggesting(suggestions: &[&'static str]) -> Self {
        Self {
            suggestions: suggestions.to_vec(),
            ..Self::default()
        }
    }

    fn asked(&self) -> Vec<String> {
        let asked = self.asked.lock().unwrap();
        asked.iter().map(|(word, _)| word.clone()).collect()
    }
}

impl Speller for FakeSpeller {
    fn is_misspelled(&self, word: &str, language: &str) -> bool {
        (self.asked.lock().unwrap()).push((word.to_owned(), language.to_owned()));
        !self
            .known
            .iter()
            .any(|known| known.eq_ignore_ascii_case(word))
    }

    fn suggest(&self, _word: &str, _language: &str) -> Vec<String> {
        self.suggestions.iter().map(ToString::to_string).collect()
    }
}

/// The words `text` gives, each checked against its range.
fn words(text: &str) -> Vec<&str> {
    tokenize(text)
        .into_iter()
        .map(|word| {
            assert_eq!(&text[word.range.clone()], word.text, "in {text:?}");
            word.text
        })
        .collect()
}

#[test]
fn tokenizer_splits_identifiers() {
    let text = "parseHttpResponse_body";
    let found: Vec<_> = tokenize(text)
        .into_iter()
        .map(|word| (word.text, word.range))
        .collect();
    assert_eq!(
        found,
        [
            ("parse", 0..5),
            ("Http", 5..9),
            ("Response", 9..17),
            ("body", 18..22),
        ]
    );

    assert_eq!(words("HTTPServer"), ["HTTP", "Server"]);
    assert_eq!(words("parseHTTPResponse"), ["parse", "HTTP", "Response"]);
    assert_eq!(words("SCREAMING_SNAKE_CASE"), ["SCREAMING"]);
    assert_eq!(words("__dunder__name"), ["dunder", "name"]);
    assert_eq!(words("well-known"), ["well", "known"]);

    // An apostrophe inside a word stays there, a quote's doesn't.
    assert_eq!(words("don't"), ["don't"]);
    assert_eq!(words("don’t"), ["don’t"]);
    assert_eq!(words("'quoted' dogs' it's"), ["quoted", "dogs", "it's"]);
    assert_eq!(words("O'Brien"), ["O'Brien"]);
    assert_eq!(words("I'm"), Vec::<&str>::new());

    // Non-ASCII letters are kept, the byte ranges still on char boundaries.
    assert_eq!(words("naïve Größe résumé"), ["naïve", "Größe", "résumé"]);
    assert_eq!(words("Привет мир"), ["Привет", "мир"]);
    assert_eq!(words("caféBar"), ["café", "Bar"]);
    assert_eq!(words("ÉcoleNormale"), ["École", "Normale"]);
    // A decomposed é is one letter of one word.
    assert_eq!(words("re\u{301}sume\u{301}"), ["re\u{301}sume\u{301}"]);
    assert_eq!(words("cafe\u{301}Bar"), ["cafe\u{301}", "Bar"]);

    // Prose, around what's skipped.
    assert_eq!(
        words("see https://example.com/a for the (nice) word."),
        ["see", "for", "the", "nice", "word"]
    );
    // A comment or string's own marks aren't in the way.
    assert_eq!(words("//wrong word"), ["wrong", "word"]);
    assert_eq!(words("///wrong word"), ["wrong", "word"]);
    assert_eq!(words("//!wrong word"), ["wrong", "word"]);
    assert_eq!(words("/*wrong word*/"), ["wrong", "word"]);
    assert_eq!(words("**bold** _em_"), ["bold"]);
    assert_eq!(words("wait...what"), ["wait", "what"]);
    assert_eq!(words("\\nfirst second\\n\\n"), ["first", "second"]);
    assert_eq!(words("first\\x41second"), ["first"]);
    assert_eq!(words("first\\0second"), ["first", "second"]);
    // A known limit: nothing tells this path from those escapes.
    assert_eq!(words("src\\test"), ["src", "est"]);
    assert_eq!(
        words("first\\nsecond\\tthird"),
        ["first", "second", "third"]
    );
    assert_eq!(
        words("key=value note:later"),
        ["key", "value", "note", "later"]
    );
    assert_eq!(words("say \"hello\""), ["say", "hello"]);
    assert_eq!(words(""), Vec::<&str>::new());
}

#[test]
fn tokenizer_skips_non_words() {
    for text in [
        // URLs.
        "https://example.com/path?q=wrod&r=(teh),more",
        "(ftp://wrod.example.com)",
        "\"https://wrod.example.com\"",
        "www.wrodexample.com",
        // Email addresses.
        "someone@example.com",
        "first.last+tag@wrod.example.org",
        "<someone@example.com>",
        "git@github.com:wrod/wrod.git",
        // File paths.
        "/usr/local/wrod",
        "./wrod/teh",
        "../wrod",
        "~/wrod/teh",
        "src/wrod",
        "//wrod/teh",
        "and/or",
        "wrod/teh.txt",
        "wrod.txt",
        ".gitignore",
        "C:\\Users\\wrod",
        "assets\\wrod\\tehh",
        "wrod\\zeh",
        "assets\\\\wrod\\\\tehh",
        "assets\\_wrod",
        "assets\\-wrod",
        "assets\\\\_wrod",
        "assets\\\\-wrod",
        "assets\\.wrod",
        // Quoted, which changes nothing.
        "'.gitignore'",
        "’.gitignore’",
        "'C:\\Users\\wrod'",
        "'..\\wrod'",
        "'assets\\wrod'",
        // No more does an escaped apostrophe make two words.
        "wrod\\'teh",
        ".\\wrod\\teh",
        "..\\wrod",
        "\\\\server\\wrod",
        "c:/users/wrod",
        // Dotted and `::` names.
        "self.wrodName",
        "std::wrod::Teh",
        // Hex literals.
        "0xdeadbeef",
        "0xDEADBEEF",
        "0XDEAD_BEEF",
        "0xdeadbeefu64",
        // Words with digits.
        "utf8",
        "h264",
        "UTF8",
        "x86_64",
        "i18n",
        "3rd",
        "foo2bar",
        // Under 3 letters.
        "a",
        "ab",
        "é",
        "日本",
        "Hi",
        // All capitals, 2 to 5 letters.
        "OK",
        "URL",
        "HTTP",
        "HTTPS",
        "HTTP_URL",
        "URLs",
        "APIs",
        "HTTP's",
        "DON'T",
        // After a sigil.
        "$HOME",
        "$wrodName",
        "$wrod_teh",
        "${wrod}",
        "$(wrod)",
        "@param",
        "@wrodName",
        "@wrod_teh",
        // Punctuation alone.
        "--- !!! ...",
    ] {
        assert_eq!(words(text), Vec::<&str>::new(), "{text}");
    }

    // Past those limits they are words.
    assert_eq!(words("ASSEMBLY"), ["ASSEMBLY"]);
    assert_eq!(words("abc ABCDEF"), ["abc", "ABCDEF"]);
    // A digit or sigil costs only the part it's in.
    assert_eq!(words("parse_utf8_body"), ["parse", "body"]);
    assert_eq!(words("parseUtf8Body"), ["parse", "Body"]);
    assert_eq!(words("h264Decoder"), ["Decoder"]);
    assert_eq!(words("UTF8String"), ["String"]);
    assert_eq!(words("$first second"), ["second"]);
    assert_eq!(words("costs $5 today"), ["costs", "today"]);
    // A trailing or spaced @ is no sigil, and no email address.
    assert_eq!(words("hello@ there"), ["hello", "there"]);
    // Words beside a skipped address still count.
    assert_eq!(
        words("mail someone@example.com or see /etc/hosts now"),
        ["mail", "see", "now"]
    );
}

#[test]
fn misspellings_use_the_speller_and_the_dictionary() {
    let speller = FakeSpeller::knowing(&["hello"]);
    let mut dictionary = Dictionary::default();
    let text = "hello wrld hello wrld Wrld teh";
    let found = check_text(&speller, &dictionary, text, "en-GB");
    let ranges: Vec<_> = found.iter().map(|found| found.range.clone()).collect();
    assert_eq!(ranges, [6..10, 17..21, 22..26, 27..30]);
    assert_eq!(found[0].word, "wrld");
    assert_eq!(found[2].word, "Wrld");
    for found in &found {
        assert_eq!(&text[found.range.clone()], found.word);
    }
    // Once for each distinct word, in the language it was given.
    assert_eq!(speller.asked(), ["hello", "wrld", "Wrld", "teh"]);
    assert!(
        (speller.asked.lock().unwrap().iter()).all(|(_, language)| language == "en-GB"),
        "the language reaches the speller"
    );

    // The dictionary and the session's ignores, in any case, without asking.
    dictionary.ignore_word("WRLD");
    let speller = FakeSpeller::knowing(&["hello"]);
    let found = check_text(&speller, &dictionary, text, "auto");
    assert_eq!(
        (found.iter().map(|found| found.word.as_str())).collect::<Vec<_>>(),
        ["teh"]
    );
    assert_eq!(speller.asked(), ["hello", "teh"]);

    dictionary.add_word("Teh").unwrap();
    let speller = FakeSpeller::knowing(&["hello"]);
    assert_eq!(check_text(&speller, &dictionary, text, "auto"), []);
    assert_eq!(speller.asked(), ["hello"]);

    // Either apostrophe is the same word.
    dictionary.add_word("wouldn’t've").unwrap();
    assert!(dictionary.contains("Wouldn't've"));
    assert!(dictionary.contains("WOULDN’T'VE"));

    // Skipped words never reach the speller, and no speller finds nothing.
    let speller = FakeSpeller::default();
    assert_eq!(
        check_text(&speller, &dictionary, "$wrod https://a.b/wrod", "auto"),
        []
    );
    assert_eq!(speller.asked(), Vec::<String>::new());
    assert_eq!(
        check_text(&NoSpeller, &Dictionary::default(), "wrld teh", "auto"),
        []
    );
    assert_eq!(NoSpeller.suggest("wrld", "auto"), Vec::<String>::new());
}

#[test]
fn dictionary_file_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    // Created with its parent directory.
    let path = dir.path().join("deep/er/dictionary.txt");
    let mut dictionary = Dictionary::load(path.clone());
    assert!(!dictionary.contains("tenmoku"));
    assert!(!path.exists(), "loading writes nothing");

    dictionary.add_word("Tenmoku").unwrap();
    dictionary.add_word("kiln").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "tenmoku\nkiln\n");
    assert!(dictionary.contains("TENMOKU"));

    // Duplicates aren't written twice, in any case.
    dictionary.add_word("TENMOKU").unwrap();
    dictionary.add_word("kiln").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "tenmoku\nkiln\n");

    // Unicode's rules for case, not just lowercasing.
    let unicode = dir.path().join("unicode.txt");
    let mut dictionary = Dictionary::load(unicode.clone());
    dictionary.add_word("Straße").unwrap();
    for word in ["straße", "STRASSE", "strasse", "STRAẞE"] {
        assert!(dictionary.contains(word), "{word}");
        dictionary.add_word(word).unwrap();
    }
    assert_eq!(fs::read_to_string(&unicode).unwrap(), "straße\n");
    let mut dictionary = Dictionary::load(unicode.clone());
    assert!(dictionary.contains("STRASSE"), "as loaded from the file");
    dictionary.ignore_word("ΣΑΣ");
    for word in ["σας", "σασ", "Σας"] {
        assert!(dictionary.contains(word), "{word}");
    }
    // Dotted and dotless i are different letters, whatever `ı`'s capital is.
    let dotless = dir.path().join("dotless.txt");
    let mut dictionary = Dictionary::load(dotless.clone());
    dictionary.add_word("file").unwrap();
    assert!(dictionary.contains("FILE"));
    assert!(!dictionary.contains("fıle"));
    dictionary.add_word("fıle").unwrap();
    assert_eq!(fs::read_to_string(&dotless).unwrap(), "file\nfıle\n");
    let mut dictionary = Dictionary::load(dotless.clone());
    assert!(dictionary.contains("fıle"));
    dictionary.ignore_word("sıla");
    assert!(!dictionary.contains("sila"));
    assert!(!dictionary.contains("SILA"), "its capital is an i's");
    // Other folds that lowercase alone misses.
    dictionary.ignore_word("oﬃce");
    assert!(dictionary.contains("OFFICE"));
    dictionary.ignore_word("ſand");
    assert!(dictionary.contains("sand"));
    assert!(!dictionary.contains("stras"));

    // Nor one another copy of the file wrote since this one loaded.
    let mut other = Dictionary::load(path.clone());
    other.add_word("glaze").unwrap();
    dictionary.add_word("Glaze").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "tenmoku\nkiln\nglaze\n");
    assert!(dictionary.contains("glaze"));

    // Loaded again; comments and blanks aren't words.
    let edited = dir.path().join("edited.txt");
    fs::write(&edited, "# my words\n\n  Tenmoku  \r\n   \n# kiln\nglaze").unwrap();
    let mut dictionary = Dictionary::load(edited.clone());
    assert!(dictionary.contains("tenmoku"));
    assert!(dictionary.contains("glaze"));
    assert!(!dictionary.contains("kiln"));
    assert!(!dictionary.contains("my words"));
    assert!(!dictionary.contains("# kiln"));
    assert!(!dictionary.contains(""));
    // A last line with no newline doesn't swallow the next word.
    dictionary.add_word("slip").unwrap();
    let reloaded = Dictionary::load(edited.clone());
    assert!(reloaded.contains("glaze"));
    assert!(reloaded.contains("slip"));
    assert!(
        fs::read_to_string(&edited)
            .unwrap()
            .ends_with("glaze\nslip\n")
    );

    // What couldn't be read back as that word isn't written.
    let before = fs::read_to_string(&edited).unwrap();
    for word in ["", "  ", "two words", "two\nlines", "#hash"] {
        dictionary.add_word(word).unwrap();
        assert!(!dictionary.contains(word), "{word:?}");
    }
    assert_eq!(fs::read_to_string(&edited).unwrap(), before);

    // Ignored words last the session, and aren't written.
    dictionary.ignore_word("Sgraffito");
    assert!(dictionary.contains("sgraffito"));
    assert_eq!(fs::read_to_string(&edited).unwrap(), before);
    assert!(!Dictionary::load(edited.clone()).contains("sgraffito"));

    // A missing file or one that can't be read is empty, not an error: a
    // directory, and bytes that aren't text.
    assert!(!Dictionary::load(dir.path().join("missing.txt")).contains("glaze"));
    assert!(!Dictionary::load(dir.path().into()).contains("deep"));
    let binary = dir.path().join("binary.txt");
    fs::write(&binary, b"glaze\n\xff\xfe\n").unwrap();
    assert!(!Dictionary::load(binary).contains("glaze"));

    // A write that fails is reported, and the word still counts for now.
    let mut dictionary = Dictionary::load(dir.path().into());
    assert!(dictionary.add_word("celadon").is_err());
    assert!(dictionary.contains("celadon"));

    // With no file, words last the session.
    let mut dictionary = Dictionary::default();
    dictionary.add_word("shino").unwrap();
    assert!(dictionary.contains("Shino"));
    assert_eq!(dictionary.path(), None);

    // The user's is in the config directory: under test, one the test
    // names, a temporary one here.
    let config = tempfile::tempdir().unwrap();
    set_test_config_dir(Some(config.path().to_owned()));
    let mut user = Dictionary::user();
    let file = config.path().join(DICTIONARY_FILE);
    assert_eq!(user.path(), Some(file.as_path()));
    user.add_word("tenmoku").unwrap();
    assert!(Dictionary::user().contains("Tenmoku"));
    set_test_config_dir(None);
    assert_eq!(Dictionary::user().path(), None);
}

/// Under test, the user's dictionary isn't the one the environment names:
/// without a test's own config directory it reads and writes no file, so
/// the developer's dictionary is never touched.
#[test]
fn user_dictionary_ignores_the_environment_under_test() {
    let env = tempfile::tempdir().unwrap();
    let file = env.path().join(DICTIONARY_FILE);
    fs::write(&file, "glaze\n").unwrap();
    // SAFETY: nextest runs each test in a process of its own, and nothing
    // else in this test reads the environment meanwhile.
    unsafe { std::env::set_var("TENMOKU_CONFIG_DIR", env.path()) };
    assert_eq!(fe_config::paths::config_dir().as_deref(), Some(env.path()));
    let mut user = Dictionary::user();
    assert_eq!(user.path(), None);
    assert!(!user.contains("glaze"), "the environment's file isn't read");
    user.add_word("celadon").unwrap();
    assert!(user.contains("celadon"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "glaze\n", "nor written");
}

/// Writers that all find a word missing still write it once, and every
/// other word they write stays whole on its own line.
#[test]
fn dictionary_writers_at_once_write_a_word_once() {
    const WRITERS: usize = 8;
    let dir = tempfile::tempdir().unwrap();
    for round in 0..20 {
        let path = dir.path().join(format!("{round}.txt"));
        let barrier = Barrier::new(WRITERS);
        thread::scope(|scope| {
            for writer in 0..WRITERS {
                let (path, barrier) = (path.clone(), &barrier);
                scope.spawn(move || {
                    let mut dictionary = Dictionary::load(path);
                    barrier.wait();
                    dictionary.add_word("glaze").unwrap();
                    dictionary.add_word(&format!("word{writer}")).unwrap();
                });
            }
        });
        let text = fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort_unstable();
        let mut expected = vec!["glaze".to_owned()];
        expected.extend((0..WRITERS).map(|writer| format!("word{writer}")));
        assert_eq!(lines, expected, "round {round}");
    }
}

#[test]
fn suggestions_pass_through_the_speller() {
    let speller = FakeSpeller::suggesting(&["the", "tea", "ten"]);
    assert_eq!(suggest(&speller, "teh", "auto"), ["the", "tea", "ten"]);
    assert_eq!(suggest(&speller, "Teh", "auto"), ["The", "Tea", "Ten"]);
    assert_eq!(suggest(&speller, "TEH", "auto"), ["THE", "TEA", "TEN"]);
    // A mix of cases says nothing about a suggestion's.
    assert_eq!(suggest(&speller, "tEh", "auto"), ["the", "tea", "ten"]);

    // Capped at 8.
    let many = FakeSpeller::suggesting(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    assert_eq!(suggest(&many, "wrod", "auto").len(), MAX_SUGGESTIONS);
    assert_eq!(MAX_SUGGESTIONS, 8);
    assert_eq!(suggest(&many, "wrod", "auto")[7], "h");

    // A suggestion keeps its own capitals, and cased alike they're one.
    let names = FakeSpeller::suggesting(&["McDonald", "Paris", "paris", "PARIS"]);
    assert_eq!(
        suggest(&names, "pariss", "auto"),
        ["McDonald", "Paris", "paris", "PARIS"]
    );
    assert_eq!(
        suggest(&names, "Pariss", "auto"),
        ["McDonald", "Paris", "PARIS"]
    );
    assert_eq!(suggest(&names, "PARISS", "auto"), ["MCDONALD", "PARIS"]);

    // Non-ASCII, and a capital that grows.
    let accented = FakeSpeller::suggesting(&["école", "straße"]);
    assert_eq!(suggest(&accented, "Ecole", "auto"), ["École", "Straße"]);
    assert_eq!(suggest(&accented, "ECOLE", "auto"), ["ÉCOLE", "STRASSE"]);

    let none = FakeSpeller::default();
    assert_eq!(suggest(&none, "Wrod", "auto"), Vec::<String>::new());
    assert_eq!(suggest(&NoSpeller, "Wrod", "auto"), Vec::<String>::new());
}
