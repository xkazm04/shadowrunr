//! Write the contract's JSON Schema into `packages/contract/schema/` (or `--out <dir>`).
//!
//! `cargo run -p shadowrunr-types --example export-schema [-- --out <dir>]`

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/contract/schema");
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--out", Some(dir)) => out = PathBuf::from(dir),
            _ => {
                eprintln!("usage: export-schema [--out <dir>]");
                return ExitCode::from(2);
            }
        }
    }
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("{}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    for (name, text) in shadowrunr_types::schema::export_files() {
        let path = out.join(name);
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}
