//! THE PHYSICAL SCOPE'S OWN UI SURFACE (formerly src/Render/PhysicalOpticsSurface.cpp), verified against CP2077
//! a7de82945c03e041. The shared world UI atlas has only 1/2/4 slots at 2048/1024/512: a physical scope must not compete
//! with city screens for its camera's pixel resolution. A port panel's renderer publishes through its own native
//! 2048-square texture (one per weapon -- the panels share it by quadrant, composited in a defined order, so
//! transparent mesh submission order cannot hide the reticle); the pooled slot's bookkeeping stays the allocator's.
//!
//! Native engine ownership ends with the renderer, before its pools are torn down; nothing here releases native
//! handles from static destruction (Rust statics are never dropped). Registered from the C++ file (Boot, 83).
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_engine::code::{expect, image_base, Pattern};
use cvr_engine::hooks::{install_chain, Detour};
use cvr_host::log;
use std::sync::{Arc, LazyLock, Mutex, Weak};

const ZERO64: AtomicU64 = AtomicU64::new(0);
const ZERO32: AtomicU32 = AtomicU32::new(0);
/// 1: port panels publish through their own surface; 0: the shared atlas.
#[no_mangle]
pub static CyberpunkVR_OpticsOwnSurface: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_OpticsSurfaceCreated: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_OpticsSurfaceReleased: AtomicU64 = AtomicU64::new(0);
/// Publish calls that passed each check in optics_size: 0 any renderer with a texture, 1 renderer type, 2 provider
/// type, 3 live component, 4 port panel name, 5 canvas size; 6 port panel textures set.
#[no_mangle]
pub static CyberpunkVR_OpticsSurfaceStage: [AtomicU64; 8] = [ZERO64; 8];
/// The last port panel's canvas size as read (rejected sizes show why a panel went to the shared atlas).
#[no_mangle]
pub static CyberpunkVR_OpticsSurfaceCanvas: [AtomicU32; 2] = [ZERO32; 2];

static BASE: AtomicUsize = AtomicUsize::new(0);
fn base() -> usize {
    BASE.load(Relaxed)
}
/// The engine's reads and writes, as the C++ did them: plain accesses of live engine objects.
unsafe fn at<T: Copy>(p: usize, offset: usize) -> T {
    // SAFETY: the caller's live engine object.
    unsafe { core::ptr::read_unaligned((p + offset) as *const T) }
}
unsafe fn put<T: Copy>(p: usize, offset: usize, value: T) {
    // SAFETY: the caller's live engine object.
    unsafe { core::ptr::write_unaligned((p + offset) as *mut T, value) }
}
/// The weak count of a handle's control block (its strong count is at +0).
unsafe fn weak_add(control: usize) {
    // SAFETY: a live control block.
    unsafe { AtomicI32::from_ptr((control + 4) as *mut i32).fetch_add(1, SeqCst) };
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct NativeHandle {
    object: usize,
    control: usize,
}
/// One engine handle released by the native function at RVA `RELEASE` (a strong handle's, or a weak one's).
#[derive(Default)]
struct Lease<const RELEASE: usize> {
    handle: NativeHandle,
}
impl<const RELEASE: usize> Drop for Lease<RELEASE> {
    fn drop(&mut self) {
        if self.handle.control != 0 {
            // SAFETY: the engine's release of this kind of handle.
            unsafe {
                let release: unsafe extern "system" fn(*mut NativeHandle) =
                    core::mem::transmute(base() + RELEASE);
                release(&mut self.handle);
            }
        }
    }
}
const STRONG_RELEASE: usize = 0x198368;
const WEAK_RELEASE: usize = 0x409774;
const PROVIDER_RELEASE: usize = 0x1c075c;

struct Surface {
    // Dropped in this order: the weak owner, then the texture.
    owner: Lease<WEAK_RELEASE>,
    texture: Lease<STRONG_RELEASE>,
    width: u32,
    height: u32,
}
impl Drop for Surface {
    fn drop(&mut self) {
        if self.texture.handle.object != 0 {
            CyberpunkVR_OpticsSurfaceReleased.fetch_add(1, Relaxed);
        }
    }
}
struct Binding {
    surface: Arc<Surface>,
    /// The weak component: never prolongs its game lifetime.
    owner: Lease<WEAK_RELEASE>,
}
#[derive(Default)]
struct Surfaces {
    entries: FastMap<usize, Option<Arc<Surface>>>,
    bindings: FastMap<usize, Arc<Binding>>,
    groups: FastMap<usize, Weak<Surface>>,
}
static OWNED: LazyLock<Mutex<Surfaces>> = LazyLock::new(Default::default);
fn owned() -> std::sync::MutexGuard<'static, Surfaces> {
    OWNED.lock().unwrap_or_else(|p| p.into_inner())
}

type PublishFn = unsafe extern "system" fn(usize) -> usize;
type DestroyFn = unsafe extern "system" fn(usize, u8) -> usize;
type SetTextureFn = unsafe extern "system" fn(usize, usize) -> usize;
static mut ORIG_PUBLISH: *mut c_void = core::ptr::null_mut();
static mut ORIG_DESTROY: *mut c_void = core::ptr::null_mut();
static mut ORIG_SET_TEXTURE: *mut c_void = core::ptr::null_mut();

/// One native ink atlas per weapon, a quadrant per panel.
const PANEL_UV: [[f32; 4]; 3] = [
    [0.0, 0.0, 0.5, 0.5],
    [0.5, 0.0, 1.0, 0.5],
    [0.5, 0.5, 1.0, 1.0],
];
const PANEL_CAMERA: u64 = red4ext_abi::fnv::fnv1a64(b"vr_optic_panel");
const PANEL_BUILTIN_GUILLOTINE: u64 = red4ext_abi::fnv::fnv1a64(b"vr_builtin_guillotine_panel");
const PANEL_BUILTIN_HERCULES: u64 = red4ext_abi::fnv::fnv1a64(b"vr_builtin_hercules_panel");
const PANEL_FRONT: u64 = red4ext_abi::fnv::fnv1a64(b"vr_optic_reticle_front_panel");
const PANEL_REAR: u64 = red4ext_abi::fnv::fnv1a64(b"vr_optic_reticle_rear_panel");
/// A port panel's slot in the weapon's atlas (main's 36d046482): 0 the full-resolution camera feed -- the scope's
/// panel and the two guns with an integral lens (Guillotine, Hercules) --, 1 the front reticle, 2 the rear.
const fn optics_panel_slot(name: u64) -> Option<usize> {
    match name {
        PANEL_CAMERA | PANEL_BUILTIN_GUILLOTINE | PANEL_BUILTIN_HERCULES => Some(0),
        PANEL_FRONT => Some(1),
        PANEL_REAR => Some(2),
        _ => None,
    }
}
const _: () = assert!(PANEL_CAMERA == 0x77eb_ca88_3421_763e && PANEL_FRONT == 0xb2ae_acea_1ebe_33bd && PANEL_REAR == 0x02a6_5679_8651_2faa);
// ent::IComponent: the name at +0x40, the handle to itself at +8, the owner at +0x50.

unsafe extern "system" fn set_texture(component: usize, texture: usize) -> usize {
    // SAFETY (whole function): the engine's component, the trampoline written at install.
    unsafe {
        let original: SetTextureFn = core::mem::transmute(ORIG_SET_TEXTURE);
        if optics_panel_slot(at::<u64>(component, 0x40)).is_none() {
            return original(component, texture);
        }
        CyberpunkVR_OpticsSurfaceStage[6].fetch_add(1, Relaxed);
        // WorldLayer unregister destroys the renderer BEFORE clearing the component texture: the old surface stays
        // alive until the native setter and the material invalidation have completed (renderer ownership alone is
        // insufficient).
        let previous;
        {
            let mut guard = owned();
            let s = &mut *guard;
            // A detached component can expire before its queued UI removal calls the setter. Its weak reference
            // makes these leftovers safe to reap.
            s.bindings.retain(|_, b| {
                let control = b.owner.handle.control;
                control != 0 && AtomicI32::from_ptr(control as *mut i32).load(SeqCst) != 0
            });
            previous = s.bindings.get(&component).cloned();
            let mut next = previous
                .as_ref()
                .filter(|p| p.surface.texture.handle.object == texture)
                .map(|p| p.surface.clone());
            if next.is_none() {
                next = s
                    .entries
                    .values()
                    .flatten()
                    .find(|surface| surface.texture.handle.object == texture)
                    .cloned();
            }
            match next {
                Some(next) => {
                    if previous
                        .as_ref()
                        .is_none_or(|p| !Arc::ptr_eq(&p.surface, &next))
                    {
                        let handle: NativeHandle = at(component, 8);
                        let binding = Binding {
                            surface: next,
                            owner: Lease { handle },
                        };
                        if handle.control != 0 {
                            weak_add(handle.control);
                        }
                        s.bindings.insert(component, Arc::new(binding));
                    }
                }
                None => {
                    if previous.is_some() {
                        s.bindings.remove(&component);
                    }
                }
            }
        }
        let result = original(component, texture);
        drop(previous);
        result
    }
}

/// A port panel's renderer: its owner and panel slot (the canvas size is checked and reported).
unsafe fn optics_size(renderer: usize) -> Option<(u32, u32, usize, usize)> {
    let stage = &CyberpunkVR_OpticsSurfaceStage;
    let base = base();
    stage[0].fetch_add(1, Relaxed);
    // SAFETY (whole function): the engine's renderer and the native functions checked at install.
    unsafe {
        if at::<usize>(renderer, 0) != base + 0x3111dc8 {
            return None;
        }
        stage[1].fetch_add(1, Relaxed);
        let mut provider = Lease::<PROVIDER_RELEASE>::default();
        let lock: unsafe extern "system" fn(*mut NativeHandle, *const c_void) =
            core::mem::transmute(base + 0x27c10c);
        lock(&mut provider.handle, (renderer + 400) as *const c_void);
        if provider.handle.object == 0 || at::<usize>(provider.handle.object, 0) != base + 0x30636e8
        {
            return None;
        }
        stage[2].fetch_add(1, Relaxed);
        let mut component = Lease::<STRONG_RELEASE>::default();
        let get: unsafe extern "system" fn(*const c_void, *mut NativeHandle) =
            core::mem::transmute(base + 0x189688);
        get(
            (provider.handle.object + 8) as *const c_void,
            &mut component.handle,
        );
        let object = component.handle.object;
        if object == 0 {
            return None;
        }
        stage[3].fetch_add(1, Relaxed);
        let slot = optics_panel_slot(at::<u64>(object, 0x40))?;
        stage[4].fetch_add(1, Relaxed);
        let owner = at::<usize>(object, 0x50);
        if owner == 0 {
            return None;
        }
        let (w, h) = (at::<f32>(object, 0x2b0), at::<f32>(object, 0x2b4));
        CyberpunkVR_OpticsSurfaceCanvas[0].store(w.to_bits(), Relaxed);
        CyberpunkVR_OpticsSurfaceCanvas[1].store(h.to_bits(), Relaxed);
        if !w.is_finite() || !h.is_finite() || w < 256.0 || h < 256.0 || w > 2048.0 || h > 2048.0 {
            return None;
        }
        stage[5].fetch_add(1, Relaxed);
        let (mut width, mut height) = (w.ceil() as u32, h.ceil() as u32);
        if slot != 0 {
            // Native reticle artwork uses a 2048-square logical canvas. A 1024 surface preserves thin lines without
            // allocating two extra 16MB RTs.
            width = width.min(1024);
            height = height.min(1024);
        }
        Some((width, height, owner, slot))
    }
}

/// The renderer's surface: its own, the weapon's (the owner's group), or a new native texture.
unsafe fn get_surface(renderer: usize, owner: usize) -> Option<Arc<Surface>> {
    let mut guard = owned();
    let s = &mut *guard;
    if let Some(Some(entry)) = s.entries.get(&renderer) {
        return Some(entry.clone());
    }
    s.entries.insert(renderer, None);
    s.groups.retain(|_, group| group.strong_count() > 0);
    // SAFETY: the renderer's live owner.
    let identity: NativeHandle = unsafe { at(owner, 8) };
    if let Some(group) = s.groups.entry(owner).or_default().upgrade() {
        if group.owner.handle.control == identity.control {
            s.entries.insert(renderer, Some(group.clone()));
            return Some(group);
        }
    }
    let mut next = Surface {
        owner: Lease { handle: identity },
        texture: Lease::default(),
        width: 0,
        height: 0,
    };
    if identity.control != 0 {
        // SAFETY: the owner's live control block.
        unsafe { weak_add(identity.control) };
    }
    // The same descriptor and factory as WorldDTManager's standalone (full-size) texture path, RVA 82099C. The game
    // creates its GPU backing and transitions.
    #[repr(C)]
    struct Desc {
        width: u32,
        height: u32,
        flags: u32,
        reserved: u32,
        backing: usize,
    }
    const _: () = assert!(core::mem::size_of::<Desc>() == 24);
    const SIZE: u32 = 2048;
    // RGBA_Float16 retains scene HDR through ink.
    let desc = Desc {
        width: SIZE,
        height: SIZE,
        flags: 0x13070100,
        reserved: 0,
        backing: 0,
    };
    // SAFETY: the native factory checked at install.
    unsafe {
        let create: unsafe extern "system" fn(*mut NativeHandle, *const Desc) -> *mut NativeHandle =
            core::mem::transmute(base() + 0x5ea8a4);
        create(&mut next.texture.handle, &desc);
    }
    if next.texture.handle.object == 0 || next.texture.handle.control == 0 {
        return None;
    }
    next.width = SIZE;
    next.height = SIZE;
    let next = Arc::new(next);
    s.entries.insert(renderer, Some(next.clone()));
    s.groups.insert(owner, Arc::downgrade(&next));
    CyberpunkVR_OpticsSurfaceCreated.fetch_add(1, Relaxed);
    log!(
        "[optics-surface] native UI texture {}x{} for renderer={:016X}\n",
        next.width,
        next.height,
        renderer
    );
    Some(next)
}

unsafe fn bind_window(window: usize, texture: usize, uv: &[f32; 4]) {
    if window == 0 {
        return;
    }
    // SAFETY (whole function): the renderer's live virtual window and the native setters checked at install.
    unsafe {
        // 2.5: the virtual window's fields from +4D8 on moved by 0x10 (texture 4D8 -> 4E8, rect 4F0 -> 500).
        let texture_changed = at::<usize>(window, 0x4E8) != texture;
        let rect_changed = at::<[u32; 4]>(window, 0x500) != uv.map(f32::to_bits);
        if rect_changed {
            let set_rect: unsafe extern "system" fn(usize, *const f32) =
                core::mem::transmute(base() + 0x30fc34);
            set_rect(window, uv.as_ptr());
        }
        if texture_changed || rect_changed {
            let set: unsafe extern "system" fn(usize, usize, u8) =
                core::mem::transmute(base() + 0x30fd04);
            set(window, texture, 1);
        }
    }
}

unsafe extern "system" fn publish(renderer: usize) -> usize {
    // SAFETY (whole function): the engine's renderer, the trampoline written at install.
    unsafe {
        let original: PublishFn = core::mem::transmute(ORIG_PUBLISH);
        if at::<usize>(renderer, 16) == 0 {
            return original(renderer);
        }
        let Some((_, _, owner, slot)) = optics_size(renderer) else {
            return original(renderer);
        };
        let pooled_texture = at::<usize>(renderer, 16);
        let pooled_uv = at::<[f32; 4]>(renderer, 24);
        let surface = if CyberpunkVR_OpticsOwnSurface.load(Relaxed) != 0 {
            get_surface(renderer, owner)
        } else {
            None
        };
        let texture = surface
            .as_ref()
            .map_or(pooled_texture, |s| s.texture.handle.object);
        let uv = if surface.is_some() {
            PANEL_UV[slot]
        } else {
            pooled_uv
        };
        bind_window(at::<usize>(renderer, 128), texture, &uv);
        // Published through the original component / material binder: only this call sees the dedicated surface;
        // the allocator retains its own slot bookkeeping.
        put(renderer, 16, texture);
        put(renderer, 24, uv);
        let result = original(renderer);
        put(renderer, 16, pooled_texture);
        put(renderer, 24, pooled_uv);
        drop(surface);
        result
    }
}

unsafe extern "system" fn destroy(renderer: usize, flags: u8) -> usize {
    let surface = owned().entries.remove(&renderer);
    // SAFETY: the trampoline written at install.
    let result =
        unsafe { core::mem::transmute::<*mut c_void, DestroyFn>(ORIG_DESTROY)(renderer, flags) };
    // The renderer's surface outlives the native destroy.
    drop(surface);
    result
}

const PUBLISH: usize = 0x606660;
const PUBLISH_BYTES: &str = "48 89 5C 24 08 48 89 74 24 10 48 89 7C 24 18 55 48 8B EC";
const DESTROY: usize = 0xdd98a4;
const DESTROY_BYTES: &str = "48 89 5C 24 08 57 48 83 EC 20 48 8B F9 8B DA";
const SET_TEXTURE: usize = 0x6069f0;
const SET_TEXTURE_BYTES: &str = "40 53 48 83 EC 40 48 8B 81 30 02 00 00";
/// The native functions the detours call.
const CALLED: [(usize, &str); 8] = [
    (0x5ea8a4, "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 30"),
    (0x30fc34, "48 83 EC 28 F3 0F 10 42 0C 0F 2F 42 04"),
    (
        0x30fd04,
        "48 89 5C 24 08 57 48 83 EC 20 48 89 91 E8 04 00 00",
    ),
    (0x27c10c, "48 8B 42 08 4C 8B C2 4C 8B C9"),
    (0x189688, "48 83 22 00 4C 8B C2 48 83 62 08 00"),
    (PROVIDER_RELEASE, "48 89 5C 24 08 57 48 83 EC 30 48 8B F9"),
    (STRONG_RELEASE, "40 53 48 83 EC 20 48 8B D9 48 8B 49 08"),
    (WEAK_RELEASE, "40 53 48 83 EC 30 48 8B D9 48 8B 49 08"),
];

/// Installed from src/Render/PhysicalOpticsSurface.cpp's registration (Boot, 83): the renderer's publish and
/// destroy and the component's texture setter, as one chain, after every native function they call is checked.
///
/// # Safety
/// Called once by the hook registry.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_physical_optics_surface_install() -> bool {
    let base = image_base();
    BASE.store(base, Relaxed);
    if base == 0
        || !CALLED
            .iter()
            .all(|&(rva, bytes)| Pattern::parse(bytes).is_some_and(|p| expect(base + rva, &p)))
    {
        return false;
    }
    let detours = [
        Detour {
            name: "WorldUiRendererPublish",
            target: PUBLISH,
            absolute: false,
            detour: publish as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_PUBLISH),
            expect: Some(PUBLISH_BYTES),
        },
        Detour {
            name: "WorldUiRendererDestroy",
            target: DESTROY,
            absolute: false,
            detour: destroy as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_DESTROY),
            expect: Some(DESTROY_BYTES),
        },
        Detour {
            name: "WorldWidgetSetTexture",
            target: SET_TEXTURE,
            absolute: false,
            detour: set_texture as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_SET_TEXTURE),
            expect: Some(SET_TEXTURE_BYTES),
        },
    ];
    // SAFETY: each detour has its target's signature and reads its trampoline from its slot.
    unsafe { install_chain("physical-optics-surface", &detours) }.is_ok()
}
