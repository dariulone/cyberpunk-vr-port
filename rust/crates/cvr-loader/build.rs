//! The proxy's exports: every function of the system's powrprof.dll (exports.txt, read from Windows 11's), each a jump
//! through the table the DLL fills from the real one. Writes the linker's .def and the stubs' assembly.
use std::fmt::Write as _;

fn main() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let list = std::fs::read_to_string(dir.join("exports.txt")).unwrap();
    let names: Vec<&str> = list.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut def = String::from("LIBRARY powrprof\r\nEXPORTS\r\n");
    let mut asm = String::new();
    let mut table = String::from("pub const NAMES: &[&str] = &[\n");
    for (i, n) in names.iter().enumerate() {
        assert!(n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'), "an export name: {n}");
        let _ = write!(def, "    {n}\r\n");
        let _ = write!(asm, ".globl {n}\n{n}:\n    jmp qword ptr [rip + {{real}} + {}]\n", i * 8);
        let _ = writeln!(table, "    \"{n}\",");
    }
    table.push_str("];\n");
    let _ = write!(
        table,
        "pub const COUNT: usize = {};\ncore::arch::global_asm!(\".text\\n{}\", real = sym crate::REAL);\n",
        names.len(),
        asm.replace('\n', "\\n")
    );
    std::fs::write(out.join("powrprof.def"), def).unwrap();
    std::fs::write(out.join("stubs.rs"), table).unwrap();
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", out.join("powrprof.def").display());
    println!("cargo:rerun-if-changed=exports.txt");
    println!("cargo:rerun-if-changed=build.rs");
}
