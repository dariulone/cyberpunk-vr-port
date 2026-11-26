//! cvr-tool: the port's development CLI (docs/rust-migration-plan.md, stages 0, 2 and 8).
//!
//!   cvr-tool sdk-hashes <AddressHashes.hpp> <out.rs>   RED4ext.SDK's address hashes as Rust constants
//!   cvr-tool engine-addresses <EngineRvas.hpp> <GameBuild.hpp> <out.rs>   the engine address table for Rust
//!   cvr-tool pe-exports <dll>                          the exports of a PE image, one per line (name rva)
//!   cvr-tool natives <Registration.cpp>...             the script natives a C++ registration table names
//!   cvr-tool sha256 <file>...                          SHA-256 of files
//!   cvr-tool manifest <dir>                            every file under dir: relative path, bytes, SHA-256
use std::env;
use std::path::Path;
use std::process::ExitCode;

use cvr_tool::{engine_addresses, manifest, natives, pe, sdk_hashes};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("sdk-hashes") if args.len() == 3 => sdk_hashes::run(Path::new(&args[1]), Path::new(&args[2])),
        Some("engine-addresses") if args.len() == 4 => {
            engine_addresses::run(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]))
        }
        Some("pe-exports") if args.len() == 2 => pe::print_exports(Path::new(&args[1])),
        Some("natives") if args.len() >= 2 => natives::run(&args[1..]),
        Some("sha256") if args.len() >= 2 => manifest::print_hashes(&args[1..]),
        Some("manifest") if args.len() == 2 => manifest::print_manifest(Path::new(&args[1])),
        _ => Err("usage: cvr-tool sdk-hashes|pe-exports|natives|sha256|manifest ... (see src/main.rs)".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cvr-tool: {error}");
            ExitCode::FAILURE
        }
    }
}
