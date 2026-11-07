//! Signs a Velopack feed for the spike updater (T1427), with the same code the
//! app checks it with.
//!
//! ```text
//! sign_update_feed sign FEED                 # private key on stdin; writes FEED.sig,
//!                                            # prints the key's public half
//! sign_update_feed generate --out-private FILE   # FILE at mode 600; prints the public key
//! ```
//!
//! The private key is the base64 of a 32-byte Ed25519 seed (Sparkle 2's
//! `generate_keys -x` export is the same). It is read from stdin only, never
//! from the command line, where `ps` would show it.

use std::{
    io::{Read as _, Write as _},
    path::PathBuf,
    process::ExitCode,
};

#[allow(dead_code)]
#[path = "../src/update_signature.rs"]
mod update_signature;

const USAGE: &str = "usage: sign_update_feed sign FEED  (private key on stdin)\n       \
                     sign_update_feed generate --out-private FILE";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["sign", feed] => sign(PathBuf::from(feed)),
        ["generate", "--out-private", file] => generate(PathBuf::from(file)),
        ["-h" | "--help"] => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("sign_update_feed: {why}");
            ExitCode::FAILURE
        }
    }
}

fn sign(feed: PathBuf) -> Result<(), String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|error| format!("reading the private key from stdin: {error}"))?;
    if text.trim().is_empty() {
        return Err("no private key on stdin".into());
    }
    let key =
        update_signature::PrivateKey::from_base64(&text).map_err(|error| error.to_string())?;
    let bytes = std::fs::read(&feed).map_err(|error| format!("{}: {error}", feed.display()))?;
    let mut sig_path = feed.into_os_string();
    sig_path.push(".sig");
    let sig_path = PathBuf::from(sig_path);
    let signature = update_signature::sign(&key, &bytes);
    std::fs::write(&sig_path, signature + "\n")
        .map_err(|error| format!("{}: {error}", sig_path.display()))?;
    eprintln!("signed {} bytes; wrote {}", bytes.len(), sig_path.display());
    // The key's public half, for a caller to check against the one it built in.
    println!("{}", key.public_base64());
    Ok(())
}

fn generate(out: PathBuf) -> Result<(), String> {
    let (public, private) = update_signature::generate();
    // Never over an existing key; created at 600, not chmod-ed after.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&out)
        .map_err(|error| format!("{}: {error}", out.display()))?;
    file.write_all(format!("{private}\n").as_bytes())
        .map_err(|error| format!("{}: {error}", out.display()))?;
    println!("{public}");
    Ok(())
}
