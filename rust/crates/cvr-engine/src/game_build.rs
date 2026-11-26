//! THE GAME BUILD THIS PORT BELONGS TO (formerly src/Core/GameBuild.cpp).
//!
//! Every RVA, code guard and patch site was verified against exactly one Cyberpunk2077.exe. RED4ext loads the plugin
//! on any build (it declares itself runtime-independent, which the loader needs), so nothing else would stop it from
//! detouring the old addresses of a patched exe -- the crash on boot a game update would otherwise be.
//!
//! On another build the plugin registers its natives (the scripts that declare them must still load) and installs
//! nothing: no detour, no patch, no device hook. The game runs flat and says why.
//!
//!   CVR_ALLOW_UNKNOWN_BUILD=1   development only: every old address trusted on a build it was never checked against
//!   CVR_PRETEND_UNKNOWN_BUILD=1 the right build treated as a stranger, to test what a player on a patched game gets
use crate::addresses::build;
use cvr_host::wide::z;
use std::sync::OnceLock;
use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows_sys::Win32::System::Diagnostics::Debug::IMAGE_NT_HEADERS64;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE};

#[derive(Debug, Clone)]
pub struct Identity {
    pub timestamp: u32,
    pub size_of_image: u32,
    pub product_version: String,
    pub overridden: bool,
    pub supported: bool,
    pub expected: String,
    pub running: String,
}

/// The decision itself, separated from the process so it can be tested: the PE header must match, unless an
/// override says otherwise.
pub fn decide(timestamp: u32, size_of_image: u32, allow_unknown: bool, pretend_unknown: bool) -> (bool, bool) {
    let matches = !pretend_unknown && timestamp == build::TIMESTAMP && size_of_image == build::SIZE_OF_IMAGE;
    let supported = matches || allow_unknown;
    let overridden = allow_unknown && !matches;
    (supported, overridden)
}

fn env_is_one(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v.to_string_lossy().starts_with('1'))
}

/// The product version ("2.5") as the exe's resources state it: only for the message, the match is the PE header.
fn product_version(exe_path: &[u16]) -> Option<String> {
    // SAFETY: Win32 version-resource calls on an owned buffer sized by the first call.
    unsafe {
        let mut ignored = 0u32;
        let bytes = GetFileVersionInfoSizeW(exe_path.as_ptr(), &mut ignored);
        if bytes == 0 || bytes > (1 << 20) {
            return None;
        }
        let mut block = vec![0u8; bytes as usize];
        if GetFileVersionInfoW(exe_path.as_ptr(), 0, bytes, block.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let mut langs: *mut u16 = core::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(block.as_ptr().cast(), z("\\VarFileInfo\\Translation").as_ptr(),
            &mut langs as *mut _ as *mut *mut core::ffi::c_void, &mut len) == 0
            || len < 4
        {
            return None;
        }
        let key = format!("\\StringFileInfo\\{:04x}{:04x}\\ProductVersion", *langs, *langs.add(1));
        let mut text: *mut u16 = core::ptr::null_mut();
        if VerQueryValueW(block.as_ptr().cast(), z(key).as_ptr(), &mut text as *mut _ as *mut *mut core::ffi::c_void,
            &mut len) == 0
            || text.is_null()
            || len == 0
        {
            return None;
        }
        let units = core::slice::from_raw_parts(text, len as usize);
        let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
        Some(String::from_utf16_lossy(&units[..end]))
    }
}

/// The running exe's identity, read once from the loaded image's headers.
pub fn identity() -> &'static Identity {
    static ID: OnceLock<Identity> = OnceLock::new();
    ID.get_or_init(|| {
        let (mut timestamp, mut size_of_image) = (0, 0);
        // SAFETY: the process image is mapped; headers are read only after their signatures check out.
        unsafe {
            let exe = GetModuleHandleW(core::ptr::null()) as *const u8;
            let dos = &*(exe as *const IMAGE_DOS_HEADER);
            if dos.e_magic == IMAGE_DOS_SIGNATURE {
                let nt = &*(exe.add(dos.e_lfanew as usize) as *const IMAGE_NT_HEADERS64);
                if nt.Signature == IMAGE_NT_SIGNATURE && nt.OptionalHeader.Magic == 0x20b {
                    timestamp = nt.FileHeader.TimeDateStamp;
                    size_of_image = nt.OptionalHeader.SizeOfImage;
                }
            }
        }
        let mut path = vec![0u16; 32768];
        // SAFETY: owned buffer.
        let n = unsafe { GetModuleFileNameW(core::ptr::null_mut(), path.as_mut_ptr(), path.len() as u32) } as usize;
        path.truncate(n);
        path.push(0);
        let product = product_version(&path).unwrap_or_else(|| "?".into());
        let (supported, overridden) =
            decide(timestamp, size_of_image, env_is_one("CVR_ALLOW_UNKNOWN_BUILD"), env_is_one("CVR_PRETEND_UNKNOWN_BUILD"));
        Identity {
            timestamp,
            size_of_image,
            expected: format!("{} / {:08X}", build::VERSION, build::TIMESTAMP),
            running: format!("{product} / {timestamp:08X}"),
            product_version: product,
            overridden,
            supported,
        }
    })
}

/// The running exe is the build the addresses belong to (or CVR_ALLOW_UNKNOWN_BUILD=1 says to trust them anyway).
pub fn supported() -> bool {
    identity().supported
}

/// The environment override is what made `supported` true.
pub fn overridden() -> bool {
    identity().overridden
}

/// `rva` is one remap.py could not carry to this build.
pub fn unmapped(rva: usize) -> bool {
    rva != 0 && build::UNMAPPED.contains(&rva)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate() {
        let (ts, size) = (build::TIMESTAMP, build::SIZE_OF_IMAGE);
        assert_eq!(decide(ts, size, false, false), (true, false), "the right build");
        assert_eq!(decide(ts, size, false, true), (false, false), "pretend unknown turns VR off");
        assert_eq!(decide(ts ^ 1, size, false, false), (false, false), "another build");
        assert_eq!(decide(ts, size ^ 0x1000, false, false), (false, false), "same stamp, other image size");
        assert_eq!(decide(ts ^ 1, size, true, false), (true, true), "allow unknown, overridden");
        assert_eq!(decide(ts, size, true, false), (true, false), "allow on the right build changes nothing");
        assert_eq!(decide(ts, size, true, true), (true, true), "both: the override wins, and says so");
    }

    #[test]
    fn the_test_process_is_not_the_game() {
        // cargo's test exe is no Cyberpunk2077.exe: the gate must say no unless an override is set.
        if std::env::var_os("CVR_ALLOW_UNKNOWN_BUILD").is_none() {
            assert!(!supported());
        }
        assert!(identity().expected.starts_with(build::VERSION));
    }

    /// The generated table is what the headers say: tools/patchday edits the headers, a stale table fails here.
    #[test]
    fn addresses_follow_the_headers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let rvas = std::fs::read_to_string(root.join("include/Stereo/EngineRvas.hpp")).unwrap();
        let build_hpp = std::fs::read_to_string(root.join("include/Core/GameBuild.hpp")).unwrap();
        let table = cvr_tool::engine_addresses::parse(&rvas, &build_hpp).unwrap();
        let committed = std::fs::read_to_string(root.join("rust/crates/cvr-engine/src/addresses.rs")).unwrap();
        assert_eq!(
            committed.replace("\r\n", "\n"),
            cvr_tool::engine_addresses::render(&table),
            "rust/crates/cvr-engine/src/addresses.rs is stale: run cvr-tool engine-addresses"
        );
    }
}
