//! RED4ext::CName: a name is its FNV-1a 64 hash; the engine's name pool keeps the text for the names it knows.
use crate::assert_layout;
use crate::fnv::fnv1a64;
use crate::hashes;
use crate::reloc::Reloc;
use core::ffi::CStr;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CName(pub u64);
assert_layout!(CName, size = 8);

const EMPTY: u64 = fnv1a64(b"");
const NONE: u64 = fnv1a64(b"None");

static ADD_CSTR: Reloc = Reloc::new(hashes::CNamePool_AddCstr);
static GET: Reloc = Reloc::new(hashes::CNamePool_Get);

impl CName {
    pub const NONE: CName = CName(0);

    /// The name's value without touching the pool (`CName(const char*)`): "" and "None" are the empty name.
    pub const fn hashed(text: &str) -> CName {
        let h = fnv1a64(text.as_bytes());
        if h == EMPTY || h == NONE {
            CName(0)
        } else {
            CName(h)
        }
    }

    /// `CNamePool::Add(const char*)`: the name, its text registered with the engine so it can be printed and looked
    /// up by scripts. Falls back to the plain hash when the pool function cannot be resolved.
    pub fn add(text: &CStr) -> CName {
        type AddFn = unsafe extern "C" fn(*mut CName, *const core::ffi::c_char) -> *mut CName;
        let mut out = CName(0);
        // SAFETY: CNamePool_AddCstr's signature per CNamePool-inl.hpp; the pool is thread-safe.
        match unsafe { ADD_CSTR.func::<AddFn>() } {
            Some(add) => {
                unsafe { add(&mut out, text.as_ptr()) };
                out
            }
            None => CName::hashed(text.to_str().unwrap_or("")),
        }
    }

    /// `CNamePool::Get`: the name's text, "" when the pool does not know it. The text lives as long as the pool.
    pub fn text(self) -> &'static str {
        type GetFn = unsafe extern "C" fn(*const CName) -> *const core::ffi::c_char;
        // SAFETY: CNamePool_Get's signature per CNamePool-inl.hpp; its strings are never freed.
        let Some(get) = (unsafe { GET.func::<GetFn>() }) else { return "" };
        let p = unsafe { get(&self) };
        if p.is_null() {
            return "";
        }
        unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
    }

    /// `CName::ToString()`: the pool's own NUL-terminated text, null when the pool does not know the name (or cannot be
    /// resolved).
    pub fn raw_text(self) -> *const core::ffi::c_char {
        type GetFn = unsafe extern "C" fn(*const CName) -> *const core::ffi::c_char;
        // SAFETY: CNamePool_Get's signature per CNamePool-inl.hpp.
        let Some(get) = (unsafe { GET.func::<GetFn>() }) else { return core::ptr::null() };
        // SAFETY: the pool's lookup.
        unsafe { get(&self) }
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::CName;
    #[test]
    fn empty_and_none_are_zero() {
        assert_eq!(CName::hashed(""), CName(0));
        assert_eq!(CName::hashed("None"), CName(0));
        assert_ne!(CName::hashed("Int32"), CName(0));
    }
}
