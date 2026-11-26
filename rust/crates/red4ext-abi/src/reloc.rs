//! Engine addresses. A function or variable of the game is named by an address hash (`hashes`) and resolved by the
//! RED4ext loader that runs in the process (`RED4ext_ResolveAddress`), as the SDK's `UniversalRelocBase` does -- or,
//! without RED4ext (the port's own loader, 2026-11-24), from the address library the game ships beside its executable
//! (bin\x64\cyberpunk2077_addresses.json, CDPR's linker map: a hash, its section and the offset in it), the file
//! RED4ext itself reads. Both give the same addresses; the C++ half resolves through `cvr_resolve_address` as well
//! (include/Core/PortAddressResolver.hpp), so the whole DLL asks one resolver.
//!
//! Unlike the SDK, a hash RED4ext does not know is not a reason to terminate the game: `resolve` returns `None` and
//! the caller turns the feature off (stage 2's rule: a failed address leaves no half-installed chain).
use core::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

type ResolveFn = unsafe extern "C" fn(u32) -> usize;

static RESOLVER: AtomicUsize = AtomicUsize::new(0);

fn resolver() -> Option<ResolveFn> {
    let cached = RESOLVER.load(Ordering::Acquire);
    if cached != 0 {
        // SAFETY: only ever stored from a successful GetProcAddress of that signature.
        return Some(unsafe { core::mem::transmute::<usize, ResolveFn>(cached) });
    }
    let module_name: Vec<u16> = "RED4ext.dll\0".encode_utf16().collect();
    // SAFETY: plain Win32 lookups; the module is RED4ext's own, which outlives every plugin.
    let address = unsafe {
        let module = GetModuleHandleW(module_name.as_ptr());
        if module.is_null() {
            return None;
        }
        GetProcAddress(module, c"RED4ext_ResolveAddress".as_ptr().cast())?
    };
    RESOLVER.store(address as usize, Ordering::Release);
    // SAFETY: as above.
    Some(unsafe { core::mem::transmute::<usize, ResolveFn>(address as usize) })
}

/// The address of `hash` in the running game, or `None` when neither RED4ext nor the game's address library knows it.
pub fn resolve(hash: u32) -> Option<usize> {
    if let Some(f) = resolver() {
        // SAFETY: RED4ext's resolver takes any u32 and returns 0 for an unknown one.
        let address = unsafe { f(hash) };
        if address != 0 {
            return Some(address);
        }
    }
    let table = library();
    let i = table.binary_search_by_key(&hash, |e| e.0).ok()?;
    Some(image_base() + table[i].1 as usize)
}

/// The C++ half's resolver (include/Core/PortAddressResolver.hpp): 0 for an unknown hash.
#[no_mangle]
pub extern "C" fn cvr_resolve_address(hash: u32) -> usize {
    resolve(hash).unwrap_or(0)
}

/// The SDK's resolver for the C++ half: the address, or -- as RED4ext's SDK does with a hash it cannot find -- a
/// message and the process ended (the engine function is called right after; there is no going on without it).
#[no_mangle]
pub extern "C" fn cvr_resolve_address_or_exit(hash: u32) -> usize {
    if let Some(a) = resolve(hash) {
        return a;
    }
    let text: Vec<u16> = format!(
        "CyberpunkVRPort could not find the engine address of hash {hash} in this game build.\nThe game will now close.\0"
    )
    .encode_utf16()
    .collect();
    let title: Vec<u16> = "CyberpunkVRPort\0".encode_utf16().collect();
    // SAFETY: NUL-terminated strings alive for the calls.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            core::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
        windows_sys::Win32::System::Threading::TerminateProcess(windows_sys::Win32::System::Threading::GetCurrentProcess(), 1);
    }
    0
}

/// Whether the game's address library was read (the port's own loader without RED4ext), and how many entries it has.
pub fn library_entries() -> usize {
    LIBRARY.get().map_or(0, Vec::len)
}

static LIBRARY: std::sync::OnceLock<Vec<(u32, u32)>> = std::sync::OnceLock::new();

/// bin\x64\cyberpunk2077_addresses.json read once: (hash, RVA) sorted by hash.
fn library() -> &'static [(u32, u32)] {
    LIBRARY.get_or_init(|| {
        let mut path = vec![0u16; 32768];
        // SAFETY: the buffer is ours, its size passed.
        let n = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(core::ptr::null_mut(), path.as_mut_ptr(), path.len() as u32)
        } as usize;
        if n == 0 || n >= path.len() {
            return Vec::new();
        }
        let exe = std::path::PathBuf::from(String::from_utf16_lossy(&path[..n]));
        let Some(file) = exe.parent().map(|d| d.join("cyberpunk2077_addresses.json")) else { return Vec::new() };
        let Ok(text) = std::fs::read(file) else { return Vec::new() };
        // SAFETY: the running executable's own headers.
        let sections = unsafe { section_addresses(image_base()) };
        parse_library(&text, &sections)
    })
}

/// The virtual address of each section of the image at `base`, in the header's order (the library's 1-based index).
///
/// # Safety
/// `base` is a loaded PE image.
pub unsafe fn section_addresses(base: usize) -> Vec<u32> {
    let read32 = |o: usize| unsafe { core::ptr::read_unaligned((base + o) as *const u32) };
    let read16 = |o: usize| unsafe { core::ptr::read_unaligned((base + o) as *const u16) };
    let pe = read32(0x3C) as usize;
    let count = read16(pe + 6) as usize;
    let optional = read16(pe + 20) as usize;
    let first = pe + 24 + optional;
    (0..count).map(|i| read32(first + i * 40 + 12)).collect()
}

/// The library's entries: `{"hash": "<decimal>", ..., "offset": "<section>:<hex offset>"}`, each to (hash, RVA) by
/// the sections' addresses; sorted by hash, the first of a repeated hash kept.
pub fn parse_library(text: &[u8], sections: &[u32]) -> Vec<(u32, u32)> {
    const HASH: &[u8] = b"{\"hash\": \"";
    const OFFSET: &[u8] = b"\"offset\": \"";
    let find = |from: usize, needle: &[u8]| -> Option<usize> {
        let first = needle[0];
        let mut i = from;
        while i + needle.len() <= text.len() {
            match text[i..].iter().position(|&b| b == first) {
                Some(p) => i += p,
                None => return None,
            }
            if i + needle.len() <= text.len() && &text[i..i + needle.len()] == needle {
                return Some(i);
            }
            i += 1;
        }
        None
    };
    let mut out = Vec::with_capacity(text.len() / 160);
    let mut at = 0;
    while let Some(h) = find(at, HASH) {
        let mut i = h + HASH.len();
        let mut hash: u64 = 0;
        while i < text.len() && text[i].is_ascii_digit() {
            hash = hash * 10 + u64::from(text[i] - b'0');
            i += 1;
        }
        let Some(o) = find(i, OFFSET) else { break };
        let mut j = o + OFFSET.len();
        let hex = |j: &mut usize| -> u64 {
            let mut v: u64 = 0;
            while *j < text.len() {
                let d = match text[*j] {
                    c @ b'0'..=b'9' => c - b'0',
                    c @ b'a'..=b'f' => c - b'a' + 10,
                    c @ b'A'..=b'F' => c - b'A' + 10,
                    _ => break,
                };
                v = v * 16 + u64::from(d);
                *j += 1;
            }
            v
        };
        let section = hex(&mut j) as usize;
        if j < text.len() && text[j] == b':' {
            j += 1;
            let offset = hex(&mut j);
            if hash <= u64::from(u32::MAX) && (1..=sections.len()).contains(&section) {
                let rva = u64::from(sections[section - 1]) + offset;
                if rva <= u64::from(u32::MAX) {
                    out.push((hash as u32, rva as u32));
                }
            }
        }
        at = j;
    }
    out.sort_by_key(|e| e.0);
    out.dedup_by_key(|e| e.0);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_library_format() {
        let text = br#"{"Linker map timestamp": "x","Addresses": [{"hash": "139922550","secondary hash": "22","symbol": "A::B","offset": "0001:0018971c"},{"hash": "5","secondary hash": "ab","symbol": "C","offset": "0003:00000010"},{"hash": "139922550","secondary hash": "2","symbol": "dup","offset": "0001:00000001"}]}"#;
        let t = super::parse_library(text, &[0x1000, 0x2000, 0x3000]);
        assert_eq!(t, vec![(5, 0x3010), (139922550, 0x1000 + 0x18971c)]);
    }
    /// The game's own library read whole (cargo test --release -- --ignored real_library --nocapture).
    #[test]
    #[ignore]
    fn real_library() {
        let path = r"C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\cyberpunk2077_addresses.json";
        let t0 = std::time::Instant::now();
        let text = std::fs::read(path).unwrap();
        let t1 = t0.elapsed();
        let t = super::parse_library(&text, &[0x1000, 0x2000000, 0x3000000, 0x4000000, 0x5000000, 0x6000000, 0x7000000, 0x8000000, 0x9000000, 0xA000000]);
        println!("read {:?}, parsed {:?}: {} entries", t1, t0.elapsed() - t1, t.len());
        let get = |h: u32| t.binary_search_by_key(&h, |e| e.0).ok().map(|i| t[i].1);
        println!("CRTTISystem_Get {:x?} WinMain {:x?} AddState {:x?} QuickExit {:x?}", get(crate::hashes::CRTTISystem_Get), get(240386859), get(4223801011), get(388826167));
        assert!(t.len() > 1000);
    }
}

/// The game executable's image base.
pub fn image_base() -> usize {
    // SAFETY: the process image is always loaded.
    unsafe { GetModuleHandleW(core::ptr::null()) as usize }
}

/// A lazily resolved engine function or variable, cached for the process lifetime (the SDK's `static
/// UniversalRelocFunc` pattern).
pub struct Reloc {
    hash: u32,
    address: AtomicUsize,
}

impl Reloc {
    pub const fn new(hash: u32) -> Self {
        Reloc { hash, address: AtomicUsize::new(0) }
    }

    pub fn get(&self) -> Option<usize> {
        let a = self.address.load(Ordering::Relaxed);
        if a != 0 {
            return Some(a);
        }
        let a = resolve(self.hash)?;
        self.address.store(a, Ordering::Relaxed);
        Some(a)
    }

    /// The resolved address as a function pointer of type `F`.
    ///
    /// # Safety
    /// `F` must be the `extern "C"` function pointer type of the engine function behind the hash.
    pub unsafe fn func<F: Copy>(&self) -> Option<F> {
        const { assert!(core::mem::size_of::<F>() == core::mem::size_of::<usize>()) };
        // SAFETY: size checked above; the caller vouches for the signature.
        self.get().map(|a| unsafe { core::mem::transmute_copy::<usize, F>(&a) })
    }
}
