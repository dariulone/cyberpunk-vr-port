//! build, release and sdk: the plugin built from a working copy (CMake as Cargo's sub-builder for the C++ parts the
//! owner keeps -- the RED4ext SDK, ImGui, the FBT solver, the frame generation SDKs; cmake/RustStaticlib.cmake runs
//! cargo for the Rust), the package and the symbols beside it.
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::R;

/// The RED4ext SDK the port is built against: the public 2.5 branch (maximegmd/RED4ext.SDK game-2.5) at BASE, the
/// port's four commits for 2.5 (3.0.88) on top -- externals/patches/RED4ext.SDK, `git format-patch BASE..` of the
/// development tree -- and the tree they make, checked by its hash.
const SDK_URL: &str = "https://github.com/maximegmd/RED4ext.SDK";
const SDK_BASE: &str = "fd1867297719e26c6fbd54734c7ce82448d32c6f";
const SDK_TREE: &str = "c58aadc60a69365affa714e3bda0ec691bff10b6";
const GENERATOR: &str = "Visual Studio 18 2026";
pub(crate) const DEFAULT_BUILD_DIR: &str = "build\\patchday\\sdk25-build";
/// The plugin's output folder in a build tree.
pub(crate) const RELEASE_OUT: &str = "bin\\red4ext\\plugins\\cyberpunkvrport\\Release";

fn run(cmd: &mut Command, what: &str) -> R<()> {
    let status = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}
fn output(cmd: &mut Command) -> Option<String> {
    cmd.output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}
fn tree_of(dir: &Path) -> Option<String> {
    output(Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD^{tree}"]))
}

/// The SDK at `dir`: there already with the pinned tree, or made there (clone, BASE, the patches).
pub(crate) fn sdk(root: &Path, dir: &Path) -> R<()> {
    if dir.exists() {
        return match tree_of(dir) {
            Some(t) if t == SDK_TREE => {
                println!("[sdk] {}: the pinned tree {SDK_TREE}", dir.display());
                Ok(())
            }
            other => Err(format!(
                "{} holds another RED4ext SDK (tree {}), not the pinned {SDK_TREE}: move it aside",
                dir.display(),
                other.unwrap_or_else(|| "unknown".into())
            )),
        };
    }
    let patches_dir = root.join("externals\\patches\\RED4ext.SDK");
    let mut patches: Vec<PathBuf> = fs::read_dir(&patches_dir)
        .map_err(|e| format!("{}: {e}", patches_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "patch"))
        .collect();
    patches.sort();
    println!("[sdk] {SDK_URL} at {SDK_BASE} + {} patch(es) -> {}", patches.len(), dir.display());
    if let Some(p) = dir.parent() {
        fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    run(Command::new("git").args(["clone", "-q", "--filter=blob:none", "--no-checkout", SDK_URL]).arg(dir), "git clone")?;
    run(Command::new("git").arg("-C").arg(dir).args(["-c", "advice.detachedHead=false", "checkout", "-q", SDK_BASE]), "git checkout")?;
    // a neutral identity for the commits `am` makes (the tree is what is checked, not the commits)
    let mut am = Command::new("git");
    am.arg("-C").arg(dir).args(["-c", "user.name=xtask", "-c", "user.email=xtask@localhost", "am", "-q", "--keep-cr"]).args(&patches);
    run(&mut am, "git am")?;
    match tree_of(dir) {
        Some(t) if t == SDK_TREE => {
            println!("[sdk] the pinned tree {SDK_TREE}");
            Ok(())
        }
        other => Err(format!("the patched SDK's tree is {}, not the pinned {SDK_TREE}", other.unwrap_or_else(|| "unknown".into()))),
    }
}

/// A value of a CMakeCache.txt.
fn cache_value(cache: &str, key: &str) -> Option<String> {
    cache.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(':')).and_then(|r| r.split_once('=')).map(|(_, v)| v.to_string()))
}

pub(crate) struct BuildOpts {
    pub build_dir: PathBuf,
    pub config: String,
    pub vs_instance: Option<String>,
}
impl BuildOpts {
    pub(crate) fn parse(root: &Path, args: &[String]) -> R<(Self, Vec<String>)> {
        let mut o = BuildOpts { build_dir: root.join(DEFAULT_BUILD_DIR), config: "Release".into(), vs_instance: std::env::var("CVR_VS_INSTANCE").ok() };
        let mut rest = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--build-dir" => o.build_dir = root.join(it.next().ok_or("--build-dir needs a value")?),
                "--config" => o.config = it.next().ok_or("--config needs a value")?.clone(),
                "--vs-instance" => o.vs_instance = Some(it.next().ok_or("--vs-instance needs a value")?.clone()),
                _ => rest.push(a.clone()),
            }
        }
        Ok((o, rest))
    }
}

/// The plugin built: the build tree configured first when it has no cache (the Rust build, the pinned SDK inside it),
/// an existing tree's SDK checked against the pin.
pub(crate) fn build(root: &Path, o: &BuildOpts) -> R<()> {
    let cache_file = o.build_dir.join("CMakeCache.txt");
    // a configure that stopped half way leaves a cache but no generated projects: done again
    let generated = o.build_dir.join("CMakeFiles\\generate.stamp").exists();
    if let (Ok(cache), true) = (fs::read_to_string(&cache_file), generated) {
        let sdk_dir = cache_value(&cache, "CVR_RED4EXT_SDK_DIR").ok_or("CMakeCache.txt has no CVR_RED4EXT_SDK_DIR")?;
        if cache_value(&cache, "CVR_RUST_VRIK").as_deref() != Some("ON") {
            return Err(format!("{} is not a Rust build tree (CVR_RUST_VRIK is not ON)", o.build_dir.display()));
        }
        match tree_of(Path::new(&sdk_dir)) {
            Some(t) if t == SDK_TREE => println!("[build] {}: the SDK at {sdk_dir} is the pinned tree", o.build_dir.display()),
            other => {
                return Err(format!("the build tree's SDK ({sdk_dir}) is tree {}, not the pinned {SDK_TREE}", other.unwrap_or_else(|| "unknown".into())))
            }
        }
    } else {
        // the SDK a half-made cache already names, else one inside the build tree
        let sdk_dir = fs::read_to_string(&cache_file)
            .ok()
            .and_then(|c| cache_value(&c, "CVR_RED4EXT_SDK_DIR"))
            .map_or_else(|| o.build_dir.join("framework-src\\RED4ext.SDK"), PathBuf::from);
        sdk(root, &sdk_dir)?;
        let mut cfg = Command::new("cmake");
        cfg.arg("-S").arg(root).arg("-B").arg(&o.build_dir).args(["-G", GENERATOR, "-A", "x64", "-DCVR_RUST_VRIK=ON"]);
        cfg.arg(format!("-DCVR_RED4EXT_SDK_DIR={}", sdk_dir.display()));
        if let Some(vs) = &o.vs_instance {
            cfg.arg(format!("-DCMAKE_GENERATOR_INSTANCE={vs}"));
        }
        println!("[build] configuring {}", o.build_dir.display());
        run(&mut cfg, "cmake configure")?;
    }
    println!("[build] building cyberpunkvrport_stereo ({})", o.config);
    let mut b = Command::new("cmake");
    b.arg("--build").arg(&o.build_dir).args(["--config", &o.config, "--target", "cyberpunkvrport_stereo", "--", "/m", "/v:minimal"]);
    run(&mut b, "cmake build")?;
    loader(root, o)
}

/// The port's own loader (crates/cvr-loader: bin\x64\powrprof.dll), built in release beside the plugin's output.
fn loader(root: &Path, o: &BuildOpts) -> R<()> {
    let target = o.build_dir.join("cvr-loader-target");
    println!("[build] building the loader powrprof.dll");
    let mut c = Command::new("cargo");
    c.args(["build", "--release", "-p", "cvr-loader", "--manifest-path"])
        .arg(root.join("rust\\Cargo.toml"))
        .arg("--target-dir")
        .arg(&target);
    run(&mut c, "cargo build cvr-loader")?;
    let out = o.build_dir.join(RELEASE_OUT);
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for name in ["powrprof.dll", "powrprof.pdb"] {
        let src = target.join("release").join(name);
        if src.exists() {
            fs::copy(&src, out.join(name)).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- PE and PDB
pub(crate) struct Pe {
    data: Vec<u8>,
    sections: Vec<(u32, u32, u32, u32)>, // virtual address, virtual size, raw pointer, raw size
    dirs: Vec<(u32, u32)>,
}
fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
impl Pe {
    pub(crate) fn read(path: &Path) -> R<Self> {
        let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bad = || format!("{}: not a PE32+ image", path.display());
        let pe = u32_at(&data, 0x3C).ok_or_else(bad)? as usize;
        if data.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(bad());
        }
        let nsec = u16_at(&data, pe + 6).ok_or_else(bad)? as usize;
        let opt_size = u16_at(&data, pe + 20).ok_or_else(bad)? as usize;
        let opt = pe + 24;
        if u16_at(&data, opt) != Some(0x20B) {
            return Err(bad());
        }
        let ndirs = u32_at(&data, opt + 108).ok_or_else(bad)? as usize;
        let dirs = (0..ndirs.min(16)).filter_map(|i| Some((u32_at(&data, opt + 112 + i * 8)?, u32_at(&data, opt + 116 + i * 8)?))).collect();
        let st = opt + opt_size;
        let sections = (0..nsec)
            .filter_map(|i| {
                let s = st + i * 40;
                Some((u32_at(&data, s + 12)?, u32_at(&data, s + 8)?, u32_at(&data, s + 20)?, u32_at(&data, s + 16)?))
            })
            .collect();
        Ok(Pe { data, sections, dirs })
    }
    fn offset(&self, rva: u32) -> Option<usize> {
        self.sections
            .iter()
            .find(|&&(va, vs, _, rs)| rva >= va && rva < va + vs.max(rs))
            .map(|&(va, _, raw, _)| (rva - va + raw) as usize)
    }
    fn c_str(&self, off: usize) -> String {
        let end = self.data[off..].iter().position(|&b| b == 0).map_or(self.data.len(), |p| off + p);
        String::from_utf8_lossy(&self.data[off..end]).to_string()
    }
    /// The exported names.
    pub(crate) fn exports(&self) -> Vec<String> {
        let Some(&(rva, size)) = self.dirs.first() else { return Vec::new() };
        if rva == 0 || size == 0 {
            return Vec::new();
        }
        let Some(dir) = self.offset(rva) else { return Vec::new() };
        let n = u32_at(&self.data, dir + 24).unwrap_or(0) as usize;
        let names = u32_at(&self.data, dir + 32).and_then(|r| self.offset(r));
        let Some(names) = names else { return Vec::new() };
        (0..n).filter_map(|i| u32_at(&self.data, names + i * 4).and_then(|r| self.offset(r))).map(|o| self.c_str(o)).collect()
    }
    /// The CodeView record: the PDB's GUID (as stored), its age and the path the linker wrote.
    pub(crate) fn codeview(&self) -> Option<([u8; 16], u32, String)> {
        let &(rva, size) = self.dirs.get(6)?;
        let dir = self.offset(rva)?;
        (0..size as usize / 28).find_map(|i| {
            let e = dir + i * 28;
            if u32_at(&self.data, e + 12)? != 2 {
                return None; // not IMAGE_DEBUG_TYPE_CODEVIEW
            }
            let raw = u32_at(&self.data, e + 24)? as usize;
            if self.data.get(raw..raw + 4)? != b"RSDS" {
                return None;
            }
            let mut guid = [0u8; 16];
            guid.copy_from_slice(self.data.get(raw + 4..raw + 20)?);
            Some((guid, u32_at(&self.data, raw + 20)?, self.c_str(raw + 24)))
        })
    }
}
/// A RED4ext plugin: the loader's three entries.
pub(crate) fn is_red4ext_plugin(exports: &[String]) -> bool {
    ["Query", "Main", "Supports"].iter().all(|n| exports.iter().any(|e| e == n))
}
pub(crate) fn guid_text(g: &[u8; 16]) -> String {
    let d1 = u32::from_le_bytes([g[0], g[1], g[2], g[3]]);
    let d2 = u16::from_le_bytes([g[4], g[5]]);
    let d3 = u16::from_le_bytes([g[6], g[7]]);
    let tail: String = g[8..].iter().map(|b| format!("{b:02X}")).collect();
    format!("{d1:08X}-{d2:04X}-{d3:04X}-{}-{}", &tail[..4], &tail[4..])
}

/// The PDB's GUID and age (MSF 7.00: the stream directory through the block map, stream 1 the PDB info stream --
/// version, signature, age, GUID).
pub(crate) fn pdb_identity(path: &Path) -> R<([u8; 16], u32)> {
    let mut f = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut head = [0u8; 56];
    f.read_exact(&mut head).map_err(|e| e.to_string())?;
    if &head[..24] != b"Microsoft C/C++ MSF 7.00" {
        return Err(format!("{}: not an MSF 7.00 PDB", path.display()));
    }
    let word = |o: usize| u32::from_le_bytes([head[o], head[o + 1], head[o + 2], head[o + 3]]) as u64;
    let (block, dir_bytes, map_block) = (word(32), word(44), word(52));
    let mut read_at = |off: u64, len: usize| -> R<Vec<u8>> {
        let mut v = vec![0u8; len];
        f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
        f.read_exact(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    };
    let dir_blocks = dir_bytes.div_ceil(block) as usize;
    let map = read_at(map_block * block, dir_blocks * 4)?;
    let mut dir = Vec::with_capacity(dir_bytes as usize);
    for i in 0..dir_blocks {
        let b = u32_at(&map, i * 4).ok_or("short block map")? as u64;
        dir.extend(read_at(b * block, block as usize)?);
    }
    let streams = u32_at(&dir, 0).ok_or("empty stream directory")? as usize;
    let sizes: Vec<u64> = (0..streams).map(|i| u32_at(&dir, 4 + i * 4).unwrap_or(0) as u64).collect();
    // stream 1's first block: after the sizes, the blocks of stream 0
    let s0_blocks = if sizes[0] == u32::MAX as u64 { 0 } else { sizes[0].div_ceil(block) as usize };
    let first = u32_at(&dir, 4 + streams * 4 + s0_blocks * 4).ok_or("no PDB info stream")? as u64;
    let info = read_at(first * block, 28)?;
    let age = u32_at(&info, 8).ok_or("short PDB info stream")?;
    let mut guid = [0u8; 16];
    guid.copy_from_slice(&info[12..28]);
    Ok((guid, age))
}

/// The symbols beside the package: dist\CyberpunkVRPort-<version>-symbols with the PDB the DLL names (its GUID and
/// age checked against the DLL's CodeView record) and SYMBOLS.txt.
pub(crate) fn symbols(root: &Path, release: &Path, version: &str, commit: &str, zip: bool) -> R<PathBuf> {
    let dll = release.join("cyberpunkvrport.dll");
    let pdb = release.join("cyberpunkvrport.pdb");
    let (guid, age, linked) = Pe::read(&dll)?.codeview().ok_or("the DLL has no CodeView record")?;
    let (pdb_guid, pdb_age) = pdb_identity(&pdb)?;
    if guid != pdb_guid || age != pdb_age {
        return Err(format!(
            "the PDB does not match the DLL: DLL {} age {age}, PDB {} age {pdb_age}",
            guid_text(&guid),
            guid_text(&pdb_guid)
        ));
    }
    let dist_root = root.join("dist");
    let out = dist_root.join(format!("CyberpunkVRPort-{version}-symbols"));
    if out.exists() {
        if out.parent() != Some(dist_root.as_path()) {
            return Err("refusing to replace symbols outside dist".into());
        }
        fs::remove_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    fs::copy(&pdb, out.join("cyberpunkvrport.pdb")).map_err(|e| format!("{}: {e}", pdb.display()))?;
    let id = format!("{}{age:X}", guid_text(&guid).replace('-', ""));
    let text = format!(
        "CyberpunkVRPort {version} -- symbols of red4ext\\plugins\\cyberpunkvrport\\cyberpunkvrport.dll\n\
         \n\
         cyberpunkvrport.pdb    GUID {}  age {age}  (symbol server id {id})\n\
         \n\
         For a crash report or a dump of this exact build: put cyberpunkvrport.pdb beside cyberpunkvrport.dll, or in a\n\
         symbol path as cyberpunkvrport.pdb\\{id}\\cyberpunkvrport.pdb. The DLL's own record names it as\n\
         {linked}\n\
         \n\
         Built from commit {commit}.",
        guid_text(&guid)
    );
    fs::write(out.join("SYMBOLS.txt"), text.replace('\n', "\r\n") + "\r\n").map_err(|e| e.to_string())?;
    println!("[symbols] {} (GUID {} age {age}: the DLL's)", out.display(), guid_text(&guid));
    if zip {
        let archive = PathBuf::from(format!("{}.zip", out.display()));
        crate::write_zip(&out, &archive)?;
        println!("[symbols] {}", archive.display());
    }
    Ok(out)
}

/// `git status` of tracked files: whether the commit is the whole source.
pub(crate) fn tracked_clean(root: &Path) -> bool {
    output(Command::new("git").arg("-C").arg(root).args(["status", "--porcelain", "--untracked-files=no"])).is_some_and(|s| s.is_empty())
}
