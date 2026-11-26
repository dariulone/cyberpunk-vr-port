//! Nothing unwinds into the engine. Every function the game or RED4ext calls runs its body under `guard`, which turns
//! a Rust panic into a log line and the caller's fallback value. (A Windows SEH fault in engine memory is a different
//! thing: `catch_unwind` does not see it, and the port's guarded reads keep their native adapter until a Rust path is
//! proven -- docs/rust-migration-plan.md, "Хуки и ошибки доступа".)
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Once;

static HOOK: Once = Once::new();

/// Panics go to the plugin's log, with their location, instead of stderr (which the game does not have).
pub fn install_panic_log() {
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "(non-string panic)".into());
            let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            crate::log::write(&format!("[panic] {message} at {location}\n"));
            crate::log::flush();
            // and appended to log/panic.txt, which no launch truncates: a panic where nothing catches it (a native the
            // VM calls) ends the game with no crash report, and the next start would overwrite the log
            let path = crate::paths::file(crate::paths::Folder::Log, "panic.txt");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                use std::io::Write;
                let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                let thread = std::thread::current().name().map(str::to_string).unwrap_or_else(|| format!("{:?}", std::thread::current().id()));
                let trace = std::backtrace::Backtrace::force_capture();
                let _ = writeln!(f, "[{when}] [{thread}] {message} at {location}\n{trace}\n");
            }
        }));
    });
}

/// Runs `body`; a panic is logged (by the hook) and `fallback` returned. `what` names the entry point in the log.
pub fn guard<R>(what: &str, fallback: R, body: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            crate::log::write(&format!("[panic] {what}: caught at the engine boundary, fallback returned\n"));
            fallback
        }
    }
}
