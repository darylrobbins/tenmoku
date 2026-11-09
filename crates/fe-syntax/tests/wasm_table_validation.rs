//! A Wasm grammar's static tables are checked when it's loaded: each hostile
//! grammar here has one table corrupted (by patching a valid module's data
//! segment in the test) and is an error naming its file, not an out-of-bounds
//! read in the parser.
//!
//! The modules patched are `tree-sitter-json.wasm` (ABI 14, with fields,
//! aliases and small states), `tree-sitter-badlex.wasm` (ABI 15) and
//! `tree-sitter-badscan.wasm` (ABI 15, with an external scanner); see
//! `fixtures/tree-sitter-json.wasm.md` and `fixtures/hostile-grammars.md`.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Once,
    time::Duration,
};

use fe_buffer::Buffer;
use fe_syntax::{
    GrammarDeadlines, GrammarError, GrammarSpec, Syntax, register_language, set_grammar_deadlines,
};

/// Where the language's data segment lands in a fresh store's memory: its
/// `__memory_base`. Pointers the module's relocations don't cover (the null
/// ones) are absolute, so a patch that points one at the module's own data
/// adds this. `a_supertype_table_is_read_where_the_patch_points` fails if
/// it's wrong.
const MEMORY_BASE: u32 = 131_100;

// Offsets of the fields of a `TSLanguage` compiled to wasm32.
const SYMBOL_COUNT: usize = 4;
const TOKEN_COUNT: usize = 12;
const EXTERNAL_TOKEN_COUNT: usize = 16;
const STATE_COUNT: usize = 20;
const LARGE_STATE_COUNT: usize = 24;
const PRODUCTION_ID_COUNT: usize = 28;
const FIELD_COUNT: usize = 32;
const PARSE_TABLE: usize = 40;
const SMALL_PARSE_TABLE: usize = 44;
const SMALL_PARSE_TABLE_MAP: usize = 48;
const PARSE_ACTIONS: usize = 52;
const SYMBOL_NAMES: usize = 56;
const FIELD_NAMES: usize = 60;
const FIELD_MAP_SLICES: usize = 64;
const FIELD_MAP_ENTRIES: usize = 68;
const SYMBOL_METADATA: usize = 72;
const PUBLIC_SYMBOL_MAP: usize = 76;
const ALIAS_MAP: usize = 80;
const ALIAS_SEQUENCES: usize = 84;
const LEX_MODES: usize = 88;
const MAX_ALIAS_SEQUENCE_LENGTH: usize = 36;
const ALIAS_COUNT: usize = 8;
const KEYWORD_CAPTURE_TOKEN: usize = 100;
const EXTERNAL_STATES: usize = 104;
const EXTERNAL_SYMBOL_MAP: usize = 108;
const PRIMARY_STATE_IDS: usize = 132;
const NAME: usize = 136;
const RESERVED_WORDS: usize = 140;
const MAX_RESERVED_WORD_SET_SIZE: usize = 144;
const SUPERTYPE_COUNT: usize = 148;
const SUPERTYPE_SYMBOLS: usize = 152;
const SUPERTYPE_MAP_SLICES: usize = 156;
const SUPERTYPE_MAP_ENTRIES: usize = 160;

/// A copy of a fixture module, patched through offsets in its data segment.
struct Module {
    bytes: Vec<u8>,
    /// The data segment's offset in `bytes`.
    data: usize,
    /// The code section's range in `bytes`.
    code: std::ops::Range<usize>,
    /// The language's, in the segment.
    language: usize,
}

fn fixture_file(name: &str) -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("tests/fixtures").join(name)
}

/// `value` as a signed LEB128.
fn sleb(mut value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0) {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
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
    fn load(grammar: &str) -> Self {
        let bytes = fs::read(fixture_file(&format!("tree-sitter-{grammar}.wasm"))).unwrap();
        // The sections after the 8-byte header; the data section is id 11,
        // holding one active segment at `__memory_base`.
        let mut at = 8;
        let mut code = 0..0;
        let data = loop {
            let id = bytes[at];
            at += 1;
            let size = leb(&bytes, &mut at);
            if id == 10 {
                code = at..at + size;
            }
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
            code,
            language: 0,
        };
        // The language: an ABI version, then plausible counts, then a
        // parse table inside the segment.
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
        assert_eq!(found.len(), 1, "{grammar}: {found:?}");
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

    fn set_u32(&mut self, at: usize, value: u32) {
        self.bytes[self.data + at..][..4].copy_from_slice(&value.to_le_bytes());
    }

    /// The language's field at `field`, a count.
    fn count(&self, field: usize) -> usize {
        self.u32(self.language + field) as usize
    }

    fn set_count(&mut self, field: usize, value: u32) {
        self.set_u32(self.language + field, value);
    }

    /// Where the table `field` points, in the segment.
    fn table(&self, field: usize) -> usize {
        self.u32(self.language + field) as usize
    }

    /// Points `field` at `at` in the segment, for a field the module leaves
    /// null, which its relocations don't cover (so the address is absolute).
    fn point_at(&mut self, field: usize, at: usize) {
        self.set_count(field, MEMORY_BASE + at as u32);
    }

    /// Makes the pointer in the data at `slot` the raw `value`. The module's
    /// `__wasm_apply_data_relocs` stores each pointer as `__memory_base` plus
    /// a constant, so for a pointer it relocates, the store is dropped.
    fn set_pointer(&mut self, slot: usize, value: u32) {
        let slot_leb = sleb(slot as i64);
        // Either `global.get 0; i32.const slot; i32.add` or `i32.const slot;
        // global.get 0; i32.add`, then `global.get 0; i32.const K; i32.add`.
        let spellings = [
            [
                &[0x23, 0x00, 0x41][..],
                &slot_leb,
                &[0x6a, 0x23, 0x00, 0x41],
            ]
            .concat(),
            [
                &[0x41][..],
                &slot_leb,
                &[0x23, 0x00, 0x6a, 0x23, 0x00, 0x41],
            ]
            .concat(),
        ];
        let code = &mut self.bytes[self.code.clone()];
        for needle in spellings {
            let Some(found) = code.windows(needle.len()).position(|w| w == needle) else {
                continue;
            };
            let mut at = found + needle.len();
            while code[at] & 0x80 != 0 {
                at += 1;
            }
            at += 2; // The last byte of the constant, and `i32.add`.
            if code[at..at + 2] == [0x22, 0x00] {
                at += 2; // `local.tee 0`.
            }
            assert_eq!(code[at..at + 3], [0x36, 0x02, 0x00], "an i32.store");
            // `drop`, `drop`, `nop`.
            code[at..at + 3].copy_from_slice(&[0x1a, 0x1a, 0x01]);
            break;
        }
        self.set_u32(slot, value);
    }

    /// Points the pointer at `slot` outside the module's memory.
    fn point_slot_away(&mut self, slot: usize) {
        self.set_pointer(slot, 0x7fff_fff0);
    }

    /// Points the language's `field` outside the module's memory.
    fn point_away(&mut self, field: usize) {
        self.point_slot_away(self.language + field);
    }

    /// Where the first parse table cell of a symbol `terminal` or not that
    /// isn't empty is, in the large states.
    fn large_cell(&self, terminal: bool) -> usize {
        let (symbols, tokens) = (self.count(SYMBOL_COUNT), self.count(TOKEN_COUNT));
        let table = self.table(PARSE_TABLE);
        for state in 0..self.count(LARGE_STATE_COUNT) {
            for symbol in 0..symbols {
                let at = table + 2 * (state * symbols + symbol);
                if (symbol < tokens) == terminal && self.u16(at) != 0 {
                    return at;
                }
            }
        }
        panic!("no cell");
    }

    /// Where the first group of the first small state that has one starts,
    /// as `(group_count, first group)` positions in the segment.
    fn small_group(&self) -> (usize, usize) {
        let large = self.count(LARGE_STATE_COUNT);
        let map = self.table(SMALL_PARSE_TABLE_MAP);
        for state in 0..self.count(STATE_COUNT) - large {
            let index = self.u32(map + 4 * state) as usize;
            let at = self.table(SMALL_PARSE_TABLE) + 2 * index;
            if self.u16(at) > 0 {
                return (at, at + 2);
            }
        }
        panic!("no small state");
    }

    /// The actions entries the parse tables name, as indexes.
    /// The first entry of just an accept.
    fn accept_entry(&self) -> usize {
        let actions = self.table(PARSE_ACTIONS);
        (1..)
            .find(|&e| self.u8(actions + 8 * e) == 1 && self.u8(actions + 8 * (e + 1)) == 2)
            .unwrap()
    }

    /// The symbol called `name`.
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

    /// Where the reduces to the symbol called `name` are in the actions.
    fn reduces(&self, name: &str) -> Vec<usize> {
        let symbol = self.symbol(name) as u16;
        let actions = self.table(PARSE_ACTIONS);
        let mut found = Vec::new();
        for entry in self.action_entries() {
            let header = actions + 8 * entry;
            for i in 0..self.u8(header) as usize {
                let at = header + 8 * (1 + i);
                if self.u8(at) == 1 && self.u16(at + 2) == symbol && !found.contains(&at) {
                    found.push(at);
                }
            }
        }
        found
    }

    /// Where the cell of `state` (a large one) for `symbol` is.
    fn cell(&self, state: usize, symbol: usize) -> usize {
        assert!(state < self.count(LARGE_STATE_COUNT), "state {state}");
        self.table(PARSE_TABLE) + 2 * (state * self.count(SYMBOL_COUNT) + symbol)
    }

    fn action_entries(&self) -> Vec<usize> {
        let (symbols, tokens) = (self.count(SYMBOL_COUNT), self.count(TOKEN_COUNT));
        let mut entries = Vec::new();
        let table = self.table(PARSE_TABLE);
        for state in 0..self.count(LARGE_STATE_COUNT) {
            for symbol in 0..tokens {
                let value = self.u16(table + 2 * (state * symbols + symbol));
                if value != 0 {
                    entries.push(value as usize);
                }
            }
        }
        let large = self.count(LARGE_STATE_COUNT);
        let map = self.table(SMALL_PARSE_TABLE_MAP);
        for state in 0..self.count(STATE_COUNT) - large {
            let mut at = self.table(SMALL_PARSE_TABLE) + 2 * self.u32(map + 4 * state) as usize;
            let groups = self.u16(at);
            at += 2;
            for _ in 0..groups {
                let (value, count) = (self.u16(at), self.u16(at + 2) as usize);
                at += 4;
                for symbol in 0..count {
                    if (self.u16(at + 2 * symbol) as usize) < tokens {
                        entries.push(value as usize);
                    }
                }
                at += 2 * count;
            }
        }
        entries
    }

    /// Where the first action `wanted` accepts is in the actions table.
    fn action(&self, wanted: impl Fn(&Module, usize) -> bool) -> usize {
        let actions = self.table(PARSE_ACTIONS);
        for entry in self.action_entries() {
            let header = actions + 8 * entry;
            for i in 0..self.u8(header) as usize {
                let at = header + 8 * (1 + i);
                if wanted(self, at) {
                    return at;
                }
            }
        }
        panic!("no such action");
    }
}

fn short_deadlines() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        set_grammar_deadlines(GrammarDeadlines {
            load: Duration::from_secs(5),
            parse: Duration::from_secs(5),
        });
    });
}

/// A folder for one case's module, removed when the case is done.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes `grammar` patched by `patch` for a grammar called `case`.
fn spec(case: &str, grammar: &str, patch: impl FnOnce(&mut Module)) -> (Scratch, GrammarSpec) {
    short_deadlines();
    let mut module = Module::load(grammar);
    patch(&mut module);
    let dir = std::env::temp_dir().join(format!(
        "fe-syntax-table-validation-{}-{case}",
        std::process::id()
    ));
    let scratch = Scratch(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join(format!("tree-sitter-{grammar}.wasm"));
    fs::write(&wasm, &module.bytes).unwrap();
    let spec = GrammarSpec {
        language: case.into(),
        wasm,
        extensions: vec![case.into()],
        ..GrammarSpec::default()
    };
    (scratch, spec)
}

/// Loads `grammar` patched by `patch` as a grammar called `case`.
fn load(
    case: &str,
    grammar: &str,
    patch: impl FnOnce(&mut Module),
) -> (PathBuf, Result<(), GrammarError>) {
    let (_scratch, spec) = spec(case, grammar, patch);
    let result = register_language(&spec).map(|_| ());
    (spec.wasm, result)
}

/// The patched grammar is an error naming its file and saying `expected`.
fn rejected(case: &str, grammar: &str, patch: impl FnOnce(&mut Module), expected: &str) {
    match load(case, grammar, patch) {
        (wasm, Err(error @ GrammarError::Module { .. })) => {
            assert_eq!(error.file(), Some(wasm.as_path()), "{case}");
            let message = error.to_string();
            assert!(message.contains(expected), "{case}: {message}");
        }
        (_, other) => panic!("{case}: {other:?}"),
    }
}

#[test]
fn the_unpatched_fixtures_load() {
    for grammar in ["json", "badlex", "badscan"] {
        let (_, result) = load(&format!("plain-{grammar}"), grammar, |_| {});
        result.unwrap_or_else(|error| panic!("{grammar}: {error}"));
    }
}

#[test]
fn counts_the_tables_cant_have_are_errors() {
    for (field, value) in [
        (SYMBOL_COUNT, 0),
        (SYMBOL_COUNT, 0xffff),
        (TOKEN_COUNT, 0),
        (TOKEN_COUNT, 26),
        (EXTERNAL_TOKEN_COUNT, 16),
        (STATE_COUNT, 1),
        (STATE_COUNT, 70_000),
        (LARGE_STATE_COUNT, 33),
        (PRODUCTION_ID_COUNT, 70_000),
        (PRODUCTION_ID_COUNT, 0),
        (FIELD_COUNT, 70_000),
    ] {
        rejected(
            &format!("count-{field}-{value}"),
            "json",
            |m| m.set_count(field, value),
            "invalid language tables",
        );
    }
    rejected(
        "count-supertypes",
        "badlex",
        |m| m.set_count(SUPERTYPE_COUNT, 100),
        "invalid language tables",
    );
}

#[test]
fn a_parse_table_naming_what_isnt_there_is_an_error() {
    rejected(
        "parse-table-state",
        "json",
        |m| {
            let cell = m.large_cell(false);
            m.set_u16(cell, 0xffff);
        },
        "invalid parse table: a state",
    );
    rejected(
        "parse-table-action",
        "json",
        |m| {
            let cell = m.large_cell(true);
            m.set_u16(cell, 0xffff);
        },
        "invalid parse table: an action index",
    );
    // A table that isn't there.
    rejected(
        "parse-table-away",
        "json",
        |m| m.point_away(PARSE_TABLE),
        "invalid language memory address",
    );
}

#[test]
fn a_small_parse_table_naming_what_isnt_there_is_an_error() {
    rejected(
        "small-map-entry",
        "json",
        |m| {
            let map = m.table(SMALL_PARSE_TABLE_MAP);
            m.set_u32(map, 0x00ff_ffff);
        },
        "invalid small parse table map",
    );
    rejected(
        "small-group-runs-over",
        "json",
        |m| {
            let (_, group) = m.small_group();
            m.set_u16(group + 2, 0xffff);
        },
        "invalid small parse table: a group runs past the end",
    );
    rejected(
        "small-group-count-runs-over",
        "json",
        |m| {
            let (count, _) = m.small_group();
            m.set_u16(count, 0xffff);
        },
        "invalid",
    );
    rejected(
        "small-group-value",
        "json",
        |m| {
            let (_, group) = m.small_group();
            m.set_u16(group, 0xffff);
        },
        "invalid parse table:",
    );
    rejected(
        "small-table-away",
        "json",
        |m| m.point_away(SMALL_PARSE_TABLE_MAP),
        "invalid language memory address",
    );
}

#[test]
fn parse_actions_naming_what_isnt_there_are_errors() {
    // A shift to a state the grammar hasn't.
    rejected(
        "action-shift",
        "json",
        |m| {
            let at = m.action(|m, at| m.u8(at) == 0 && m.u8(at + 4) == 0);
            m.set_u16(at + 2, 0xffff);
        },
        "invalid parse actions: a shift to a state",
    );
    // A reduce to a symbol, of a production, and of more children than the
    // alias sequences hold.
    rejected(
        "action-reduce-symbol",
        "json",
        |m| {
            let at = m.action(|m, at| m.u8(at) == 1);
            m.set_u16(at + 2, 0xffff);
        },
        "invalid parse actions: a reduce to a symbol",
    );
    rejected(
        "action-reduce-production",
        "json",
        |m| {
            let at = m.action(|m, at| m.u8(at) == 1);
            m.set_u16(at + 6, 0xffff);
        },
        "invalid parse actions: a reduce of a production",
    );
    rejected(
        "action-reduce-children",
        "json",
        |m| {
            let at = m.action(|m, at| m.u8(at) == 1 && m.u16(at + 6) != 0);
            m.set_u8(at + 1, 200);
        },
        "invalid parse actions: a reduce of more children",
    );
    rejected(
        "action-unknown-type",
        "json",
        |m| {
            let at = m.action(|_, _| true);
            m.set_u8(at, 9);
        },
        "invalid parse actions: an action of an unknown type",
    );
    // An entry whose actions run off the end of the table.
    rejected(
        "action-entry-runs-over",
        "json",
        |m| {
            let entry = m.action_entries()[0];
            let header = m.table(PARSE_ACTIONS) + 8 * entry;
            m.set_u8(header, 255);
        },
        "invalid parse actions: an entry runs past",
    );
    rejected(
        "actions-away",
        "json",
        |m| m.point_away(PARSE_ACTIONS),
        "invalid language memory address",
    );
}

#[test]
fn lex_modes_naming_what_isnt_there_are_errors() {
    // ABI 14: 4-byte modes, and the grammar has no external scanner.
    rejected(
        "lex-modes-14",
        "json",
        |m| {
            let modes = m.table(LEX_MODES);
            m.set_u16(modes + 4 + 2, 1);
        },
        "invalid lex modes: an external lex state",
    );
    // ABI 15: 6-byte modes, and one external token.
    rejected(
        "lex-modes-15",
        "badscan",
        |m| {
            let modes = m.table(LEX_MODES);
            m.set_u16(modes + 6 + 2, 0xffff);
        },
        "invalid lex modes: an external lex state",
    );
    rejected(
        "lex-modes-away",
        "json",
        |m| m.point_away(LEX_MODES),
        "invalid language memory address",
    );
    // A reserved word set the table of reserved words can't hold.
    rejected(
        "reserved-words-away",
        "badlex",
        |m| {
            m.set_u16(m.language + MAX_RESERVED_WORD_SET_SIZE, 1);
            let modes = m.table(LEX_MODES);
            m.set_u16(modes + 6 + 4, 3);
            m.point_away(RESERVED_WORDS);
        },
        "invalid language memory address",
    );
    rejected(
        "keyword-capture-token",
        "json",
        |m| m.set_u16(m.language + KEYWORD_CAPTURE_TOKEN, 0x0fff),
        "invalid keyword capture token",
    );
}

#[test]
fn primary_state_ids_naming_a_missing_state_are_errors() {
    for (case, grammar) in [("primary-14", "json"), ("primary-15", "badlex")] {
        rejected(
            case,
            grammar,
            |m| {
                let ids = m.table(PRIMARY_STATE_IDS);
                m.set_u16(ids + 2, 0xffff);
            },
            "invalid primary state ids: a state",
        );
    }
    rejected(
        "primary-away",
        "badlex",
        |m| m.point_away(PRIMARY_STATE_IDS),
        "invalid language memory address",
    );
}

#[test]
fn an_external_scanner_symbol_map_naming_a_missing_symbol_is_an_error() {
    rejected(
        "external-symbol-map",
        "badscan",
        |m| {
            let map = m.table(EXTERNAL_SYMBOL_MAP);
            m.set_u16(map, 0xffff);
        },
        "invalid external scanner symbol map",
    );
    rejected(
        "external-symbol-map-away",
        "badscan",
        |m| m.point_away(EXTERNAL_SYMBOL_MAP),
        "invalid language memory address",
    );
}

#[test]
fn alias_tables_naming_what_isnt_there_are_errors() {
    rejected(
        "alias-sequences",
        "json",
        |m| {
            let sequences = m.table(ALIAS_SEQUENCES);
            m.set_u16(sequences + 2 * 5, 0xfff0);
        },
        "invalid alias sequences",
    );
    rejected(
        "alias-sequences-away",
        "json",
        |m| m.point_away(ALIAS_SEQUENCES),
        "invalid language memory address",
    );
    // An alias map is symbol, count, aliases..., ended by a null symbol.
    rejected(
        "alias-map-symbol",
        "json",
        |m| {
            let map = m.table(ALIAS_MAP);
            m.set_u16(map, 0x7000);
            m.set_u16(map + 2, 0);
            m.set_u16(map + 4, 0);
        },
        "invalid alias map: a symbol",
    );
    rejected(
        "alias-map-alias",
        "json",
        |m| {
            let map = m.table(ALIAS_MAP);
            m.set_u16(map, 3);
            m.set_u16(map + 2, 1);
            m.set_u16(map + 4, 0xfff0);
            m.set_u16(map + 6, 0);
        },
        "invalid alias map: an alias",
    );
    rejected(
        "alias-map-unended",
        "json",
        |m| {
            let map = m.table(ALIAS_MAP);
            m.set_u16(map, 3);
            m.set_u16(map + 2, 0xffff);
        },
        "invalid",
    );
}

#[test]
fn field_tables_naming_what_isnt_there_are_errors() {
    rejected(
        "field-entries",
        "json",
        |m| {
            let entries = m.table(FIELD_MAP_ENTRIES);
            m.set_u16(entries, 0x7000);
        },
        "invalid field map entries: a field",
    );
    // Slices past any entries the module could hold.
    rejected(
        "field-slices",
        "json",
        |m| {
            let slices = m.table(FIELD_MAP_SLICES);
            m.set_u16(slices + 4, 0xffff);
            m.set_u16(slices + 6, 0xffff);
        },
        "invalid language memory address",
    );
    rejected(
        "field-names-away",
        "json",
        |m| {
            let names = m.table(FIELD_NAMES);
            m.point_slot_away(names + 4);
        },
        "invalid language memory address",
    );
    rejected(
        "field-name-missing",
        "json",
        |m| {
            let names = m.table(FIELD_NAMES);
            m.set_pointer(names + 4, 0);
        },
        "invalid field names: a missing name",
    );
    rejected(
        "field-slices-away",
        "json",
        |m| m.point_away(FIELD_MAP_SLICES),
        "invalid language memory address",
    );
    rejected(
        "field-entries-away",
        "json",
        |m| m.point_away(FIELD_MAP_ENTRIES),
        "invalid language memory address",
    );
}

#[test]
fn symbol_tables_naming_what_isnt_there_are_errors() {
    rejected(
        "public-symbol-map",
        "json",
        |m| {
            let map = m.table(PUBLIC_SYMBOL_MAP);
            m.set_u16(map + 4, 0xffff);
        },
        "invalid public symbol map",
    );
    rejected(
        "symbol-name-away",
        "json",
        |m| {
            let names = m.table(SYMBOL_NAMES);
            m.point_slot_away(names + 8);
        },
        "invalid language memory address",
    );
    rejected(
        "symbol-name-missing",
        "json",
        |m| {
            let names = m.table(SYMBOL_NAMES);
            m.set_pointer(names + 8, 0);
        },
        "invalid symbol names: a missing name",
    );
    rejected(
        "symbol-names-away",
        "json",
        |m| m.point_away(SYMBOL_NAMES),
        "invalid language memory address",
    );
    rejected(
        "symbol-metadata-away",
        "json",
        |m| m.point_away(SYMBOL_METADATA),
        "invalid language memory address",
    );
    rejected(
        "name-away",
        "badlex",
        |m| m.point_away(NAME),
        "invalid language memory address",
    );
    // Names counted past the module's memory.
    rejected(
        "symbol-names-counted-away",
        "json",
        |m| m.set_count(SYMBOL_COUNT, 65_000),
        "invalid language memory address",
    );
    rejected(
        "field-names-counted-away",
        "json",
        |m| m.set_count(FIELD_COUNT, 65_000),
        "invalid language memory address",
    );
}

/// Lays a supertype table out in `badlex`'s unused alias sequences: the
/// symbols at 0, the slices at 4 and the entries at 8.
fn supertypes(m: &mut Module) -> usize {
    let scratch = m.table(ALIAS_MAP);
    m.set_count(SUPERTYPE_COUNT, 1);
    m.point_at(SUPERTYPE_SYMBOLS, scratch);
    m.point_at(SUPERTYPE_MAP_SLICES, scratch + 4);
    m.point_at(SUPERTYPE_MAP_ENTRIES, scratch + 8);
    for at in 0..16 {
        m.set_u8(scratch + at, 0);
    }
    scratch
}

#[test]
fn a_supertype_table_is_read_where_the_patch_points() {
    let (_, result) = load("supertype-ok", "badlex", |m| {
        let scratch = supertypes(m);
        m.set_u16(scratch + 4, 0);
        m.set_u16(scratch + 6, 1);
    });
    result.unwrap();
}

/// Gives `badlex` an alias and returns its id, the symbol count: a
/// supertype's subtypes can be aliases, as tree-sitter-c-sharp's are.
///
/// The name and metadata tables are then read one entry further. The names
/// end the data segment, so they start one entry earlier, in the padding
/// after the language, which moves each name along by one: a load doesn't
/// mind. The metadata runs into the public symbol map, whose second entry
/// would mark the alias a supertype unless it's symbol 0.
fn alias(m: &mut Module) -> u16 {
    m.set_count(ALIAS_COUNT, 1);
    let names = m.table(SYMBOL_NAMES);
    assert_eq!(m.u32(names - 4), 0, "padding before the names");
    m.set_u32(names - 4, MEMORY_BASE + m.u32(names));
    m.set_pointer(m.language + SYMBOL_NAMES, MEMORY_BASE + names as u32 - 4);
    let public = m.table(PUBLIC_SYMBOL_MAP);
    m.set_u16(public + 2, 0);
    m.count(SYMBOL_COUNT) as u16
}

#[test]
fn a_supertype_map_entry_can_be_an_alias() {
    let (_, result) = load("supertype-entry-alias", "badlex", |m| {
        let scratch = supertypes(m);
        m.set_u16(scratch + 6, 1);
        let alias = alias(m);
        m.set_u16(scratch + 8, alias);
    });
    result.unwrap();
}

#[test]
fn a_supertype_map_entry_past_the_aliases_is_an_error() {
    // One past the last name, with an alias and without.
    rejected(
        "supertype-entry-past-alias",
        "badlex",
        |m| {
            let scratch = supertypes(m);
            m.set_u16(scratch + 6, 1);
            let alias = alias(m);
            m.set_u16(scratch + 8, alias + 1);
        },
        "invalid supertype map entries",
    );
    rejected(
        "supertype-entry-no-alias",
        "badlex",
        |m| {
            let scratch = supertypes(m);
            m.set_u16(scratch + 6, 1);
            let past = m.count(SYMBOL_COUNT) as u16;
            m.set_u16(scratch + 8, past);
        },
        "invalid supertype map entries",
    );
}

#[test]
fn supertype_tables_naming_what_isnt_there_are_errors() {
    rejected(
        "supertype-symbol",
        "badlex",
        |m| {
            let scratch = supertypes(m);
            m.set_u16(scratch, 0xffff);
        },
        "invalid supertype symbols",
    );
    rejected(
        "supertype-entry",
        "badlex",
        |m| {
            let scratch = supertypes(m);
            m.set_u16(scratch + 6, 1);
            m.set_u16(scratch + 8, 0xffff);
        },
        "invalid supertype map entries",
    );
    rejected(
        "supertype-slices-away",
        "badlex",
        |m| {
            supertypes(m);
            m.point_away(SUPERTYPE_MAP_SLICES);
        },
        "invalid language memory address",
    );
    rejected(
        "supertype-entries-away",
        "badlex",
        |m| {
            let scratch = supertypes(m);
            m.set_u16(scratch + 6, 1);
            m.point_away(SUPERTYPE_MAP_ENTRIES);
        },
        "invalid language memory address",
    );
    rejected(
        "supertype-symbols-away",
        "badlex",
        |m| {
            supertypes(m);
            m.point_away(SUPERTYPE_SYMBOLS);
        },
        "invalid language memory address",
    );
    // A symbol marked a supertype that the supertype map doesn't hold: the
    // parser would look its subtypes up past the slices.
    rejected(
        "supertype-unlisted",
        "badlex",
        |m| {
            let metadata = m.table(SYMBOL_METADATA);
            m.set_u8(metadata + 3 + 2, 1);
        },
        "invalid symbol metadata: a supertype",
    );
}

#[test]
fn what_the_parser_reads_around_the_cells_is_checked_too() {
    // The empty action entry an absent terminal reads, which must be empty.
    rejected(
        "action-zero",
        "json",
        |m| {
            let actions = m.table(PARSE_ACTIONS);
            m.set_u8(actions, 3);
        },
        "invalid parse actions: a first entry",
    );
    // A small table's group of no symbols, and a symbol the grammar hasn't
    // under a value of zero.
    rejected(
        "small-group-empty",
        "json",
        |m| {
            let (_, group) = m.small_group();
            m.set_u16(group + 2, 0);
        },
        "invalid",
    );
    rejected(
        "small-group-zero-value-symbol",
        "json",
        |m| {
            let (_, group) = m.small_group();
            m.set_u16(group, 0);
            m.set_u16(group + 4, 0xfff0);
        },
        "invalid parse table: a symbol",
    );
    // An accept on a token that isn't the end of input.
    rejected(
        "action-accept",
        "json",
        |m| {
            let at = m.action(|m, at| m.u8(at) == 0 && m.u8(at + 4) == 0);
            m.set_u8(at, 2);
        },
        "invalid parse actions: an accept",
    );
    // The alias map is read whatever the alias sequences' dimensions.
    rejected(
        "alias-map-no-sequences",
        "json",
        |m| {
            m.set_u16(m.language + MAX_ALIAS_SEQUENCE_LENGTH, 0);
            let map = m.table(ALIAS_MAP);
            m.set_u16(map, 0x7000);
            m.set_u16(map + 2, 0);
            m.set_u16(map + 4, 0);
        },
        "invalid alias map: a symbol",
    );
}

#[test]
fn external_tokens_and_reserved_words_name_terminals() {
    // An external token that is a nonterminal.
    rejected(
        "external-nonterminal",
        "badscan",
        |m| {
            let map = m.table(EXTERNAL_SYMBOL_MAP);
            m.set_u16(map, 4);
        },
        "invalid external scanner symbol map",
    );
    // A reserved word set holding a symbol the grammar hasn't.
    rejected(
        "reserved-word-symbol",
        "badlex",
        |m| {
            let scratch = m.table(ALIAS_MAP) + 8;
            m.set_u16(scratch, 0xffff);
            m.set_u16(m.language + MAX_RESERVED_WORD_SET_SIZE, 1);
            let modes = m.table(LEX_MODES);
            m.set_u16(modes + 6 + 4, 1);
            m.point_at(RESERVED_WORDS, scratch);
        },
        "invalid reserved words",
    );
    // A symbol past the grammar's own, marked a supertype.
    rejected(
        "supertype-alias",
        "badlex",
        |m| {
            // The grammar's last symbol becomes an alias.
            m.set_count(SYMBOL_COUNT, 3);
            m.set_count(ALIAS_COUNT, 1);
            let metadata = m.table(SYMBOL_METADATA);
            m.set_u8(metadata + 3 * 3 + 2, 1);
        },
        "invalid symbol metadata: a supertype",
    );
}

/// A parse of `text` in the patched JSON grammar finishes: whether it failed.
fn parse(case: &str, patch: impl FnOnce(&mut Module), text: &'static str) -> bool {
    let (_scratch, spec) = spec(case, "json", patch);
    let language = register_language(&spec).unwrap();
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

/// A parse of `text` in the patched JSON grammar finishes, as a failure.
fn parse_fails(case: &str, patch: impl FnOnce(&mut Module), text: &'static str) {
    assert!(parse(case, patch, text), "{case}");
}

#[test]
fn an_accept_without_a_lookahead_stops_the_parse() {
    // The first state has no lookahead (lex state -1) and accepts: the
    // parser would read the missing lookahead.
    parse_fails(
        "accept-no-lookahead",
        |m| {
            let modes = m.table(LEX_MODES);
            m.set_u16(modes + 4, 0xffff);
            let accept = m.accept_entry();
            let symbols = m.count(SYMBOL_COUNT);
            let table = m.table(PARSE_TABLE);
            m.set_u16(table + 2 * symbols, accept as u16);
        },
        "{}\n",
    );
    // And a shift of it.
    parse_fails(
        "shift-no-lookahead",
        |m| {
            let modes = m.table(LEX_MODES);
            m.set_u16(modes + 4, 0xffff);
            let shift = m.action_entries()[0];
            let symbols = m.count(SYMBOL_COUNT);
            let table = m.table(PARSE_TABLE);
            m.set_u16(table + 2 * symbols, shift as u16);
        },
        "{}\n",
    );
}

const BIG: &str = concat!(
    "1111111111111111111111111111111111111111111111111111111111111111",
    "1111111111111111111111111111111111111111111111111111111111111111",
    "1111111111111111111111111111111111111111111111111111111111111111",
    "1111111111111111111111111111111111111111111111111111111111111111",
    "1111111111111111111111111111111111111111111111111111111111111111",
);

/// Lets a number token be followed by the end of input accepted, the shape of
/// a stack that is a bare token: a number of over 255 bytes is a leaf on the
/// heap, as an external token is. With `again`, a number may follow a number
/// as well, which leaves two of them (and no root) under the accept.
fn accepts_after_a_number(m: &mut Module, again: bool) {
    let number = m.symbol("number");
    let first = m.cell(1, number);
    let entry = m.u16(first) as usize;
    let shift = m.table(PARSE_ACTIONS) + 8 * (entry + 1);
    assert_eq!(m.u8(shift), 0, "a shift");
    // Shifting it goes to a large state, whose cells can be patched.
    let after = 2;
    m.set_u16(shift + 2, after as u16);
    let accept = m.accept_entry();
    let eof = m.cell(after, 0);
    m.set_u16(eof, accept as u16);
    if again {
        let repeat = m.cell(after, number);
        m.set_u16(repeat, entry as u16);
    }
}

#[test]
fn an_accepted_stack_that_is_not_one_root_is_not_rebuilt() {
    // Nothing on the stack but the end of input.
    parse(
        "accept-empty-stack",
        |m| {
            let accept = m.accept_entry();
            let eof = m.cell(1, 0);
            m.set_u16(eof, accept as u16);
        },
        "",
    );
    // A bare heap leaf, as an external token is.
    parse("accept-leaf", |m| accepts_after_a_number(m, false), BIG);
    // Two of them.
    let two: &'static str = Box::leak(format!("{BIG} {BIG}").into_boxed_str());
    parse(
        "accept-two-leaves",
        |m| accepts_after_a_number(m, true),
        two,
    );
}

#[test]
fn an_accepted_root_that_exceeds_its_alias_sequence_is_not_rebuilt() {
    // "1 2" is two values on the stack (the states 2 and 3). At its end the
    // reduce of both into a repeat becomes a reduce of the second into a
    // document of production 1, whose alias sequence (of length 1: the one
    // reduce of production 1 is made production 0) holds one structural
    // child, and the document goes to the accepting state 30 from the first
    // value's state: the first value is a second structural child.
    parse_fails(
        "accept-over-capacity",
        |m| {
            m.set_u16(m.language + MAX_ALIAS_SEQUENCE_LENGTH, 1);
            for at in m.reduces("pair") {
                m.set_u16(at + 6, 0);
            }
            let document = m.symbol("document");
            let reduce = m.table(PARSE_ACTIONS) + 8 * (m.u16(m.cell(3, 0)) as usize + 1);
            assert_eq!(m.u8(reduce), 1, "a reduce");
            m.set_u8(reduce + 1, 1);
            m.set_u16(reduce + 2, document as u16);
            m.set_u16(reduce + 6, 1);
            let goto = m.cell(2, document);
            m.set_u16(goto, 30);
        },
        "1 2\n",
    );
}

#[test]
fn a_capture_token_inline_subtrees_cant_hold_is_an_error() {
    // The keyword fallback sets an inline subtree's symbol, below 255, to it.
    rejected(
        "capture-token-wide",
        "json",
        |m| {
            m.set_count(SYMBOL_COUNT, 300);
            m.set_count(TOKEN_COUNT, 300);
            m.set_u16(m.language + KEYWORD_CAPTURE_TOKEN, 256);
        },
        "invalid keyword capture token",
    );
}

#[test]
fn external_states_outside_memory_are_an_error() {
    // The size of the states is the distance to the next table: spoofed by
    // an address in a field this grammar leaves unused.
    rejected(
        "external-states-away",
        "badscan",
        |m| {
            m.point_slot_away(m.language + EXTERNAL_STATES);
            m.set_count(FIELD_NAMES, 0x7fff_fff8);
        },
        "invalid external scanner states",
    );
}
