//! The weapon offset, owned here (formerly OpenXRManager's members, set by SetWeaponOffsets): the pitch, yaw and roll in
//! degrees and the translation the right hand's published pose is offset by. include/Runtimes/OpenXRManager.hpp's
//! members are references to them; the hands' publication (published.rs) reads them here.
use crate::calibration::AtomicF32;

#[repr(C)]
pub struct WeaponShared {
    /// Pitch, yaw, roll in degrees.
    pub euler: [AtomicF32; 3],
    /// The translation.
    pub offset: [AtomicF32; 3],
}
const _: () = assert!(core::mem::size_of::<WeaponShared>() == 24, "mirrored by include/Runtimes/OpenXRManager.hpp");

pub static WEAPON: WeaponShared = WeaponShared {
    euler: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
    offset: [AtomicF32::zero(), AtomicF32::zero(), AtomicF32::zero()],
};
#[no_mangle]
pub extern "C" fn cvr_xr_weapon_shared() -> *const WeaponShared {
    &WEAPON
}

impl WeaponShared {
    pub fn euler(&self) -> [f32; 3] {
        [self.euler[0].load(), self.euler[1].load(), self.euler[2].load()]
    }
    pub fn offset(&self) -> [f32; 3] {
        [self.offset[0].load(), self.offset[1].load(), self.offset[2].load()]
    }
}

#[cfg(test)]
mod tests {
    //! The defaults the C++ members had and the order the header's references take.
    use super::*;

    #[test]
    fn defaults_and_order() {
        let w = WeaponShared {
            euler: [AtomicF32::new(1.0), AtomicF32::new(2.0), AtomicF32::new(3.0)],
            offset: [AtomicF32::new(4.0), AtomicF32::new(5.0), AtomicF32::new(6.0)],
        };
        assert_eq!((w.euler(), w.offset()), ([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]));
        assert_eq!(core::mem::offset_of!(WeaponShared, offset), 12);
        assert_eq!((WEAPON.euler(), WEAPON.offset()), ([0.0; 3], [0.0; 3]));
    }
}
