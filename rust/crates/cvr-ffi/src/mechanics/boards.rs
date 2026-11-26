//! The blackboards the Stereo mod's modules read (GetAllBlackboardDefs, Game.GetBlackboardSystem()): a definition's
//! field ids, the system's and an entity's boards, and the UIInteractions hub (FromVariant's InteractionChoiceHubData).
use crate::script_call::{call_global_handle, call_handle, method, method_owned, property_of_kind, Value};
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{Class, IType, RttiKind};

/// GetAllBlackboardDefs().
pub(super) fn defs() -> Result<Handle, String> {
    // SAFETY: a global script function without parameters, on the main thread.
    unsafe { call_global_handle(core::ptr::null_mut(), "GetAllBlackboardDefs", &[]) }
        .filter(|h| !h.is_null())
        .ok_or_else(|| "GetAllBlackboardDefs failed".into())
}

/// `defs.<name>`: a blackboard definition, held by the definitions.
pub(super) fn definition<'a>(defs: &'a Handle, name: &str) -> Result<&'a Handle, String> {
    // SAFETY: the definitions' handle property, alive while they are.
    let def = unsafe { property_of_kind(defs.instance, name, RttiKind::Handle as u8).map(|p| &*(p as *const Handle)) };
    def.filter(|h| !h.is_null()).ok_or_else(|| format!("no {name} definition"))
}

/// `def.<name>`: a field's id (a BlackboardID_* value), in place in the definition.
pub(super) fn field(def: &Handle, name: &str) -> Result<*mut c_void, String> {
    // SAFETY: the definition's struct property.
    unsafe { property_of_kind(def.instance, name, RttiKind::Class as u8) }.ok_or_else(|| format!("no {name} field"))
}

/// Game.GetBlackboardSystem():Get(def): null without the system or the board.
pub(super) fn system_board(def: &Handle) -> Result<Handle, String> {
    let system_held = super::game_system("GetBlackboardSystem");
    let system = system_held.instance;
    if system.is_null() {
        return Ok(Handle::null());
    }
    // SAFETY: the blackboard system; the definition held by the caller.
    unsafe { call_handle(system, "Get", &[Value::Handle(def)]) }.ok_or_else(|| "the blackboard system's Get failed".into())
}

/// Game.GetBlackboardSystem():GetLocalInstanced(entity, def): null without the system or the board.
pub(super) fn local_board(entity: u64, def: &Handle) -> Result<Handle, String> {
    let system_held = super::game_system("GetBlackboardSystem");
    let system = system_held.instance;
    if system.is_null() {
        return Ok(Handle::null());
    }
    // SAFETY: as above; the EntityID by value.
    unsafe { call_handle(system, "GetLocalInstanced", &[Value::U64(entity), Value::Handle(def)]) }
        .ok_or_else(|| "GetLocalInstanced failed".into())
}

/// `board:Get<kind>(id)` for a plain result (GetBool, GetInt, GetFloat, GetEntityID).
pub(super) fn get<R: crate::script_call::Ret>(board: &Handle, getter: &str, id: *mut c_void) -> Result<R, String> {
    // SAFETY: the caller's board, held; the id in place in its definition.
    unsafe { method::<R>(board.instance, getter, &[Value::Ptr(id)]) }.ok_or_else(|| format!("{getter} failed"))
}

/// UI_System.IsInMenu, as the modules read it: None when the board is not there.
pub(super) fn in_menu(defs: &Handle) -> Result<Option<bool>, String> {
    let ui = definition(defs, "UI_System")?;
    let board = system_board(ui)?;
    if board.is_null() {
        return Ok(None);
    }
    get::<bool>(&board, "GetBool", field(ui, "IsInMenu")?).map(Some)
}

/// Variant: its type (the low bit: the value is inline) and the value, inline or allocated.
const VARIANT_INLINE: usize = 1;

/// The number of choices in a Variant holding an InteractionChoiceHubData (FromVariant's `hub.choices`); None when it
/// holds nothing or a value without choices.
///
/// # Safety
/// `variant` is a live Variant.
unsafe fn variant_choices(variant: *const u8) -> Option<u32> {
    // SAFETY: the caller's Variant: its type word, then the value inline or behind a pointer.
    unsafe {
        let word = *(variant as *const usize);
        let ty = ((word & !VARIANT_INLINE) as *const IType).as_ref()?;
        let data = if word & VARIANT_INLINE != 0 { variant.add(8) as *mut c_void } else { *(variant.add(8) as *const *mut c_void) };
        if data.is_null() || ty.kind() != RttiKind::Class as u8 {
            return None;
        }
        let class = &*(ty as *const IType as *const Class);
        let choices = class.property(CName::hashed("choices"))?;
        if choices.ty.as_ref()?.kind() != RttiKind::Array as u8 {
            return None;
        }
        // DynArray: entries, capacity, size.
        Some(*(choices.value_ptr::<u8>(data).add(0x0C) as *const u32))
    }
}

/// `board:GetVariant(id)`'s value read by `read` (the struct's data and class) when the variant holds a struct; None
/// when it holds nothing.
pub(super) fn variant_struct<T>(board: &Handle, id: *mut c_void, read: impl FnOnce(*mut c_void, &Class) -> Option<T>) -> Result<Option<T>, String> {
    // SAFETY: the board, held; the Variant result owned until it is read.
    let variant = unsafe { method_owned(board.instance, "GetVariant", &[Value::Ptr(id)]) }.ok_or("GetVariant failed")?;
    // SAFETY: the Variant just returned: its type word, then the value inline or behind a pointer.
    unsafe {
        let v = variant.as_ptr();
        let word = *(v as *const usize);
        let Some(ty) = ((word & !VARIANT_INLINE) as *const IType).as_ref() else { return Ok(None) };
        let data = if word & VARIANT_INLINE != 0 { v.add(8) as *mut c_void } else { *(v.add(8) as *const *mut c_void) };
        if data.is_null() || ty.kind() != RttiKind::Class as u8 {
            return Ok(None);
        }
        Ok(read(data, &*(ty as *const IType as *const Class)))
    }
}

/// The UIInteractions board's InteractionChoiceHub (FromVariant(ui:GetVariant(...))): Err(()) without the board, else
/// its choices (None: no hub in it).
pub(super) fn hub_choices(defs: &Handle) -> Result<Result<Option<u32>, ()>, String> {
    let ui = definition(defs, "UIInteractions")?;
    let hub = field(ui, "InteractionChoiceHub")?;
    let board = system_board(ui)?;
    if board.is_null() {
        return Ok(Err(()));
    }
    // SAFETY: the board, held; the Variant result owned until it is read.
    let variant = unsafe { method_owned(board.instance, "GetVariant", &[Value::Ptr(hub)]) }.ok_or("GetVariant failed")?;
    // SAFETY: the Variant just returned.
    Ok(Ok(unsafe { variant_choices(variant.as_ptr()) }))
}
