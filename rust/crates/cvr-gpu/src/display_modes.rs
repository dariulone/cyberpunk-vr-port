//! Making the VR resolution appear in the game's own mode list (formerly src/Hooks/DisplayModes.cpp).
//!
//! The engine will not render at a resolution its enumeration does not contain, and the headset's per-eye size is
//! not a desktop mode. So GetDisplayModeList is answered with the real list PLUS the one we need, which is the
//! smallest lie that gets the engine to agree.
//!
//! THE LIST IS QUERIED TWICE, and both calls must agree. The first asks how many modes there are with a null buffer,
//! the second asks for them. Adding an entry to the second without counting it in the first is a buffer overrun in
//! the caller, so the forced size is looked for in the real list before either answer.
//!
//! The adapter and output hooks exist only to reach the outputs: an IDXGIOutput is obtained from an adapter, and
//! there is no other point at which one can be intercepted. Hooks/SwapChain.cpp calls InstallAdapterHook with the
//! adapter the factory hands out; the C++ names forward here.
use core::ffi::c_void;

use core::sync::atomic::{AtomicBool, Ordering};

use cvr_engine::vtable;
use cvr_host::log;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED,
    DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL,
};
use windows::Win32::Graphics::Dxgi::{DXGI_MODE_DESC1, IDXGIOutput1};
use windows::core::{GUID, Interface};

use crate::desktop_mirror::forced_display_mode;

const DXGI_ERROR_INVALID_CALL: i32 = 0x887A_0001_u32 as i32;
const DXGI_ERROR_MORE_DATA: i32 = 0x887A_0003_u32 as i32;
const ENUM_OUTPUTS: usize = 7;
const GET_DISPLAY_MODE_LIST: usize = 8;
const GET_DISPLAY_MODE_LIST1: usize = 19;

/// A DXGI mode description, either generation.
pub trait Mode: Copy {
    fn size(&self) -> (u32, u32);
    fn forced(format: DXGI_FORMAT, width: u32, height: u32) -> Self;
}
impl Mode for DXGI_MODE_DESC {
    fn size(&self) -> (u32, u32) {
        (self.Width, self.Height)
    }
    fn forced(format: DXGI_FORMAT, width: u32, height: u32) -> Self {
        Self {
            Width: width,
            Height: height,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 90,
                Denominator: 1,
            },
            Format: format,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
        }
    }
}
impl Mode for DXGI_MODE_DESC1 {
    fn size(&self) -> (u32, u32) {
        (self.Width, self.Height)
    }
    fn forced(format: DXGI_FORMAT, width: u32, height: u32) -> Self {
        Self {
            Width: width,
            Height: height,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 90,
                Denominator: 1,
            },
            Format: format,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            Stereo: false.into(),
        }
    }
}

type ListFn<M> = unsafe extern "system" fn(*mut c_void, DXGI_FORMAT, u32, *mut u32, *mut M) -> i32;

/// The list with the forced size appended: the original's answer, then the count/size protocol around the extra
/// entry. `count` is not null.
///
/// # Safety
/// The arguments are the caller's GetDisplayModeList(1) arguments.
pub unsafe fn answer<M: Mode>(
    original: ListFn<M>,
    output: *mut c_void,
    format: DXGI_FORMAT,
    flags: u32,
    count: *mut u32,
    modes: *mut M,
    forced: (u32, u32),
) -> i32 {
    // SAFETY: the caller's pointers, as GetDisplayModeList documents them.
    unsafe {
        let capacity = if modes.is_null() { 0 } else { *count };
        let mut listed = *count;
        let hr = original(output, format, flags, &mut listed, modes);
        if hr < 0 && hr != DXGI_ERROR_MORE_DATA {
            *count = listed;
            return hr;
        }
        let (width, height) = forced;
        let has = !modes.is_null()
            && (0..listed as usize).any(|i| (*modes.add(i)).size() == (width, height));
        if width == 0 || height == 0 || has {
            *count = listed;
            return if hr == DXGI_ERROR_MORE_DATA { hr } else { 0 };
        }
        let required = listed + 1;
        if modes.is_null() {
            *count = required;
            return 0;
        }
        if capacity <= listed || hr == DXGI_ERROR_MORE_DATA {
            *count = required;
            return DXGI_ERROR_MORE_DATA;
        }
        *modes.add(listed as usize) = M::forced(format, width, height);
        *count = required;
        0
    }
}

unsafe fn vtable_of(object: *mut c_void) -> *mut *mut c_void {
    // SAFETY: a live COM object: its first field is the vtable.
    unsafe { *(object as *mut *mut *mut c_void) }
}

unsafe fn original_of<F: Copy>(object: *mut c_void, slot: usize) -> Option<F> {
    // SAFETY: the slot's recorded original has the hook's own signature.
    unsafe {
        let p = vtable::original(vtable_of(object), slot);
        (!p.is_null()).then(|| core::mem::transmute_copy::<*mut c_void, F>(&p))
    }
}

unsafe extern "system" fn get_display_mode_list(
    output: *mut c_void,
    format: DXGI_FORMAT,
    flags: u32,
    count: *mut u32,
    modes: *mut DXGI_MODE_DESC,
) -> i32 {
    // SAFETY: called by DXGI's callers through the patched slot.
    unsafe {
        let original = original_of::<ListFn<DXGI_MODE_DESC>>(output, GET_DISPLAY_MODE_LIST);
        hooked(0, original, output, format, flags, count, modes)
    }
}

unsafe extern "system" fn get_display_mode_list1(
    output: *mut c_void,
    format: DXGI_FORMAT,
    flags: u32,
    count: *mut u32,
    modes: *mut DXGI_MODE_DESC1,
) -> i32 {
    // SAFETY: called by DXGI's callers through the patched slot.
    unsafe {
        let original = original_of::<ListFn<DXGI_MODE_DESC1>>(output, GET_DISPLAY_MODE_LIST1);
        hooked(1, original, output, format, flags, count, modes)
    }
}

/// The first filled list of each generation is logged once (the C++ hook did not log).
static LOGGED: [AtomicBool; 2] = [const { AtomicBool::new(false) }; 2];

unsafe fn hooked<M: Mode>(
    generation: usize,
    original: Option<ListFn<M>>,
    output: *mut c_void,
    format: DXGI_FORMAT,
    flags: u32,
    count: *mut u32,
    modes: *mut M,
) -> i32 {
    // Without the original there is nothing to answer, and the count is left as it was.
    let Some(original) = original else {
        return DXGI_ERROR_INVALID_CALL;
    };
    // SAFETY: the caller's arguments; a null count is the original's to refuse.
    unsafe {
        if count.is_null() {
            return original(output, format, flags, count, modes);
        }
        let forced = forced_display_mode();
        let hr = answer(original, output, format, flags, count, modes, forced);
        if hr == 0 && !modes.is_null() && !LOGGED[generation].swap(true, Ordering::Relaxed) {
            let listed = *count as usize;
            let present = (0..listed).any(|i| (*modes.add(i)).size() == forced);
            log!(
                "DisplayModes: GetDisplayModeList{} format={} answered {} mode(s), forced {}x{} {}\n",
                if generation == 1 { "1" } else { "" },
                format.0,
                listed,
                forced.0,
                forced.1,
                if present { "listed" } else { "not listed" }
            );
        }
        hr
    }
}

/// InstallOutputHook: both list methods of an output's vtable(s).
///
/// # Safety
/// `output` is null or a live IDXGIOutput.
pub unsafe fn install_output(output: *mut c_void) {
    if output.is_null() {
        return;
    }
    // SAFETY: a live output; QueryInterface/Release by their vtable slots.
    unsafe {
        let table = vtable_of(output);
        if table.is_null() {
            return;
        }
        vtable::patch(
            table,
            GET_DISPLAY_MODE_LIST,
            get_display_mode_list as *mut c_void,
        );
        type QueryInterface =
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
        type Release = unsafe extern "system" fn(*mut c_void) -> u32;
        let query: QueryInterface = core::mem::transmute(*table);
        let mut output1: *mut c_void = core::ptr::null_mut();
        if query(output, &IDXGIOutput1::IID, &mut output1) >= 0 && !output1.is_null() {
            vtable::patch(
                vtable_of(output1),
                GET_DISPLAY_MODE_LIST1,
                get_display_mode_list1 as *mut c_void,
            );
            let release: Release = core::mem::transmute(*vtable_of(output1).add(2));
            release(output1);
        }
    }
}

type EnumOutputsFn = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;

unsafe extern "system" fn enum_outputs(
    adapter: *mut c_void,
    index: u32,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: called by DXGI's callers through the patched slot.
    unsafe {
        let hr = match original_of::<EnumOutputsFn>(adapter, ENUM_OUTPUTS) {
            Some(original) => original(adapter, index, output),
            None => DXGI_ERROR_INVALID_CALL,
        };
        if hr >= 0 && !output.is_null() && !(*output).is_null() {
            install_output(*output);
        }
        hr
    }
}

/// InstallAdapterHook: the adapter's EnumOutputs, which hooks every output it hands out.
///
/// # Safety
/// `adapter` is null or a live IDXGIAdapter.
pub unsafe fn install_adapter(adapter: *mut c_void) {
    if adapter.is_null() {
        return;
    }
    // SAFETY: a live adapter.
    unsafe {
        let table = vtable_of(adapter);
        if !table.is_null() {
            vtable::patch(table, ENUM_OUTPUTS, enum_outputs as *mut c_void);
        }
    }
}

/// InstallOutputHook (Hooks/SwapChainInternal.hpp).
///
/// # Safety
/// As [`install_output`].
#[no_mangle]
pub unsafe extern "C" fn cvr_display_modes_install_output(output: *mut c_void) {
    // SAFETY: forwarded from the caller.
    unsafe { install_output(output) }
}

/// InstallAdapterHook (Hooks/SwapChainInternal.hpp).
///
/// # Safety
/// As [`install_adapter`].
#[no_mangle]
pub unsafe extern "C" fn cvr_display_modes_install_adapter(adapter: *mut c_void) {
    // SAFETY: forwarded from the caller.
    unsafe { install_adapter(adapter) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

    const REAL: [(u32, u32); 2] = [(1920, 1080), (2560, 1440)];

    unsafe extern "system" fn real(
        _: *mut c_void,
        format: DXGI_FORMAT,
        _: u32,
        count: *mut u32,
        modes: *mut DXGI_MODE_DESC,
    ) -> i32 {
        // SAFETY: the test's own pointers.
        unsafe {
            if modes.is_null() {
                *count = REAL.len() as u32;
                return 0;
            }
            let n = (*count as usize).min(REAL.len());
            for (i, &(w, h)) in REAL.iter().take(n).enumerate() {
                *modes.add(i) = DXGI_MODE_DESC::forced(format, w, h);
            }
            let short = (*count as usize) < REAL.len();
            *count = n as u32;
            if short { DXGI_ERROR_MORE_DATA } else { 0 }
        }
    }

    fn ask(forced: (u32, u32), capacity: Option<u32>) -> (i32, u32, Vec<(u32, u32)>) {
        let mut modes = vec![DXGI_MODE_DESC::default(); capacity.unwrap_or(0) as usize];
        let mut count = capacity.unwrap_or(0);
        let buffer = if capacity.is_some() {
            modes.as_mut_ptr()
        } else {
            core::ptr::null_mut()
        };
        // SAFETY: local buffers.
        let hr = unsafe {
            answer(
                real as ListFn<DXGI_MODE_DESC>,
                core::ptr::null_mut(),
                DXGI_FORMAT_R8G8B8A8_UNORM,
                0,
                &mut count,
                buffer,
                forced,
            )
        };
        let listed = modes
            .iter()
            .take(count.min(modes.len() as u32) as usize)
            .map(|m| m.size())
            .collect();
        (hr, count, listed)
    }

    #[test]
    fn the_two_calls_agree() {
        // Count first: one more than the real list.
        assert_eq!(ask((2160, 2160), None).1, 3);
        // Then the list: the real modes and the forced one, with the 90 Hz refresh.
        let (hr, count, listed) = ask((2160, 2160), Some(3));
        assert_eq!((hr, count), (0, 3));
        assert_eq!(listed, vec![(1920, 1080), (2560, 1440), (2160, 2160)]);
        // A buffer with room for the real list only: more data, and the count that fits everything.
        let (hr, count, _) = ask((2160, 2160), Some(2));
        assert_eq!((hr, count), (DXGI_ERROR_MORE_DATA, 3));
        // Shorter than the real list: the original's own MORE_DATA, the count still includes the extra one.
        let (hr, count, _) = ask((2160, 2160), Some(1));
        assert_eq!((hr, count), (DXGI_ERROR_MORE_DATA, 2));
    }

    #[test]
    fn nothing_added_when_present_or_unset() {
        let (hr, count, listed) = ask((2560, 1440), Some(4));
        assert_eq!((hr, count), (0, 2));
        assert_eq!(listed, REAL.to_vec());
        assert_eq!(ask((0, 2160), None).1, 2);
        assert_eq!(ask((2160, 0), Some(4)).1, 2);
        // The count call cannot see the list, so a size already present is still counted; the list call corrects it.
        assert_eq!(ask((2560, 1440), None).1, 3);
    }

    #[test]
    fn mode1_has_the_same_fill() {
        let mode = DXGI_MODE_DESC1::forced(DXGI_FORMAT_R8G8B8A8_UNORM, 2064, 2208);
        assert_eq!(mode.size(), (2064, 2208));
        assert_eq!(
            (mode.RefreshRate.Numerator, mode.RefreshRate.Denominator),
            (90, 1)
        );
        assert!(!mode.Stereo.as_bool());
    }

    // The real thing: this machine's first adapter, its outputs reached through the hooked EnumOutputs, and both list
    // generations asked the way the engine asks (count, then the list). Skipped where there is no display.
    #[test]
    fn a_real_output_lists_the_forced_mode() {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, DXGI_ENUM_MODES, IDXGIFactory1, IDXGIOutput,
        };
        extern "C" fn width() -> u32 {
            4128
        }
        extern "C" fn height() -> u32 {
            2232
        }
        extern "C" fn window() -> u32 {
            0
        }
        crate::desktop_mirror::cvr_desktop_mirror_attach(width, height, window);
        // SAFETY: COM objects of this test; the hooks are installed through the registry like in the plugin.
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return;
            };
            let Ok(adapter) = factory.EnumAdapters1(0) else {
                return;
            };
            install_adapter(adapter.as_raw());
            let Ok(output): windows::core::Result<IDXGIOutput> = adapter.EnumOutputs(0) else {
                eprintln!("no output on this adapter: skipped");
                return;
            };
            let format = DXGI_FORMAT_R8G8B8A8_UNORM;
            let mut count = 0u32;
            output
                .GetDisplayModeList(format, DXGI_ENUM_MODES(0), &mut count, None)
                .unwrap();
            let mut modes = vec![DXGI_MODE_DESC::default(); count as usize];
            output
                .GetDisplayModeList(
                    format,
                    DXGI_ENUM_MODES(0),
                    &mut count,
                    Some(modes.as_mut_ptr()),
                )
                .unwrap();
            assert_eq!(count as usize, modes.len(), "the two calls agree");
            assert!(
                modes.iter().any(|m| m.size() == (4128, 2232)),
                "{} modes without the forced one",
                count
            );

            let output1: IDXGIOutput1 = output.cast().unwrap();
            let mut count1 = 0u32;
            output1
                .GetDisplayModeList1(format, DXGI_ENUM_MODES(0), &mut count1, None)
                .unwrap();
            let mut modes1 = vec![DXGI_MODE_DESC1::default(); count1 as usize];
            output1
                .GetDisplayModeList1(
                    format,
                    DXGI_ENUM_MODES(0),
                    &mut count1,
                    Some(modes1.as_mut_ptr()),
                )
                .unwrap();
            assert_eq!(count1 as usize, modes1.len());
            let forced = modes1
                .iter()
                .find(|m| m.size() == (4128, 2232))
                .expect("forced mode in list1");
            assert_eq!(
                (forced.RefreshRate.Numerator, forced.RefreshRate.Denominator),
                (90, 1)
            );
            // A buffer one short of the answer: MORE_DATA, never an overrun.
            let mut short = count - 1;
            let mut buffer = vec![DXGI_MODE_DESC::default(); short as usize];
            let error = output
                .GetDisplayModeList(
                    format,
                    DXGI_ENUM_MODES(0),
                    &mut short,
                    Some(buffer.as_mut_ptr()),
                )
                .unwrap_err();
            assert_eq!(error.code().0, DXGI_ERROR_MORE_DATA);
            assert_eq!(short, count);
        }
    }
}
