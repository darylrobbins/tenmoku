//! A reachable state with no lex state (`UINT16_MAX`) and an empty EOF entry
//! gives the parser no lookahead and no action: the parse of a grammar with
//! one ends, rather than reading the missing lookahead. The module patched is
//! `tree-sitter-json.wasm`; see `fixtures/tree-sitter-json.wasm.md`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_buffer::Buffer;
use fe_syntax::{GrammarDeadlines, GrammarSpec, Syntax, register_language, set_grammar_deadlines};

const SYMBOL_COUNT: usize = 4;
const TOKEN_COUNT: usize = 12;
const STATE_COUNT: usize = 20;
const LARGE_STATE_COUNT: usize = 24;
const PARSE_TABLE: usize = 40;
const LEX_MODES: usize = 88;

struct Module {
    bytes: Vec<u8>,
    data: usize,
    language: usize,
}

fn leb(bytes: &[u8], at: &mut usize) -> usize {
    let (mut value, mut shift) = (0usize, 0);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= usize::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

impl Module {
    fn load() -> Self {
        // Read at run time: a path compiled in can name a deleted snapshot.
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
        let file = Path::new(&manifest).join("tests/fixtures/tree-sitter-json.wasm");
        let bytes = fs::read(file).unwrap();
        // The data section is id 11, holding one active segment.
        let mut at = 8;
        let data = loop {
            let id = bytes[at];
            at += 1;
            let size = leb(&bytes, &mut at);
            if id == 11 {
                let mut p = at;
                assert_eq!(leb(&bytes, &mut p), 1, "one segment");
                assert_eq!(leb(&bytes, &mut p), 0, "an active segment");
                while bytes[p] != 0x0b {
                    p += 1;
                }
                p += 1;
                leb(&bytes, &mut p);
                break p;
            }
            at += size;
        };
        let mut module = Self {
            bytes,
            data,
            language: 0,
        };
        let segment = module.bytes.len() - module.data;
        let found: Vec<usize> = (0..segment.saturating_sub(40))
            .step_by(4)
            .filter(|&at| {
                let abi = module.u32(at);
                let (symbols, tokens, states, large) = (
                    module.u32(at + SYMBOL_COUNT),
                    module.u32(at + TOKEN_COUNT),
                    module.u32(at + STATE_COUNT),
                    module.u32(at + LARGE_STATE_COUNT),
                );
                (14..=15).contains(&abi)
                    && (1..500).contains(&symbols)
                    && (1..=symbols).contains(&tokens)
                    && (2..500).contains(&states)
                    && large <= states
                    && (module.u32(at + PARSE_TABLE) as usize) < segment
            })
            .collect();
        assert_eq!(found.len(), 1, "{found:?}");
        module.language = found[0];
        module
    }

    fn set_u16(&mut self, at: usize, value: u16) {
        self.bytes[self.data + at..][..2].copy_from_slice(&value.to_le_bytes());
    }

    fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.bytes[self.data + at..][..4].try_into().unwrap())
    }

    fn field(&self, field: usize) -> usize {
        self.u32(self.language + field) as usize
    }
}

/// Removes its folder when the test ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_state_with_no_lex_state_and_an_empty_eof_entry_is_an_error() {
    set_grammar_deadlines(GrammarDeadlines {
        load: Duration::from_secs(5),
        parse: Duration::from_secs(5),
    });
    let mut m = Module::load();
    // State 1, the first state, has no lex state and no entry for the end of
    // input: the parser has no lookahead and nothing to do with one.
    let modes = m.field(LEX_MODES);
    m.set_u16(modes + 4, 0xffff);
    assert!(m.field(LARGE_STATE_COUNT) > 1, "state 1 is a large state");
    let eof = m.field(PARSE_TABLE) + 2 * m.field(SYMBOL_COUNT);
    m.set_u16(eof, 0);

    let dir: PathBuf =
        std::env::temp_dir().join(format!("fe-syntax-empty-eof-{}", std::process::id()));
    let _scratch = Scratch(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("tree-sitter-json.wasm");
    fs::write(&wasm, &m.bytes).unwrap();
    let spec = GrammarSpec {
        language: "empty-eof".into(),
        wasm,
        extensions: vec!["empty-eof".into()],
        ..GrammarSpec::default()
    };
    // Either the load refuses it or the parse fails; neither reads NULL.
    let Ok(language) = register_language(&spec) else {
        return;
    };
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let buffer = Buffer::new("{}\n");
        let mut syntax = Syntax::new(language, &buffer.snapshot());
        syntax.finish(syntax.parse_job().run());
        send.send(syntax.failed())
    });
    let failed = receive
        .recv_timeout(Duration::from_secs(30))
        .expect("the parse finishes");
    assert!(failed, "the parse is an error");
}
