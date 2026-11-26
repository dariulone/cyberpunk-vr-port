//! The OpenXR runtime asked for (formerly ConfigurePreferredOpenXRRuntime and its helpers in OpenXRManager.cpp): with
//! xr_runtime=1 the loader is pointed at SteamVR's manifest through XR_RUNTIME_JSON. The manifest is found from
//! %LOCALAPPDATA%\openvr\openvrpaths.vrpath (SteamVR's own runtime list, read directly), else openvr_api.dll's
//! VR_GetRuntimePath, else the Steam install in the registry. Elevated, the loader ignores the variable: said so.
//!
//! Paths are ANSI, as the C++ half used them (the A functions, a 2048-byte limit).
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_host::log;
use std::ffi::{CStr, CString};

/// Strips trailing slashes and joins with a backslash (JoinPath, TrimTrailingSlashes).
pub fn join_path(base: &str, suffix: &str) -> Option<String> {
    if base.is_empty() || suffix.is_empty() {
        return None;
    }
    let trimmed = base.trim_end_matches(['\\', '/']);
    let joined = format!("{trimmed}\\{suffix}");
    (joined.len() < 2048).then_some(joined)
}

/// The runtime roots listed under "runtime" in an openvrpaths.vrpath text (JSON strings, backslashes escaped), or
/// None when there is no list.
pub fn vrpath_runtime_roots(text: &str) -> Option<Vec<String>> {
    let bytes = text.as_bytes();
    let key = text.find("\"runtime\"")?;
    let open = key + text[key..].find('[')?;
    let close = open + text[open..].find(']')?;
    let mut roots = Vec::new();
    let mut at = open;
    loop {
        let Some(q) = text[at + 1..].find('"').map(|i| at + 1 + i) else { break };
        if q >= close {
            break;
        }
        let mut root = Vec::new();
        let mut i = q + 1;
        while i < close && bytes[i] != b'"' {
            if bytes[i] == b'\\' && i + 1 < close {
                i += 1;
            }
            root.push(bytes[i]);
            i += 1;
        }
        roots.push(String::from_utf8_lossy(&root).into_owned());
        at = i;
    }
    Some(roots)
}

#[cfg(not(test))]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, FreeLibrary, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel, TOKEN_MANDATORY_LABEL, TOKEN_QUERY};
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesA, FILE_ATTRIBUTE_DIRECTORY, INVALID_FILE_ATTRIBUTES};
    use windows_sys::Win32::System::Environment::{GetEnvironmentVariableA, SetEnvironmentVariableA};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameA, GetProcAddress, LoadLibraryA};
    use windows_sys::Win32::System::Registry::{RegGetValueA, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_SZ};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    fn c(s: &str) -> CString {
        CString::new(s).unwrap_or_default()
    }
    pub fn file_exists(path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        // SAFETY: a NUL-terminated path.
        let attrs = unsafe { GetFileAttributesA(c(path).as_ptr().cast()) };
        attrs != INVALID_FILE_ATTRIBUTES && attrs & FILE_ATTRIBUTE_DIRECTORY == 0
    }
    pub fn env(name: &str) -> Option<String> {
        let mut buf = vec![0u8; 2048];
        // SAFETY: a buffer of the size passed.
        let n = unsafe { GetEnvironmentVariableA(c(name).as_ptr().cast(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
        (n > 0 && n < buf.len()).then(|| String::from_utf8_lossy(&buf[..n]).into_owned())
    }
    fn registry_string(root: HKEY, sub_key: &str, value: &str) -> Option<String> {
        let mut buf = vec![0u8; 2048];
        let mut size = buf.len() as u32;
        let mut ty = 0u32;
        // SAFETY: NUL-terminated names; a buffer of the size passed.
        let status = unsafe {
            RegGetValueA(root, c(sub_key).as_ptr().cast(), c(value).as_ptr().cast(), RRF_RT_REG_SZ, &mut ty, buf.as_mut_ptr().cast(), &mut size)
        };
        if status != 0 || ty != REG_SZ || buf[0] == 0 {
            return None;
        }
        // SAFETY: NUL-terminated by the registry (the last byte forced as the C++ did).
        *buf.last_mut()? = 0;
        Some(unsafe { CStr::from_ptr(buf.as_ptr().cast()) }.to_string_lossy().into_owned())
    }

    /// TryGetSteamVRRuntimeJsonFromVrPath.
    pub fn from_vrpath() -> Option<String> {
        let local = env("LOCALAPPDATA").filter(|l| l.len() < 260)?;
        let vrpath = join_path(&local, "openvr\\openvrpaths.vrpath")?;
        let text = std::fs::read(&vrpath).ok()?;
        let text = String::from_utf8_lossy(&text);
        let Some(roots) = vrpath_runtime_roots(&text) else {
            log!("OpenXRManager: {} has no runtime list.\n", vrpath);
            return None;
        };
        for root in roots {
            let Some(json) = (!root.is_empty()).then(|| join_path(&root, "steamxr_win64.json")).flatten() else { continue };
            if file_exists(&json) {
                log!("OpenXRManager: SteamVR runtime resolved via openvrpaths.vrpath: \"{}\"\n", json);
                return Some(json);
            }
            log!("OpenXRManager: openvrpaths.vrpath names \"{}\", but it has no steamxr_win64.json.\n", root);
        }
        None
    }

    /// TryGetSteamVRRuntimeJsonFromOpenVR: the game's own openvr_api.dll first, then the search path's.
    pub fn from_openvr() -> Option<String> {
        // SAFETY (whole body): plain Win32 module calls; the export's documented signature.
        unsafe {
            let mut module = core::ptr::null_mut();
            let mut exe = vec![0u8; 260];
            let n = GetModuleFileNameA(core::ptr::null_mut(), exe.as_mut_ptr(), exe.len() as u32) as usize;
            if n > 0 {
                let path = String::from_utf8_lossy(&exe[..n]).into_owned();
                if let Some(dir) = path.rfind('\\').map(|i| &path[..i]) {
                    if let Some(local) = join_path(dir, "openvr_api.dll").filter(|p| file_exists(p)) {
                        module = LoadLibraryA(c(&local).as_ptr().cast());
                    }
                }
            }
            if module.is_null() {
                module = LoadLibraryA(c"openvr_api.dll".as_ptr().cast());
            }
            if module.is_null() {
                log!("OpenXRManager: SteamVR runtime request could not load openvr_api.dll. Falling back to registry lookup.\n");
                return None;
            }
            let Some(proc_addr) = GetProcAddress(module, c"VR_GetRuntimePath".as_ptr().cast()) else {
                log!("OpenXRManager: openvr_api.dll loaded but VR_GetRuntimePath export is missing.\n");
                FreeLibrary(module);
                return None;
            };
            let get_runtime_path: unsafe extern "C" fn(*mut u8, u32, *mut i32) -> bool = core::mem::transmute(proc_addr);
            let mut root = vec![0u8; 2048];
            let mut error = 0i32;
            let ok = get_runtime_path(root.as_mut_ptr(), root.len() as u32, &mut error);
            FreeLibrary(module);
            if !ok || root[0] == 0 {
                log!("OpenXRManager: VR_GetRuntimePath failed (error={}).\n", error);
                return None;
            }
            let root = CStr::from_ptr(root.as_ptr().cast()).to_string_lossy().into_owned();
            let json = join_path(&root, "steamxr_win64.json")?;
            if !file_exists(&json) {
                log!("OpenXRManager: SteamVR runtime root found via openvr_api.dll, but steamxr_win64.json is missing at \"{}\".\n", json);
                return None;
            }
            log!("OpenXRManager: SteamVR runtime resolved via openvr_api.dll: \"{}\"\n", json);
            Some(json)
        }
    }

    /// TryGetSteamVRRuntimeJsonFromRegistry: the Steam install, then SteamVR under it.
    pub fn from_registry() -> Option<String> {
        let steam = registry_string(HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath")
            .or_else(|| registry_string(HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"))
            .or_else(|| registry_string(HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam", "InstallPath"))?;
        let json = join_path(&steam, "steamapps\\common\\SteamVR\\steamxr_win64.json")?;
        if !file_exists(&json) {
            log!("OpenXRManager: Steam install found, but SteamVR OpenXR manifest is missing at \"{}\".\n", json);
            return None;
        }
        log!("OpenXRManager: SteamVR runtime resolved via Steam install: \"{}\"\n", json);
        Some(json)
    }

    /// The process runs at high integrity (as administrator).
    pub fn elevated() -> bool {
        // SAFETY: the process's own token, queried into a local buffer, closed.
        unsafe {
            let mut token: HANDLE = core::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut buffer = [0u64; 8];
            let mut size = 0u32;
            let mut high = false;
            if GetTokenInformation(token, TokenIntegrityLevel, buffer.as_mut_ptr().cast(), core::mem::size_of_val(&buffer) as u32, &mut size) != 0 {
                let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
                let count = *GetSidSubAuthorityCount(label.Label.Sid) as u32;
                let level = *GetSidSubAuthority(label.Label.Sid, count - 1);
                high = level >= 0x3000; // SECURITY_MANDATORY_HIGH_RID
            }
            CloseHandle(token);
            high
        }
    }
    pub fn set_runtime_json(json: &str) -> Result<(), u32> {
        // SAFETY: NUL-terminated name and value.
        if unsafe { SetEnvironmentVariableA(c"XR_RUNTIME_JSON".as_ptr().cast(), c(json).as_ptr().cast()) } == 0 {
            // SAFETY: the calling thread's last error.
            return Err(unsafe { GetLastError() });
        }
        Ok(())
    }
}

#[cfg(not(test))]
extern "C" {
    fn GetXrRuntimeMode() -> i32;
}

/// ConfigurePreferredOpenXRRuntime: with xr_runtime=1, XR_RUNTIME_JSON set to SteamVR's manifest before the instance.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_configure_preferred_runtime() {
    // SAFETY: a plain C getter.
    if unsafe { GetXrRuntimeMode() } != 1 {
        return;
    }
    let Some(json) = win::from_vrpath().or_else(win::from_openvr).or_else(win::from_registry) else {
        log!("OpenXRManager: xr_runtime=1 requested SteamVR, but no SteamVR OpenXR runtime manifest was found. Using system default runtime.\n");
        return;
    };
    if win::env("XR_RUNTIME_JSON").as_deref() == Some(json.as_str()) {
        log!("OpenXRManager: XR_RUNTIME_JSON already points to SteamVR: \"{}\"\n", json);
        return;
    }
    if let Err(gle) = win::set_runtime_json(&json) {
        log!("OpenXRManager: Failed to set XR_RUNTIME_JSON to SteamVR manifest \"{}\" (gle={}).\n", json, gle);
        return;
    }
    log!("OpenXRManager: xr_runtime=1 forcing SteamVR OpenXR runtime via XR_RUNTIME_JSON=\"{}\"\n", json);
    if win::elevated() {
        log!(
            "OpenXRManager: WARNING the game runs as administrator: the OpenXR loader ignores XR_RUNTIME_JSON there, so xr_runtime=1 cannot pick SteamVR and the system's OpenXR runtime is used. Start the game without administrator rights, or make SteamVR the system OpenXR runtime.\n"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_runtime_list_is_read_with_its_escapes() {
        let text = r#"{ "config" : [ "C:\\Steam\\config" ], "runtime" : [ "C:\\Program Files (x86)\\Steam\\steamapps\\common\\SteamVR", "D:\\Other" ], "version" : 1 }"#;
        let roots = vrpath_runtime_roots(text).unwrap();
        assert_eq!(roots, vec!["C:\\Program Files (x86)\\Steam\\steamapps\\common\\SteamVR".to_string(), "D:\\Other".to_string()]);
        assert!(vrpath_runtime_roots("{}").is_none());
        assert_eq!(join_path("C:\\a\\\\", "b.json").as_deref(), Some("C:\\a\\b.json"));
        assert_eq!(join_path("", "b"), None);
    }
}
