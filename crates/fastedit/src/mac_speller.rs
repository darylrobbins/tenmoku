//! The macOS system spell checker as the app's [`Speller`]: the system's
//! dictionaries, languages and the words the user taught it.
//!
//! `NSSpellChecker` isn't documented as thread-safe, so every call goes
//! through one mutex. Nothing is learned into or ignored in the system-wide
//! dictionary: Tenmoku's own dictionary owns that.

use std::sync::Mutex;

use fe_workbench::spell::Speller;
use objc2::rc::Retained;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{NSInteger, NSNotFound, NSRange, NSString};

/// `NSTextCheckingTypeSpelling`.
const CHECK_SPELLING: u64 = 1 << 1;

/// `"auto"`: the system picks the language from the text.
const AUTO: &str = "auto";

/// The one lock on `NSSpellChecker`, which is shared by the whole process:
/// every `MacSpeller` goes through it, tag allocation included.
static CHECKER: Mutex<()> = Mutex::new(());

/// What asks the system's spell checker, one call at a time.
pub(crate) struct MacSpeller {
    /// This speller's spell document tag.
    tag: NSInteger,
}

impl MacSpeller {
    pub(crate) fn new() -> Self {
        let _guard = CHECKER.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: a class method with no preconditions, under the lock.
        let tag = unsafe { NSSpellChecker::uniqueSpellDocumentTag() };
        Self { tag }
    }

    /// Runs `f` with the checker set up for `language`: for `"auto"`, the
    /// system's own language identification and no language; for a tag, no
    /// identification and the tag, or `None` if the system lacks it.
    fn with<R>(
        &self,
        language: &str,
        f: impl FnOnce(&NSSpellChecker, Option<&NSString>, NSInteger) -> R,
    ) -> Option<R> {
        let _guard = CHECKER.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: calls are serialised by the lock.
        let checker: Retained<NSSpellChecker> = unsafe { NSSpellChecker::sharedSpellChecker() };
        if language == AUTO {
            // SAFETY: as above.
            unsafe { checker.setAutomaticallyIdentifiesLanguages(true) };
            return Some(f(&checker, None, self.tag));
        }
        // SAFETY: as above.
        let available = unsafe { checker.availableLanguages() };
        let wanted = language.replace('-', "_");
        let known = available.to_vec().into_iter().any(|have| {
            let have = have.to_string();
            have == wanted
                || wanted
                    .strip_prefix(have.as_str())
                    .is_some_and(|r| r.starts_with('_'))
                || have
                    .strip_prefix(wanted.as_str())
                    .is_some_and(|r| r.starts_with('_'))
        });
        if !known {
            return None;
        }
        // SAFETY: as above.
        unsafe { checker.setAutomaticallyIdentifiesLanguages(false) };
        let wanted = NSString::from_str(&wanted);
        Some(f(&checker, Some(&wanted), self.tag))
    }
}

/// Checks `string` as the system identifies its language: whether a word is
/// misspelled, and the dictionary language it was identified as. The legacy
/// check with no language uses the selected language instead.
fn check_identified(
    checker: &NSSpellChecker,
    string: &NSString,
    tag: NSInteger,
) -> (bool, Option<String>) {
    let mut orthography = None;
    // SAFETY: valid arguments; a null word count is allowed.
    let results = unsafe {
        checker.checkString_range_types_options_inSpellDocumentWithTag_orthography_wordCount(
            string,
            NSRange::new(0, string.len_utf16()),
            CHECK_SPELLING,
            None,
            tag,
            Some(&mut orthography),
            std::ptr::null_mut(),
        )
    };
    // SAFETY: an orthography the check returned.
    let language = orthography.map(|o| unsafe { o.dominantLanguage() }.to_string());
    (!results.is_empty(), language.filter(|l| l != "und"))
}

impl Drop for MacSpeller {
    fn drop(&mut self) {
        let _guard = CHECKER.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: under the lock, closing the tag this speller opened.
        unsafe { NSSpellChecker::sharedSpellChecker().closeSpellDocumentWithTag(self.tag) };
    }
}

impl Default for MacSpeller {
    fn default() -> Self {
        Self::new()
    }
}

impl Speller for MacSpeller {
    fn is_misspelled(&self, word: &str, language: &str) -> bool {
        let string = NSString::from_str(word);
        self.with(language, |checker, language, tag| match language {
            None => check_identified(checker, &string, tag).0,
            Some(language) => {
                // SAFETY: valid arguments; a null word count is allowed.
                let range = unsafe {
                    checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                        &string,
                        0,
                        Some(language),
                        false,
                        tag,
                        std::ptr::null_mut(),
                    )
                };
                range.location != NSNotFound as usize
            }
        })
        .unwrap_or(false)
    }

    fn suggest(&self, word: &str, language: &str) -> Vec<String> {
        let string = NSString::from_str(word);
        let range = NSRange::new(0, string.len_utf16());
        self.with(language, |checker, language, tag| {
            // Auto: the language the system identified the word as.
            let identified = match language {
                None => check_identified(checker, &string, tag)
                    .1
                    .map(|l| NSString::from_str(&l)),
                Some(_) => None,
            };
            let language = language.or(identified.as_deref());
            // SAFETY: the range covers the whole string.
            let guesses = unsafe {
                checker.guessesForWordRange_inString_language_inSpellDocumentWithTag(
                    range, &string, language, tag,
                )
            };
            guesses
                .map(|list| list.to_vec().into_iter().map(|g| g.to_string()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
    }
}
