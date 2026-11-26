//! The per-view render-mask report (src/Stereo/FrameGraph.cpp, `render_mask_report`): with the runtime diagnostics on,
//! at most every 20 s, for each named render-mask category (its descriptor in the image: a word, then the 32-word
//! mask it needs) whether MAIN's and VRCAM's frame-graph contexts carry every bit of it (their masks at +0x2688),
//! logged as one `[rmask]` line (`name=MV`, `-` where a view lacks it), the line kept to the .cpp's 1400-byte buffer.
//! A context or descriptor that cannot be read ends the report without a line (the window still taken).
//!
//! The C++ half passes VRCAM's context, the image base and its category table to `cvr_fg_render_mask_report`
//! (MAIN's context is frame_graph's); the reads go through ReadProcessMemory where the .cpp read them under SEH. The
//! unit tests below check the line and the mask test.
use core::ffi::{c_char, CStr};

/// RenderMaskEntry (include/Stereo/StereoInternal.hpp).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RenderMaskEntry {
    pub name: *const c_char,
    pub desc_rva: u32,
}
const _: () = assert!(core::mem::size_of::<RenderMaskEntry>() == 16);

/// Whether a view's 32-word mask carries every bit the category needs.
pub fn carries(have: &[u64; 32], need: &[u64; 32]) -> bool {
    have.iter().zip(need).all(|(h, n)| h & n == *n)
}
/// The report's categories part: `name=MV ` per category, as the .cpp's snprintf loop wrote it into its buffer (an
/// entry written only while fewer than 1352 bytes are used; the buffer holds 1399 characters).
pub fn categories(entries: &[(&str, bool, bool)]) -> String {
    let (mut line, mut used) = (String::new(), 0usize);
    for (name, m, v) in entries {
        if used < 1400 - 48 {
            let entry = format!("{name}={}{} ", if *m { 'M' } else { '-' }, if *v { 'V' } else { '-' });
            used += entry.len();
            line += &entry;
        }
    }
    line.truncate(1399);
    line
}

#[cfg(feature = "rust-build")]
pub(crate) mod abi {
    use super::*;
    use crate::instance_layout::read;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    #[allow(non_upper_case_globals)]
    extern "C" {
        static CyberpunkVR_RuntimeDiagnostics: i32;
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    static LAST: AtomicU64 = AtomicU64::new(0);

    /// The named render-mask categories (formerly src/Stereo/NodeDispatch.cpp's kRenderMasks): a name and its
    /// descriptor's RVA in the image. CyberpunkVR_RenderMaskGrant is a bit per row, so rows are only ever appended.
    struct Masks([RenderMaskEntry; 27]);
    // SAFETY: the names are static literals.
    unsafe impl Sync for Masks {}
    const fn e(name: &'static CStr, desc_rva: u32) -> RenderMaskEntry {
        RenderMaskEntry { name: name.as_ptr(), desc_rva }
    }
    static MASKS: Masks = Masks([
        e(c"DistantLights", 0x3C1CDF0),       // bit 0 -- the unlit lamps (granted by default)
        e(c"AutoGrass", 0x3C1D120),           // bit 1 -- terrain scatter and its cascade (granted by default)
        e(c"Foliage", 0x3C1D010),             // bit 2
        e(c"Decals", 0x3C1CBD0),              // bit 3
        e(c"Terrain", 0x3C1CF00),             // bit 4
        e(c"EnvProbes", 0x3C1D230),           // bit 5
        e(c"Emissive", 0x3C1D340),            // bit 6
        e(c"LightChannels", 0x3C1D450),       // bit 7
        e(c"Fog", 0x3C1D560),                 // bit 8
        e(c"Lights", 0x3C1CCE0),              // bit 9
        e(c"ClearLighting", 0x3C1D780),       // bit 10
        e(c"GameplayPostProcess", 0x3C1C9B0), // bit 11 -- scanner/focus tint (granted by default)
        e(c"HUD", 0x3C1C8A0),                 // bit 12 -- the HUD is composited by the port: left off
        e(c"GeometrySkinned", 0x3C1BBE0),     // bit 13 -- skinned meshes: characters, and cloth
        e(c"GeometryStatic", 0x3C1BAD0),      // bit 14
        e(c"GeometryProxies", 0x3C1BCF0),     // bit 15
        e(c"DepthPrepass", 0x3C1BE00),        // bit 16
        e(c"GBuffer", 0x3C1BF10),             // bit 17
        e(c"GBufferLate", 0x3C1C020),         // bit 18
        e(c"Forward", 0x3C1C240),             // bit 19
        e(c"ForwardNoTXAA", 0x3C1C350),       // bit 20
        e(c"Particles", 0x3C1CAC0),           // bit 21
        e(c"Unlit", 0x3C1C680),               // bit 22
        e(c"WeaponPlane", 0x3C1C130),         // bit 23 -- the first-person weapon plane
        e(c"TopDownCarProxy", 0x3C1C790),     // bit 24
        e(c"DebugDraw", 0x3C1C570),           // bit 25
        e(c"Discarded", 0x3C1C460),           // bit 26
    ]);
    /// render_mask_report: VRCAM's context as the render mask last saw it, the image base and the categories.
    pub extern "C" fn cvr_cpp_render_mask_report() {
        let vrcam = crate::moved_globals::g_vrcam_ctx_seen.load(std::sync::atomic::Ordering::Acquire);
        // SAFETY: the base, set once at install; the table's static names.
        unsafe {
            let base = crate::stereo_shared::g_exe_base as usize;
            cvr_fg_render_mask_report(vrcam, base, MASKS.0.as_ptr(), MASKS.0.len() as u32);
        }
    }
    /// A category's descriptor RVA.
    pub extern "C" fn cvr_cpp_render_mask_desc(index: u32) -> u32 {
        MASKS.0.get(index as usize).map_or(0, |m| m.desc_rva)
    }
    /// The categories' count.
    pub extern "C" fn cvr_cpp_render_mask_count() -> u32 {
        MASKS.0.len() as u32
    }

    fn words(at: u64) -> Option<[u64; 32]> {
        let mut b = [0u8; 256];
        if at == 0 || !read(at, &mut b) {
            return None;
        }
        let mut out = [0u64; 32];
        for (o, c) in out.iter_mut().zip(b.chunks_exact(8)) {
            *o = u64::from_le_bytes(c.try_into().unwrap());
        }
        Some(out)
    }

    /// # Safety
    /// `masks` holds `count` entries with live names.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_fg_render_mask_report(vrcam: usize, base: usize, masks: *const RenderMaskEntry, count: u32) {
        // SAFETY: the C++ half's switch.
        if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } == 0 {
            return;
        }
        // SAFETY: a plain Win32 call.
        let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
        let last = LAST.load(Relaxed);
        if last != 0 && now - last < 20_000 {
            return;
        }
        let main = crate::frame_graph::cvr_fg_main_ctx();
        if main == 0 || vrcam == 0 || base == 0 {
            return;
        }
        LAST.store(now, Relaxed);
        let table = if masks.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(masks, count as usize) } };
        let mut entries = Vec::with_capacity(table.len());
        for entry in table {
            let need = words((base as u64).wrapping_add(u64::from(entry.desc_rva)).wrapping_add(8));
            let (have_main, have_vrcam) = (words(main as u64 + 0x2688), words(vrcam as u64 + 0x2688));
            let (Some(need), Some(hm), Some(hv)) = (need, have_main, have_vrcam) else { return };
            // SAFETY: the table's names are the C++ half's literals.
            let name = if entry.name.is_null() { "" } else { unsafe { CStr::from_ptr(entry.name) }.to_str().unwrap_or("") };
            entries.push((name, carries(&hm, &need), carries(&hv, &need)));
        }
        let line = format!("[rmask] per-view render-mask categories (M = main has it, V = vrcam has it): {}", categories(&entries));
        let text = std::ffi::CString::new(line).unwrap_or_default();
        // SAFETY: a C string for the C++ half's log.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    //! The line and the mask test.
    use super::*;

    #[test]
    fn masks() {
        let mut need = [0u64; 32];
        need[3] = 0b1010;
        let mut have = [0u64; 32];
        have[3] = 0b1110;
        assert!(carries(&have, &need));
        have[3] = 0b0110;
        assert!(!carries(&have, &need));
        assert!(carries(&[0; 32], &[0; 32]));
    }
    #[test]
    fn lines() {
        assert_eq!(categories(&[("Sky", true, false), ("Water", false, true)]), "Sky=M- Water=-V ");
        // Entries stop once 1352 bytes are used; the line never passes 1399.
        let long: Vec<(&str, bool, bool)> = (0..200).map(|_| ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", true, true)).collect();
        let line = categories(&long);
        assert_eq!(line.len(), 1352 / 30 * 30 + 30);
        // Five-byte entries: the one written at 1350 bytes is the last.
        let short: Vec<(&str, bool, bool)> = (0..300).map(|_| ("A", false, false)).collect();
        assert_eq!(categories(&short).len(), 1355);
    }
}
