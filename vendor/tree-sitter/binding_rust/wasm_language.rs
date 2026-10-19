use std::{
    cell::Cell,
    error,
    ffi::{CStr, CString},
    fmt,
    mem::{self, MaybeUninit},
    os::raw::c_char,
};

pub use wasmtime_c_api::wasmtime;

use crate::{Language, LanguageError, Parser, ffi, ts_free};

// Force Cargo to include wasmtime-c-api as a dependency of this crate,
// even though it is only used by the C code.
#[expect(unused, reason = "forces Cargo to link wasmtime-c-api")]
fn use_wasmtime() {
    wasmtime_c_api::wasm_engine_new();
}

#[repr(C)]
#[derive(Clone)]
#[allow(non_camel_case_types)]
pub struct wasm_engine_t {
    pub(crate) engine: wasmtime::Engine,
}

pub struct WasmStore(*mut ffi::TSWasmStore);

// fastedit patch: a way to stop the Wasm code a thread runs, so a grammar
// whose initializer, lexer or scanner never returns can be stopped. Each store
// asks it once an epoch while Wasm code runs, so it only takes effect on an
// engine with epoch interruption on, whose epoch something increments
// regularly (`Engine::increment_epoch`).
thread_local! {
    static INTERRUPT: Cell<Option<*const (dyn Fn() -> bool + 'static)>> =
        const { Cell::new(None) };
}

/// Runs `f`, with Wasm code this thread runs inside it asking `interrupt`
/// once an epoch whether to stop. Code it stops fails: a load or
/// `Parser::set_language` returns an error, a parse `None`. A panic in
/// `interrupt` stops the code too; it isn't resumed.
///
/// This is a fastedit patch; see `vendor/tree-sitter/README.md`.
pub fn with_wasm_interrupt<R>(interrupt: &dyn Fn() -> bool, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<*const (dyn Fn() -> bool + 'static)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            INTERRUPT.with(|cell| cell.set(self.0));
        }
    }
    // SAFETY: only the lifetime is erased. The pointer is read only while
    // `f` runs, on this thread, and `Restore` puts the previous one back
    // before `interrupt` goes out of scope, even if `f` panics.
    let interrupt: *const (dyn Fn() -> bool + 'static) =
        unsafe { mem::transmute(interrupt as *const (dyn Fn() -> bool + '_)) };
    let _restore = Restore(INTERRUPT.with(|cell| cell.replace(Some(interrupt))));
    f()
}

/// Called by `wasm_store.c` each epoch while Wasm code runs: whether to stop
/// it ([`with_wasm_interrupt`]).
#[unsafe(no_mangle)]
extern "C" fn ts_wasm_thread_interrupted() -> bool {
    let Some(interrupt) = INTERRUPT.try_with(Cell::get).ok().flatten() else {
        return false;
    };
    // SAFETY: see `with_wasm_interrupt`. A panic can't unwind into C.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*interrupt)() }))
        .unwrap_or(true)
}

unsafe impl Send for WasmStore {}
unsafe impl Sync for WasmStore {}

#[derive(Debug, PartialEq, Eq)]
pub struct WasmError {
    pub kind: WasmErrorKind,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WasmErrorKind {
    Parse,
    Compile,
    Instantiate,
    Other,
}

impl WasmStore {
    pub fn new(engine: &wasmtime::Engine) -> Result<Self, WasmError> {
        unsafe {
            let mut error = MaybeUninit::<ffi::TSWasmError>::uninit();
            let store = ffi::ts_wasm_store_new(
                std::ptr::from_ref::<wasmtime::Engine>(engine)
                    .cast_mut()
                    .cast(),
                error.as_mut_ptr(),
            );
            if store.is_null() {
                Err(WasmError::new(error.assume_init()))
            } else {
                Ok(Self(store))
            }
        }
    }

    pub fn load_language(&mut self, name: &str, bytes: &[u8]) -> Result<Language, WasmError> {
        let name = CString::new(name).unwrap();
        unsafe {
            let mut error = MaybeUninit::<ffi::TSWasmError>::uninit();
            let language = ffi::ts_wasm_store_load_language(
                self.0,
                name.as_ptr(),
                bytes.as_ptr().cast::<c_char>(),
                bytes.len() as u32,
                error.as_mut_ptr(),
            );
            if language.is_null() {
                Err(WasmError::new(error.assume_init()))
            } else {
                Ok(Language(language))
            }
        }
    }

    #[must_use]
    pub fn language_count(&self) -> usize {
        unsafe { ffi::ts_wasm_store_language_count(self.0) }
    }
}

impl WasmError {
    unsafe fn new(error: ffi::TSWasmError) -> Self {
        let message = unsafe { CStr::from_ptr(error.message) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { ts_free(error.message.cast()) };
        Self {
            kind: match error.kind {
                ffi::TSWasmErrorKindParse => WasmErrorKind::Parse,
                ffi::TSWasmErrorKindCompile => WasmErrorKind::Compile,
                ffi::TSWasmErrorKindInstantiate => WasmErrorKind::Instantiate,
                _ => WasmErrorKind::Other,
            },
            message,
        }
    }
}

impl Language {
    #[must_use]
    pub fn is_wasm(&self) -> bool {
        unsafe { ffi::ts_language_is_wasm(self.0) }
    }
}

impl Parser {
    pub fn set_wasm_store(&mut self, store: WasmStore) -> Result<(), LanguageError> {
        unsafe { ffi::ts_parser_set_wasm_store(self.0.as_ptr(), store.0) };
        mem::forget(store);
        Ok(())
    }

    pub fn take_wasm_store(&mut self) -> Option<WasmStore> {
        let ptr = unsafe { ffi::ts_parser_take_wasm_store(self.0.as_ptr()) };
        if ptr.is_null() {
            None
        } else {
            Some(WasmStore(ptr))
        }
    }
}

impl Drop for WasmStore {
    fn drop(&mut self) {
        unsafe { ffi::ts_wasm_store_delete(self.0) };
    }
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let kind = match self.kind {
            WasmErrorKind::Parse => "Failed to parse Wasm",
            WasmErrorKind::Compile => "Failed to compile Wasm",
            WasmErrorKind::Instantiate => "Failed to instantiate Wasm module",
            WasmErrorKind::Other => "Unknown error",
        };
        write!(f, "{kind}: {}", self.message)
    }
}

impl error::Error for WasmError {}
