//! THE PORT'S INPUT BINDINGS IN THE GAME'S INPUT CONFIGURATION (formerly src/Core/InputConfig.cpp; the XML merge is
//! cvr_settings::xml_merge, byte for byte pugixml's).
//!
//! With Input Loader installed, the port's files are handed to it (r6\input) and it merges them. Without it, the
//! game's r6\config inputContexts.xml and inputUserMappings.xml are merged with the port's config\input\*.xml into
//! config\input\merged, and the engine's Player/Input InputContextFile / InputMappingFile config variables are pointed
//! at them (relative to r6\, like their defaults) before the engine reads them.
use std::path::{Path, PathBuf};

use cvr_engine::code;
use cvr_host::log;
use cvr_host::paths::{self, Folder};
use cvr_settings::xml_merge::{self, relative, Element};

const CONFIG_STRING_VAR_VTABLE_RVA: usize = 0x31D8940;
const INPUT_CONTEXT_FILE_VAR_RVA: usize = 0x3C29DA0;
const INPUT_MAPPING_FILE_VAR_RVA: usize = 0x3C29CE0;

/// <game>\ from <game>\bin\x64\Cyberpunk2077.exe.
fn game_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.parent()?.to_path_buf())
}
fn port_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(paths::dir(Folder::Config).join("input"))
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}
fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Byte for byte the same bindings.
fn same_bindings(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(a), Ok(b)) if a == b)
}

/// RetireInputLoaderCopies: a desktop launch takes back the copies of the port's bindings handed to Input Loader
/// (r6\input), the ones still byte for byte the port's; the desktop game plays with the game's own bindings.
pub fn retire_input_loader_copies() {
    let Some(root) = game_root() else { return };
    for source in port_files() {
        let copy = root.join("r6").join("input").join(source.file_name().unwrap_or_default());
        let file = std::fs::symlink_metadata(&copy).is_ok_and(|m| m.file_type().is_file());
        if file && same_bindings(&source, &copy) {
            if let Err(error) = std::fs::remove_file(&copy) {
                log!("[input] Could not retire the port's Input Loader copy {}: {}\n", copy.display(), error);
            }
        }
    }
}

/// Input Loader merges r6\input: the port's files are put there, as they are, and the variables are left to it.
fn hand_to_input_loader(root: &Path, files: &[PathBuf]) {
    let target = root.join("r6").join("input");
    let _ = std::fs::create_dir_all(&target);
    for file in files {
        let to = target.join(file.file_name().unwrap_or_default());
        if same_bindings(file, &to) {
            continue;
        }
        let failed = std::fs::copy(file, &to).is_err();
        log!(
            "[input] {} -> {} for Input Loader{}\n",
            name_of(file),
            to.display(),
            if failed { " FAILED" } else { "" }
        );
    }
}

/// One port file into the two documents. Returns the nodes merged.
fn merge(file: &Path, contexts: &mut Element, mappings: &mut Element) -> usize {
    let parsed = std::fs::read(file)
        .map_err(|_| "File was not found")
        .and_then(|t| xml_merge::parse(&t));
    let bindings = match &parsed {
        Ok(top) => top.iter().find(|e| e.name == "bindings"),
        Err(_) => None,
    };
    let Some(bindings) = bindings else {
        log!(
            "[input] {}: not a <bindings> document ({})\n",
            name_of(file),
            parsed.as_ref().err().copied().unwrap_or("No error")
        );
        return 0;
    };
    let mut skipped = Vec::new();
    let merged = xml_merge::merge(bindings, contexts, mappings, &mut skipped);
    for name in skipped {
        log!(
            "[input] {}: <{}> is no input binding, skipped\n",
            name_of(file),
            name
        );
    }
    merged
}

/// A string config variable, checked to be the one named before its own setter (vtable slot 3) is called.
fn set_variable(rva: usize, name: &str, value: &str) -> bool {
    let base = code::image_base();
    let variable = base + rva;
    let known = (|| {
        let vtable = code::read::<usize>(variable)?;
        let variable_name = code::read::<usize>(variable + 8)?;
        let group = code::read::<usize>(variable + 16)?;
        if vtable != base + CONFIG_STRING_VAR_VTABLE_RVA || variable_name == 0 || group == 0 {
            return None;
        }
        // SAFETY: the engine's static NUL-terminated names.
        let (n, g) = unsafe {
            (
                core::ffi::CStr::from_ptr(variable_name as *const core::ffi::c_char),
                core::ffi::CStr::from_ptr(group as *const core::ffi::c_char),
            )
        };
        (n.to_bytes() == name.as_bytes() && g.to_bytes() == b"Player/Input").then_some(vtable)
    })();
    let Some(vtable) = known else {
        log!(
            "[input] config variable {} not where this build keeps it: left alone\n",
            name
        );
        return false;
    };
    #[repr(C)]
    struct StringView {
        text: *const u8,
        length: u32,
    }
    let text = std::ffi::CString::new(value).unwrap_or_default();
    let view = StringView {
        text: text.as_ptr() as *const u8,
        length: value.len() as u32,
    };
    // SAFETY: the variable's own SetFromString (vtable +0x18), checked above to be the named string variable.
    let ok = unsafe {
        let set = *((vtable as *const usize).add(3));
        code::guarded_call3(
            set,
            variable as *mut core::ffi::c_void,
            &view as *const StringView as *mut core::ffi::c_void,
            core::ptr::null_mut(),
        )
    }
    .unwrap_or(false);
    log!(
        "[input] Player/Input {} = {}{}\n",
        name,
        value,
        if ok { "" } else { " (REFUSED)" }
    );
    ok
}

/// Apply: the port's bindings into the game's input configuration (Input Loader's way when it is installed).
#[no_mangle]
pub extern "C" fn cvr_input_config_apply() {
    let started = std::time::Instant::now();
    let Some(root) = game_root() else { return };
    let files = port_files();
    if files.is_empty() {
        return;
    }
    let plugins = paths::dir(Folder::Root)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    if plugins
        .join("input_loader")
        .join("input_loader.dll")
        .exists()
    {
        log!("[input] Input Loader is installed: it merges the port's bindings (r6\\input)\n");
        hand_to_input_loader(&root, &files);
        return;
    }
    // Input Loader's ini, left behind by a removed Input Loader, would point both variables back at its stale merge:
    // it is set aside, not deleted.
    let orphan = root
        .join("engine")
        .join("config")
        .join("platform")
        .join("pc")
        .join("input_loader.ini");
    if orphan.exists() {
        let mut aside = orphan.clone().into_os_string();
        aside.push(".orphaned");
        let aside = PathBuf::from(aside);
        let failed = std::fs::rename(&orphan, &aside).is_err();
        log!(
            "[input] Input Loader is gone but its ini was left: moved to {}{}\n",
            name_of(&aside),
            if failed { " FAILED" } else { "" }
        );
    }
    let vanilla = root.join("r6").join("config");
    let read = |name: &str| {
        std::fs::read(vanilla.join(name))
            .ok()
            .and_then(|t| xml_merge::parse(&t).ok())
    };
    let (Some(mut contexts), Some(mut mappings)) =
        (read("inputContexts.xml"), read("inputUserMappings.xml"))
    else {
        log!("[input] the game's r6\\config input files could not be read: the port's bindings are not added\n");
        return;
    };
    let (Some(ci), Some(mi)) = (
        contexts.iter().position(|e| e.name == "bindings"),
        mappings.iter().position(|e| e.name == "bindings"),
    ) else {
        log!("[input] the game's r6\\config input files could not be read: the port's bindings are not added\n");
        return;
    };
    let mut merged = 0;
    for file in &files {
        let (c, m) = (&mut contexts[ci], &mut mappings[mi]);
        merged += merge(file, c, m);
    }
    // Written beside the port's own files; the variables take the path relative to r6\, like their defaults.
    let out = paths::dir(Folder::Config).join("input").join("merged");
    let _ = std::fs::create_dir_all(&out);
    let contexts_file = out.join("inputContexts.xml");
    let mappings_file = out.join("inputUserMappings.xml");
    if std::fs::write(&contexts_file, xml_merge::write(&contexts)).is_err()
        || std::fs::write(&mappings_file, xml_merge::write(&mappings)).is_err()
    {
        log!(
            "[input] could not write the merged input files to {}\n",
            out.display()
        );
        return;
    }
    let r6 = root.join("r6");
    let (Some(contexts_path), Some(mappings_path)) =
        (relative(&contexts_file, &r6), relative(&mappings_file, &r6))
    else {
        return;
    };
    log!(
        "[input] {} bindings from {} file(s) merged into the game's input configuration ({:.1} ms)\n",
        merged,
        files.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    set_variable(
        INPUT_CONTEXT_FILE_VAR_RVA,
        "InputContextFile",
        &contexts_path,
    );
    set_variable(
        INPUT_MAPPING_FILE_VAR_RVA,
        "InputMappingFile",
        &mappings_path,
    );
}
