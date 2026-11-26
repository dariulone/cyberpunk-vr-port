//! The stereo families' development package (src/Render/NativeStereoFamilies.cpp, Load, Binary and Hash): the manifest
//! (schema 1, purpose development-linkage-only, 1..=512 families) names, per original shader pair, its native vertex
//! and pixel shader identities (SHA-256 and size), the replacement DXIL beside the manifest (`<sha256>.dxil`, its size
//! and hash checked), the paired bindings and the resource uses (their contracts are stereo_family_contracts'), and
//! optionally a native camera variant -- the one vertex-only family's (`nativeCameraVs`) or a per-stage ABI
//! (`nativeCamera`: schema 1, the main eye first, a 1024-byte eye stride and a 2048-byte binding, its stages matching
//! the uses' camera bindings and the shaders' cameraPatched flags). The shaders total at most 128 MiB; a family's pair
//! of identities appears once. SHA-256 is BCrypt's, as the .cpp's was.
//!
//! jsoncpp reads the manifest (the .cpp's reader, as configured before) and lends its tree flattened (jsoncpp_tree),
//! with jsoncpp's semantics kept: a member of what is neither an object nor null is jsoncpp's LogicError, a number is
//! isUInt (integral reals too), a string is asString. Each failure is the .cpp's message (the file system's own errors
//! are rendered by Rust).
//!
//! The .cpp's Load parses and forwards to `cvr_nsf_load` and copies the assets into its own map; its Hash forwards to
//! `cvr_nsf_sha256`. The unit tests below check the hash, the numbers and a package on disk.
use crate::jsoncpp_tree::{Json, JsonNode};
use crate::stereo_family_contracts::{digest, pair_valid, pairs_unique, stage_cameras, use_valid, PairedBinding, ResourceUse};
use std::path::Path;

pub const HASH_LIMIT: usize = 8 * 1024 * 1024;
pub const SHADER_LIMIT: u32 = 1024 * 1024;
pub const PACKAGE_LIMIT: u64 = 128 * 1024 * 1024;
/// The vertex-only family with a native camera variant of its vertex shader.
const NATIVE_CAMERA_VS: &str = "17f7e646a02eeaa6ea496dd75a21a494588b29dd926a545fb52bee45dec68117";

/// One family's assets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asset {
    pub index: u32,
    pub vs_bytes: u32,
    pub ps_bytes: u32,
    pub camera_stages: u32,
    pub vs_hash: Vec<u8>,
    pub ps_hash: Vec<u8>,
    pub vs: Vec<u8>,
    pub ps: Vec<u8>,
    pub camera_vs: Vec<u8>,
    pub camera_ps: Vec<u8>,
    pub pairs: Vec<PairedBinding>,
    pub uses: Vec<ResourceUse>,
}

type Outcome<T> = Result<T, String>;
fn require(value: bool, message: &str) -> Outcome<()> {
    if value { Ok(()) } else { Err(message.to_string()) }
}
fn logic<T>(result: Result<T, &'static str>) -> Outcome<T> {
    result.map_err(str::to_string)
}

/// Hash: lowercase hex SHA-256 of 1..=8 MiB.
pub fn sha256_hex(data: &[u8]) -> Outcome<String> {
    use std::sync::OnceLock;
    use windows_sys::Win32::Security::Cryptography::{BCryptHash, BCryptOpenAlgorithmProvider, BCRYPT_SHA256_ALGORITHM};
    require(!data.is_empty() && data.len() <= HASH_LIMIT, "Invalid shader/root/manifest size")?;
    static PROVIDER: OnceLock<usize> = OnceLock::new();
    let provider = *PROVIDER.get_or_init(|| {
        let mut handle = core::ptr::null_mut();
        // SAFETY: an out-parameter; the provider lives as long as the process, as the .cpp's static did.
        let status = unsafe { BCryptOpenAlgorithmProvider(&mut handle, BCRYPT_SHA256_ALGORITHM, core::ptr::null(), 0) };
        if status < 0 { 0 } else { handle as usize }
    });
    require(provider != 0, "SHA256 provider unavailable")?;
    let mut out = [0u8; 32];
    // SAFETY: the provider's handle; the input and the 32-byte output are this function's.
    let status = unsafe {
        BCryptHash(provider as *mut core::ffi::c_void, core::ptr::null(), 0, data.as_ptr(), data.len() as u32, out.as_mut_ptr(), 32)
    };
    require(status >= 0, "SHA256 failed")?;
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}
/// Number: present, isUInt and at most `limit`.
fn number(value: &Json, name: &str, limit: u32) -> Outcome<u32> {
    let present = logic(value.is_member(name))?;
    let found = if present { logic(value.find(name))?.as_uint() } else { None };
    match found {
        Some(v) if v <= limit => Ok(v),
        _ => Err("Invalid manifest number".to_string()),
    }
}
fn member<'a>(value: &'a Json, name: &str) -> Outcome<&'a Json> {
    logic(value.find(name))
}
fn string(value: &Json, name: &str) -> Outcome<Vec<u8>> {
    logic(member(value, name)?.as_string())
}
/// Binary: `<sha256>.dxil` beside the manifest, of its stated size and hash.
fn binary(dir: &Path, value: &Json) -> Outcome<Vec<u8>> {
    use std::io::Read;
    let hash = string(value, "sha256")?;
    let name = string(value, "file")?;
    let size = number(value, "bytes", SHADER_LIMIT)?;
    let expected = [hash.as_slice(), b".dxil"].concat();
    require(size >= 32 && digest(&hash) && name == expected, "Invalid shader filename/hash")?;
    let path = dir.join(String::from_utf8_lossy(&name).as_ref());
    let length = std::fs::metadata(&path).map_err(|e| format!("file_size: {e}: \"{}\"", path.display()))?.len();
    require(length == u64::from(size), "Shader file size differs")?;
    let mut bytes = vec![0u8; size as usize];
    let read = std::fs::File::open(&path).and_then(|mut f| f.read_exact(&mut bytes)).is_ok();
    require(read && sha256_hex(&bytes)?.as_bytes() == hash.as_slice(), "Shader file hash differs")?;
    Ok(bytes)
}
/// Load, after jsoncpp has read the manifest's text into `manifest`: the families' assets.
pub fn load(manifest: &Json, dir: &Path) -> Outcome<Vec<Asset>> {
    let schema = number(manifest, "schema", u32::MAX)?;
    let supported = schema == 1 && member(manifest, "purpose")?.is_text("development-linkage-only") && {
        let families = member(manifest, "families")?.elements();
        families.is_some_and(|f| !f.is_empty() && f.len() <= 512)
    };
    require(supported, "Unsupported family manifest")?;
    let families = member(manifest, "families")?.elements().unwrap_or_default();
    let mut assets: Vec<Asset> = Vec::new();
    let mut bytes = 0u64;
    for row in families {
        let mut a = Asset { index: assets.len() as u32 + 1, ..Asset::default() };
        a.vs_hash = string(row, "nativeVs")?;
        a.ps_hash = string(row, "nativePs")?;
        a.vs_bytes = number(row, "nativeVsBytes", SHADER_LIMIT)?;
        a.ps_bytes = number(row, "nativePsBytes", SHADER_LIMIT)?;
        require(
            digest(&a.vs_hash)
                && a.vs_bytes >= 32
                && ((a.ps_hash.is_empty() && a.ps_bytes == 0) || (digest(&a.ps_hash) && a.ps_bytes >= 32)),
            "Invalid original shader identity",
        )?;
        a.vs = binary(dir, member(row, "vs")?)?;
        if a.ps_bytes != 0 {
            a.ps = binary(dir, member(row, "ps")?)?;
        }
        bytes += (a.vs.len() + a.ps.len()) as u64;
        require(bytes <= PACKAGE_LIMIT, "Shader package exceeds memory bound")?;
        let pairs = member(row, "pairs")?.elements();
        let uses = member(row, "uses")?.elements();
        require(
            pairs.is_some_and(|p| p.len() <= 512) && uses.is_some_and(|u| u.len() <= 1024),
            "Invalid resource contract",
        )?;
        let pixel = a.ps_bytes != 0;
        for p in pairs.unwrap_or_default() {
            let binding = PairedBinding {
                kind: number(p, "type", 3)? as i32,
                visibility: number(p, "visibility", 5)? as i32,
                source_register: number(p, "register", 0x7fff_fffe)?,
                private_space: number(p, "space", 65535)?,
            };
            require(pair_valid(&binding, pixel), "Invalid pair contract")?;
            a.pairs.push(binding);
        }
        for u in uses.unwrap_or_default() {
            let usage = ResourceUse {
                kind: number(u, "type", 3)?,
                visibility: number(u, "visibility", 5)?,
                space: number(u, "space", u32::MAX)?,
                shader_register: number(u, "register", u32::MAX)?,
                count: number(u, "count", u32::MAX)?,
                bytes: number(u, "bytes", 65536)?,
            };
            require(use_valid(&usage, pixel), "Invalid resource use")?;
            a.uses.push(usage);
        }
        require(pairs_unique(&a.pairs, &a.uses), "Pair has no unique original resource")?;
        if logic(row.is_member("nativeCameraVs"))? {
            let camera = member(row, "nativeCameraVs")?;
            let family = a.vs_hash == NATIVE_CAMERA_VS.as_bytes()
                && a.ps_bytes == 0
                && a.uses.len() == 2
                && member(camera, "mainFirst")?.as_bool() == Some(true)
                && number(camera, "eyeStride", u32::MAX)? == 1024;
            require(family, "Unsupported native camera family")?;
            require(!logic(row.is_member("nativeCamera"))?, "Ambiguous native camera contract")?;
            a.camera_vs = binary(dir, camera)?;
            a.camera_stages = 1;
        } else if logic(row.is_member("nativeCamera"))? {
            let camera = member(row, "nativeCamera")?;
            let abi = number(camera, "schema", u32::MAX)? == 1
                && member(camera, "mainFirst")?.as_bool() == Some(true)
                && number(camera, "eyeStride", u32::MAX)? == 1024
                && number(camera, "bindingSize", u32::MAX)? == 2048;
            require(abi, "Unsupported native camera ABI")?;
            a.camera_stages = number(camera, "stages", 3)?;
            require(a.camera_stages != 0, "Native camera stages missing")?;
            let expected = stage_cameras(&a.uses);
            require(expected.is_some(), "Unsupported native stage camera binding")?;
            let expected = expected.unwrap_or_default();
            // camera["vs"] is asked only once the stages match (the .cpp's &&).
            let vertex = expected == a.camera_stages
                && member(member(camera, "vs")?, "cameraPatched")?.as_bool() == Some(expected & 1 != 0);
            require(vertex, "Native vertex camera contract differs")?;
            a.camera_vs = binary(dir, member(camera, "vs")?)?;
            if a.ps_bytes != 0 {
                let ps = member(camera, "ps")?;
                let patched = member(ps, "cameraPatched")?.as_bool();
                require(patched == Some(expected & 2 != 0), "Native pixel camera contract differs")?;
                a.camera_ps = binary(dir, ps)?;
            } else {
                require(!logic(camera.is_member("ps"))? && expected & 2 == 0, "Unexpected native camera pixel shader")?;
            }
        }
        bytes += (a.camera_vs.len() + a.camera_ps.len()) as u64;
        require(bytes <= PACKAGE_LIMIT, "Camera shader package exceeds memory bound")?;
        let duplicate = assets.iter().any(|b| b.vs_hash == a.vs_hash && b.ps_hash == a.ps_hash);
        require(!duplicate, "Duplicate family")?;
        assets.push(a);
    }
    Ok(assets)
}

// ---- the flat views the C++ half reads ------------------------------------------------------------------------------

/// A byte array the C++ half copies.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FamilyBytes {
    pub p: *const u8,
    pub n: usize,
}
#[repr(C)]
pub struct FamilyAssetView {
    pub index: u32,
    pub vs_bytes: u32,
    pub ps_bytes: u32,
    pub camera_stages: u32,
    pub vs_hash: FamilyBytes,
    pub ps_hash: FamilyBytes,
    pub vs: FamilyBytes,
    pub ps: FamilyBytes,
    pub camera_vs: FamilyBytes,
    pub camera_ps: FamilyBytes,
    pub pairs: *const PairedBinding,
    pub pair_count: usize,
    pub uses: *const ResourceUse,
    pub use_count: usize,
}
#[repr(C)]
pub struct FamilyManifestView {
    pub assets: *const FamilyAssetView,
    pub asset_count: usize,
    /// The manifest text's SHA-256, NUL-terminated.
    pub manifest_hash: [u8; 65],
}
const _: () = assert!(
    core::mem::size_of::<FamilyAssetView>() == 144 && core::mem::size_of::<FamilyManifestView>() == 88,
    "mirrored by NativeStereoFamilies.cpp"
);
struct ManifestHolder {
    _assets: Vec<Asset>,
    _views: Vec<FamilyAssetView>,
    view: FamilyManifestView,
}
fn lend(bytes: &[u8]) -> FamilyBytes {
    FamilyBytes { p: bytes.as_ptr(), n: bytes.len() }
}
fn hold(assets: Vec<Asset>, hash: &str) -> Box<ManifestHolder> {
    let views: Vec<FamilyAssetView> = assets
        .iter()
        .map(|a| FamilyAssetView {
            index: a.index,
            vs_bytes: a.vs_bytes,
            ps_bytes: a.ps_bytes,
            camera_stages: a.camera_stages,
            vs_hash: lend(&a.vs_hash),
            ps_hash: lend(&a.ps_hash),
            vs: lend(&a.vs),
            ps: lend(&a.ps),
            camera_vs: lend(&a.camera_vs),
            camera_ps: lend(&a.camera_ps),
            pairs: a.pairs.as_ptr(),
            pair_count: a.pairs.len(),
            uses: a.uses.as_ptr(),
            use_count: a.uses.len(),
        })
        .collect();
    let mut manifest_hash = [0u8; 65];
    manifest_hash[..hash.len().min(64)].copy_from_slice(&hash.as_bytes()[..hash.len().min(64)]);
    // Moving the vectors into the holder keeps their heap buffers, which the views point at.
    let view = FamilyManifestView { assets: views.as_ptr(), asset_count: views.len(), manifest_hash };
    Box::new(ManifestHolder { _assets: assets, _views: views, view })
}
/// The message into the caller's buffer, NUL-terminated and truncated to it.
fn report(error: *mut u8, capacity: usize, message: &str) {
    if error.is_null() || capacity == 0 {
        return;
    }
    let n = message.len().min(capacity - 1);
    // SAFETY: the caller's buffer of `capacity` bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(message.as_ptr(), error, n);
        *error.add(n) = 0;
    }
}
/// # Safety
/// `ptr` is readable for `n` items (or `n` is 0).
unsafe fn items<'a, T>(ptr: *const T, n: usize) -> &'a [T] {
    if n == 0 || ptr.is_null() {
        return &[];
    }
    // SAFETY: as the caller vouches.
    unsafe { core::slice::from_raw_parts(ptr, n) }
}

/// Load after the parse: the assets' view in `out` and a handle for cvr_nsf_manifest_free, or null with the message in
/// `error`. `nodes[0]` is the manifest's root; `dir` the manifest's folder (UTF-16); `text` the manifest's bytes.
///
/// # Safety
/// The nodes (with their strings), `dir` and `text` are readable for their counts; `out` and `error` (for `capacity`)
/// are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nsf_load(
    nodes: *const JsonNode,
    count: usize,
    dir: *const u16,
    dir_length: usize,
    text: *const u8,
    text_length: usize,
    out: *mut *const FamilyManifestView,
    error: *mut u8,
    capacity: usize,
) -> *mut core::ffi::c_void {
    use std::os::windows::ffi::OsStringExt;
    // SAFETY: as the caller vouches.
    let (nodes, dir, text) = unsafe { (items(nodes, count), items(dir, dir_length), items(text, text_length)) };
    let Some(manifest) = (unsafe { Json::from_nodes(nodes, 0) }) else {
        report(error, capacity, "Invalid family manifest JSON");
        return core::ptr::null_mut();
    };
    let folder = std::path::PathBuf::from(std::ffi::OsString::from_wide(dir));
    let loaded = load(&manifest, &folder).and_then(|assets| Ok((assets, sha256_hex(text)?)));
    match loaded {
        Ok((assets, hash)) => {
            let holder = hold(assets, &hash);
            // SAFETY: as the caller vouches.
            unsafe { *out = &holder.view };
            Box::into_raw(holder).cast()
        }
        Err(message) => {
            report(error, capacity, &message);
            core::ptr::null_mut()
        }
    }
}
/// # Safety
/// `handle` is null or one cvr_nsf_load returned, freed once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_manifest_free(handle: *mut core::ffi::c_void) {
    if !handle.is_null() {
        // SAFETY: as the caller vouches.
        drop(unsafe { Box::from_raw(handle.cast::<ManifestHolder>()) });
    }
}
/// Hash: the 64 hex digits and a NUL into `out`, or false with the message in `error`.
///
/// # Safety
/// `data` is readable for `size` (or null); `out` holds 65 bytes; `error` holds `capacity`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf_sha256(data: *const u8, size: usize, out: *mut u8, error: *mut u8, capacity: usize) -> bool {
    // A null pointer is the .cpp's empty input.
    // SAFETY: as the caller vouches.
    let input = if data.is_null() { &[][..] } else { unsafe { items(data, size) } };
    match sha256_hex(input) {
        Ok(hash) => {
            // SAFETY: the caller's 65 bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(hash.as_ptr(), out, 64);
                *out.add(64) = 0;
            }
            true
        }
        Err(message) => {
            report(error, capacity, &message);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    //! The hash, the numbers and a package on disk.
    use super::*;
    use serde_json::{json, Value};

    fn tree(value: &Value) -> Json {
        match value {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::Number(n) if n.is_f64() => match n.as_f64() {
                Some(f) if f >= 0.0 && f <= f64::from(u32::MAX) && f.fract() == 0.0 => Json::RealUInt(f as u32),
                _ => Json::Real,
            },
            Value::Number(n) => n.as_u64().and_then(|u| u32::try_from(u).ok()).map_or(Json::Integer, Json::UInt),
            Value::String(s) => Json::Str(s.as_bytes().to_vec()),
            Value::Array(items) => Json::Array(items.iter().map(tree).collect()),
            Value::Object(members) => Json::Object(members.iter().map(|(k, v)| (k.as_bytes().to_vec(), tree(v))).collect()),
        }
    }
    /// A package folder with one vertex-only family and its shaders.
    struct Package {
        dir: std::path::PathBuf,
    }
    impl Drop for Package {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    fn shader(dir: &Path, seed: u8) -> Value {
        let bytes: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(7).wrapping_add(seed)).collect();
        let hash = sha256_hex(&bytes).unwrap();
        std::fs::write(dir.join(format!("{hash}.dxil")), &bytes).unwrap();
        json!({"sha256": hash, "file": format!("{hash}.dxil"), "bytes": 64})
    }
    fn package(name: &str) -> (Package, Value) {
        let dir = std::env::temp_dir().join(format!("cvr_family_manifest_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vs = shader(&dir, 1);
        let manifest = json!({"schema": 1, "purpose": "development-linkage-only", "families": [{
            "nativeVs": "a".repeat(64), "nativeVsBytes": 4096, "nativePsBytes": 0, "vs": vs,
            "pairs": [{"type": 2, "visibility": 1, "register": 5, "space": 7}],
            "uses": [{"type": 2, "visibility": 1, "space": 0, "register": 5, "count": 1, "bytes": 256}]}]});
        (Package { dir }, manifest)
    }

    #[test]
    fn hash() {
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b"").unwrap_err(), "Invalid shader/root/manifest size");
    }
    #[test]
    fn numbers() {
        let value = tree(&json!({"a": 3, "b": 3.0, "c": 3.5, "d": -1, "e": "3"}));
        assert_eq!(number(&value, "a", 3), Ok(3));
        // jsoncpp's isUInt holds for an integral real.
        assert_eq!(number(&value, "b", 3), Ok(3));
        for name in ["c", "d", "e", "missing"] {
            assert_eq!(number(&value, name, 9), Err("Invalid manifest number".to_string()));
        }
        assert_eq!(number(&value, "a", 2), Err("Invalid manifest number".to_string()));
        // A member of a non-object is jsoncpp's LogicError; of null, absent.
        assert_eq!(number(&Json::Str(b"x".to_vec()), "a", 9), Err(crate::jsoncpp_tree::NOT_AN_OBJECT.to_string()));
        assert_eq!(number(&Json::Null, "a", 9), Err("Invalid manifest number".to_string()));
    }
    #[test]
    fn loads() {
        let (package, manifest) = package("loads");
        let assets = load(&tree(&manifest), &package.dir).unwrap();
        assert_eq!(assets.len(), 1);
        let a = &assets[0];
        assert_eq!((a.index, a.vs_bytes, a.ps_bytes, a.vs.len(), a.ps_hash.is_empty()), (1, 4096, 0, 64, true));
        assert_eq!((a.pairs.len(), a.uses.len(), a.camera_stages), (1, 1, 0));
    }
    #[test]
    fn rejects() {
        let (package, manifest) = package("rejects");
        let case = |edit: &dyn Fn(&mut Value)| {
            let mut value = manifest.clone();
            edit(&mut value);
            load(&tree(&value), &package.dir).unwrap_err()
        };
        assert_eq!(case(&|v| v["purpose"] = json!("x")), "Unsupported family manifest");
        assert_eq!(case(&|v| v["schema"] = json!(2)), "Unsupported family manifest");
        assert_eq!(case(&|v| v["families"] = json!([])), "Unsupported family manifest");
        assert_eq!(case(&|v| v["families"][0]["nativeVsBytes"] = json!(31)), "Invalid original shader identity");
        assert_eq!(case(&|v| v["families"][0]["nativePs"] = json!("b".repeat(64))), "Invalid original shader identity");
        assert_eq!(case(&|v| v["families"][0]["vs"]["bytes"] = json!(65)), "Shader file size differs");
        assert_eq!(case(&|v| v["families"][0]["vs"]["file"] = json!("other.dxil")), "Invalid shader filename/hash");
        assert_eq!(case(&|v| v["families"][0]["vs"]["sha256"] = json!(["x"])), "Type is not convertible to string");
        assert_eq!(case(&|v| v["families"][0]["pairs"][0]["visibility"] = json!(5)), "Invalid pair contract");
        assert_eq!(case(&|v| v["families"][0]["uses"][0]["count"] = json!(0)), "Invalid resource use");
        assert_eq!(case(&|v| v["families"][0]["uses"][0]["register"] = json!(6)), "Pair has no unique original resource");
        assert_eq!(case(&|v| v["families"][0]["uses"] = json!({})), "Invalid resource contract");
        assert_eq!(
            case(&|v| v["families"][0]["nativeCameraVs"] = json!({"mainFirst": true, "eyeStride": 1024})),
            "Unsupported native camera family"
        );
        assert_eq!(
            case(&|v| v["families"][0]["nativeCamera"] = json!({"schema": 1, "mainFirst": true, "eyeStride": 1024, "bindingSize": 2048, "stages": 0})),
            "Native camera stages missing"
        );
        assert_eq!(case(&|v| v["families"][0] = json!(5)), crate::jsoncpp_tree::NOT_AN_OBJECT);
        // A second family with the same identities.
        let duplicate = |v: &mut Value| {
            let first = v["families"][0].clone();
            v["families"].as_array_mut().unwrap().push(first);
        };
        assert_eq!(case(&duplicate), "Duplicate family");
        // A shader file longer than stated, then one whose bytes no longer hash to its name.
        let name = manifest["families"][0]["vs"]["file"].as_str().unwrap().to_string();
        std::fs::write(package.dir.join(&name), vec![0u8; 65]).unwrap();
        assert_eq!(load(&tree(&manifest), &package.dir).unwrap_err(), "Shader file size differs");
        std::fs::write(package.dir.join(&name), vec![0u8; 64]).unwrap();
        assert_eq!(load(&tree(&manifest), &package.dir).unwrap_err(), "Shader file hash differs");
    }
}
