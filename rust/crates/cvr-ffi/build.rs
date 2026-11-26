//! The unit tests' program (`cargo test -p cvr-ffi`): cvr_rust is a static library the C++ half's DLL links, and the
//! C++ half's seams it calls (ImGui, the FBT IK, im3d, the RED4ext SDK's glue -- inventory.md's boundary files) exist only
//! in that DLL. The test program is linked with them left unresolved (MSVC /FORCE:UNRESOLVED): the tests run the Rust
//! logic, and a test that reached a C++ seam would fault at that call instead of hiding behind a stub.
fn main() {
    // (the package links nothing else: cvr_rust is a staticlib and an rlib, archived, not linked)
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/FORCE:UNRESOLVED");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
