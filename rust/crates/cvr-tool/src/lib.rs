//! The port's development tooling as a library: the CLI (src/main.rs) and other crates' tests share it.
pub mod engine_addresses;
pub mod manifest;
pub mod natives;
pub mod pe;
pub mod sdk_hashes;

pub type Result<T = ()> = std::result::Result<T, String>;