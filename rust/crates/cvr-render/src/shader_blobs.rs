//! The replacement shaders the device hooks swap in (src/Stereo/DeviceHooks.cpp, `sight_blob_ready` and
//! `vision_cs_blob_ready`), read once from the plugin's shader folder and held for the process: the sight's pixel and
//! vertex shaders (CyberpunkVR_SightPs.dxil, CyberpunkVR_SightVs.dxil -- both or neither: the pixel shader reads the
//! glass size the vertex shader writes, so half a swap would be worse than none), the sight's path variants once both
//! are in (CyberpunkVR_SightPsScope.dxil; a missing variant keeps the collimator build on that path), and the scanner
//! outline's compute shader (CyberpunkVR_VisionCs.dxil). A file counts when it is over 64 bytes and under 4 MiB and
//! starts with the DXBC container's magic.
//!
//! Since B650 the four are made from Rust (rust/shaders/passes/src/sight.rs, sight_ps.rs, vision.rs) and embedded by
//! cvr-gpu; the files are read only when CVR_SHADER_HLSL names them (the way back, the DXIL DXC made from src/Shaders).
//!
//! The device hooks ask `cvr_dh_sight_ready` / `cvr_dh_vision_ready` (the first ask reads, under the lock, and logs what
//! it found through the stereo log) and borrow the bytes with `cvr_dh_shader_blob` (0 the sight's pixel shader, 1 its
//! vertex shader, 2 its scope variant, 3 the outline; none until read). The unit tests below read files in the temp
//! folder.
use std::path::Path;
use std::sync::OnceLock;

/// The sight's path variants, by index.
pub const SIGHT_VARIANT_FILES: [&str; 1] = ["CyberpunkVR_SightPsScope.dxil"];

/// The sight's shaders, once both are read.
pub struct Sight {
    pub ps: Vec<u8>,
    pub vs: Vec<u8>,
    pub variants: [Vec<u8>; SIGHT_VARIANT_FILES.len()],
}

/// A DXBC container from the file: over 64 bytes, under 4 MiB, with its magic.
pub fn read_dxbc(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    (bytes.len() > 64 && bytes.len() < (4 << 20) && bytes.starts_with(b"DXBC")).then_some(bytes)
}
/// sight_blob_ready's read: the pixel then the vertex shader (the vertex shader not tried without the pixel shader),
/// then each variant.
pub fn load_sight(dir: &Path, log: &mut impl FnMut(String)) -> Option<Sight> {
    let mut pair = Vec::with_capacity(2);
    for name in ["CyberpunkVR_SightPs.dxil", "CyberpunkVR_SightVs.dxil"] {
        let path = dir.join(name);
        match read_dxbc(&path) {
            Some(bytes) => pair.push(bytes),
            None => {
                log(format!("[pso] sight replacement NOT found ({}) -- original shaders kept", path.display()));
                return None;
            }
        }
    }
    let variants = SIGHT_VARIANT_FILES.map(|name| {
        let blob = read_dxbc(&dir.join(name)).unwrap_or_default();
        let what = if blob.is_empty() { "NOT found -- that path keeps the collimator build" } else { "loaded" };
        log(format!("[pso] sight variant {name} {what} ({} B)", blob.len()));
        blob
    });
    let vs = pair.pop().unwrap_or_default();
    let ps = pair.pop().unwrap_or_default();
    log(format!("[pso] sight replacement loaded: PS {} B, VS {} B", ps.len(), vs.len()));
    Some(Sight { ps, vs, variants })
}
/// vision_cs_blob_ready's read.
pub fn load_vision(dir: &Path, log: &mut impl FnMut(String)) -> Option<Vec<u8>> {
    let path = dir.join("CyberpunkVR_VisionCs.dxil");
    match read_dxbc(&path) {
        Some(bytes) => {
            log(format!("[pso] outline CS replacement loaded: {} B", bytes.len()));
            Some(bytes)
        }
        None => {
            log(format!("[pso] outline CS replacement NOT found ({}) -- original kept", path.display()));
            None
        }
    }
}

static SIGHT: OnceLock<Option<Sight>> = OnceLock::new();
static VISION: OnceLock<Option<Vec<u8>>> = OnceLock::new();

/// The bytes of a shader that was read (0 the sight's pixel shader, 1 its vertex shader, 2 + n its path variant n,
/// then the outline); none until it was read or when it is missing.
pub fn blob(which: u32) -> Option<&'static [u8]> {
    let sight = SIGHT.get().and_then(Option::as_ref);
    let variants = SIGHT_VARIANT_FILES.len() as u32;
    let bytes: &'static [u8] = match which {
        0 => &sight?.ps,
        1 => &sight?.vs,
        n if n >= 2 && n < 2 + variants => &sight?.variants[(n - 2) as usize],
        n if n == 2 + variants => VISION.get().and_then(Option::as_ref)?,
        _ => return None,
    };
    (!bytes.is_empty()).then_some(bytes)
}

// ---- the C ABI (the Rust build: the stereo log and the plugin's folders are the DLL's)

#[cfg(feature = "rust-build")]
mod abi {
    use super::*;
    use core::ffi::c_char;

    extern "C" {
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text) -- the stereo log, its switch and its rate cap.
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    fn stereo_log(text: String) {
        let line = std::ffi::CString::new(text).unwrap_or_default();
        // SAFETY: a C string for the C++ half's log.
        unsafe { cvr_cpp_stereo_log(line.as_ptr()) };
    }
    fn shaders() -> &'static Path {
        cvr_host::paths::dir(cvr_host::paths::Folder::Shaders)
    }

    #[no_mangle]
    pub extern "C" fn cvr_dh_sight_ready() -> bool {
        use cvr_gpu::dxil::{hlsl_for, SIGHT_REFLEX_PS, SIGHT_REFLEX_PS_SCOPE, SIGHT_REFLEX_VS};
        SIGHT
            .get_or_init(|| {
                if hlsl_for(SIGHT_REFLEX_PS) || hlsl_for(SIGHT_REFLEX_VS) || hlsl_for(SIGHT_REFLEX_PS_SCOPE) {
                    return load_sight(shaders(), &mut stereo_log);
                }
                stereo_log(format!(
                    "[pso] sight replacement made from Rust: PS {} B, VS {} B, scope PS {} B",
                    SIGHT_REFLEX_PS.len(),
                    SIGHT_REFLEX_VS.len(),
                    SIGHT_REFLEX_PS_SCOPE.len()
                ));
                Some(Sight { ps: SIGHT_REFLEX_PS.to_vec(), vs: SIGHT_REFLEX_VS.to_vec(), variants: [SIGHT_REFLEX_PS_SCOPE.to_vec()] })
            })
            .is_some()
    }
    #[no_mangle]
    pub extern "C" fn cvr_dh_vision_ready() -> bool {
        use cvr_gpu::dxil::{hlsl_for, VISION_HIGHLIGHT_CS};
        VISION
            .get_or_init(|| {
                if hlsl_for(VISION_HIGHLIGHT_CS) {
                    return load_vision(shaders(), &mut stereo_log);
                }
                stereo_log(format!("[pso] outline CS replacement made from Rust: {} B", VISION_HIGHLIGHT_CS.len()));
                Some(VISION_HIGHLIGHT_CS.to_vec())
            })
            .is_some()
    }
    /// # Safety
    /// `size` is writable.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_dh_shader_blob(which: u32, size: *mut usize) -> *const u8 {
        let bytes = blob(which).unwrap_or_default();
        unsafe { *size = bytes.len() };
        if bytes.is_empty() { core::ptr::null() } else { bytes.as_ptr() }
    }
}

#[cfg(test)]
mod tests {
    //! Files in the temp folder.
    use super::*;

    fn dxbc(len: usize) -> Vec<u8> {
        let mut v = b"DXBC".to_vec();
        v.resize(len, 7);
        v
    }
    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cvr_shader_blobs_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn containers() {
        let dir = folder("containers");
        for (name, bytes) in [("ok", dxbc(65)), ("short", dxbc(64)), ("magic", b"DXIL".iter().copied().chain([0u8; 100]).collect()), ("big", dxbc(4 << 20))] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        assert_eq!(read_dxbc(&dir.join("ok")).map(|b| b.len()), Some(65));
        assert!(read_dxbc(&dir.join("short")).is_none());
        assert!(read_dxbc(&dir.join("magic")).is_none());
        assert!(read_dxbc(&dir.join("big")).is_none());
        assert!(read_dxbc(&dir.join("none")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn sight() {
        let dir = folder("sight");
        let mut lines = Vec::new();
        // Without the pixel shader the vertex shader is not tried.
        std::fs::write(dir.join("CyberpunkVR_SightVs.dxil"), dxbc(100)).unwrap();
        assert!(load_sight(&dir, &mut |l| lines.push(l)).is_none());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("CyberpunkVR_SightPs.dxil") && lines[0].ends_with("-- original shaders kept"));
        // Both: loaded; a missing variant is said and left empty.
        std::fs::write(dir.join("CyberpunkVR_SightPs.dxil"), dxbc(200)).unwrap();
        lines.clear();
        let s = load_sight(&dir, &mut |l| lines.push(l)).unwrap();
        assert_eq!((s.ps.len(), s.vs.len(), s.variants[0].len()), (200, 100, 0));
        assert_eq!(lines, [
            "[pso] sight variant CyberpunkVR_SightPsScope.dxil NOT found -- that path keeps the collimator build (0 B)".to_string(),
            "[pso] sight replacement loaded: PS 200 B, VS 100 B".to_string(),
        ]);
        std::fs::write(dir.join("CyberpunkVR_SightPsScope.dxil"), dxbc(300)).unwrap();
        lines.clear();
        assert_eq!(load_sight(&dir, &mut |l| lines.push(l)).unwrap().variants[0].len(), 300);
        assert_eq!(lines[0], "[pso] sight variant CyberpunkVR_SightPsScope.dxil loaded (300 B)");
        // The outline.
        lines.clear();
        assert!(load_vision(&dir, &mut |l| lines.push(l)).is_none());
        assert!(lines[0].starts_with("[pso] outline CS replacement NOT found ("));
        std::fs::write(dir.join("CyberpunkVR_VisionCs.dxil"), dxbc(90)).unwrap();
        lines.clear();
        assert_eq!(load_vision(&dir, &mut |l| lines.push(l)).map(|b| b.len()), Some(90));
        assert_eq!(lines, ["[pso] outline CS replacement loaded: 90 B".to_string()]);
        // Nothing is held before a read.
        assert!(blob(0).is_none() && blob(3).is_none() && blob(9).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
