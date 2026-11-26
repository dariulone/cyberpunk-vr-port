//! MinHook, from the submodule the port pins (externals/minhook, v1.3.4-14-gd94c64d, BSD-2-Clause), compiled into
//! this crate: the DLL has exactly one copy of it, and the C++ half calls the same MH_* symbols through
//! externals/minhook/include/MinHook.h until its hooks move to cvr_engine::hooks.
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../externals/minhook");
    let src = root.join("src");
    let files = ["buffer.c", "hook.c", "trampoline.c", "hde/hde32.c", "hde/hde64.c"];
    let mut build = cc::Build::new();
    build.include(root.join("include")).include(&src).warnings(false);
    for f in files {
        let path = src.join(f);
        println!("cargo:rerun-if-changed={}", path.display());
        build.file(path);
    }
    build.compile("minhook");
    // The guarded accesses to engine memory, formerly native/seh.c, are Rust's since B655 (src/seh.rs).
}
