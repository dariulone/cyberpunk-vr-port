//! The instance reservation witness's runtime (src/Render/NativeInstanceWitness.cpp; its rules are instance_witness):
//! preparation before any game entry detours -- the guarded allocator's 81 bytes kept, the thunk's XSAVE choice
//! (src/Hooks/NativeInstanceReservationThunk.asm reads it only once marked ready), the module pinned, the thunk's
//! unwind extent, a relay within rel32 of the witness site (`jmp [rip]` to the thunk) and the site's five-byte CALL,
//! all checked against the live code --; the installation -- unless the caller vouches for quiescence, a balanced
//! suspension of every other process thread (inventoried until two snapshots agree, none stopped inside the site's
//! five bytes), the patch written and verified, rolled back when it does not verify, the threads resumed (a thread
//! that would not resume is kept pending and resumed before the next call) --; and the installed check.
//!
//! The header's Prepare, Install and Installed forward to the `cvr_iw_rt_*` functions below; the thunk reads the
//! XSAVE mask, mode and readiness defined here. The unit tests below check the thread inventory and a suspended
//! thread's context.

/// The process's other threads' ids, sorted (`ids` bounds their number); None when the snapshot fails, is cut short
/// or there are more threads than `ids` holds.
pub fn thread_inventory(ids: &mut [u32], process: u32, current: u32) -> Option<usize> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread: u32,
        owner: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> *mut core::ffi::c_void;
        fn Thread32First(snapshot: *mut core::ffi::c_void, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: *mut core::ffi::c_void, entry: *mut ThreadEntry) -> i32;
    }
    const SNAPTHREAD: u32 = 4;
    const OWNER_END: u32 = 16;
    let mut count = 0usize;
    // SAFETY: a snapshot of the system's threads, closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let size = core::mem::size_of::<ThreadEntry>() as u32;
    let mut entry = ThreadEntry { size, usage: 0, thread: 0, owner: 0, base_priority: 0, delta_priority: 0, flags: 0 };
    // SAFETY: the entry carries its size.
    let mut okay = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    if okay {
        loop {
            if entry.size < OWNER_END {
                okay = false;
                break;
            }
            if entry.owner == process && entry.thread != current {
                if count == ids.len() {
                    okay = false;
                    break;
                }
                ids[count] = entry.thread;
                count += 1;
            }
            entry.size = size;
            // SAFETY: as above.
            if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
                break;
            }
        }
        // SAFETY: a plain Win32 call.
        if okay && unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
            okay = false;
        }
    }
    // SAFETY: the snapshot's own handle.
    unsafe { CloseHandle(snapshot) };
    if !okay {
        return None;
    }
    ids[..count].sort_unstable();
    Some(count)
}
/// A thread context at the alignment GetThreadContext requires: C++'s CONTEXT declares DECLSPEC_ALIGN(16),
/// windows-sys' does not, and at an 8-byte address the call fails with ERROR_NOACCESS.
#[repr(C, align(16))]
struct AlignedContext(windows_sys::Win32::System::Diagnostics::Debug::CONTEXT);
const _: () = assert!(core::mem::align_of::<AlignedContext>() == 16);
/// A suspended thread's instruction pointer (CONTEXT_CONTROL); None when its context cannot be read.
///
/// # Safety
/// `handle` is a thread handle with THREAD_GET_CONTEXT, the thread suspended (or the caller's own).
pub unsafe fn thread_instruction(handle: *mut core::ffi::c_void) -> Option<usize> {
    use windows_sys::Win32::System::Diagnostics::Debug::{GetThreadContext, CONTEXT_CONTROL_AMD64};
    // SAFETY: plain integers.
    let mut context = AlignedContext(unsafe { core::mem::zeroed() });
    context.0.ContextFlags = CONTEXT_CONTROL_AMD64;
    // SAFETY: as the caller vouches; an out-parameter at its required alignment.
    (unsafe { GetThreadContext(handle, &mut context.0) } != 0).then_some(context.0.Rip as usize)
}

#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
#[allow(non_upper_case_globals)]
mod abi {
    use super::{thread_inventory, thread_instruction};
    use crate::instance_witness::{body, call_patch, patch_boundary, ALLOCATOR_BYTES, ALLOCATOR_RVA, ORIGINAL_SITE, SITE_RVA};
    use crate::upload_reservation_runtime::{allocate_relay, cpu_xstate, relay_code, Xstate};
    use core::ffi::c_void;
    use core::mem::{size_of, zeroed};
    use core::ptr::{null, null_mut};
    use core::sync::atomic::{AtomicU32, Ordering::*};
    use std::sync::{Mutex, MutexGuard};
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Diagnostics::Debug::{FlushInstructionCache, RtlLookupFunctionEntry, WriteProcessMemory};
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GetModuleHandleW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
    };
    use windows_sys::Win32::System::Memory::{
        VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_PRIVATE, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, GetExitCodeThread, GetProcessIdOfThread,
        GetThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_LIMITED_INFORMATION,
        THREAD_SUSPEND_RESUME,
    };

    /// The thunk's XSAVE mask and mode, read only while marked ready.
    #[no_mangle]
    pub static mut CvrNativeInstanceReservationXstateMask: u64 = 0;
    #[no_mangle]
    pub static mut CvrNativeInstanceReservationXstateMode: u32 = 0;
    #[no_mangle]
    pub static CvrNativeInstanceReservationXstateReady: AtomicU32 = AtomicU32::new(0);
    extern "C" {
        /// src/Hooks/NativeInstanceReservationThunk.asm.
        fn CvrNativeInstanceReservationThunk();
    }

    const MAX_THREADS: usize = 4096;
    #[derive(Clone, Copy, Default)]
    struct Thread {
        handle: usize,
        id: u32,
        previous: u32,
        suspended: bool,
    }
    struct State {
        base: usize,
        thunk: usize,
        module: usize,
        relay: usize,
        thunk_bytes: usize,
        protection: u32,
        prepared: bool,
        attempted: bool,
        installed: bool,
        healthy: bool,
        original: [u8; ALLOCATOR_BYTES],
        patch: [u8; 5],
        jump: [u8; 14],
        pending: Vec<Thread>,
    }
    static STATE: Mutex<State> = Mutex::new(State {
        base: 0,
        thunk: 0,
        module: 0,
        relay: 0,
        thunk_bytes: 0,
        protection: 0,
        prepared: false,
        attempted: false,
        installed: false,
        healthy: false,
        original: [0; ALLOCATOR_BYTES],
        patch: [0; 5],
        jump: [0; 14],
        pending: Vec::new(),
    });

    fn lock(mutex: &Mutex<State>) -> MutexGuard<'_, State> {
        mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn ready(value: bool) {
        CvrNativeInstanceReservationXstateReady.swap(u32::from(value), SeqCst);
    }
    fn read(address: usize, output: &mut [u8]) -> bool {
        address != 0 && address <= usize::MAX - output.len() && crate::instance_layout::read(address as u64, output)
    }
    fn region() -> MEMORY_BASIC_INFORMATION {
        // SAFETY: plain integers and pointers.
        unsafe { zeroed() }
    }
    fn query(address: usize, output: &mut MEMORY_BASIC_INFORMATION) -> bool {
        let size = size_of::<MEMORY_BASIC_INFORMATION>();
        // SAFETY: an out-parameter of its size.
        unsafe { VirtualQuery(address as *const c_void, output, size) == size }
    }
    fn executable(protection: u32) -> bool {
        protection == PAGE_EXECUTE_READ || protection == PAGE_EXECUTE_READWRITE || protection == PAGE_EXECUTE_WRITECOPY
    }
    fn close(handle: usize) {
        // SAFETY: a thread handle this module opened.
        unsafe { CloseHandle(handle as *mut c_void) };
    }
    fn configure_xstate() -> bool {
        // SAFETY: the thunk reads these only while marked ready, which preparation cleared.
        unsafe {
            CvrNativeInstanceReservationXstateMask = 0;
            CvrNativeInstanceReservationXstateMode = 0;
        }
        match cpu_xstate() {
            Xstate::Legacy => true,
            Xstate::Unsupported => false,
            Xstate::Full { mask, .. } => {
                // SAFETY: as above.
                unsafe {
                    CvrNativeInstanceReservationXstateMask = mask;
                    CvrNativeInstanceReservationXstateMode = 1;
                }
                true
            }
        }
    }
    fn code(state: &State, patched: bool) -> bool {
        if !state.prepared
            || state.base == 0
            || state.relay == 0
            || state.module == 0
            || CvrNativeInstanceReservationXstateReady.load(Relaxed) == 0
        {
            return false;
        }
        let mut current = [0u8; ALLOCATOR_BYTES];
        let mut relay = [0u8; 14];
        let (mut site, mut storage, mut thunk) = (region(), region(), region());
        read(state.base + ALLOCATOR_RVA as usize, &mut current)
            && body(&state.original, &current, &state.patch, patched)
            && read(state.relay, &mut relay)
            && relay == state.jump
            && query(state.base + SITE_RVA as usize, &mut site)
            && site.State == MEM_COMMIT
            && site.Type == MEM_IMAGE
            && site.AllocationBase as usize == state.base
            && site.Protect == state.protection
            && executable(site.Protect)
            && query(state.relay, &mut storage)
            && storage.State == MEM_COMMIT
            && storage.Type == MEM_PRIVATE
            && storage.AllocationBase as usize == state.relay
            && storage.Protect == PAGE_EXECUTE_READ
            && query(state.thunk, &mut thunk)
            && thunk.State == MEM_COMMIT
            && thunk.Type == MEM_IMAGE
            && thunk.AllocationBase as usize == state.module
            && executable(thunk.Protect)
    }
    fn resume(thread: &mut Thread) -> bool {
        if !thread.suspended {
            return true;
        }
        for _ in 0..3 {
            // SAFETY: a thread handle this module opened and suspended.
            let previous = unsafe { ResumeThread(thread.handle as *mut c_void) };
            if previous != u32::MAX {
                thread.suspended = false;
                return previous == thread.previous.wrapping_add(1);
            }
            let mut code = 0u32;
            // SAFETY: as above; an out-parameter.
            if unsafe { GetExitCodeThread(thread.handle as *mut c_void, &mut code) } != 0 && code != STILL_ACTIVE as u32 {
                thread.suspended = false;
                return true;
            }
        }
        false
    }
    fn resume_pending(state: &mut State) -> bool {
        let mut okay = true;
        for thread in state.pending.iter_mut().rev() {
            let resumed = resume(thread);
            okay &= resumed;
            if !thread.suspended {
                close(thread.handle);
                *thread = Thread::default();
            }
        }
        state.pending.retain(|thread| thread.handle != 0);
        okay && state.pending.is_empty()
    }
    /// The suspended threads (their storage taken before any suspension: nothing allocates while they are held).
    struct Frozen {
        threads: Vec<Thread>,
    }
    impl Frozen {
        fn new() -> Self {
            Frozen { threads: Vec::with_capacity(MAX_THREADS) }
        }
        fn freeze(&mut self, site: usize) -> bool {
            // SAFETY: plain Win32 calls.
            let (process, current) = unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) };
            let mut ids = [0u32; MAX_THREADS];
            let mut again = [0u32; MAX_THREADS];
            for _ in 0..4 {
                let Some(count) = thread_inventory(&mut ids, process, current) else { return false };
                for &id in &ids[..count] {
                    if self.threads.iter().any(|thread| thread.id == id) {
                        continue;
                    }
                    if self.threads.len() == MAX_THREADS {
                        return false;
                    }
                    // SAFETY: plain Win32 calls on the thread this module opens.
                    let handle = unsafe {
                        OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION, 0, id)
                    };
                    self.threads.push(Thread { handle: handle as usize, id, previous: 0, suspended: false });
                    if handle.is_null()
                        || unsafe { GetProcessIdOfThread(handle) } != process
                        || unsafe { GetThreadId(handle) } != id
                    {
                        return false;
                    }
                    let previous = unsafe { SuspendThread(handle) };
                    let thread = self.threads.last_mut().unwrap_or_else(|| unreachable!());
                    thread.previous = previous;
                    if previous == u32::MAX {
                        return false;
                    }
                    thread.suspended = true;
                }
                let Some(other) = thread_inventory(&mut again, process, current) else { return false };
                if count != other || ids[..count] != again[..count] {
                    continue;
                }
                if count != self.threads.len() {
                    return false;
                }
                for thread in &self.threads {
                    let mut code = 0u32;
                    let handle = thread.handle as *mut c_void;
                    // SAFETY: a suspended thread's handle; an out-parameter.
                    if unsafe { GetExitCodeThread(handle, &mut code) } == 0 || code != STILL_ACTIVE as u32 {
                        return false;
                    }
                    // SAFETY: as above.
                    let Some(rip) = (unsafe { thread_instruction(handle) }) else { return false };
                    if !patch_boundary(site, rip)
                    {
                        return false;
                    }
                }
                let Some(other) = thread_inventory(&mut again, process, current) else { return false };
                if count == other && ids[..count] == again[..count] {
                    return true;
                }
            }
            false
        }
        /// Every thread resumed (one that would not is kept pending: its storage reserved before the suspension).
        fn release(mut self, state: &mut State) -> bool {
            let mut okay = true;
            for thread in self.threads.iter_mut().rev() {
                if thread.handle == 0 {
                    continue;
                }
                let resumed = resume(thread);
                okay &= resumed;
                if thread.suspended {
                    if state.pending.len() < MAX_THREADS {
                        state.pending.push(*thread);
                    }
                    okay = false;
                } else {
                    close(thread.handle);
                }
                *thread = Thread::default();
            }
            okay
        }
    }
    fn write(site: usize, bytes: &[u8; 5], protection: u32) -> bool {
        let mut ignored = 0u32;
        // SAFETY: the guarded image's five bytes.
        if unsafe { VirtualProtect(site as *const c_void, 5, PAGE_EXECUTE_READWRITE, &mut ignored) } == 0 {
            return false;
        }
        let mut written = 0usize;
        // SAFETY: as above.
        let copied = unsafe {
            WriteProcessMemory(GetCurrentProcess(), site as *const c_void, bytes.as_ptr().cast(), 5, &mut written)
        } != 0
            && written == 5;
        let flushed = unsafe { FlushInstructionCache(GetCurrentProcess(), site as *const c_void, 5) } != 0;
        let restored = unsafe { VirtualProtect(site as *const c_void, 5, protection, &mut ignored) } != 0;
        let mut current = [0u8; 5];
        copied && flushed && restored && read(site, &mut current) && current == *bytes
    }
    fn prepare_locked(state: &mut State, base: usize) -> bool {
        if state.prepared {
            return base == state.base && code(state, state.installed);
        }
        // SAFETY: the process image's handle.
        let image_base = unsafe { GetModuleHandleW(null()) } as usize;
        if state.relay != 0
            || state.attempted
            || base == 0
            || base != image_base
            || base > usize::MAX - ALLOCATOR_RVA as usize - ALLOCATOR_BYTES
        {
            return false;
        }
        ready(false);
        state.base = base;
        state.thunk = CvrNativeInstanceReservationThunk as unsafe extern "C" fn() as usize;
        if !read(base + ALLOCATOR_RVA as usize, &mut state.original)
            || !body(&state.original, &state.original, &state.patch, false)
            || !configure_xstate()
        {
            return false;
        }
        let mut site = region();
        if !query(base + SITE_RVA as usize, &mut site)
            || site.State != MEM_COMMIT
            || site.Type != MEM_IMAGE
            || site.AllocationBase as usize != base
            || !executable(site.Protect)
        {
            return false;
        }
        state.protection = site.Protect;
        let mut module = null_mut();
        // SAFETY: the thunk's address in this module; an out-parameter.
        let pinned = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                state.thunk as *const u16,
                &mut module,
            )
        } != 0;
        state.module = module as usize;
        if !pinned {
            return false;
        }
        let mut image = 0u64;
        // SAFETY: an out-parameter; no history table.
        let function = unsafe { RtlLookupFunctionEntry(state.thunk as u64, &mut image, null_mut()) };
        // SAFETY: the image's own unwind table entry when not null.
        let Some(function) = (unsafe { function.as_ref() }) else { return false };
        let (begin, end) = (function.BeginAddress, function.EndAddress);
        if image != state.module as u64
            || image.wrapping_add(u64::from(begin)) != state.thunk as u64
            || end <= begin
            || end - begin > 4096
        {
            return false;
        }
        state.thunk_bytes = (end - begin) as usize;
        state.relay = allocate_relay(base + SITE_RVA as usize, true);
        if state.relay == 0 {
            return false;
        }
        state.jump = relay_code(state.thunk as u64);
        // SAFETY: the relay's own fresh read-write page.
        unsafe { core::ptr::copy_nonoverlapping(state.jump.as_ptr(), state.relay as *mut u8, 14) };
        let patch = call_patch(base + SITE_RVA as usize, state.relay);
        state.patch = patch.unwrap_or_default();
        let mut previous = 0u32;
        if patch.is_none()
            // SAFETY: as above.
            || unsafe { VirtualProtect(state.relay as *const c_void, 14, PAGE_EXECUTE_READ, &mut previous) } == 0
            || unsafe { FlushInstructionCache(GetCurrentProcess(), state.relay as *const c_void, 14) } == 0
        {
            return false;
        }
        state.prepared = true;
        ready(true);
        if !code(state, false) {
            ready(false);
            state.prepared = false;
            return false;
        }
        true
    }

    /// Prepares the retained code storage and XSAVE choice before any game entry detours.
    #[no_mangle]
    pub extern "C" fn cvr_iw_rt_prepare(base: usize) -> bool {
        prepare_locked(&mut lock(&STATE), base)
    }
    /// Without the caller's quiescence, obtains a balanced suspension of every other process thread.
    #[no_mangle]
    pub extern "C" fn cvr_iw_rt_install(base: usize, threads_quiescent: bool) -> bool {
        let mut guard = lock(&STATE);
        let state = &mut *guard;
        if !resume_pending(state) {
            return false;
        }
        if state.installed {
            return state.healthy && base == state.base && code(state, true);
        }
        if state.attempted || !prepare_locked(state, base) || !code(state, false) {
            return false;
        }
        state.pending.reserve_exact(MAX_THREADS);
        let mut frozen = Frozen::new();
        let site = base + SITE_RVA as usize;
        if !threads_quiescent && !frozen.freeze(site) {
            frozen.release(state);
            return false;
        }
        if !code(state, false) {
            frozen.release(state);
            return false;
        }
        state.attempted = true;
        let patched = write(site, &state.patch, state.protection) && code(state, true);
        if !patched {
            let rolled_back = write(site, &ORIGINAL_SITE, state.protection) && code(state, false);
            if !rolled_back {
                ready(false);
            }
            frozen.release(state);
            return false;
        }
        state.installed = true;
        state.healthy = frozen.release(state);
        if !state.healthy {
            ready(false);
        }
        state.healthy && code(state, true)
    }
    #[no_mangle]
    pub extern "C" fn cvr_iw_rt_installed() -> bool {
        let mut guard = lock(&STATE);
        let state = &mut *guard;
        resume_pending(state) && state.installed && state.healthy && code(state, true)
    }
}

#[cfg(test)]
mod tests {
    //! The thread inventory.
    use super::*;
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId, GetThreadId};

    #[test]
    fn inventory() {
        use std::os::windows::io::AsRawHandle;
        let (stop, parked) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = parked.recv();
        });
        // SAFETY: plain Win32 calls; the worker's live handle.
        let (process, current) = unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) };
        let id = unsafe { GetThreadId(worker.as_raw_handle()) };
        let mut ids = [0u32; 4096];
        let count = thread_inventory(&mut ids, process, current).expect("snapshot");
        let seen = &ids[..count];
        assert!(seen.contains(&id) && !seen.contains(&current), "{seen:?}");
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
        // Too small a buffer fails rather than truncating.
        let mut one = [0u32; 1];
        assert_eq!(thread_inventory(&mut one, process, current), None);
        // Another process's threads are not ours.
        assert_eq!(thread_inventory(&mut ids, u32::MAX - 3, current), Some(0));
        stop.send(()).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn context() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{ResumeThread, SuspendThread};
        let (stop, parked) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = parked.recv();
        });
        let handle = worker.as_raw_handle();
        // A suspended thread's context reads, several times over (each from a fresh stack slot).
        // SAFETY: the worker's live handle (all access rights), suspended around the reads.
        unsafe {
            assert_ne!(SuspendThread(handle), u32::MAX);
            for _ in 0..8 {
                let rip = std::hint::black_box(thread_instruction(handle));
                assert!(rip.is_some_and(|ip| ip != 0), "{rip:?}");
            }
            assert_ne!(ResumeThread(handle), u32::MAX);
        }
        stop.send(()).unwrap();
        worker.join().unwrap();
    }
}
