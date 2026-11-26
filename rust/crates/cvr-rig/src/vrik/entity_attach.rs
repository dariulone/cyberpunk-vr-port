//! A thing carried by a slot of another entity, the way the game carries a weapon in a hand (formerly
//! src/Natives/EntityAttach.cpp, where the engine's attachment path is written up): the entity's root component is
//! made a child of the owner's slot component in the transform graph (sub_14044764C) and taken out again
//! (sub_1404476A8), both keeping the world transform. The calls and the reads of the attachment list go through the
//! SEH adapter, as the C++ ran them under __try. The counters stay exported by src/Natives/EntityAttach.cpp.
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};
use cvr_engine::code::{guarded_call4, image_base, read, read_bytes};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, Class, RttiSystem};
use red4ext_abi::stack::StackFrame;
use std::sync::OnceLock;

extern "C" {
    /// [0] attaches made, [1] detaches made, [2] attaches refused (no root, a root already attached, not a placed
    /// component), [3] calls that faulted.
    static CyberpunkVR_DebugEntityAttach: [AtomicU64; 4];
}

const ATTACH_RVA: usize = 0x44764C;
const DETACH_RVA: usize = 0x4476A8;
const ATTACH_PROLOGUE: [u8; 22] = [
    0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xEC, 0x30, 0x48, 0x8B, 0x89, 0xB0, 0x00, 0x00, 0x00,
];
const DETACH_PROLOGUE: [u8; 20] = [
    0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x08, 0x48, 0x89, 0x70, 0x10, 0x48, 0x89, 0x78, 0x18, 0x55, 0x48, 0x8D, 0x68, 0xE8,
];
/// entEntity::transformComponent.
const ENTITY_ROOT: usize = 0xB0;
/// IPlacedComponent: the attachment list (handles of entIAttachment, the incoming ones first) and their number. Not
/// every incoming attachment carries the transform: an animated component is also the destination of its animation
/// controller's entAnimationControlAttachment.
const ATTACHMENTS: usize = 0x70;
const INCOMING_COUNT: usize = 0x80;

/// What sub_1404478D8 reads: the slot's name, and a flag it copies into the attachment (bit 3 of +0x50, which an
/// entSlotAttachment carries by default).
#[repr(C)]
struct AttachRequest {
    slot: u64,
    flag: u8,
    pad: [u8; 7],
}

fn count(i: usize) {
    // SAFETY: the C++ half's exported counters.
    unsafe { CyberpunkVR_DebugEntityAttach[i].fetch_add(1, Ordering::Relaxed) };
}
fn verified() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let matches = |at: usize, want: &[u8]| {
            let mut got = vec![0u8; want.len()];
            read_bytes(image_base() + at, &mut got) && got == want
        };
        cvr_engine::game_build::supported() && matches(ATTACH_RVA, &ATTACH_PROLOGUE) && matches(DETACH_RVA, &DETACH_PROLOGUE)
    })
}
unsafe fn is_a(object: *const c_void, name: &str) -> bool {
    !object.is_null() && object_class(object).is_some_and(|c| c.is_a(CName::hashed(name)))
}
/// An object's class through the SEH adapter (its vtable's GetType at +0x08).
unsafe fn guarded_class(object: usize) -> Option<Option<&'static Class>> {
    let vtable = read::<usize>(object)?;
    let get_type = read::<usize>(vtable + 0x08)?;
    let class = guarded_call4(
        get_type,
        object as *mut c_void,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    )?;
    Some((class as *const Class).as_ref())
}
/// The number of transform attachments (parents) the component is the child of; -1 when it cannot be read.
unsafe fn incoming(component: usize) -> i32 {
    static TRANSFORM: OnceLock<bool> = OnceLock::new();
    let transform = CName::hashed("entITransformAttachment");
    if !*TRANSFORM.get_or_init(|| RttiSystem::get().is_some_and(|r| r.get_class(transform).is_some())) {
        return -1;
    }
    let (Some(count), Some(list)) = (read::<u16>(component + INCOMING_COUNT), read::<usize>(component + ATTACHMENTS)) else {
        return -1;
    };
    let mut parents = 0;
    for i in 0..count as usize {
        let Some(attachment) = read::<usize>(list + 16 * i) else { return -1 };
        if attachment == 0 {
            continue;
        }
        match guarded_class(attachment) {
            None => return -1,
            Some(class) => {
                if class.is_some_and(|c| c.is_a(transform)) {
                    parents += 1;
                }
            }
        }
    }
    parents
}
unsafe fn root(object: *const c_void) -> usize {
    if !is_a(object, "entEntity") {
        return 0;
    }
    *((object as *const u8).add(ENTITY_ROOT) as *const usize)
}

/// VRAttachToSlot(entity, parent, slot) -> Bool: the entity's root made a child of the parent component's slot, where
/// it is now (parent: an entSlotComponent such as the player's ItemAttachmentSlots, slot: one of its slots, WeaponLeft
/// or WeaponRight for the hands). True once the root is attached.
#[no_mangle]
pub unsafe extern "C" fn VRAttachToSlot(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let (mut entity, mut parent, mut slot) = (Handle::null(), Handle::null(), CName::default());
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut entity);
        frame.get_parameter(&mut parent);
        frame.get_parameter(&mut slot);
        frame.finish_params();
    }
    if let Some(out) = out.as_mut() {
        *out = false;
    }
    if !verified() {
        return;
    }
    let root = root(entity.instance);
    if root == 0 || !is_a(parent.instance, "entIPlacedComponent") || incoming(root) != 0 {
        count(2);
        return;
    }
    let mut request = AttachRequest {
        slot: slot.0,
        flag: 1,
        pad: [0; 7],
    };
    let call = guarded_call4(
        image_base() + ATTACH_RVA,
        entity.instance,
        &mut parent as *mut Handle as *mut c_void,
        &mut request as *mut AttachRequest as *mut c_void,
        1 as *mut c_void,
    );
    if call.is_none() {
        count(3);
        return;
    }
    let attached = incoming(root) > 0;
    count(if attached { 0 } else { 2 });
    if let Some(out) = out.as_mut() {
        *out = attached;
    }
}

/// VRDetachFromSlot(entity) -> Bool: the root back in the world where it is now. True once nothing carries it.
#[no_mangle]
pub unsafe extern "C" fn VRDetachFromSlot(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut entity = Handle::null();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut entity);
        frame.finish_params();
    }
    if let Some(out) = out.as_mut() {
        *out = false;
    }
    if !verified() {
        return;
    }
    let root = root(entity.instance);
    if root == 0 {
        return;
    }
    if incoming(root) == 0 {
        if let Some(out) = out.as_mut() {
            *out = true;
        }
        return;
    }
    let call = guarded_call4(
        image_base() + DETACH_RVA,
        entity.instance,
        1 as *mut c_void,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    );
    if call.is_none() {
        count(3);
        return;
    }
    count(1);
    if let Some(out) = out.as_mut() {
        *out = incoming(root) == 0;
    }
}
