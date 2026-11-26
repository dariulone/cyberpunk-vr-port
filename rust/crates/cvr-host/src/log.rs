//! The plugin's log: one file for every module (`log/<name>.log` in the plugin's folder), plain lines, flushed per
//! line -- every crash in the project was read from the last lines of it -- except under a burst: past 300 lines in
//! a second the writes stay buffered and the next window says so (src/Core/Log.cpp's rule, kept as it was: a
//! diagnostic in a hot path once made head tracking stutter through per-line flushes alone).
use crate::paths::{self, Folder};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

static FILE_NAME: OnceLock<String> = OnceLock::new();
static SINK: Mutex<Option<BufWriter<File>>> = Mutex::new(None);
static WINDOW_COUNT: AtomicU32 = AtomicU32::new(0);
static WINDOW_START: AtomicU64 = AtomicU64::new(0);

/// The log file's name in log/ (default `cyberpunkvrport.log`); set before the first line or not at all.
pub fn set_file_name(name: &str) {
    let _ = FILE_NAME.set(name.to_string());
}

fn open() -> Option<BufWriter<File>> {
    let name = FILE_NAME.get().map(String::as_str).unwrap_or("cyberpunkvrport.log");
    let path = paths::file(Folder::Log, name);
    // the previous launch's log kept beside it (<name>.prev.log): a game that died without a crash report (an abort
    // leaves none) keeps its last lines through the next start
    if path.exists() {
        let previous = path.with_extension("prev.log");
        let _ = std::fs::remove_file(&previous);
        let _ = std::fs::rename(&path, &previous);
    }
    // "w" with _SH_DENYNO: truncated per launch, readable and writable by others while open.
    let file = OpenOptions::new().write(true).create(true).truncate(true).open(path).ok()?;
    Some(BufWriter::with_capacity(64 * 1024, file))
}

/// Writes `text` as it is (callers end their lines with '\n', as the C++ `Log` format strings do).
pub fn write(text: &str) {
    let Ok(mut sink) = SINK.lock() else { return };
    if sink.is_none() {
        *sink = open();
    }
    let Some(out) = sink.as_mut() else { return };
    // SAFETY: plain Win32 call.
    let now = unsafe { GetTickCount64() };
    let start = WINDOW_START.load(Ordering::Relaxed);
    if now.wrapping_sub(start) >= 1000 {
        let burst = WINDOW_COUNT.swap(0, Ordering::Relaxed);
        WINDOW_START.store(now, Ordering::Relaxed);
        if burst > 300 {
            let _ = write!(
                out,
                "Log: {burst} lines in the previous second -- flush coalesced. Something is logging from a hot path.\n"
            );
        }
    }
    // CVR_LOG_QPC (tests only): each entry headed by the QPC clock in seconds, to line the log up with frame traces
    if crate::hitch::log_qpc() {
        let _ = write!(out, "{{{:.4}}} ", crate::hitch::now_s());
    }
    let _ = out.write_all(text.as_bytes());
    if WINDOW_COUNT.fetch_add(1, Ordering::Relaxed) < 300 {
        let _ = out.flush();
    }
}

/// Everything buffered out to the file (at unload).
pub fn flush() {
    if let Ok(mut sink) = SINK.lock() {
        if let Some(out) = sink.as_mut() {
            let _ = out.flush();
        }
    }
}

/// `log!("[area] text {}\n", x)`: formats and writes one entry to the plugin's log.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log::write(&::std::format!($($arg)*))
    };
}

/// The launcher's DEBUG tick-box, as the debug gate last applied it.
static DEBUG_ON: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
pub(crate) fn set_debug(on: bool) {
    DEBUG_ON.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// The verbose diagnostics (the launcher's DEBUG, or CVR_LOG_VERBOSE=1 in the environment): the lines written on
/// every state change or every few seconds -- the hands' contacts, the pass statistics, the presents -- that a
/// release log leaves out. Startup, failures and one-time lines are always written.
pub fn verbose() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();
    DEBUG_ON.load(core::sync::atomic::Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var("CVR_LOG_VERBOSE").is_ok_and(|v| v == "1"))
}

/// `vlog!(...)`: a `log!` line written only with the verbose diagnostics on (the format is not even built otherwise).
#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        if $crate::log::verbose() {
            $crate::log::write(&::std::format!($($arg)*))
        }
    };
}
