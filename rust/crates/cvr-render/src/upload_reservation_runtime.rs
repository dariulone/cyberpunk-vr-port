//! The upload reservations' runtime (src/Render/NativeUploadReservation.cpp; its rules are native::upload_reservation):
//! the engine upload-ring allocator's code guard (its whole 312-byte body by FNV-1a), the seven-byte CALL patch at
//! the successful-CAS branch through a relay allocated within rel32 of it (`jmp [rip]` to the assembly thunk,
//! src/Hooks/NativeUploadReservationThunk.asm, which saves the full enabled XSAVE state -- or FXSAVE without OSXSAVE --
//! as chosen here), its removal, the externally installed variant (a plan published for the detached debugger, its
//! receipt accepted after exact current-code checks), and the ledger of witnessed reservations behind a mutex the
//! thunk's callback only tries (contention marks the ledger lost; a lost ledger is quarantined at the next use).
//!
//! The header's functions forward to the `cvr_ur_rt_*` functions below; the thunk calls
//! `CvrNativeUploadReservationObserve` and reads the XSAVE mask and mode defined here. The relay's allocation and the
//! processor's XSAVE choice are shared with instance_witness_runtime. The unit tests below check the relay's code, the
//! site's patch, the relay's candidates, the XSAVE choice and a relay's allocation.
use crate::native::upload_reservation::{valid_xstate, RETRY_DISPLACEMENT};

/// The relay's code: `jmp [rip+0]` and the thunk's address.
pub fn relay_code(thunk: u64) -> [u8; 14] {
    let mut code = [0u8; 14];
    code[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    code[6..].copy_from_slice(&thunk.to_le_bytes());
    code
}
/// The witness site's patch: a CALL to the relay, then the allocator's retry JNE; None when the relay lies beyond a
/// rel32 of the CALL's return address.
pub fn site_patch(relay: u64, site: u64) -> Option<[u8; 7]> {
    let displacement = (relay as i64).wrapping_sub(site.wrapping_add(5) as i64);
    let relative = i32::try_from(displacement).ok()?;
    let mut patch = [0xE8, 0, 0, 0, 0, 0x75, RETRY_DISPLACEMENT];
    patch[1..5].copy_from_slice(&relative.to_le_bytes());
    Some(patch)
}
/// The relay's candidate addresses around `site`: one allocation granule (`step`) apart, below then above, out to
/// 0x7FFF0000 bytes; never 0 nor within a granule of the address space's end.
pub fn relay_candidates(site: usize, step: usize) -> impl Iterator<Item = usize> {
    let aligned = site & !(step - 1);
    (1usize..)
        .map(move |k| k * step)
        .take_while(|&distance| distance <= 0x7FFF_0000)
        .flat_map(move |distance| {
            [
                if aligned >= distance { aligned - distance } else { 0 },
                if aligned <= usize::MAX - distance { aligned + distance } else { 0 },
            ]
        })
        .filter(move |&candidate| candidate != 0 && candidate <= usize::MAX - step)
}
/// The thunk's state save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Xstate {
    /// OSXSAVE absent: FXSAVE.
    Legacy,
    /// XSAVE of the enabled components into `bytes`.
    Full { mask: u64, bytes: u32 },
    /// Neither usable.
    Unsupported,
}
/// The choice from CPUID leaf 1's ECX and the highest leaf; `full` reads XCR0 and leaf 0xD's supported mask and size.
pub fn xstate_choice(ecx1: u32, maximum: i32, full: impl FnOnce() -> (u64, u64, u32)) -> Xstate {
    if ecx1 & (1 << 27) == 0 {
        return Xstate::Legacy;
    }
    if ecx1 & (1 << 26) == 0 || maximum < 0xD {
        return Xstate::Unsupported;
    }
    let (enabled, supported, bytes) = full();
    if !valid_xstate(enabled, supported, u64::from(bytes)) {
        return Xstate::Unsupported;
    }
    Xstate::Full { mask: enabled, bytes }
}
/// The processor's choice (CPUID leaves 0, 1 and 0xD, XCR0).
#[allow(unused_unsafe)]
pub fn cpu_xstate() -> Xstate {
    use core::arch::x86_64::{__cpuid, __cpuid_count};
    #[target_feature(enable = "xsave")]
    unsafe fn enabled() -> u64 {
        // SAFETY: only reached with OSXSAVE set.
        unsafe { core::arch::x86_64::_xgetbv(0) }
    }
    // SAFETY: CPUID is always available on x86-64.
    let maximum = unsafe { __cpuid(0) }.eax as i32;
    let ecx1 = unsafe { __cpuid(1) }.ecx;
    xstate_choice(ecx1, maximum, || {
        // SAFETY: OSXSAVE is set (xstate_choice asks only then).
        let mask = unsafe { enabled() };
        let leaf = unsafe { __cpuid_count(0xD, 0) };
        (mask, u64::from(leaf.eax) | u64::from(leaf.edx) << 32, leaf.ebx)
    })
}
/// A read-write relay page in the first free allocation granule among `relay_candidates`; 0 when none is (or the
/// granularity is 0, or with `power_of_two` not a power of two).
pub fn allocate_relay(site: usize, power_of_two: bool) -> usize {
    use core::ffi::c_void;
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_FREE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
    // SAFETY: plain integers and pointers; an out-parameter.
    let mut info: SYSTEM_INFO = unsafe { core::mem::zeroed() };
    unsafe { GetSystemInfo(&mut info) };
    let step = info.dwAllocationGranularity as usize;
    if step == 0 || (power_of_two && step & (step - 1) != 0) {
        return 0;
    }
    for candidate in relay_candidates(site, step) {
        // SAFETY: as above.
        let mut free: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
        let size = core::mem::size_of::<MEMORY_BASIC_INFORMATION>();
        if unsafe { VirtualQuery(candidate as *const c_void, &mut free, size) } == 0
            || free.State != MEM_FREE
            || free.RegionSize < step
        {
            continue;
        }
        // SAFETY: a free region's address; the allocation is the relay's own.
        let result =
            unsafe { VirtualAlloc(candidate as *const c_void, step, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) };
        if !result.is_null() {
            return result as usize;
        }
    }
    0
}

#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
#[allow(non_upper_case_globals)]
mod abi {
    use super::*;
    use crate::declared_moved_exports::{
        CyberpunkVR_UploadReservationAcceptRequest as ACCEPT_REQUEST, CyberpunkVR_UploadReservationCount as COUNT,
        CyberpunkVR_UploadReservationDropped as DROPPED, CyberpunkVR_UploadReservationEnable as ENABLE,
        CyberpunkVR_UploadReservationExternalInstallState as EXTERNAL_STATE,
        CyberpunkVR_UploadReservationExternalPlanSeq as PLAN_SEQ,
        CyberpunkVR_UploadReservationExternalReceiptSeq as RECEIPT_SEQ,
        CyberpunkVR_UploadReservationPrepareRequest as PREPARE_REQUEST,
    };
    use crate::native::upload_reservation::{
        fnv, matches_external_patched_body, valid_external_plan, valid_external_receipt, ExternalInstallPlan,
        ExternalInstallReceipt, ExternalInstallState, Ledger, Range, Reservation, Row, Status, ThunkFrame, Witness,
        ALLOCATOR_BYTES, ALLOCATOR_FNV, ALLOCATOR_RVA, EXTERNAL_PLAN_VERSION, MAX_RINGS, MAX_ROWS,
        ORIGINAL_WITNESS_BYTES, RETRY_RVA, WITNESS_RVA,
    };
    use core::ffi::c_void;
    use core::mem::{size_of, zeroed};
    use core::ptr::{null, null_mut};
    use core::sync::atomic::{fence, AtomicBool, AtomicU32, Ordering::*};
    use std::sync::{LazyLock, Mutex, MutexGuard, TryLockError};
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Diagnostics::Debug::{FlushInstructionCache, RtlLookupFunctionEntry};
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GetModuleHandleW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
    };
    use windows_sys::Win32::System::Memory::{
        VirtualFree, VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_PRIVATE,
        MEM_RELEASE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, GetProcessTimes,
    };

    const ROW0: Row = Row {
        sequence: 0,
        observer_epoch: 0,
        arena_generation: 0,
        ring: 0,
        old_head: 0,
        absolute_begin: 0,
        absolute_end: 0,
        bytes: 0,
        capacity: 0,
        physical_offset: 0,
        thread: 0,
        native_caller: 0,
        native_stack: 0,
        flags: 0,
        status: 0,
        witness_rva: WITNESS_RVA,
    };
    #[no_mangle]
    pub static CyberpunkVR_UploadReservationState: AtomicU32 = AtomicU32::new(Status::Disabled as u32);
    #[no_mangle]
    pub static mut CyberpunkVR_UploadReservationRows: [Row; MAX_ROWS] = [ROW0; MAX_ROWS];
    #[no_mangle]
    // SAFETY: plain integers and byte arrays.
    pub static mut CyberpunkVR_UploadReservationExternalPlan: ExternalInstallPlan = unsafe { zeroed() };
    #[no_mangle]
    // SAFETY: plain integers.
    pub static mut CyberpunkVR_UploadReservationExternalReceipt: ExternalInstallReceipt = unsafe { zeroed() };
    /// The thunk's XSAVE mask and mode (and the area's size), chosen at installation.
    #[no_mangle]
    pub static mut CvrNativeUploadReservationXstateMask: u64 = 0;
    #[no_mangle]
    pub static mut CvrNativeUploadReservationXstateBytes: u32 = 512;
    #[no_mangle]
    pub static mut CvrNativeUploadReservationXstateMode: u32 = 0;
    extern "C" {
        /// src/Hooks/NativeUploadReservationThunk.asm.
        fn CvrNativeUploadReservationThunk();
    }

    struct Code {
        guarded_base: usize,
        guard_attempted: bool,
        guard_valid: bool,
        installed: bool,
        healthy: bool,
        relay: usize,
        site_protection: u32,
        patch: [u8; 7],
        prepared_plan: ExternalInstallPlan,
        external_relay: usize,
        external_module: usize,
        external_nonce: u64,
        external_plan_sequence: u32,
        external_prepared: bool,
        external_accepted: bool,
    }
    static CODE: Mutex<Code> = Mutex::new(Code {
        guarded_base: 0,
        guard_attempted: false,
        guard_valid: false,
        installed: false,
        healthy: false,
        relay: 0,
        site_protection: 0,
        patch: [0; 7],
        // SAFETY: plain integers and byte arrays.
        prepared_plan: unsafe { zeroed() },
        external_relay: 0,
        external_module: 0,
        external_nonce: 0,
        external_plan_sequence: 0,
        external_prepared: false,
        external_accepted: false,
    });
    struct Books {
        ledger: Ledger,
        epoch: u64,
    }
    static BOOKS: LazyLock<Mutex<Books>> =
        LazyLock::new(|| Mutex::new(Books { ledger: Ledger::new(MAX_ROWS, MAX_RINGS, 1), epoch: 1 }));
    static LOST: AtomicBool = AtomicBool::new(false);

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn base() -> usize {
        // SAFETY: the process image's handle.
        unsafe { GetModuleHandleW(null()) as usize }
    }
    fn state(value: Status) {
        CyberpunkVR_UploadReservationState.store(value as u32, Relaxed);
    }
    fn external_state(value: ExternalInstallState) {
        EXTERNAL_STATE.store(value as u32, Release);
    }
    fn read(address: usize, output: &mut [u8]) -> bool {
        address != 0
            && !output.is_empty()
            && address <= usize::MAX - output.len()
            && crate::instance_layout::read(address as u64, output)
    }
    fn query(address: u64, output: &mut MEMORY_BASIC_INFORMATION) -> bool {
        let size = size_of::<MEMORY_BASIC_INFORMATION>();
        // SAFETY: an out-parameter of its size.
        unsafe { VirtualQuery(address as usize as *const c_void, output, size) == size }
    }
    fn region() -> MEMORY_BASIC_INFORMATION {
        // SAFETY: plain integers and pointers.
        unsafe { zeroed() }
    }
    fn thunk_address() -> u64 {
        CvrNativeUploadReservationThunk as unsafe extern "C" fn() as usize as u64
    }
    fn whole_body(base: usize) -> bool {
        let mut bytes = [0u8; ALLOCATOR_BYTES];
        base != 0
            && base == self::base()
            && base <= usize::MAX - ALLOCATOR_RVA as usize
            && read(base + ALLOCATOR_RVA as usize, &mut bytes)
            && fnv(&bytes) == ALLOCATOR_FNV
    }
    fn configure_xstate() -> bool {
        // SAFETY: the thunk reads these only while its patch is installed, under the code mutex's installation.
        unsafe {
            CvrNativeUploadReservationXstateMask = 0;
            CvrNativeUploadReservationXstateBytes = 512;
            CvrNativeUploadReservationXstateMode = 0;
        }
        match cpu_xstate() {
            Xstate::Legacy => true,
            Xstate::Unsupported => false,
            Xstate::Full { mask, bytes } => {
                // SAFETY: as above.
                unsafe {
                    CvrNativeUploadReservationXstateMask = mask;
                    CvrNativeUploadReservationXstateBytes = bytes;
                    CvrNativeUploadReservationXstateMode = 1;
                }
                true
            }
        }
    }
    fn release(relay: usize) {
        // SAFETY: the relay's own allocation.
        unsafe { VirtualFree(relay as *mut c_void, 0, MEM_RELEASE) };
    }
    fn replace(site: usize, expected: &[u8; 7], replacement: &[u8; 7], written: &mut bool, previous: &mut u32) -> bool {
        *written = false;
        let mut current = [0u8; 7];
        if !read(site, &mut current) || current != *expected {
            return false;
        }
        let mut protection = 0u32;
        // SAFETY: the guarded image's seven bytes, read above.
        if unsafe { VirtualProtect(site as *const c_void, 7, PAGE_EXECUTE_READWRITE, &mut protection) } == 0 {
            return false;
        }
        *previous = protection;
        // SAFETY: writable now.
        unsafe { core::ptr::copy_nonoverlapping(replacement.as_ptr(), site as *mut u8, 7) };
        *written = true;
        // SAFETY: the same seven bytes.
        let flushed = unsafe { FlushInstructionCache(GetCurrentProcess(), site as *const c_void, 7) } != 0;
        let mut ignored = 0u32;
        let restored = unsafe { VirtualProtect(site as *const c_void, 7, protection, &mut ignored) } != 0;
        flushed && restored
    }
    fn process_identity(creation: &mut u64) -> bool {
        // SAFETY: plain integers; out-parameters.
        let (mut created, mut exited, mut kernel, mut user): (FILETIME, FILETIME, FILETIME, FILETIME) =
            unsafe { (zeroed(), zeroed(), zeroed(), zeroed()) };
        *creation = 0;
        if unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
            return false;
        }
        *creation = u64::from(created.dwLowDateTime) | u64::from(created.dwHighDateTime) << 32;
        *creation != 0
    }
    fn thunk_extent(module: usize, bytes: &mut u64) -> bool {
        *bytes = 0;
        let address = thunk_address();
        let mut image = 0u64;
        // SAFETY: an out-parameter; no history table.
        let function = unsafe { RtlLookupFunctionEntry(address, &mut image, null_mut()) };
        // SAFETY: the image's own unwind table entry when not null.
        let Some(function) = (unsafe { function.as_ref() }) else { return false };
        let (begin, end) = (function.BeginAddress, function.EndAddress);
        if image != module as u64
            || image > u64::MAX - u64::from(end)
            || image + u64::from(begin) != address
            || end <= begin
            || end - begin > 4096
        {
            return false;
        }
        *bytes = u64::from(end - begin);
        true
    }
    fn executable(protection: u32) -> bool {
        protection == PAGE_EXECUTE_READ || protection == PAGE_EXECUTE_READWRITE || protection == PAGE_EXECUTE_WRITECOPY
    }
    fn current_external_code(code: &Code, plan: &ExternalInstallPlan) -> bool {
        let mut body = [0u8; ALLOCATOR_BYTES];
        let mut relay = [0u8; 14];
        let (mut site, mut storage, mut thunk) = (region(), region(), region());
        let (mut creation, mut thunk_bytes) = (0u64, 0u64);
        valid_external_plan(plan)
            // SAFETY: plain Win32 calls.
            && plan.process_id == u64::from(unsafe { GetCurrentProcessId() })
            && process_identity(&mut creation)
            && creation == plan.process_creation_time
            && base() as u64 == plan.game_base
            && code.external_module as u64 == plan.module_pin_address
            && thunk_extent(code.external_module, &mut thunk_bytes)
            && thunk_bytes == plan.thunk_bytes
            && read(plan.allocator_address as usize, &mut body)
            && matches_external_patched_body(plan, &body)
            && read(plan.relay_address as usize, &mut relay)
            && relay == plan.relay_code
            && query(plan.site_address, &mut site)
            && site.State == MEM_COMMIT
            && site.Type == MEM_IMAGE
            && site.AllocationBase as u64 == plan.game_base
            && u64::from(site.Protect) == plan.original_protection
            && query(plan.relay_address, &mut storage)
            && storage.State == MEM_COMMIT
            && storage.Type == MEM_PRIVATE
            && storage.AllocationBase as u64 == plan.relay_address
            && storage.Protect == PAGE_EXECUTE_READ
            && query(plan.thunk_address, &mut thunk)
            && thunk.State == MEM_COMMIT
            && thunk.Type == MEM_IMAGE
            && thunk.AllocationBase as u64 == plan.module_pin_address
            && executable(thunk.Protect)
    }
    fn prepare_external(code: &mut Code, base: usize) -> bool {
        if code.external_prepared {
            external_state(if code.external_accepted {
                ExternalInstallState::Accepted
            } else {
                ExternalInstallState::Prepared
            });
            return true;
        }
        if code.installed {
            external_state(ExternalInstallState::LegacyOwned);
            return false;
        }
        // Prepared storage is never replaced: an external commit may already have made the relay reachable even
        // before its receipt is accepted here.
        if code.external_relay != 0 {
            return false;
        }
        if !code.guard_attempted {
            code.guard_attempted = true;
            code.guarded_base = base;
            code.guard_valid = whole_body(base);
        }
        if !code.guard_valid
            || code.guarded_base != base
            || !whole_body(base)
            || !configure_xstate()
            || base > usize::MAX - ALLOCATOR_RVA as usize - ALLOCATOR_BYTES
            || code.external_nonce == u64::MAX
        {
            return false;
        }
        let mut next = ExternalInstallPlan::default();
        next.version = EXTERNAL_PLAN_VERSION;
        next.nonce = code.external_nonce + 1;
        // SAFETY: plain Win32 calls.
        next.process_id = u64::from(unsafe { GetCurrentProcessId() });
        next.game_base = base as u64;
        next.allocator_address = base as u64 + ALLOCATOR_RVA;
        next.site_address = base as u64 + WITNESS_RVA;
        next.thunk_address = thunk_address();
        next.guard_fnv = ALLOCATOR_FNV;
        next.allocator_bytes = ALLOCATOR_BYTES as u64;
        next.site_bytes = 7;
        next.relay_bytes = 14;
        next.original = ORIGINAL_WITNESS_BYTES;
        if !process_identity(&mut next.process_creation_time)
            || !read(next.allocator_address as usize, &mut next.guard)
            || fnv(&next.guard) != ALLOCATOR_FNV
        {
            return false;
        }
        let mut site = region();
        if !query(next.site_address, &mut site)
            || site.State != MEM_COMMIT
            || site.Type != MEM_IMAGE
            || site.AllocationBase as usize != base
        {
            return false;
        }
        next.original_protection = u64::from(site.Protect);
        let mut module = null_mut();
        // SAFETY: the thunk's address in this module; an out-parameter.
        let pinned = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                next.thunk_address as usize as *const u16,
                &mut module,
            )
        } != 0;
        code.external_module = module as usize;
        if !pinned || !thunk_extent(code.external_module, &mut next.thunk_bytes) {
            return false;
        }
        next.module_pin_address = code.external_module as u64;
        code.external_relay = allocate_relay(next.site_address as usize, false);
        if code.external_relay == 0 {
            return false;
        }
        next.relay_address = code.external_relay as u64;
        next.relay_code = relay_code(next.thunk_address);
        // SAFETY: the relay's own fresh read-write page.
        unsafe { core::ptr::copy_nonoverlapping(next.relay_code.as_ptr(), code.external_relay as *mut u8, 14) };
        let Some(patch) = site_patch(next.relay_address, next.site_address) else { return false };
        let mut previous = 0u32;
        // SAFETY: as above.
        if unsafe { VirtualProtect(code.external_relay as *const c_void, 14, PAGE_EXECUTE_READ, &mut previous) } == 0
            || unsafe { FlushInstructionCache(GetCurrentProcess(), code.external_relay as *const c_void, 14) } == 0
        {
            return false;
        }
        next.patch = patch;
        next.checksum = crate::native::upload_reservation::external_plan_checksum(&next);
        if !valid_external_plan(&next) {
            return false;
        }
        code.prepared_plan = next;
        code.external_nonce = next.nonce;
        code.external_prepared = true;
        PLAN_SEQ.fetch_add(1, AcqRel);
        // SAFETY: the exported plan, published under its sequence.
        unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_UploadReservationExternalPlan, next) };
        code.external_plan_sequence = PLAN_SEQ.fetch_add(1, Release).wrapping_add(1);
        external_state(ExternalInstallState::Prepared);
        state(Status::Disabled);
        true
    }
    fn read_external_receipt(receipt: &mut ExternalInstallReceipt) -> bool {
        for _ in 0..3 {
            let sequence = RECEIPT_SEQ.load(Acquire);
            if sequence == 0 || sequence & 1 != 0 {
                return false;
            }
            // SAFETY: the exported receipt, read under its sequence.
            *receipt = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_UploadReservationExternalReceipt) };
            fence(Acquire);
            if sequence == RECEIPT_SEQ.load(Acquire) {
                return true;
            }
        }
        false
    }
    fn accept_external(code: &mut Code) -> bool {
        if !code.external_prepared
            || code.external_plan_sequence == 0
            || code.external_plan_sequence & 1 != 0
            || PLAN_SEQ.load(Acquire) != code.external_plan_sequence
        {
            return false;
        }
        // SAFETY: the exported plan, read under its sequence.
        let exported = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_UploadReservationExternalPlan) };
        fence(Acquire);
        if PLAN_SEQ.load(Acquire) != code.external_plan_sequence || exported != code.prepared_plan {
            return false;
        }
        let mut receipt = ExternalInstallReceipt::default();
        if !read_external_receipt(&mut receipt)
            || !valid_external_receipt(&code.prepared_plan, &receipt)
            || !current_external_code(code, &code.prepared_plan)
        {
            return false;
        }
        code.relay = code.external_relay;
        code.patch = code.prepared_plan.patch;
        code.site_protection = code.prepared_plan.original_protection as u32;
        code.installed = true;
        code.healthy = true;
        code.external_accepted = true;
        external_state(ExternalInstallState::Accepted);
        state(Status::Disabled);
        true
    }
    fn apply_loss(books: &mut Books) {
        if LOST.load(Acquire) {
            books.ledger.quarantine_all();
        }
    }

    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_prepare_code_guards(base: usize) {
        let mut code = lock(&CODE);
        if code.guard_attempted {
            return;
        }
        code.guard_attempted = true;
        code.guarded_base = base;
        code.guard_valid = whole_body(base);
        if !code.guard_valid {
            state(Status::GuardMismatch);
        }
    }
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_compatible() -> bool {
        let code = lock(&CODE);
        code.guard_valid && code.guarded_base == base()
    }
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_installed() -> bool {
        lock(&CODE).installed
    }
    /// No patch is written here: explicit requests prepare one retained relay and plan, then accept the detached
    /// debugger's receipt after exact current-code checks.
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_poll_installation(base: usize) {
        if PREPARE_REQUEST.load(Acquire) == 0 && ACCEPT_REQUEST.load(Acquire) == 0 {
            return;
        }
        let mut code = match CODE.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        ENABLE.store(0, Release);
        let prepare = PREPARE_REQUEST.swap(0, AcqRel);
        let accept = ACCEPT_REQUEST.swap(0, AcqRel);
        if prepare > 1 || accept > 1 {
            external_state(ExternalInstallState::UnsupportedRequest);
            state(Status::InstallFailure);
            return;
        }
        if prepare != 0 && !prepare_external(&mut code, if base != 0 { base } else { self::base() }) {
            if !code.installed {
                external_state(ExternalInstallState::PrepareRejected);
            }
            state(Status::InstallFailure);
        }
        if accept != 0 && !accept_external(&mut code) {
            external_state(ExternalInstallState::ReceiptRejected);
            state(Status::InstallFailure);
        }
    }
    /// The host must establish process-thread quiescence for the seven-byte patch.
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_install(base: usize, threads_quiescent: bool) -> bool {
        let mut code = lock(&CODE);
        if code.installed {
            return code.healthy && base == code.guarded_base;
        }
        if !threads_quiescent || code.external_relay != 0 {
            state(Status::InstallFailure);
            return false;
        }
        if !code.guard_attempted {
            code.guard_attempted = true;
            code.guarded_base = base;
            code.guard_valid = whole_body(base);
        }
        if !code.guard_valid || code.guarded_base != base || !whole_body(base) {
            state(Status::GuardMismatch);
            return false;
        }
        if !configure_xstate() || base > usize::MAX - WITNESS_RVA as usize || base > usize::MAX - RETRY_RVA as usize {
            state(Status::InstallFailure);
            return false;
        }
        let site = base + WITNESS_RVA as usize;
        code.relay = allocate_relay(site, false);
        if code.relay == 0 {
            state(Status::InstallFailure);
            return false;
        }
        let jump = relay_code(thunk_address());
        // SAFETY: the relay's own fresh read-write page.
        unsafe { core::ptr::copy_nonoverlapping(jump.as_ptr(), code.relay as *mut u8, 14) };
        let patch = site_patch(code.relay as u64, site as u64);
        let mut previous = 0u32;
        if patch.is_none()
            // SAFETY: as above.
            || unsafe { VirtualProtect(code.relay as *const c_void, 14, PAGE_EXECUTE_READ, &mut previous) } == 0
            || unsafe { FlushInstructionCache(GetCurrentProcess(), code.relay as *const c_void, 14) } == 0
        {
            release(code.relay);
            code.relay = 0;
            state(Status::InstallFailure);
            return false;
        }
        let patch = patch.unwrap_or_default();
        code.patch = patch;
        let mut written = false;
        if !replace(site, &ORIGINAL_WITNESS_BYTES, &patch, &mut written, &mut code.site_protection) {
            // A failure after the store still leaves a reachable executable relay.
            code.installed = written;
            code.healthy = false;
            if !code.installed {
                release(code.relay);
                code.relay = 0;
            }
            state(Status::InstallFailure);
            return false;
        }
        code.installed = true;
        code.healthy = true;
        state(Status::Disabled);
        true
    }
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_remove(threads_quiescent: bool) -> bool {
        let mut code = lock(&CODE);
        if !code.installed {
            return true;
        }
        if !threads_quiescent || code.external_accepted {
            state(Status::InstallFailure);
            return false;
        }
        let site = code.guarded_base.wrapping_add(WITNESS_RVA as usize);
        let mut current = [0u8; 7];
        if !read(site, &mut current) {
            state(Status::InstallFailure);
            return false;
        }
        if current == code.patch {
            let (mut written, mut previous) = (false, 0u32);
            let patch = code.patch;
            if !replace(site, &patch, &ORIGINAL_WITNESS_BYTES, &mut written, &mut previous) && !written {
                state(Status::InstallFailure);
                return false;
            }
        } else if current != ORIGINAL_WITNESS_BYTES {
            state(Status::InstallFailure);
            return false;
        }
        let mut previous = 0u32;
        // Retain the relay until both operations succeed, including a retry after an earlier restore wrote the
        // original bytes but failed to flush or protect them.
        // SAFETY: the guarded image's seven bytes.
        let restored = unsafe { VirtualProtect(site as *const c_void, 7, code.site_protection, &mut previous) } != 0;
        let flushed = unsafe { FlushInstructionCache(GetCurrentProcess(), site as *const c_void, 7) } != 0;
        if !restored || !flushed {
            state(Status::InstallFailure);
            return false;
        }
        code.installed = false;
        code.healthy = false;
        ENABLE.store(0, Release);
        // Quiescence must include callbacks and relay execution before their storage is freed.
        release(code.relay);
        code.relay = 0;
        code.site_protection = 0;
        state(Status::Disabled);
        true
    }
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_begin_generation(ring: u64, generation: u64, capacity: u64) -> bool {
        let mut books = lock(&BOOKS);
        apply_loss(&mut books);
        books.ledger.begin_generation(ring, generation, capacity)
    }
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_end_generation(ring: u64) {
        lock(&BOOKS).ledger.end_generation(ring);
    }
    /// # Safety
    /// `output` valid.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_ur_rt_find(sequence: u64, output: *mut Reservation) -> u64 {
        let mut books = lock(&BOOKS);
        apply_loss(&mut books);
        let (status, reservation) = books.ledger.find(sequence);
        // SAFETY: as the caller promises.
        unsafe { *output = reservation };
        status as u64
    }
    /// # Safety
    /// `reservation` and `output` valid.
    #[no_mangle]
    #[allow(clippy::too_many_arguments)]
    pub unsafe extern "C" fn cvr_ur_rt_validate_range(
        reservation: *const Reservation,
        local_offset: u64,
        physical_offset: u64,
        padded_bytes: u64,
        logical_bytes: u64,
        backing_bytes: u64,
        output: *mut Range,
    ) -> u64 {
        let mut books = lock(&BOOKS);
        apply_loss(&mut books);
        // SAFETY: as the caller promises.
        let (status, range) = books.ledger.validate_range(
            unsafe { &*reservation },
            local_offset,
            physical_offset,
            padded_bytes,
            logical_bytes,
            backing_bytes,
        );
        unsafe { *output = range };
        status as u64
    }
    #[no_mangle]
    pub extern "C" fn cvr_ur_rt_clear() {
        let mut books = lock(&BOOKS);
        if books.epoch == u64::MAX {
            books.ledger.quarantine_all();
            LOST.store(true, Release);
            state(Status::Capacity);
            return;
        }
        books.epoch += 1;
        let epoch = books.epoch;
        books.ledger.reset(epoch);
        let rows = (&raw mut CyberpunkVR_UploadReservationRows).cast::<Row>();
        for i in 0..MAX_ROWS {
            // SAFETY: the exported rows, written under the ledger's mutex.
            unsafe { rows.add(i).write(ROW0) };
        }
        LOST.store(false, Release);
        COUNT.store(0, Release);
        DROPPED.store(0, Relaxed);
    }
    /// The thunk's callback.
    ///
    /// # Safety
    /// `frame` is null or the thunk's saved frame.
    #[no_mangle]
    pub unsafe extern "C" fn CvrNativeUploadReservationObserve(frame: *const ThunkFrame) {
        // SAFETY: as the caller promises.
        let Some(frame) = (unsafe { frame.as_ref() }) else { return };
        if frame.flags & 0x40 == 0 || ENABLE.load(Relaxed) == 0 {
            return;
        }
        let mut books = match BOOKS.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                LOST.store(true, Release);
                DROPPED.fetch_add(1, Relaxed);
                state(Status::Contention);
                return;
            }
        };
        apply_loss(&mut books);
        let witness = Witness {
            ring: frame.rbx,
            old_head: frame.r9,
            absolute_begin: frame.rcx,
            absolute_end: frame.rdx,
            bytes: frame.r13,
            capacity: frame.capacity,
            native_stack: frame.native_stack,
            native_caller: frame.native_caller,
            flags: frame.flags,
        };
        // SAFETY: a plain Win32 call.
        let thread = u64::from(unsafe { GetCurrentThreadId() });
        let (result, row) = books.ledger.observe(&witness, thread, frame.r8 == frame.capacity);
        if row.sequence != 0 {
            let index = (row.sequence - 1) as usize;
            if index < MAX_ROWS {
                // SAFETY: the exported rows, written under the ledger's mutex.
                unsafe { (&raw mut CyberpunkVR_UploadReservationRows).cast::<Row>().add(index).write(row) };
            }
            COUNT.store(books.ledger.count() as u32, Release);
        } else if result == Status::Capacity {
            LOST.store(true, Release);
            DROPPED.fetch_add(1, Relaxed);
        }
        state(result);
    }
}

#[cfg(test)]
mod tests {
    //! The relay's code, the site's patch, the relay's candidates and the XSAVE choice.
    use super::*;

    #[test]
    fn relay() {
        assert_eq!(
            relay_code(0x0000_7FF8_1234_5678),
            [0xFF, 0x25, 0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0xF8, 0x7F, 0, 0]
        );
        // The CALL's rel32 counts from its return address (site + 5); the JNE retries 0x1C bytes back.
        assert_eq!(site_patch(0x1_0000_1000, 0x1_0000_0000), Some([0xE8, 0xFB, 0x0F, 0, 0, 0x75, 0xE4]));
        assert_eq!(site_patch(0x1_0000_0000, 0x1_0000_1000), Some([0xE8, 0xFB, 0xEF, 0xFF, 0xFF, 0x75, 0xE4]));
        assert_eq!(site_patch(0x1_8000_0004, 0x1_0000_0000).map(|p| p[1]), Some(0xFF));
        assert_eq!(site_patch(0x1_8000_0005, 0x1_0000_0000), None);
        assert_eq!(site_patch(0x1_0000_0000 - 0x7FFF_FFFB, 0x1_0000_0000).map(|p| p[4]), Some(0x80));
        assert_eq!(site_patch(0x1_0000_0000 - 0x7FFF_FFFC, 0x1_0000_0000), None);
    }
    #[test]
    fn candidates() {
        let near: Vec<usize> = relay_candidates(0x1_4041_9F00, 0x10000).take(5).collect();
        // Aligned down to 0x1_4041_0000: a granule below, a granule above, two below, ...
        assert_eq!(near, [0x1_4040_0000, 0x1_4042_0000, 0x1_403F_0000, 0x1_4043_0000, 0x1_403E_0000]);
        // Out to 0x7FFF0000 bytes either way: 0x7FFF granules, two candidates each.
        assert_eq!(relay_candidates(0x1_4041_9F00, 0x10000).count(), 2 * 0x7FFF);
        // Near the bottom only the addresses above are left, and never 0.
        let low: Vec<usize> = relay_candidates(0x2_0000, 0x10000).take(3).collect();
        assert_eq!(low, [0x1_0000, 0x3_0000, 0x4_0000]);
        assert!(relay_candidates(0x2_0000, 0x10000).all(|a| a != 0));
        // Near the top, none past the last granule.
        assert!(relay_candidates(usize::MAX - 0x1_FFFF, 0x10000).all(|a| a <= usize::MAX - 0x10000));
    }
    #[test]
    fn allocation() {
        let site = allocation as fn() as usize;
        let relay = allocate_relay(site, true);
        assert!(relay != 0 && relay % 0x10000 == 0);
        assert!(relay.abs_diff(site) <= 0x8000_0000, "{relay:X} {site:X}");
        assert!(site_patch(relay as u64, site as u64).is_some());
        // SAFETY: the test's own allocation.
        unsafe {
            windows_sys::Win32::System::Memory::VirtualFree(relay as *mut core::ffi::c_void, 0, windows_sys::Win32::System::Memory::MEM_RELEASE)
        };
        // The processor's choice is one of the three, consistent with valid_xstate.
        if let Xstate::Full { mask, bytes } = cpu_xstate() {
            assert!(mask & 3 == 3 && bytes >= 576);
        }
    }
    #[test]
    fn xstate() {
        let full = || (0x2E7, 0x2FF, 2696);
        assert_eq!(xstate_choice(0, 0x20, full), Xstate::Legacy);
        assert_eq!(xstate_choice(1 << 27, 0x20, full), Xstate::Unsupported);
        assert_eq!(xstate_choice(3 << 26, 0xC, full), Xstate::Unsupported);
        assert_eq!(xstate_choice(3 << 26, 0xD, full), Xstate::Full { mask: 0x2E7, bytes: 2696 });
        // A component enabled but unsupported, too small an area, or without x87/SSE.
        assert_eq!(xstate_choice(3 << 26, 0xD, || (0x2E7, 0x7, 2696)), Xstate::Unsupported);
        assert_eq!(xstate_choice(3 << 26, 0xD, || (0x7, 0x7, 575)), Xstate::Unsupported);
        assert_eq!(xstate_choice(3 << 26, 0xD, || (0x6, 0x7, 832)), Xstate::Unsupported);
    }
}
