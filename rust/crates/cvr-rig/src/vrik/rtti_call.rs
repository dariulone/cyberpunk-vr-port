//! What the natives do to an engine object through its RTTI, as the SDK's helpers did it in the C++ half: a property by
//! name (`CClass::GetProperty`, its value read in place or assigned by its type), a member function by name
//! (`CClass::GetFunction`) and `RED4ext::ExecuteFunction` with one bool argument -- a function with a result is not
//! called without a place for it, the arguments take the parameters' types, and the engine's Execute runs under SEH.
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::function::{BaseFunction, Property};
use red4ext_abi::rtti::{object_class, RttiSystem};
use red4ext_abi::stack::{CStack, StackType};

/// The object's class's property `name` (the parents' included), or None.
///
/// # Safety
/// `object` is a live ISerializable.
pub(crate) unsafe fn property(object: usize, name: &str) -> Option<&'static Property> {
    object_class(object as *const c_void)?.property_named(name)
}
/// The object's class's function `name` (static, member, then the parents').
///
/// # Safety
/// As `property`.
pub(crate) unsafe fn function(object: usize, name: &str) -> Option<&'static BaseFunction> {
    object_class(object as *const c_void)?.function(CName::hashed(name))
}
/// `RED4ext::ExecuteFunction(object, f, nullptr, args)` with no argument or one bool.
///
/// # Safety
/// `object` is a live instance of the function's class.
pub(crate) unsafe fn execute(object: usize, f: &BaseFunction, argument: Option<&mut bool>) -> bool {
    let Some(rtti) = RttiSystem::get() else { return false };
    if !f.return_type.is_null() {
        return false;
    }
    let mut args: Vec<StackType> = Vec::new();
    if let Some(argument) = argument {
        let ty = f
            .params
            .as_slice()
            .first()
            .and_then(|p| p.as_ref())
            .map_or(core::ptr::null(), |p| p.ty as *const _);
        args.push(StackType {
            ty,
            value: argument as *mut bool as *mut c_void,
        });
    }
    let Some(mut stack) = CStack::new(rtti, object as *mut c_void, &mut args, core::ptr::null_mut()) else {
        return false;
    };
    let Some(address) = f.execute_address() else { return false };
    cvr_engine::code::guarded_call3(
        address,
        f as *const BaseFunction as *mut c_void,
        &mut stack as *mut CStack as *mut c_void,
        core::ptr::null_mut(),
    )
    .unwrap_or(false)
}
/// The member function `name` called on `object` (false when the class has none).
///
/// # Safety
/// As `execute`.
pub(crate) unsafe fn call(object: usize, name: &str, argument: Option<&mut bool>) -> bool {
    match function(object, name) {
        Some(f) => execute(object, f, argument),
        None => false,
    }
}
