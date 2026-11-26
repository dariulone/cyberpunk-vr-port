//! A development probe for the menu's test tour (CVR_MENU_TOUR): an access violation in the game's executable is
//! written out with the real call stack (unwound from the exception's context) and the function the engine was
//! calling (RCX's CBaseFunction name) before the process goes on to the game's own crash handling.
use core::ffi::c_void;
use windows_sys::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};

static ONCE: std::sync::Once = std::sync::Once::new();
static FIRED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn own_base() -> u64 {
    let mut image = 0u64;
    // SAFETY: a lookup of one of our own functions' unwind data.
    unsafe { windows_sys::Win32::System::Diagnostics::Debug::RtlLookupFunctionEntry(own_base as *const () as usize as u64, &mut image, core::ptr::null_mut()) };
    image
}
fn name_at(p: u64) -> String {
    let mut word = [0u8; 8];
    if p == 0 || !cvr_engine::stack::read(p + 8, &mut word) {
        return String::new();
    }
    red4ext_abi::cname::CName(u64::from_le_bytes(word)).text().to_string()
}
unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: the OS's exception record and context for this thread.
    let (rec, ctx) = unsafe { (&*(*info).ExceptionRecord, &*(*info).ContextRecord) };
    if rec.ExceptionCode as u32 != 0xC000_0005 {
        return 0;
    }
    let exe = red4ext_abi::reloc::image_base() as u64;
    let rip = ctx.Rip;
    // an access violation in the executable (or the plugin), the first three
    let own0 = own_base();
    let ours = own0 != 0 && rip >= own0 && rip < own0 + 0x1000_0000;
    if !(ours || (rip >= exe && rip < exe + 0x800_0000)) || FIRED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 3 {
        return 0;
    }
    let own = own_base();
    cvr_host::log!(
        "[crash-probe] AV at {:#x} (exe+{:#x}); rcx {:#x} ({}), rdx {:#x}, r14 {:#x}, rbp {:#x}; exe {:#x}, plugin {:#x}\n",
        rip,
        rip.wrapping_sub(exe),
        ctx.Rcx,
        name_at(ctx.Rcx),
        ctx.Rdx,
        ctx.R14,
        ctx.Rbp,
        exe,
        own
    );
    // the frame the executor was given: its words, any that name a function
    for off in (0..0x60).step_by(8) {
        let mut w = [0u8; 8];
        if cvr_engine::stack::read(ctx.R14 + off, &mut w) {
            let v = u64::from_le_bytes(w);
            let n = if v > exe && v < exe + 0x600_0000 { String::new() } else { name_at(v) };
            cvr_host::log!("[crash-probe]   r14+{off:#x} = {v:#x} {n}\n");
        }
    }
    let mut c = *ctx;
    let mut i = 0;
    cvr_engine::stack::unwind_context(&mut c, 96, |pc, _start| {
        let (base, tag) = if pc >= own && own != 0 && pc < own + 0x1000_0000 { (own, "plugin") } else if pc >= exe && pc < exe + 0x800_0000 { (exe, "exe") } else { (0, "?") };
        cvr_host::log!("[crash-probe] #{i:02} {tag}+{:#x} ({pc:#x})\n", pc - base);
        i += 1;
    });
    cvr_host::log::flush();
    0
}
/// Installed once when the tour starts.
pub(super) fn install() {
    ONCE.call_once(|| {
        // SAFETY: a first-in-chain handler that only reads and logs.
        let h: *mut c_void = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
        cvr_host::log!("[crash-probe] installed {:?} (from Rust)\n", h);
    });
}
