//! The port's build and release tasks, run as `cargo xtask <task>` (stage 7: the build and the release are Cargo's).
//!
//!   dist [--version X.Y.Z] [--build-dir DIR] [--zip] [--force]
//!       The tester package of the Rust build under dist\CyberpunkVRPort-<version>, laid out as it lands in the game
//!       root (scripts\build_dist.ps1's layout, which stays the C++ build's packager): the plugin and its TweakDB
//!       names, the shaders, the settings preset, the grip poses, the OpenVR shim, the data the plugin reads from its
//!       lua\ folder (sounds, the camera list), the TweakDB records, the archives and their manifest, the input
//!       bindings; INSTALL.txt, CHANGELOG.txt, and UNINSTALL.txt / .bat generated from what was copied. No Lua mod, no
//!       redscript module and no script compiler: the Rust build runs no Lua and compiles no script, and Cyber Engine
//!       Tweaks and redscript are other mods' business, each from its own download. With --zip, the zip beside it
//!       (entries named with '/', the zip spec's). Everything comes from the repo or a build output -- nothing is read
//!       out of an installed game. Checked: exactly one RED4ext plugin (a DLL exporting Query, Main and Supports).
//!
//!   build [--build-dir DIR] [--config Release] [--vs-instance PATH]
//!       The plugin built from this working copy: a build tree without a cache is configured first (the Rust build,
//!       the pinned RED4ext SDK made inside it from externals\patches\RED4ext.SDK); an existing tree's SDK is checked
//!       against the pin. CMake builds what stays C++ and runs cargo for the Rust (cmake/RustStaticlib.cmake).
//!
//!   release --version X.Y.Z [build's options] [--force]
//!       build, then dist --zip, then the symbols: dist\CyberpunkVRPort-<version>-symbols(.zip) -- the PDB the DLL
//!       names (GUID and age checked against the DLL's CodeView record) and SYMBOLS.txt.
//!
//!   sdk DIR
//!       The pinned RED4ext SDK made in DIR (or DIR checked).
//!
//!   shaders [--check]
//!       The port's own shaders from Rust to DXIL (rust/shaders: rust-gpu -> naga -> DXC), on the nightly that
//!       rust/shaders/rust-toolchain.toml pins; the DXIL lands in rust/crates/cvr-gpu/shaders (committed). With
//!       --check, made again and compared with the committed files.
mod release;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

type R<T> = Result<T, String>;

const USAGE: &str = "usage: cargo xtask dist [--version X.Y.Z] [--build-dir DIR] [--zip] [--force]\n\
                     \x20      cargo xtask build [--build-dir DIR] [--config Release] [--vs-instance PATH]\n\
                     \x20      cargo xtask release --version X.Y.Z [--build-dir DIR] [--vs-instance PATH] [--force]\n\
                     \x20      cargo xtask sdk DIR\n\
                     \x20      cargo xtask shaders [--check]";

/// shaders: rust/shaders' builder run on the nightly its rust-toolchain.toml pins (rust-gpu's code generator is built
/// against exactly that one), with none of this stable cargo's environment leaking into it.
fn shaders(root: &Path, args: &[String]) -> R<()> {
    let dir = root.join("rust\\shaders");
    let pin = fs::read_to_string(dir.join("rust-toolchain.toml")).map_err(|e| format!("rust/shaders/rust-toolchain.toml: {e}"))?;
    let channel = pin
        .lines()
        .find_map(|l| l.trim().strip_prefix("channel").and_then(|r| r.trim().strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string()))
        .ok_or("rust/shaders/rust-toolchain.toml names no channel")?;
    let mut cmd = std::process::Command::new("cargo");
    cmd.current_dir(&dir).arg(format!("+{channel}")).args(["run", "--release", "-p", "cvr-shader-builder", "--"]).args(args);
    for var in ["RUSTUP_TOOLCHAIN", "RUSTC", "RUSTC_WRAPPER", "RUSTDOC", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
        cmd.env_remove(var);
    }
    println!("[shaders] rust/shaders on {channel}");
    let status = cmd.status().map_err(|e| format!("cargo +{channel}: {e} (rustup toolchain install {channel} --component rust-src,rustc-dev,llvm-tools)"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("the shader builder failed ({status})"))
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    let result = match args.first().map(String::as_str) {
        Some("dist") => dist(&args[1..]),
        Some("build") => release::BuildOpts::parse(&root, &args[1..]).and_then(|(o, rest)| match rest.first() {
            Some(a) => Err(format!("unknown argument {a}")),
            None => release::build(&root, &o),
        }),
        Some("release") => release_task(&root, &args[1..]),
        Some("sdk") => match args.get(1) {
            Some(dir) => release::sdk(&root, &root.join(dir)),
            None => Err(USAGE.into()),
        },
        Some("shaders") => shaders(&root, &args[1..]),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The repository (this crate is rust\crates\xtask).
fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.ancestors().nth(3).map(Path::to_path_buf).unwrap_or(here)
}

/// A version as the old script accepted it: d.d.d with an optional -suffix.
fn valid_version(v: &str) -> bool {
    let (core, suffix) = match v.split_once('-') {
        Some((c, s)) => (c, Some(s)),
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    // three numbers, or four for a release between two (0.2.5.5)
    let core_ok = (3..=4).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let suffix_ok = suffix.is_none_or(|s| {
        s.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    });
    core_ok && suffix_ok
}

/// PowerShell's -like for the skip patterns: an exact name or "*.ext".
fn like(name: &str, pattern: &str) -> bool {
    match pattern.strip_prefix('*') {
        Some(tail) => name.to_ascii_lowercase().ends_with(&tail.to_ascii_lowercase()),
        None => name.eq_ignore_ascii_case(pattern),
    }
}

/// The data the plugin reads from its lua\ folder (the port's former Lua mods' folders), shipped there: (folder in
/// mods\cet, the files' pattern, how many at least). Their Lua is Rust's and is not shipped.
const LUA_DATA: [(&str, &str, usize); 3] = [
    ("CyberpunkVRPort_Stereo", "vrcam.json", 1),
    ("CyberpunkVRPort_Lobby\\sounds", "*.ogg", 6),
    ("CyberpunkVRPort_Menu\\sounds", "*.wav", 4),
];
/// *.md too: the notes beside a script are for whoever maintains it, not for a tester's game folder.
const SKIP_FILES: [&str; 7] = ["db.sqlite3", "*.log", "*.bak", "*.orig", "*.rar", "*.zip", "*.md"];

/// A directory's entries as PowerShell's Get-ChildItem lists them (by name).
fn entries(dir: &Path) -> R<Vec<fs::DirEntry>> {
    let mut v: Vec<fs::DirEntry> = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().collect();
    v.sort_by_key(|e| e.file_name().to_string_lossy().to_ascii_lowercase());
    Ok(v)
}

struct Package {
    out: PathBuf,
    /// (path relative to the package, bytes; 0 for a folder copied whole, "<folder>\  (n files)")
    manifest: Vec<(String, u64)>,
}
impl Package {
    /// Add-File: one file to its place in the package, recorded.
    fn add(&mut self, src: &Path, rel: &str) -> R<()> {
        let dst = self.out.join(rel);
        if let Some(p) = dst.parent() {
            fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        fs::copy(src, &dst).map_err(|e| format!("{} -> {}: {e}", src.display(), dst.display()))?;
        let bytes = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
        self.manifest.push((rel.to_string(), bytes));
        Ok(())
    }
    /// Copy-Tree: a folder copied whole (the skip patterns left out); how many files.
    fn tree(&self, src: &Path, rel: &str) -> R<usize> {
        let dst = self.out.join(rel);
        fs::create_dir_all(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        let mut n = 0;
        let list = entries(src)?;
        for e in list.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_file())) {
            let name = e.file_name().to_string_lossy().to_string();
            if SKIP_FILES.iter().any(|p| like(&name, p)) {
                continue;
            }
            fs::copy(e.path(), dst.join(&name)).map_err(|err| format!("{}: {err}", e.path().display()))?;
            n += 1;
        }
        for e in list.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            let name = e.file_name().to_string_lossy().to_string();
            n += self.tree(&e.path(), &format!("{rel}\\{name}"))?;
        }
        Ok(n)
    }
    fn folder(&mut self, rel: &str, n: usize) {
        self.manifest.push((format!("{rel}\\  ({n} files)"), 0));
    }
}

/// Need: a file that has to be there.
fn need(p: PathBuf, what: &str) -> R<PathBuf> {
    if p.exists() {
        Ok(p)
    } else {
        Err(format!("{what} not found: {}", p.display()))
    }
}

/// The DXIL files of a folder (by name).
fn dxil_in(dir: &Path) -> R<Vec<PathBuf>> {
    Ok(entries(dir)?
        .into_iter()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && like(&e.file_name().to_string_lossy(), "*.dxil"))
        .map(|e| e.path())
        .collect())
}

fn short_commit(root: &Path) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
/// Get-Date -Format "yyyy-MM-dd": today, local time.
fn today() -> String {
    // SAFETY: a plain Win32 query filling the struct.
    let t = unsafe {
        let mut t = core::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// A note's text with Windows' newlines throughout, one after the last line (the old script mixed the templates' LF
/// with the lists' CRLF; cmd looks labels up wrongly in a .bat with LF lines, and UNINSTALL.bat calls its labels).
fn set_content(path: &Path, text: &str) -> R<()> {
    let text = format!("{}\r\n", text.replace("\r\n", "\n").replace('\n', "\r\n"));
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

const INSTALL: &str = include_str!("templates/install.txt");
const UNINSTALL: &str = include_str!("templates/uninstall.txt");
const UNINSTALL_BAT: &str = include_str!("templates/uninstall.bat");
const GIT: &str = "$(git -C $RepoRoot rev-parse --short HEAD 2>$null)";
const DATE: &str = "$(Get-Date -Format \"yyyy-MM-dd\")";

fn dist(args: &[String]) -> R<()> {
    let mut version = "0.2.5.9".to_string();
    let mut build_dir = release::DEFAULT_BUILD_DIR.to_string();
    let (mut zip, mut force) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" => version = it.next().ok_or("--version needs a value")?.clone(),
            "--build-dir" => build_dir = it.next().ok_or("--build-dir needs a value")?.clone(),
            "--zip" => zip = true,
            "--force" => force = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if !valid_version(&version) {
        return Err(format!("not a version: {version}"));
    }
    let root = repo_root();
    let dist_root = root.join("dist");
    let out = dist_root.join(format!("CyberpunkVRPort-{version}"));
    if out.exists() {
        if !force {
            return Err(format!("Package already exists: {}. Use --force to rebuild it.", out.display()));
        }
        let meta = fs::symlink_metadata(&out).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || out.parent() != Some(dist_root.as_path()) {
            return Err("Refusing to replace a package outside dist or a linked directory".into());
        }
        fs::remove_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut p = Package { out: out.clone(), manifest: Vec::new() };
    // THE PORT'S OWN FOLDER AT THE GAME'S ROOT (2026-11-24), loaded by its own loader: RED4ext is not needed.
    let plugin = "cyberpunkvrport";
    let release = root.join(&build_dir).join(release::RELEASE_OUT);
    let shaders = root.join("src\\Shaders");

    // ---- the plugin: ONE plugin and the TweakDB names (CET's list, read when CET is not installed)
    p.add(&need(release.join("cyberpunkvrport.dll"), "cyberpunkvrport.dll")?, &format!("{plugin}\\cyberpunkvrport.dll"))?;
    // the loader: a proxy of the system's powrprof.dll that starts the port (crates/cvr-loader)
    p.add(&need(release.join("powrprof.dll"), "the port's loader powrprof.dll")?, "bin\\x64\\powrprof.dll")?;
    for n in ["tweakdbstr.kark", "usedhashes.kark", "LICENSE.txt"] {
        p.add(&need(release.join("config\\tweakdb").join(n), &format!("TweakDB names {n}"))?, &format!("{plugin}\\config\\tweakdb\\{n}"))?;
    }
    // ---- DLSS 5 NEURAL RENDERING's runtime (cvr-render neural_render.rs): NGX loads it from beside the game's exe.
    // NVIDIA's binary is not in git; the repo's copy at externals\dlss5\nvngx_dlssnr.dll goes in when it is there
    let nr = root.join("externals\\dlss5\\nvngx_dlssnr.dll");
    if nr.exists() {
        p.add(&nr, "bin\\x64\\nvngx_dlssnr.dll")?;
        println!("  DLSS 5 NR runtime: bin\\x64\\nvngx_dlssnr.dll");
    } else {
        println!("  WARNING: no {} -- the package has no DLSS 5 Neural Rendering runtime", nr.display());
    }
    // ---- ANR FG (Apex Neural Rendering, github.com/apex-compute/ANR): the frame generator's dlssg folder, unchanged
    // (cmake/NativeFramegen.cmake fetches the pinned release beside the plugin; cvr-render anr_fg.rs loads it from
    // <plugin>\anr\dlssg), its own logs left out (SKIP_FILES) -- as main's build_dist.ps1 packs it
    let anr = release.join("anr\\dlssg");
    need(anr.join("nvngx_dlssg.dll"), "ANR FG nvngx_dlssg.dll")?;
    let anr_rel = format!("{plugin}\\anr\\dlssg");
    let anr_files = p.tree(&anr, &anr_rel)?;
    p.folder(&anr_rel, anr_files);
    // ---- the shaders, loaded by name: the engine's scope glass twins (the port's own shaders -- the optics' lens
    // velocity, lens and sight masks and source resolves since B649, the sight's vertex and pixel shaders and the
    // scanner outline since B650 -- are made from Rust and embedded in the plugin)
    // -- and the C++ build's own DXIL of those, which the plugin draws with by default again (cvr_gpu::dxil: the C++
    // build's shaders unless CVR_SHADER_RUST=1), under the C++ package's names
    for (src, name) in [
        ("scope_glass_649567EB5A039A01.dxil", "CyberpunkVR_ScopeGlass_649567EB5A039A01.dxil"),
        ("scope_glass_372EEC30C414B67A.dxil", "CyberpunkVR_ScopeGlass_372EEC30C414B67A.dxil"),
        ("sight_reflex_ps.dxil", "CyberpunkVR_SightPs.dxil"),
        ("sight_reflex_vs.dxil", "CyberpunkVR_SightVs.dxil"),
        ("sight_reflex_ps_scope.dxil", "CyberpunkVR_SightPsScope.dxil"),
        ("lens_velocity_ps.dxil", "CyberpunkVR_LensVelocityPs.dxil"),
        ("lens_mask_ps.dxil", "CyberpunkVR_LensMaskPs.dxil"),
        ("sight_mask_ps.dxil", "CyberpunkVR_SightMaskPs.dxil"),
        ("lens_source_resolve_cs.dxil", "CyberpunkVR_LensSourceResolveCs.dxil"),
        ("camera_source_resolve_cs.dxil", "CyberpunkVR_CameraSourceResolveCs.dxil"),
        ("vision_highlight_cs.dxil", "CyberpunkVR_VisionCs.dxil"),
    ] {
        p.add(&need(shaders.join(src), src)?, &format!("{plugin}\\shaders\\{name}"))?;
    }
    // in the old script's order: fog, SSR, shadow, view_dir (all), cone-AO, particles (counted), exposure, screen fx,
    // light merge (all), dxc, engine-stereo merge (counted), RT (counted), eye, bloom, particle lighting, light cull,
    // distant light, probe cull. The twins by engine shader hash: (folder in src\Shaders, folder in the package, hashes)
    let hashed: [(&str, &str, &[&str]); 3] = [
        ("asym_fog", "asym", &["3bec9b299e5c0610", "601954c7c7c00b49", "b098a198a7565e1d", "f60d19af775ca034"]),
        (
            "asym_ssr",
            "asym_ssr",
            &["842ab722bfe403cd", "b86f8871454997e6", "c827b32cf54f25fb", "8661cc86756e2ecc", "07c867b70fba5da4", "acaedc11005a92d0", "5e01aaacc7c617d2", "e5741046a837b9cc"],
        ),
        ("asym_shadow", "asym_shadow", &["753b3bed8df87d53"]),
    ];
    for (src_dir, dst_dir, hashes) in hashed {
        for h in hashes {
            p.add(&need(shaders.join(src_dir).join(format!("{h}.dxil")), &format!("{src_dir} {h}"))?, &format!("{plugin}\\shaders\\{dst_dir}\\{h}.dxil"))?;
        }
    }
    for f in dxil_in(&shaders.join("view_dir"))? {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        p.add(&f, &format!("{plugin}\\shaders\\view_dir\\{name}"))?;
    }
    for h in ["33f28d9a0e7a8352", "1cbc87916ec8e03c", "1381eb9e608485d7", "059246fd5e64ea98"] {
        p.add(&need(shaders.join("asym_ao").join(format!("{h}.dxil")), &format!("cone-AO shift twin {h}"))?, &format!("{plugin}\\shaders\\asym_ao\\{h}.dxil"))?;
    }
    let counted = |dir: &str, at_least: usize, what: &str| -> R<Vec<PathBuf>> {
        let d = need(shaders.join(dir), what)?;
        let v = dxil_in(&d)?;
        if v.len() < at_least {
            return Err(format!("{what}: {} of {at_least} in src\\Shaders\\{dir}", v.len()));
        }
        Ok(v)
    };
    for f in counted("asym_particles", 106, "particle-facing twins")? {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        p.add(&f, &format!("{plugin}\\shaders\\asym_particles\\{name}"))?;
    }
    for name in ["1a6a7c32da1959d6.dxil", "d1b593e255bfcb11.dxil"] {
        p.add(&need(shaders.join("asym_expo").join(name), &format!("auto-exposure twin {name}"))?, &format!("{plugin}\\shaders\\asym_expo\\{name}"))?;
    }
    p.add(&need(shaders.join("screen_fx\\6924aa01ad878186.dxil"), "Sandevistan screen-effect twin")?, &format!("{plugin}\\shaders\\screen_fx\\6924aa01ad878186.dxil"))?;
    for f in dxil_in(&need(shaders.join("stereo_light_merge"), "merged tiled-light twins")?)? {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        p.add(&f, &format!("{plugin}\\shaders\\stereo_light_merge\\{name}"))?;
    }
    // the DirectX shader compiler single-pass stereo rewrites the engine's shaders with, from the Windows SDK
    let dxc = PathBuf::from("C:\\Program Files (x86)\\Windows Kits\\10\\bin\\10.0.26100.0\\x64");
    for n in ["dxcompiler.dll", "dxil.dll"] {
        p.add(&need(dxc.join(n), &format!("DirectX shader compiler {n}"))?, &format!("{plugin}\\dxc\\{n}"))?;
    }
    for f in counted("stereo_merge", 332, "engine-stereo merge twins")? {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        p.add(&f, &format!("{plugin}\\shaders\\stereo_merge\\{name}"))?;
    }
    for f in counted("stereo_rt", 38, "ray-traced reflection twins")? {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        p.add(&f, &format!("{plugin}\\shaders\\stereo_rt\\{name}"))?;
    }
    for h in ["8a6c46c528ce5160", "c944329b340f33c2", "4145873fbf34bf87", "6ceb2ea336059978"] {
        p.add(&need(shaders.join("stereo_eye").join(format!("{h}.dxil")), &format!("engine-stereo eye twin {h}"))?, &format!("{plugin}\\shaders\\stereo_eye\\{h}.dxil"))?;
    }
    for (dir, h, what) in [
        ("stereo_bloom", "83848f96b80814d5", "stereo bloom prefilter"),
        ("stereo_particle_lighting", "e4c84ae0e5616f91", "stereo particle-lighting allocator"),
        ("stereo_light_cull", "37851ec24d58e673", "stereo light-culling twin"),
        ("stereo_distant_light", "6a1544708bf0067e", "stereo distant-light culler"),
        ("stereo_probe_cull", "355ba40185eeae65", "stereo probe-culling twin"),
    ] {
        p.add(&need(shaders.join(dir).join(format!("{h}.dxil")), what)?, &format!("{plugin}\\shaders\\{dir}\\{h}.dxil"))?;
    }
    // ---- the settings preset, merged once on the first launch
    p.add(&need(root.join("mods\\config\\UserSettings.json"), "UserSettings.json")?, &format!("{plugin}\\config\\UserSettings.json"))?;
    // ---- the captured grip poses, enumerated
    let config = root.join("mods\\config");
    for e in entries(&config)? {
        let name = e.file_name().to_string_lossy().to_string();
        if e.file_type().is_ok_and(|t| t.is_file()) && name.starts_with("CyberpunkVR_") && like(&name, "*.ini") {
            p.add(&e.path(), &format!("{plugin}\\grips\\{name}"))?;
        }
    }
    // ---- the baked prop grasps (cvr-rig grasp/baked.rs reads them from grips\, main's build_dist.ps1 packs them so)
    p.add(&need(config.join("CyberpunkVR_MeshGrasps.bin"), "baked prop grasps")?, &format!("{plugin}\\grips\\CyberpunkVR_MeshGrasps.bin"))?;
    // ---- the OpenVR shim
    p.add(&need(config.join("openvr_api.dll"), "openvr_api.dll")?, "bin\\x64\\openvr_api.dll")?;
    // ---- the data the plugin reads from its lua\ folder; the TweakDB records
    for (dir, pattern, at_least) in LUA_DATA {
        let src = root.join("mods\\cet").join(dir);
        let files: Vec<fs::DirEntry> = entries(&src)?
            .into_iter()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && like(&e.file_name().to_string_lossy(), pattern))
            .collect();
        if files.len() < at_least {
            return Err(format!("mods\\cet\\{dir}: {} of {at_least} {pattern}", files.len()));
        }
        for e in files {
            p.add(&e.path(), &format!("{plugin}\\lua\\{dir}\\{}", e.file_name().to_string_lossy()))?;
        }
    }
    let rel = format!("{plugin}\\config\\tweaks");
    let n = p.tree(&root.join("mods\\tweaks"), &rel)?;
    p.folder(&rel, n);
    // ---- the packed archives (exact *.archive names) and their one manifest
    let archives = root.join("mods\\archive");
    let listing = entries(&archives)?;
    let packed: Vec<&fs::DirEntry> = listing.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && like(&e.file_name().to_string_lossy(), "*.archive")).collect();
    // ONE ARCHIVE: every packed asset is in cyberpunkvrport.archive (the HUD hints' atlas too, since 0.2.6)
    if packed.len() != 1 || packed[0].file_name() != "cyberpunkvrport.archive" {
        let names: Vec<String> = packed.iter().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        return Err(format!("mods\\archive must hold exactly one archive, cyberpunkvrport.archive (found: {})", names.join(", ")));
    }
    for e in packed {
        p.add(&e.path(), &format!("{plugin}\\archive\\{}", e.file_name().to_string_lossy()))?;
    }
    let manifests: Vec<&fs::DirEntry> = listing.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && like(&e.file_name().to_string_lossy(), "*.archive.xl")).collect();
    if manifests.len() != 1 || manifests[0].file_name() != "cyberpunkvrport.archive.xl" {
        let names: Vec<String> = manifests.iter().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        return Err(format!("mods\\archive must hold exactly one manifest, cyberpunkvrport.archive.xl (found: {})", names.join(", ")));
    }
    p.add(&manifests[0].path(), &format!("{plugin}\\config\\archive.yaml"))?;
    // ---- the input bindings the plugin merges
    let input = config.join("input");
    if input.exists() {
        for b in ["CyberpunkVRPort_ScannerHud.xml", "CyberpunkVRPort_PhotoMode.xml"] {
            p.add(&need(input.join(b), b)?, &format!("{plugin}\\config\\input\\{b}"))?;
        }
    }

    // ---- the notes: INSTALL.txt, CHANGELOG.txt, UNINSTALL.txt / .bat (generated from what was copied)
    let commit = short_commit(&root);
    let date = today();
    let install = INSTALL.replace(GIT, &commit).replace(DATE, &date).replace("$Version", &version);
    set_content(&out.join("INSTALL.txt"), &install)?;
    let changelog = root.join("CHANGELOG.md");
    match fs::read_to_string(&changelog) {
        Ok(text) => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            set_content(&out.join("CHANGELOG.txt"), text)?;
            println!("  {:<64}{:>10}", "CHANGELOG.txt", thousands(fs::metadata(out.join("CHANGELOG.txt")).map(|m| m.len()).unwrap_or(0)));
        }
        Err(_) => println!("[!] CHANGELOG.md not found -- the package ships without one"),
    }
    let mut own_dirs: Vec<String> =
        vec![plugin.to_string(), "red4ext\\plugins\\cyberpunkvrport".to_string(), "red4ext\\plugins\\CyberpunkVR_Stereo".to_string()];
    for (path, bytes) in &p.manifest {
        if *bytes == 0 {
            if let Some(dir) = path.strip_suffix(" files)").and_then(|s| s.rsplit_once("\\  (")).map(|(d, _)| d) {
                own_dirs.push(dir.to_string());
            }
        }
    }
    sort_unique(&mut own_dirs);
    let mut loose: Vec<String> = p
        .manifest
        .iter()
        .filter(|(path, bytes)| *bytes > 0 && !own_dirs.iter().any(|d| path.starts_with(&format!("{d}\\"))))
        // the loader stays: without cyberpunkvrport\ it starts nothing and the game runs flat (UNINSTALL.txt says so)
        .filter(|(path, _)| !path.eq_ignore_ascii_case("bin\\x64\\powrprof.dll"))
        .map(|(path, _)| path.clone())
        .collect();
    loose.push("engine\\config\\platform\\pc\\vrcam_cpu_tweaks.ini".into());
    sort_unique(&mut loose);
    let dir_lines = own_dirs.iter().map(|d| format!("    {d}\\")).collect::<Vec<_>>().join("\r\n");
    let loose_lines = loose.iter().map(|f| format!("    {f}")).collect::<Vec<_>>().join("\r\n");
    let uninstall = UNINSTALL
        .replace(GIT, &commit)
        .replace(DATE, &date)
        .replace("$Version", &version)
        .replace("$dirLines", &dir_lines)
        .replace("$looseLines", &loose_lines);
    set_content(&out.join("UNINSTALL.txt"), &uninstall)?;
    let bat_dirs = own_dirs.iter().map(|d| format!("call :killdir \"{d}\"")).collect::<Vec<_>>().join("\r\n");
    let bat_files = loose.iter().map(|f| format!("call :killfile \"{f}\"")).collect::<Vec<_>>().join("\r\n");
    let bat = UNINSTALL_BAT
        .replace("$($ownDirs.Count)", &own_dirs.len().to_string())
        .replace("$($loose.Count)", &loose.len().to_string())
        .replace("$Version", &version)
        .replace("$batDirs", &bat_dirs)
        .replace("$batFiles", &bat_files);
    set_content(&out.join("UNINSTALL.bat"), &bat)?;

    // ---- report and the package's own checks
    println!("dist\\CyberpunkVRPort-{version}");
    for (path, bytes) in &p.manifest {
        if *bytes > 0 {
            println!("  {path:<62} {:>10}", thousands(*bytes));
        } else {
            println!("  {path}");
        }
    }
    let mut all = Vec::new();
    walk(&out, &mut all);
    if all.iter().any(|f| f.components().any(|c| c.as_os_str() == "CyberpunkVRPort_QuickBoot")) {
        return Err("QuickBoot must not be included in a release package".into());
    }
    // the Rust build runs no Lua and compiles no script: nothing of either may ride along
    if let Some(f) = all.iter().find(|f| {
        f.extension().is_some_and(|e| e.eq_ignore_ascii_case("lua") || e.eq_ignore_ascii_case("reds"))
            || f.file_name().is_some_and(|n| n.eq_ignore_ascii_case("scc_lib.dll") || n.eq_ignore_ascii_case("scc.exe"))
    }) {
        return Err(format!("{} must not be in the package: the plugin runs no Lua and compiles no script", f.display()));
    }
    // exactly one plugin, the port's (it keeps RED4ext's entries too): every DLL in the package asked for them
    let plugins: Vec<&PathBuf> = all
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")))
        .filter(|f| release::Pe::read(f).is_ok_and(|pe| release::is_red4ext_plugin(&pe.exports())))
        .collect();
    let expected = out.join(format!("{plugin}\\cyberpunkvrport.dll"));
    if plugins.len() != 1 || plugins[0] != &expected {
        let names: Vec<String> = plugins.iter().map(|p| p.display().to_string()).collect();
        return Err(format!("the package must hold exactly one plugin, the port's (found: {})", names.join(", ")));
    }
    if !out.join("bin\\x64\\powrprof.dll").exists() {
        return Err("the package must hold the port's loader, bin\\x64\\powrprof.dll".into());
    }
    let dlls = all.iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll"))).count();
    println!("  one plugin: {plugin}\\cyberpunkvrport.dll, its loader bin\\x64\\powrprof.dll ({dlls} DLLs in the package)");
    if out.join("r6\\scripts\\cache").exists() {
        return Err("Shared script cache must not be owned by the release package".into());
    }
    if out.join("r6\\scripts\\MoreOccluders.reds").exists() {
        return Err("More Occluders is not part of the port and must not be packaged".into());
    }
    let total: u64 = all.iter().filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum();
    println!();
    println!("  {} files, {} bytes total", all.len(), thousands(total));
    if zip {
        let archive = PathBuf::from(format!("{}.zip", out.display()));
        let entries = write_zip(&out, &archive)?;
        println!(
            "  packaged -> {}  ({} bytes, {entries} entries, '/' paths)",
            archive.display(),
            thousands(fs::metadata(&archive).map(|m| m.len()).unwrap_or(0))
        );
    }
    Ok(())
}

/// release: build, the package with its zip, the symbols.
fn release_task(root: &Path, args: &[String]) -> R<()> {
    let (o, rest) = release::BuildOpts::parse(root, args)?;
    let mut version = None;
    let mut force = false;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" => version = Some(it.next().ok_or("--version needs a value")?.clone()),
            "--force" => force = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let version = version.ok_or("release needs --version")?;
    if !valid_version(&version) {
        return Err(format!("not a version: {version}"));
    }
    if !release::tracked_clean(root) {
        println!("[release] note: tracked files differ from the commit -- the package is not the commit's alone");
    }
    release::build(root, &o)?;
    let mut d = vec!["--version".to_string(), version.clone(), "--build-dir".into(), o.build_dir.display().to_string(), "--zip".into()];
    if force {
        d.push("--force".into());
    }
    dist(&d)?;
    release::symbols(root, &o.build_dir.join(release::RELEASE_OUT), &version, &short_commit(root), true)?;
    Ok(())
}

/// Every file under `dir`.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(list) = fs::read_dir(dir) else { return };
    for e in list.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), out),
            Ok(t) if t.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}
/// Sort-Object -Unique over paths: case-insensitive, one of each.
fn sort_unique(v: &mut Vec<String>) {
    v.sort_by_key(|s| s.to_ascii_lowercase());
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
}
/// "{0:N0}": thousands with commas.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------- the zip
/// CRC-32 (IEEE, the zip's).
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    let mut c = !0u32;
    for &b in data {
        c = t[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}
/// The package as a zip: every file stored under its relative path with '/', each empty folder as "folder/"; the
/// names checked against the spec (no '\', no leading '/', no drive). Stored, not deflated: the archives and DXIL in
/// it are compressed already. ZIP64 is not written (the package stays far below 4 GB).
fn write_zip(root: &Path, archive: &Path) -> R<usize> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort_by_key(|f| f.to_string_lossy().to_ascii_lowercase());
    let mut dirs = Vec::new();
    collect_empty_dirs(root, &mut dirs);
    dirs.sort_by_key(|d| d.to_string_lossy().to_ascii_lowercase());
    let rel = |p: &Path| -> String { p.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default() };
    let mut out = std::io::BufWriter::new(fs::File::create(archive).map_err(|e| format!("{}: {e}", archive.display()))?);
    let mut central = Vec::new();
    let mut offset: u64 = 0;
    let mut count = 0usize;
    let (time, date) = dos_now();
    let mut entry = |name: &str, data: &[u8], out: &mut std::io::BufWriter<fs::File>| -> R<()> {
        if name.contains('\\') || name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
            return Err(format!("Zip entry name breaks the zip spec: {name}"));
        }
        let crc = crc32(data);
        let size = u32::try_from(data.len()).map_err(|_| format!("{name}: over 4 GB"))?;
        let off = u32::try_from(offset).map_err(|_| "the zip is over 4 GB".to_string())?;
        let mut local = Vec::new();
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0x0800u16.to_le_bytes()); // names in UTF-8
        local.extend_from_slice(&0u16.to_le_bytes()); // stored
        local.extend_from_slice(&time.to_le_bytes());
        local.extend_from_slice(&date.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        out.write_all(&local).map_err(|e| e.to_string())?;
        out.write_all(data).map_err(|e| e.to_string())?;
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0u8; 8]); // extra, comment, disk start, internal attributes
        let external: u32 = if name.ends_with('/') { 0x10 } else { 0x20 };
        central.extend_from_slice(&external.to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
        offset += local.len() as u64 + data.len() as u64;
        count += 1;
        Ok(())
    };
    for f in &files {
        let data = fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        entry(&rel(f), &data, &mut out)?;
    }
    for d in &dirs {
        entry(&format!("{}/", rel(d)), &[], &mut out)?;
    }
    let start = u32::try_from(offset).map_err(|_| "the zip is over 4 GB".to_string())?;
    out.write_all(&central).map_err(|e| e.to_string())?;
    let mut end = Vec::new();
    end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    end.extend_from_slice(&[0u8; 4]);
    let n = u16::try_from(count).map_err(|_| "over 65535 entries".to_string())?;
    end.extend_from_slice(&n.to_le_bytes());
    end.extend_from_slice(&n.to_le_bytes());
    end.extend_from_slice(&(central.len() as u32).to_le_bytes());
    end.extend_from_slice(&start.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    out.write_all(&end).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(count)
}
fn collect_empty_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(list) = fs::read_dir(dir) else { return };
    for e in list.flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            let p = e.path();
            if fs::read_dir(&p).map(|mut r| r.next().is_none()).unwrap_or(false) {
                out.push(p.clone());
            }
            collect_empty_dirs(&p, out);
        }
    }
}
/// Now as MS-DOS time and date (local).
fn dos_now() -> (u16, u16) {
    // SAFETY: a plain Win32 query filling the struct.
    let t = unsafe {
        let mut t = core::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    let time = (t.wHour << 11) | (t.wMinute << 5) | (t.wSecond / 2);
    let date = (t.wYear.saturating_sub(1980) << 9) | (t.wMonth << 5) | t.wDay;
    (time, date)
}
