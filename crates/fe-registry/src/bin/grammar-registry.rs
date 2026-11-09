//! Builds the first-party grammars' registry: each language's artifact and
//! `index.json` (`fe_registry::grammars`). `scripts/build-grammar-registry.sh`
//! runs it.
//!
//! ```text
//! grammar-registry generate --grammars <dir> --wasm <dir> --out <dir> --serial <n>
//! grammar-registry packs --index <file> [<pack>]
//! grammar-registry finalize --index <file> --tarballs <dir>
//! ```
//!
//! - `generate` writes `<out>/languages/<language>/` and `<out>/index.json`,
//!   without the packs' tarballs. A language in no pack is a warning.
//! - `packs` prints the index's packs, one a line; given a pack, its
//!   languages, one a line, and it fails naming a pack the index hasn't.
//! - `finalize` fills in each pack's tarball,
//!   `<tarballs>/Tenmoku-language-pack-<pack>.tar.gz`, and rewrites the index.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use fe_registry::grammars::{self, Index};

const USAGE: &str = "usage:
  grammar-registry generate --grammars <dir> --wasm <dir> --out <dir> --serial <n>
  grammar-registry packs --index <file> [<pack>]
  grammar-registry finalize --index <file> --tarballs <dir>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("grammar-registry: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or(USAGE)?;
    let (options, positional) = options(rest)?;
    let path = |name: &str| -> Result<PathBuf, String> {
        options
            .get(name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("{command} needs --{name}\n{USAGE}"))
    };
    let only = |allowed: &[&str], positional_max: usize| -> Result<(), String> {
        if let Some(name) = options
            .keys()
            .find(|name| !allowed.contains(&name.as_str()))
        {
            return Err(format!("{command} takes no --{name}\n{USAGE}"));
        }
        if positional.len() > positional_max {
            return Err(format!(
                "{command} takes no {:?}\n{USAGE}",
                positional[positional_max]
            ));
        }
        Ok(())
    };
    match command.as_str() {
        "generate" => {
            only(&["grammars", "wasm", "out", "serial"], 0)?;
            let serial = options
                .get("serial")
                .ok_or(format!("generate needs --serial\n{USAGE}"))?;
            let serial: u64 = (serial.parse())
                .map_err(|_| format!("--serial {serial:?} isn't a whole number"))?;
            let generated = grammars::generate(&path("grammars")?, &path("wasm")?, serial)
                .map_err(|error| error.to_string())?;
            for warning in &generated.warnings {
                eprintln!("grammar-registry: warning: {warning}");
            }
            let out = path("out")?;
            generated.write(&out).map_err(|error| error.to_string())?;
            println!(
                "wrote {} languages and {}",
                generated.index.languages.len(),
                out.join(grammars::INDEX_FILE).display()
            );
            Ok(())
        }
        "packs" => {
            only(&["index"], 1)?;
            let index_path = path("index")?;
            let index = read_index(&index_path)?;
            match positional.first() {
                None => index.packs.keys().for_each(|pack| println!("{pack}")),
                Some(pack) => {
                    let pack = index.packs.get(pack).ok_or_else(|| {
                        let packs: Vec<&str> = index.packs.keys().map(String::as_str).collect();
                        format!(
                            "{} has no pack {pack:?} (it has {})",
                            index_path.display(),
                            packs.join(", ")
                        )
                    })?;
                    pack.languages
                        .iter()
                        .for_each(|language| println!("{language}"));
                }
            }
            Ok(())
        }
        "finalize" => {
            only(&["index", "tarballs"], 0)?;
            let index_path = path("index")?;
            let mut index = read_index(&index_path)?;
            grammars::finalize(&mut index, &path("tarballs")?)
                .map_err(|error| error.to_string())?;
            grammars::write_index(&index_path, &index).map_err(|error| error.to_string())?;
            println!("wrote {}", index_path.display());
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn read_index(path: &Path) -> Result<Index, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Index::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

type Options = BTreeMap<String, String>;

/// `--name value` pairs, each name once, and the other arguments.
fn options(args: &[String]) -> Result<(Options, Vec<String>), String> {
    let mut options = BTreeMap::new();
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let Some(name) = arg.strip_prefix("--") else {
            positional.push(arg.clone());
            continue;
        };
        let value = args
            .next()
            .ok_or(format!("--{name} needs a value\n{USAGE}"))?;
        if options.insert(name.to_owned(), value.clone()).is_some() {
            return Err(format!("--{name} is given twice"));
        }
    }
    Ok((options, positional))
}
