//! An accepted stack holds one structural tree (the root) besides extras and
//! the end of input, or an ERROR root that error recovery made of the whole
//! stack. A corrupt grammar that accepts a stack with stray structural trees
//! under a root fails to parse; error recovery still parses. The module
//! patched is `tree-sitter-json.wasm`; see `fixtures/tree-sitter-json.wasm.md`.

use std::{fs, path::Path, sync::Once, time::Duration};

use fe_buffer::Buffer;
use fe_syntax::{GrammarDeadlines, GrammarSpec, Syntax, register_language, set_grammar_deadlines};

const SYMBOL_COUNT: usize = 4;
const TOKEN_COUNT: usize = 12;
const STATE_COUNT: usize = 20;
const LARGE_STATE_COUNT: usize = 24;
const PARSE_TABLE: usize = 40;
const PARSE_ACTIONS: usize = 52;
const SYMBOL_NAMES: usize = 56;

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

    fn u8(&self, at: usize) -> u8 {
        self.bytes[self.data + at]
    }

    fn set_u8(&mut self, at: usize, value: u8) {
        self.bytes[self.data + at] = value;
    }

    fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes(self.bytes[self.data + at..][..2].try_into().unwrap())
    }

    fn set_u16(&mut self, at: usize, value: u16) {
        self.bytes[self.data + at..][..2].copy_from_slice(&value.to_le_bytes());
    }

    fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.bytes[self.data + at..][..4].try_into().unwrap())
    }

    fn count(&self, field: usize) -> usize {
        self.u32(self.language + field) as usize
    }

    fn table(&self, field: usize) -> usize {
        self.u32(self.language + field) as usize
    }

    fn cell(&self, state: usize, symbol: usize) -> usize {
        assert!(state < self.count(LARGE_STATE_COUNT), "state {state}");
        self.table(PARSE_TABLE) + 2 * (state * self.count(SYMBOL_COUNT) + symbol)
    }

    fn symbol(&self, name: &str) -> usize {
        let names = self.table(SYMBOL_NAMES);
        (0..self.count(SYMBOL_COUNT))
            .find(|&i| {
                let at = self.u32(names + 4 * i) as usize;
                let bytes = &self.bytes[self.data + at..];
                bytes.starts_with(name.as_bytes()) && bytes[name.len()] == 0
            })
            .unwrap()
    }
}

/// A parse of `text` in the JSON grammar patched by `patch` finishes: whether
/// it failed.
fn parse(case: &str, patch: impl FnOnce(&mut Module), text: &'static str) -> bool {
    static SET: Once = Once::new();
    SET.call_once(|| {
        set_grammar_deadlines(GrammarDeadlines {
            load: Duration::from_secs(5),
            parse: Duration::from_secs(5),
        });
    });
    let mut module = Module::load();
    patch(&mut module);
    let dir = std::env::temp_dir().join(format!("fe-syntax-accepted-stack-{}", std::process::id()));
    let dir = dir.join(case);
    fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("tree-sitter-json.wasm");
    fs::write(&wasm, &module.bytes).unwrap();
    let language = register_language(&GrammarSpec {
        language: case.into(),
        wasm,
        extensions: vec![case.into()],
        ..GrammarSpec::default()
    })
    .unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let buffer = Buffer::new(text);
        let mut syntax = Syntax::new(language, &buffer.snapshot());
        syntax.finish(syntax.parse_job().run());
        send.send(syntax.failed())
    });
    receive
        .recv_timeout(Duration::from_secs(30))
        .expect("the parse finishes")
}

#[test]
fn a_stack_with_stray_structural_trees_fails_to_parse() {
    // "1 2" is two values on the stack (the states 2 and 3). At its end the
    // reduce of the second into a document of production 0 (which has no
    // alias sequence to overrun) goes to the accepting state 30 from the
    // first value's state: the first value is left beside the root.
    let stray = |m: &mut Module| {
        let document = m.symbol("document");
        let reduce = m.table(PARSE_ACTIONS) + 8 * (m.u16(m.cell(3, 0)) as usize + 1);
        assert_eq!(m.u8(reduce), 1, "a reduce");
        m.set_u8(reduce + 1, 1);
        m.set_u16(reduce + 2, document as u16);
        m.set_u16(reduce + 6, 0);
        let goto = m.cell(2, document);
        m.set_u16(goto, 30);
    };
    assert!(parse("stray-trees", stray, "1 2\n"));
    // The same grammar, unpatched, parses valid text and recovers from errors
    // (an ERROR root takes the whole stack, however many trees it holds).
    assert!(!parse("valid", |_| {}, "[1, 2]\n"));
    assert!(!parse("recovers", |_| {}, "[1 2 }\n"));
    assert!(!parse("recovers-at-end", |_| {}, "{\"a\": [1, 2\n"));
}
