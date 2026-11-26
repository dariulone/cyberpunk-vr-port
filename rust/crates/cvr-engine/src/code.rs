//! Engine memory and code: guarded reads and writes (the guarded accesses of seh.rs, formerly native/seh.c and
//! include/Utils/MemorySafe.hpp), the expected-instruction check made before any patch, and the module scan
//! (include/Utils/AobScanner.hpp).
use core::ffi::c_void;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetModuleHandleW};
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_GUARD,
    PAGE_READONLY, PAGE_READWRITE,
};

extern "C" {
    fn cvr_seh_copy_from(dst: *mut c_void, src: *const c_void, n: usize) -> i32;
    fn cvr_seh_copy_to(dst: *mut c_void, src: *const c_void, n: usize) -> i32;
    fn cvr_seh_call3(f: usize, a: *mut c_void, b: *mut c_void, c: *mut c_void, result: *mut u8) -> i32;
    fn cvr_seh_call4p(f: usize, a: *mut c_void, b: *mut c_void, c: *mut c_void, d: *mut c_void, result: *mut *mut c_void) -> i32;
    fn cvr_seh_call4x(f: usize, a: u64, b: u64, c: u64, d: u64, rax: *mut u64, xmm0: *mut u64) -> i32;
    fn cvr_seh_call6p(f: usize, a: *mut c_void, b: *mut c_void, c: *mut c_void, d: *mut c_void, e: *mut c_void, g: *mut c_void, result: *mut *mut c_void) -> i32;
    fn cvr_seh_call10x(f: usize, args: *const u64, rax: *mut u64, xmm0: *mut u64) -> i32;
    fn cvr_seh_cmpxchg32(target: *mut i32, exchange: i32, comparand: i32, found: *mut i32) -> i32;
    fn cvr_seh_add_ref(object: *mut c_void) -> i32;
}

/// `f(a, b, c, d)` at engine address `f` (up to four pointer-sized arguments; a void function's result is RAX as it
/// was), called from inside the SEH adapter: its pointer result, or `None` when the call faulted.
///
/// # Safety
/// `f` must be an engine function taking up to four pointer-sized (or smaller integer) arguments, and the arguments
/// what it expects.
pub unsafe fn guarded_call4(f: usize, a: *mut c_void, b: *mut c_void, c: *mut c_void, d: *mut c_void) -> Option<*mut c_void> {
    let mut result = core::ptr::null_mut();
    if f == 0 {
        return None;
    }
    // SAFETY: the caller vouches for the function and its arguments; the adapter catches a fault.
    (unsafe { cvr_seh_call4p(f, a, b, c, d, &mut result) } != 0).then_some(result)
}

/// `f(a, b, c, d)` at address `f` with each argument in both its integer and its float register (a float argument's
/// bits in the low 32), called from inside a guarded range (seh.rs): (rax, xmm0's low 64 bits), or `None` when the call
/// faulted -- PhysX's methods that take or give a float (setMass, getMass).
///
/// # Safety
/// `f` must be a function of four or fewer arguments, each integer-sized or a float, and the arguments what it expects.
pub unsafe fn guarded_call4x(f: usize, args: [u64; 4]) -> Option<(u64, u64)> {
    if f == 0 {
        return None;
    }
    let (mut rax, mut xmm0) = (0u64, 0u64);
    // SAFETY: the caller vouches for the function and its arguments; the adapter catches a fault.
    (unsafe { cvr_seh_call4x(f, args[0], args[1], args[2], args[3], &mut rax, &mut xmm0) } != 0).then_some((rax, xmm0))
}

/// `f(args[0..10])` at address `f`, the first four arguments in both their integer and their float registers and the
/// other six on the stack (a float argument's bits in the low 32), called from inside a guarded range (seh.rs): (rax,
/// xmm0's low 64 bits), or `None` when the call faulted -- PhysX's PxGeometryQuery::sweep (nine arguments, two of them
/// floats). Unused trailing arguments are passed as 0.
///
/// # Safety
/// `f` must be a function of ten or fewer arguments, each integer-sized or a float, and the arguments what it expects.
pub unsafe fn guarded_call10x(f: usize, args: [u64; 10]) -> Option<(u64, u64)> {
    if f == 0 {
        return None;
    }
    let (mut rax, mut xmm0) = (0u64, 0u64);
    // SAFETY: the caller vouches for the function and its arguments; the adapter catches a fault.
    (unsafe { cvr_seh_call10x(f, args.as_ptr(), &mut rax, &mut xmm0) } != 0).then_some((rax, xmm0))
}

/// `void *f(a, b, c, d, e, f)` at address `f`, called from inside a guarded range (seh.rs): its result (rax), or
/// `None` when the call faulted -- for a function of six pointer-sized arguments (PhysX's
/// PxGeometryQuery::computePenetration takes all six by address).
///
/// # Safety
/// `f` must be a function taking six pointer-sized arguments (or fewer: the rest are ignored), and the arguments what
/// it expects.
pub unsafe fn guarded_call6(f: usize, args: [*mut c_void; 6]) -> Option<*mut c_void> {
    let mut result = core::ptr::null_mut();
    if f == 0 {
        return None;
    }
    // SAFETY: the caller vouches for the function and its arguments; the adapter catches a fault.
    (unsafe { cvr_seh_call6p(f, args[0], args[1], args[2], args[3], args[4], args[5], &mut result) } != 0).then_some(result)
}

/// `bool f(a, b, c)` at engine address `f`, called from inside a guarded range (seh.rs): its result, or `None`
/// when the call faulted. Nothing but the engine's frames is unwound on a fault.
///
/// # Safety
/// `f` must be an engine function taking three pointer-sized arguments (or fewer: the rest are ignored) and returning
/// bool, and the arguments what it expects.
pub unsafe fn guarded_call3(f: usize, a: *mut c_void, b: *mut c_void, c: *mut c_void) -> Option<bool> {
    let mut result = 0u8;
    if f == 0 {
        return None;
    }
    // SAFETY: the caller vouches for the function and its arguments; the adapter catches a fault.
    (unsafe { cvr_seh_call3(f, a, b, c, &mut result) } != 0).then_some(result != 0)
}

/// A guarded 32-bit compare-exchange at `address` (exchange when it holds `comparand`): the value found, or None when
/// the access faulted.
///
/// # Safety
/// `address` is meant to be a live, 4-byte-aligned engine word; a fault is caught, not a wrong but mapped word.
pub unsafe fn guarded_cmpxchg32(address: usize, exchange: u32, comparand: u32) -> Option<u32> {
    let mut found = 0i32;
    // SAFETY: the access runs inside seh.rs's guarded range.
    (unsafe { cvr_seh_cmpxchg32(address as *mut i32, exchange as i32, comparand as i32, &mut found) } != 0).then_some(found as u32)
}

/// A guarded AddRef of the COM-shaped engine object at `object` (the C++ `__try { object->AddRef(); }` of the
/// observers that pin a resource or allocator found in an engine record): true when the call ran -- the caller then
/// owns one reference -- false when reading its vtable or the call faulted.
///
/// # Safety
/// `object` is meant to be a live COM object; a fault is caught, not a wrong but mapped object.
pub unsafe fn guarded_add_ref(object: usize) -> bool {
    // SAFETY: the access and the call run inside seh.rs's guarded range.
    object != 0 && unsafe { cvr_seh_add_ref(object as *mut c_void) } != 0
}

/// `len` bytes from `address` lie in one committed region with read access (VirtualQuery; the C++ `Readable` checks
/// of the natives that walk engine objects of an unverified layout).
pub fn readable_range(address: usize, len: usize) -> bool {
    if address == 0 {
        return false;
    }
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    // SAFETY: VirtualQuery only describes memory.
    if unsafe { VirtualQuery(address as *const c_void, &mut mbi, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0
        || mbi.State != MEM_COMMIT
        || mbi.Protect & (PAGE_READWRITE | PAGE_READONLY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_READ) == 0
    {
        return false;
    }
    address + len <= mbi.BaseAddress as usize + mbi.RegionSize
}

/// The game executable's image base.
pub fn image_base() -> usize {
    // SAFETY: the process image is always loaded.
    unsafe { GetModuleHandleW(core::ptr::null()) as usize }
}

/// An RVA of Cyberpunk2077.exe as an address in this process.
pub fn rva(rva: usize) -> usize {
    image_base() + rva
}

/// Copies `out.len()` bytes from `address`; false (and `out` unspecified) when any byte faults.
pub fn read_bytes(address: usize, out: &mut [u8]) -> bool {
    if address == 0 {
        return false;
    }
    // SAFETY: the adapter catches the access violation a bad address raises.
    unsafe { cvr_seh_copy_from(out.as_mut_ptr().cast(), address as *const c_void, out.len()) != 0 }
}

/// A `T` read from engine memory that may not be there (`ReadFloatSafe`, `ReadPtrSafe`, ...).
pub fn read<T: Copy>(address: usize) -> Option<T> {
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    if address == 0 {
        return None;
    }
    // SAFETY: as read_bytes; T is plain data (Copy) and fully written on success.
    let ok = unsafe { cvr_seh_copy_from(value.as_mut_ptr().cast(), address as *const c_void, size_of::<T>()) != 0 };
    ok.then(|| unsafe { value.assume_init() })
}

/// A `T` read from memory of no known layout -- words taken for pointers while scanning an object -- through
/// ReadProcessMemory on this process: an unmapped address reads as `None` without an access violation. A guarded
/// `read` would catch the fault, but the engine's vectored handler sees it first and reports a crash (the game stops).
pub fn peek<T: Copy>(address: usize) -> Option<T> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    if address < 0x10000 || address > 0x7FFF_FFFF_FFFF - size_of::<T>() {
        return None;
    }
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    let mut copied = 0usize;
    // SAFETY: the kernel copies into the local value and reports an unreadable source as a failure.
    let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), address as *const c_void, value.as_mut_ptr().cast(), size_of::<T>(), &mut copied) } != 0
        && copied == size_of::<T>();
    // SAFETY: T is plain data (Copy), fully written when the copy succeeded.
    ok.then(|| unsafe { value.assume_init() })
}

/// Writes `value` to engine memory that may not be there (`WriteFloatSafe`); false when it faults.
pub fn write<T: Copy>(address: usize, value: T) -> bool {
    if address == 0 {
        return false;
    }
    // SAFETY: as read_bytes, the other way.
    unsafe { cvr_seh_copy_to(address as *mut c_void, (&value as *const T).cast(), size_of::<T>()) != 0 }
}

/// Copies `bytes` to engine memory that may not be there; false when any byte faults (some may have been written).
pub fn write_bytes(address: usize, bytes: &[u8]) -> bool {
    if address == 0 {
        return false;
    }
    // SAFETY: as read_bytes, the other way.
    unsafe { cvr_seh_copy_to(address as *mut c_void, bytes.as_ptr().cast(), bytes.len()) != 0 }
}

/// A byte pattern: "48 8B 05 ?? ?? ?? ?? C3" (`??` or `?` matches anything).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern(pub Vec<Option<u8>>);

impl Pattern {
    pub fn parse(text: &str) -> Option<Pattern> {
        let bytes: Option<Vec<Option<u8>>> = text
            .split_whitespace()
            .map(|t| if t.chars().all(|c| c == '?') { Some(None) } else { u8::from_str_radix(t, 16).ok().map(Some) })
            .collect();
        bytes.filter(|b| !b.is_empty()).map(Pattern)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn matches(&self, bytes: &[u8]) -> bool {
        bytes.len() >= self.0.len() && self.0.iter().zip(bytes).all(|(p, b)| p.is_none_or(|p| p == *b))
    }
}

/// The code at `address` is what the port was written against: the check every patch site makes first, so a build
/// that moved an instruction gets a refusal instead of a corrupted function.
pub fn expect(address: usize, pattern: &Pattern) -> bool {
    let mut buf = vec![0u8; pattern.len()];
    read_bytes(address, &mut buf) && pattern.matches(&buf)
}

fn readable(protect: u32) -> bool {
    protect & PAGE_GUARD == 0
        && protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE) != 0
}

/// The first match of `pattern` in [start, end), committed readable pages only.
pub fn scan(start: usize, end: usize, pattern: &Pattern) -> Option<usize> {
    let mut current = start;
    while current < end {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
        // SAFETY: VirtualQuery only describes memory.
        if unsafe { VirtualQuery(current as *const c_void, &mut mbi, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            break;
        }
        let region_end = (mbi.BaseAddress as usize + mbi.RegionSize).min(end);
        if mbi.State == MEM_COMMIT && readable(mbi.Protect) && region_end > current {
            // SAFETY: committed and readable for its whole extent at query time.
            let bytes = unsafe { core::slice::from_raw_parts(current as *const u8, region_end - current) };
            if let Some(i) = bytes.windows(pattern.len()).position(|w| pattern.matches(w)) {
                return Some(current + i);
            }
            // A match could straddle into the next region: back up by pattern length - 1.
            let next = mbi.BaseAddress as usize + mbi.RegionSize;
            current = if next >= end { end } else { next.saturating_sub(pattern.len() - 1).max(current + 1) };
            continue;
        }
        current = mbi.BaseAddress as usize + mbi.RegionSize;
    }
    None
}

/// `FindPattern` over a loaded module's image (null name: the game executable).
pub fn find_in_module(module: Option<&str>, pattern: &Pattern) -> Option<usize> {
    let name = module.map(cvr_host::wide::z);
    // SAFETY: plain module lookup.
    let base = unsafe { GetModuleHandleW(name.as_ref().map_or(core::ptr::null(), |n| n.as_ptr())) } as usize;
    if base == 0 {
        return None;
    }
    let dos_lfanew: u32 = read(base + 0x3c)?;
    let size_of_image: u32 = read(base + dos_lfanew as usize + 24 + 56)?;
    scan(base, base + size_of_image as usize, pattern)
}

/// `FindPattern` as include/Utils/AobScanner.hpp has it, for the hooks ported from it: the first address in the
/// named module (ANSI name, as GetModuleHandleA takes it) where every byte with mask 'x' equals the pattern's;
/// candidates in [base, base + SizeOfImage - len), region by region, committed and readable regions only, a
/// candidate's comparison free to run on into the next region as the C++ one does.
pub fn find_pattern(module: &str, pattern: &[u8], mask: &[u8]) -> Option<usize> {
    let mut name = module.as_bytes().to_vec();
    name.push(0);
    // SAFETY: plain module lookup by a NUL-terminated name.
    let base = unsafe { GetModuleHandleA(name.as_ptr()) } as usize;
    if base == 0 || pattern.len() < mask.len() {
        return None;
    }
    let dos_lfanew: u32 = read(base + 0x3c)?;
    let size_of_image: u32 = read(base + dos_lfanew as usize + 24 + 56)?;
    let len = mask.len();
    let mut current = base;
    let end = base + size_of_image as usize - len;
    while current < end {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
        // SAFETY: VirtualQuery only describes memory.
        if unsafe { VirtualQuery(current as *const c_void, &mut mbi, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            break;
        }
        let region = mbi.BaseAddress as usize + mbi.RegionSize;
        if mbi.State == MEM_COMMIT && mbi.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE) != 0 {
            let region_end = region.min(end);
            for p in current..region_end {
                // SAFETY: inside the module image, as the C++ scan reads it.
                let found = (0..len).all(|j| mask[j] != b'x' || unsafe { *((p + j) as *const u8) } == pattern[j]);
                if found {
                    return Some(p);
                }
            }
        }
        current = region;
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_reads_survive_bad_addresses() {
        assert_eq!(read::<u64>(0), None);
        assert_eq!(read::<u64>(8), None, "null page");
        assert_eq!(read::<u32>(0x7FFF_FFFF_0000), None, "unmapped");
        assert!(!write(16usize, 1.0f32));
        let value = 0x1234_5678u32;
        assert_eq!(read::<u32>(&value as *const u32 as usize), Some(0x1234_5678));
        let mut target = 0.0f32;
        assert!(write(&mut target as *mut f32 as usize, 2.5f32));
        assert_eq!(target, 2.5);
    }

    #[test]
    fn patterns() {
        let p = Pattern::parse("48 8B ?? 05 ?").unwrap();
        assert_eq!(p.0, vec![Some(0x48), Some(0x8B), None, Some(0x05), None]);
        assert!(p.matches(&[0x48, 0x8B, 0xFF, 0x05, 0x00, 0x99]));
        assert!(!p.matches(&[0x48, 0x8C, 0xFF, 0x05, 0x00]));
        assert!(!p.matches(&[0x48, 0x8B]));
        assert!(Pattern::parse("zz").is_none() && Pattern::parse("").is_none());
        let haystack = [0u8, 1, 2, 0x48, 0x8B, 7, 0x05, 9, 10];
        let base = haystack.as_ptr() as usize;
        assert_eq!(scan(base, base + haystack.len(), &p), Some(base + 3));
        assert!(expect(base + 3, &p) && !expect(base + 4, &p));
    }

    #[test]
    fn masked_scan_as_the_cpp_one() {
        // The PE signature, the second byte a wildcard, in the test executable itself.
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_str().unwrap().to_string();
        let at = find_pattern(&name, b"P?\0\0", b"x?xx").expect("PE signature");
        assert_eq!(read::<u32>(at), Some(0x0000_4550));
        assert!(find_pattern("no-such-module.dll", b"P", b"x").is_none());
    }

    #[test]
    fn finds_in_the_test_executable() {
        // The PE signature is in every image.
        let p = Pattern::parse("50 45 00 00").unwrap();
        assert!(find_in_module(None, &p).is_some());
    }
}
