//! `Navigation::related`: routed as definitions are, skipping providers
//! that keep the default.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fe_buffer::Text;
use fe_nav::{
    Definitions, NavError, NavRequest, Navigation, NavigationProvider, Precision, Readiness,
    Related, WorkspaceSymbol,
};

/// A ready provider that answers `related` with `word`, or keeps the
/// default, counting calls to it.
struct Fake {
    name: &'static str,
    word: Option<&'static str>,
    related_calls: AtomicUsize,
}

impl Fake {
    fn new(name: &'static str, word: Option<&'static str>) -> Arc<Fake> {
        Arc::new(Fake {
            name,
            word,
            related_calls: AtomicUsize::new(0),
        })
    }

    fn asked(&self) -> usize {
        self.related_calls.load(Ordering::Relaxed)
    }
}

impl NavigationProvider for Fake {
    fn name(&self) -> &str {
        self.name
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _language: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(
        &self,
        _req: &NavRequest,
        _cancel: &AtomicBool,
    ) -> Result<Definitions, NavError> {
        Err(NavError::Unsupported)
    }

    fn related(
        &self,
        _kind: Related,
        _req: &NavRequest,
        _cancel: &AtomicBool,
    ) -> Result<Definitions, NavError> {
        self.related_calls.fetch_add(1, Ordering::Relaxed);
        match self.word {
            Some(word) => Ok(Definitions {
                word: Some(word.to_owned()),
                ..Definitions::default()
            }),
            None => Err(NavError::Unsupported),
        }
    }

    fn workspace_symbols(
        &self,
        _query: &str,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Err(NavError::Unsupported)
    }
}

/// A ready provider that keeps every default, `related` included.
struct Plain;

impl NavigationProvider for Plain {
    fn name(&self) -> &str {
        "plain"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _language: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(
        &self,
        _req: &NavRequest,
        _cancel: &AtomicBool,
    ) -> Result<Definitions, NavError> {
        Err(NavError::Unsupported)
    }

    fn workspace_symbols(
        &self,
        _query: &str,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Err(NavError::Unsupported)
    }
}

fn request() -> NavRequest {
    NavRequest {
        root: PathBuf::from("/project"),
        rel: PathBuf::from("src/main.rs"),
        language: Some("rust"),
        text: Text::new("helper()\n"),
        offset: 2,
        buffer: None,
    }
}

#[test]
fn related_goes_to_the_first_ready_provider_that_answers() {
    let (first, second) = (
        Fake::new("first", Some("first")),
        Fake::new("second", Some("second")),
    );
    let nav = Navigation::new();
    nav.add(Arc::new(Plain), 30);
    nav.add(first.clone(), 20);
    nav.add(second.clone(), 10);
    let cancel = AtomicBool::new(false);
    for kind in [
        Related::Declaration,
        Related::TypeDefinition,
        Related::Implementation,
    ] {
        let answer = nav.related(kind, &request(), &cancel).unwrap();
        assert_eq!(answer.provider, "first");
        assert_eq!(answer.value.word.as_deref(), Some("first"));
    }
    assert_eq!((first.asked(), second.asked()), (3, 0));
}

#[test]
fn related_is_unsupported_when_no_provider_answers() {
    let nav = Navigation::new();
    let cancel = AtomicBool::new(false);
    assert_eq!(
        nav.related(Related::Declaration, &request(), &cancel),
        Err(NavError::Unsupported)
    );
    nav.add(Arc::new(Plain), 0);
    assert_eq!(
        nav.related(Related::Implementation, &request(), &cancel),
        Err(NavError::Unsupported)
    );
}
