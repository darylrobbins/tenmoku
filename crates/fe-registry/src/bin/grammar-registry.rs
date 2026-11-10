//! Builds the first-party grammars' registry: each language's artifact and
//! `index.json` (`fe_registry::grammars`). `scripts/build-grammar-registry.sh`
//! runs it.
//!
//! ```text
//! grammar-registry generate --grammars <dir> --wasm <dir> --out <dir> --serial <n>
//! grammar-registry packs --index <file> [<pack>]
//! grammar-registry finalize --index <file> --tarballs <dir>
//! grammar-registry languages --index <file> [<language>]
//! grammar-registry tarballs --index <file>
//! grammar-registry serial --index <file>
//! grammar-registry sign --index <file> [--registry-toml <file>]     # private key on stdin
//! grammar-registry verify --index <file> [--sig <file>] [--registry-toml <file>]
//! grammar-registry generate-key --out-private <file>
//! ```
//!
//! - `generate` writes `<out>/languages/<language>/` and `<out>/index.json`,
//!   without the packs' tarballs. A language in no pack is a warning.
//! - `packs` prints the index's packs, one a line; given a pack, its
//!   languages, one a line, and it fails naming a pack the index hasn't.
//! - `finalize` fills in each pack's tarball,
//!   `<tarballs>/Tenmoku-language-pack-<pack>.tar.gz`, and rewrites the index.
//! - `languages` prints each language as `<language>\t<tag>\t<version>\t<name>`,
//!   one a line; given a language, its files as `<name>\t<size>\t<sha256>`.
//! - `tarballs` prints each pack's tarball as `<name>\t<size>\t<sha256>`,
//!   failing if one isn't filled in. `scripts/publish-grammar-registry.sh`
//!   reads these and `languages`'.
//! - `serial` prints the index's serial, once it has parsed: the publish
//!   script compares the published index's with its own.
//! - `sign` reads the base64 private key from stdin, never the command line,
//!   and writes `<file>.sig`, refusing a key that isn't `registry.toml`'s
//!   `public_key`'s (`fe_registry::signing`). `--registry-toml` defaults to
//!   `extensions/grammars/registry.toml`, as for `verify`.
//! - `verify` checks `<file>.sig` (or `--sig`) against `registry.toml`'s
//!   `public_key`, then that the index parses.
//! - `generate-key` writes a new private key to a new file at mode 600 and
//!   prints its public half, for `registry.toml` (key rotation in
//!   `docs/release/grammar-registry.md`).

use std::{
    collections::BTreeMap,
    fs,
    io::{IsTerminal as _, Read as _, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
};

use fe_registry::{
    grammars::{self, Index},
    signing,
};

/// `--registry-toml`'s default, from the repository's root.
const REGISTRY_TOML: &str = "extensions/grammars/registry.toml";

const USAGE: &str = "usage:
  grammar-registry generate --grammars <dir> --wasm <dir> --out <dir> --serial <n>
  grammar-registry packs --index <file> [<pack>]
  grammar-registry finalize --index <file> --tarballs <dir>
  grammar-registry languages --index <file> [<language>]
  grammar-registry tarballs --index <file>
  grammar-registry serial --index <file>
  grammar-registry sign --index <file> [--registry-toml <file>]   (private key on stdin)
  grammar-registry verify --index <file> [--sig <file>] [--registry-toml <file>]
  grammar-registry generate-key --out-private <file>";

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
    if command == "sign" {
        return sign(rest);
    }
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
        "languages" => {
            only(&["index"], 1)?;
            let index_path = path("index")?;
            let index = read_index(&index_path)?;
            match positional.first() {
                None => {
                    for (key, language) in &index.languages {
                        println!(
                            "{key}\t{}\t{}\t{}",
                            language.tag, language.version, language.name
                        );
                    }
                }
                Some(key) => {
                    let language = index.languages.get(key).ok_or_else(|| {
                        format!("{} has no language {key:?}", index_path.display())
                    })?;
                    for file in &language.files {
                        println!("{}\t{}\t{}", file.name, file.size, file.sha256);
                    }
                }
            }
            Ok(())
        }
        "serial" => {
            only(&["index"], 0)?;
            println!("{}", read_index(&path("index")?)?.serial);
            Ok(())
        }
        "tarballs" => {
            only(&["index"], 0)?;
            let index = read_index(&path("index")?)?;
            for (key, pack) in &index.packs {
                let tarball = pack.tarball.as_ref().ok_or_else(|| {
                    format!("the pack {key} has no tarball: run grammar-registry finalize")
                })?;
                println!("{}\t{}\t{}", tarball.name, tarball.size, tarball.sha256);
            }
            Ok(())
        }
        "verify" => {
            only(&["index", "sig", "registry-toml"], 0)?;
            let index_path = path("index")?;
            let sig = options.get("sig").map(PathBuf::from);
            let registry_toml = registry_toml(&options);
            signing::verify_index(&index_path, sig.as_deref(), &registry_toml)
                .map_err(|error| error.to_string())?;
            println!(
                "{} verifies against {}'s public_key",
                index_path.display(),
                registry_toml.display()
            );
            Ok(())
        }
        "generate-key" => {
            only(&["out-private"], 0)?;
            generate_key(&path("out-private")?)
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn registry_toml(options: &Options) -> PathBuf {
    PathBuf::from(
        options
            .get("registry-toml")
            .map_or(REGISTRY_TOML, String::as_str),
    )
}

/// `sign`: its arguments are checked without echoing any, since a key given
/// by mistake on the command line mustn't reach a log as well.
fn sign(args: &[String]) -> Result<(), String> {
    let refused = || {
        format!(
            "sign takes --index <file> [--registry-toml <file>] and nothing else: the private \
             key is read from stdin only, never from the command line\n{USAGE}"
        )
    };
    let (options, positional) = options(args).map_err(|_| refused())?;
    let allowed = ["index", "registry-toml"];
    if !positional.is_empty() || options.keys().any(|name| !allowed.contains(&name.as_str())) {
        return Err(refused());
    }
    let index = options
        .get("index")
        .map(PathBuf::from)
        .ok_or_else(refused)?;
    let registry_toml = registry_toml(&options);
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("grammar-registry: reading the private key from stdin");
    }
    let mut key = String::new();
    stdin
        .read_to_string(&mut key)
        .map_err(|error| format!("reading the private key from stdin: {error}"))?;
    if key.trim().is_empty() {
        return Err("no private key on stdin".to_owned());
    }
    let sig =
        signing::sign_index(&index, &registry_toml, &key).map_err(|error| error.to_string())?;
    println!("wrote {}", sig.display());
    Ok(())
}

fn generate_key(out: &Path) -> Result<(), String> {
    let (public, private) = signing::generate_key();
    // Never over an existing key; created at 600, not chmod-ed after.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(out)
        .map_err(|error| format!("{}: {error}", out.display()))?;
    file.write_all(format!("{private}\n").as_bytes())
        .map_err(|error| format!("{}: {error}", out.display()))?;
    println!("{public}");
    Ok(())
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
