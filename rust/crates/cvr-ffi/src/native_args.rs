//! A native's arguments read off its stack frame as `RED4ext::GetParameter` reads them (Scripting/Utils.hpp), and the
//! engine's handle arrays a native fills.
use core::ffi::c_void;
use core::mem::ManuallyDrop;

use red4ext_abi::cstring::CString as EngineString;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::Handle;
use red4ext_abi::stack::StackFrame;

pub(crate) struct Args<'a>(pub(crate) &'a mut StackFrame);
impl Args<'_> {
    /// The next argument, of a type whose default value is the engine's (numbers, CName, TweakDBID, Vector4,
    /// Quaternion, handles).
    pub(crate) fn get<T: Default>(&mut self) -> T {
        let mut value = T::default();
        // SAFETY: the parameters in the order Registration names them.
        unsafe { self.0.get_parameter(&mut value) };
        value
    }
    /// A String argument.
    pub(crate) fn text(&mut self) -> String {
        let mut text = EngineString::empty();
        // SAFETY: a String parameter.
        unsafe { self.0.get_parameter(&mut text) };
        text.as_str().to_owned()
    }
    /// An array:CName argument.
    pub(crate) fn names(&mut self) -> Vec<u64> {
        let mut names = DynArray::<u64>::default();
        // SAFETY: an array:CName parameter, freed after it is copied.
        unsafe { self.0.get_parameter(&mut names) };
        let out = names.as_slice().to_vec();
        names.free();
        out
    }
    /// A struct argument of the named type (a gameShapeData), constructed by its type and destroyed with it.
    pub(crate) fn owned(&mut self, type_name: &str) -> Option<crate::script_call::Owned> {
        let ty = red4ext_abi::rtti::RttiSystem::get()?.get_type(red4ext_abi::cname::CName::hashed(type_name));
        let mut value = ty.and_then(crate::script_call::Owned::of_type);
        match value.as_mut() {
            // SAFETY: a constructed value of the parameter's type.
            Some(v) => unsafe { self.0.get_parameter(v.as_mut_ptr()) },
            None => {
                // the argument still read, into room no struct of the game's passes
                let mut sink = [0u128; 32];
                unsafe { self.0.get_parameter(&mut sink) };
            }
        }
        value
    }
    /// Past the arguments (`frame->code++`).
    pub(crate) fn finish(self) {
        self.0.finish_params();
    }
}

/// The arguments of a native the VM is running.
///
/// # Safety
/// `frame` is the frame the VM passed the native.
pub(crate) unsafe fn args<'a>(frame: *mut StackFrame) -> Args<'a> {
    // SAFETY: the caller's.
    Args(unsafe { &mut *frame })
}

/// A handle the engine holds, looked at without taking a reference.
///
/// # Safety
/// `at` points at a live engine Handle.
pub(crate) unsafe fn borrowed(at: *const [usize; 2]) -> ManuallyDrop<Handle> {
    // SAFETY: the caller's; ManuallyDrop: the reference stays the engine's.
    let [instance, ref_count] = unsafe { *at };
    ManuallyDrop::new(Handle {
        instance: instance as *mut c_void,
        ref_count: ref_count as *mut _,
    })
}

/// The handles of an engine DynArray<Handle<T>>, borrowed.
///
/// # Safety
/// `array` is a live DynArray of handles that nothing changes while the slice is used.
pub(crate) unsafe fn handles<'a>(array: *const c_void) -> &'a [[usize; 2]] {
    // SAFETY: the caller's.
    unsafe { (*(array as *const DynArray<[usize; 2]>)).as_slice() }
}

/// `out->PushBack(handle)`: one more strong reference, in a result array of handles.
///
/// # Safety
/// `out` is a DynArray<Handle<T>> result slot.
pub(crate) unsafe fn push_handle(out: *mut c_void, handle: &Handle) {
    unsafe { push_owned(out, handle.clone()) };
}

/// The handle itself moved into a result array of handles.
///
/// # Safety
/// `out` is a DynArray<Handle<T>> result slot.
pub(crate) unsafe fn push_owned(out: *mut c_void, handle: Handle) {
    let handle = ManuallyDrop::new(handle);
    // SAFETY: the caller's; the array's element is the handle's two words.
    let array = unsafe { &mut *(out as *mut DynArray<[usize; 2]>) };
    if !array.push([handle.instance as usize, handle.ref_count as usize]) {
        drop(ManuallyDrop::into_inner(handle));
    }
}

/// `*out = handle` on a handle result: the old value released.
///
/// # Safety
/// `out` is a Handle<T> result slot.
pub(crate) unsafe fn set_handle(out: *mut c_void, handle: Handle) {
    // SAFETY: the caller's.
    drop(unsafe { core::ptr::replace(out as *mut Handle, handle) });
}
