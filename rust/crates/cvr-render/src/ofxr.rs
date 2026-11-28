use std::cell::OnceCell;
use std::mem::{size_of, size_of_val};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BridgeStatus {
    Absent,
    Loaded,
    Unverified,
}

/// The process-wide presenter caches the result of its first scan, including failure.
#[derive(Default)]
pub(crate) struct BridgeCheck(OnceCell<BridgeStatus>);

impl BridgeCheck {
    pub(crate) fn status(&self) -> BridgeStatus {
        self.check(scan_modules)
    }

    fn check(&self, scan: impl FnOnce() -> Option<bool>) -> BridgeStatus {
        *self.0.get_or_init(|| match scan() {
            Some(false) => BridgeStatus::Absent,
            Some(true) => BridgeStatus::Loaded,
            None => BridgeStatus::Unverified,
        })
    }
}

fn is_bridge_name(name: &[u16]) -> bool {
    let prefix = b"XR_APILAYER_XRFrameBridge";
    name.get(..prefix.len()).is_some_and(|start| {
        start.iter().zip(prefix).all(|(&unit, expected)| {
            u8::try_from(unit).is_ok_and(|byte| byte.eq_ignore_ascii_case(expected))
        })
    })
}

fn scan_modules() -> Option<bool> {
    // SAFETY: the pseudo-handle of the current process does not need to be closed.
    let process = unsafe { GetCurrentProcess() };
    let mut modules: Vec<HMODULE> = vec![std::ptr::null_mut(); 1024];

    // The first snapshot gives us the required size when the initial buffer is too small.
    for attempt in 0..2 {
        let capacity = u32::try_from(size_of_val(modules.as_slice())).ok()?;
        let mut needed = 0;
        // SAFETY: modules is writable for capacity bytes, and needed receives the required size.
        if unsafe { EnumProcessModules(process, modules.as_mut_ptr(), capacity, &mut needed) } == 0
        {
            return None;
        }
        let needed = needed as usize;
        let module_size = size_of::<HMODULE>();
        if needed == 0 || needed % module_size != 0 {
            return None;
        }
        let count = needed / module_size;
        if count > modules.len() {
            if attempt == 0 {
                modules.resize(count, std::ptr::null_mut());
            }
            continue;
        }

        let mut complete = true;
        let mut name = [0u16; 260];
        for &module in &modules[..count] {
            // SAFETY: module came from this process's snapshot and name holds name.len() UTF-16 units.
            let len = unsafe {
                GetModuleBaseNameW(process, module, name.as_mut_ptr(), name.len() as u32)
            };
            if len == 0 {
                complete = false;
            } else if is_bridge_name(&name[..len as usize]) {
                return Some(true);
            }
        }
        // A DLL can unload between the snapshot and the name lookup, leaving the result inconclusive.
        return complete.then_some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_once_for_every_outcome() {
        for (detected, expected) in [
            (Some(false), BridgeStatus::Absent),
            (Some(true), BridgeStatus::Loaded),
            (None, BridgeStatus::Unverified),
        ] {
            let check = BridgeCheck::default();
            assert_eq!(check.check(|| detected), expected);
            assert_eq!(check.check(|| panic!("rescanned")), expected);
        }
    }

    #[test]
    fn recognizes_the_bridge_prefix_case_insensitively() {
        let matches = |name: &str| is_bridge_name(&name.encode_utf16().collect::<Vec<_>>());
        assert!(matches("xr_apilayer_xrframebridge.dll"));
        assert!(matches("XR_APILAYER_XRFrameBridge64.dll"));
        assert!(!matches("XR_APILAYER_XRFrameBrid"));
        assert!(!matches("another_layer.dll"));
    }

    #[test]
    fn reads_modules_from_the_current_process() {
        assert_ne!(BridgeCheck::default().status(), BridgeStatus::Unverified);
    }
}
