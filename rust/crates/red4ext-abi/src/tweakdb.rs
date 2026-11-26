//! RED4ext::TweakDB: the game's tweak database -- the instance (`TweakDB::Get`, a relocated function) and its two
//! shared spin locks (TweakDB.hpp: `mutex00` at +0x20 over the flats, the flat data buffer and the default values;
//! `mutex01` at +0x21 over the records, queries and groups; SharedSpinLock-inl.hpp).

use core::ffi::c_void;
use core::sync::atomic::{AtomicI8, Ordering::*};

use crate::hashes;
use crate::reloc::Reloc;

static GET: Reloc = Reloc::new(hashes::TweakDB_Get);

/// `TweakDB::Get()`: the database, or null when its function cannot be resolved.
pub fn get() -> *mut c_void {
    type F = unsafe extern "C" fn() -> *mut c_void;
    // SAFETY: TweakDB_Get's signature per TweakDB-inl.hpp.
    match unsafe { GET.func::<F>() } {
        // SAFETY: the engine's accessor.
        Some(get) => unsafe { get() },
        None => core::ptr::null_mut(),
    }
}

/// `mutex00`'s offset: the flats, the flat data buffer and the default values.
pub const FLATS_LOCK: usize = 0x20;
/// `mutex01`'s offset: the records, the queries and the groups.
pub const RECORDS_LOCK: usize = 0x21;

/// A SharedSpinLock: one signed byte, -1 held exclusively, n > 0 held by n readers.
pub struct SharedSpinLock<'a>(&'a AtomicI8);

impl<'a> SharedSpinLock<'a> {
    /// The lock at `offset` in the database.
    ///
    /// # Safety
    /// `db` is the live TweakDB.
    pub unsafe fn of(db: *mut c_void, offset: usize) -> Self {
        // SAFETY: the lock byte inside the live database.
        SharedSpinLock(unsafe { &*(db.cast::<u8>().add(offset) as *const AtomicI8) })
    }
    /// `TryLockShared`: one more reader unless held exclusively.
    pub fn try_lock_shared(&self) -> bool {
        let current = self.0.load(Relaxed);
        current != -1 && self.0.compare_exchange(current, current.wrapping_add(1), SeqCst, SeqCst).is_ok()
    }
    /// `LockShared`: spins, yielding the thread every 512 tries (the count wraps at 0x4000).
    pub fn lock_shared(&self) {
        let mut tries = 0u32;
        while !self.try_lock_shared() {
            tries += 1;
            if tries == 0x4000 {
                tries = 0;
            } else if tries & 511 == 0 {
                // SAFETY: a plain yield.
                unsafe { windows_sys::Win32::System::Threading::SwitchToThread() };
            }
        }
    }
    /// `UnlockShared`: one reader fewer.
    pub fn unlock_shared(&self) {
        self.0.fetch_sub(1, SeqCst);
    }
    /// `std::shared_lock`: held shared until the guard drops.
    pub fn shared(self) -> Shared<'a> {
        self.lock_shared();
        Shared(self)
    }
    /// `TryLock`: held exclusively from free.
    pub fn try_lock(&self) -> bool {
        self.0.compare_exchange(0, -1, SeqCst, SeqCst).is_ok()
    }
    /// `Lock`: spins as LockShared does.
    pub fn lock(&self) {
        let mut tries = 0u32;
        while !self.try_lock() {
            tries += 1;
            if tries == 0x4000 {
                tries = 0;
            } else if tries & 511 == 0 {
                // SAFETY: a plain yield.
                unsafe { windows_sys::Win32::System::Threading::SwitchToThread() };
            }
        }
    }
    /// `Unlock`: free.
    pub fn unlock(&self) {
        self.0.store(0, SeqCst);
    }
    /// `std::lock_guard`: held exclusively until the guard drops.
    pub fn exclusive(self) -> Exclusive<'a> {
        self.lock();
        Exclusive(self)
    }
}
/// A SharedSpinLock held exclusively.
pub struct Exclusive<'a>(SharedSpinLock<'a>);
impl Drop for Exclusive<'_> {
    fn drop(&mut self) {
        self.0.unlock();
    }
}
/// A SharedSpinLock held shared.
pub struct Shared<'a>(SharedSpinLock<'a>);
impl Drop for Shared<'_> {
    fn drop(&mut self) {
        self.0.unlock_shared();
    }
}

// ---- the layout (TweakDB.hpp)
/// `flats`: a SortedUniqueArray<TweakDBID> (entries, capacity, size, flags).
const FLATS: usize = 0x40;
/// `recordsByID`: HashMap<TweakDBID, Handle<IScriptable>>.
const RECORDS_BY_ID: usize = 0x58;
/// `recordsByType`: HashMap<IType*, DynArray<Handle<IScriptable>>>.
const RECORDS_BY_TYPE: usize = 0x88;
/// `flatDataBuffer`.
const FLAT_DATA_BUFFER: usize = 0x148;
/// gamedataTweakDBRecord::recordID (the record is 0x48 bytes, the id last).
const RECORD_ID: usize = 0x40;

/// SortedArray's flag: the entries need sorting before a search.
const NOT_SORTED: i32 = 1;

/// The SDK's HashMap: `indexTable` (+0), `size` (+8), `capacity` (+0xC), the node list's `nodes` (+0x10); a node is
/// `next` (+0), `hashedKey` (+4), the key (+8) and the value (+0x10), 0x20 bytes for both maps here.
#[repr(C)]
struct HashMap {
    index_table: *const u32,
    size: u32,
    capacity: u32,
    nodes: *mut u8,
}
const NODE_BYTES: usize = 0x20;
const INVALID_INDEX: u32 = u32::MAX;

impl HashMap {
    /// `HashMap::Get`: the value of the node whose hashed key and key match, or null.
    ///
    /// # Safety
    /// A live map whose node keys are `u64`s compared by `same`.
    unsafe fn get(&self, hashed: u32, same: impl Fn(u64) -> bool) -> *mut u8 {
        if self.size == 0 {
            return core::ptr::null_mut();
        }
        // SAFETY: the map's own tables.
        unsafe {
            let mut index = *self.index_table.add((hashed % self.capacity) as usize);
            while index != INVALID_INDEX {
                let node = self.nodes.add(index as usize * NODE_BYTES);
                if *(node.add(4) as *const u32) == hashed && same(*(node.add(8) as *const u64)) {
                    return node.add(0x10);
                }
                index = *(node as *const u32);
            }
        }
        core::ptr::null_mut()
    }
}

/// TweakDBID's `operator==`: the name's hash and length (not the offset).
fn same_id(a: u64, b: u64) -> bool {
    a as u32 == b as u32 && (a >> 32) as u8 == (b >> 32) as u8
}
/// TweakDBID's `operator<`: by the hash, then the length.
fn id_less(a: u64, b: u64) -> bool {
    let (ha, hb) = (a as u32, b as u32);
    ha < hb || (ha == hb && ((a >> 32) as u8) < ((b >> 32) as u8))
}
/// HasTDBOffset.
fn has_offset(id: u64) -> bool {
    id >> 40 != 0
}

/// `recordsByID.Get(id)`: the record's handle, or null. The caller holds `mutex01`.
///
/// # Safety
/// `db` is the live TweakDB.
unsafe fn record_slot(db: *mut c_void, id: u64) -> *const crate::handle::Handle {
    // SAFETY: the database's map; HashMapHash<TweakDBID> is the id's low 32 bits.
    unsafe { (*(db.cast::<u8>().add(RECORDS_BY_ID) as *const HashMap)).get(id as u32, |key| same_id(key, id)).cast() }
}

/// `GetFlatValue(id)`: the flat's value object in the flat data buffer -- by the id's own offset, else the flats'
/// sorted ids (sorted first when flagged, as LowerBound does) -- or null.
///
/// # Safety
/// `db` is the live TweakDB.
pub unsafe fn flat_value(db: *mut c_void, id: u64) -> *mut c_void {
    if !crate::tweakdbid::is_valid(id) {
        return core::ptr::null_mut();
    }
    // SAFETY: the database's lock and fields.
    unsafe {
        let _held = SharedSpinLock::of(db, FLATS_LOCK).shared();
        let base = db.cast::<u8>();
        let buffer = *(base.add(FLAT_DATA_BUFFER) as *const usize);
        let offset = if has_offset(id) {
            crate::tweakdbid::offset(id)
        } else {
            let entries = *(base.add(FLATS) as *const *mut u64);
            let size = *(base.add(FLATS + 0xC) as *const u32) as usize;
            let flags = base.add(FLATS + 0x10) as *mut i32;
            if size == 0 || entries.is_null() {
                return core::ptr::null_mut();
            }
            let ids = core::slice::from_raw_parts_mut(entries, size);
            if *flags & NOT_SORTED != 0 {
                ids.sort_unstable_by(|a, b| {
                    if id_less(*a, *b) {
                        core::cmp::Ordering::Less
                    } else if id_less(*b, *a) {
                        core::cmp::Ordering::Greater
                    } else {
                        core::cmp::Ordering::Equal
                    }
                });
                *flags &= !NOT_SORTED;
            }
            let at = ids.partition_point(|entry| id_less(*entry, id));
            if at == size || !same_id(ids[at], id) {
                return core::ptr::null_mut();
            }
            crate::tweakdbid::offset(ids[at])
        };
        (buffer as isize + offset as isize) as *mut c_void
    }
}

/// `FlatValue::GetValue()` (+0xD8): the value's type and its address.
///
/// # Safety
/// `flat` is a live flat value.
pub unsafe fn flat_stack_value(flat: *mut c_void) -> crate::stack::StackType {
    type F = unsafe extern "C" fn(*mut c_void, *mut crate::stack::StackType) -> *mut crate::stack::StackType;
    let mut out = crate::stack::StackType { ty: core::ptr::null(), value: core::ptr::null_mut() };
    // SAFETY: the flat's vtable (the destructor, 13 pairs of offset getters, then GetValue).
    unsafe { crate::rtti::vfunc::<F>(flat, 0xD8)(flat, &mut out) };
    out
}

/// `FlatValue::ToTDBOffset()`: the flat's place in `TweakDB::Get()`'s buffer.
///
/// # Safety
/// `flat` is a flat value of the live database.
pub unsafe fn flat_offset(flat: *mut c_void) -> i32 {
    let db = get();
    if db.is_null() {
        return -1;
    }
    // SAFETY: the database's buffer.
    let buffer = unsafe { *(db.cast::<u8>().add(FLAT_DATA_BUFFER) as *const usize) };
    (flat as usize).wrapping_sub(buffer) as i32
}

/// `GetRecord(id)` (TryGetRecord under `mutex01`, the handle copied): the record's native class, or null; the copy
/// released as the SDK's handle is.
///
/// # Safety
/// `db` is the live TweakDB.
pub unsafe fn record_type(db: *mut c_void, id: u64) -> *mut c_void {
    if !crate::tweakdbid::is_valid(id) {
        return core::ptr::null_mut();
    }
    // SAFETY: the database's lock and map; the handle copied under the lock.
    let record = unsafe {
        let _held = SharedSpinLock::of(db, RECORDS_LOCK).shared();
        match record_slot(db, id).as_ref() {
            Some(handle) => handle.clone(),
            None => return core::ptr::null_mut(),
        }
    };
    if record.instance.is_null() {
        return core::ptr::null_mut();
    }
    type F = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    // SAFETY: ISerializable::GetNativeType (+0x00) on the held record.
    unsafe { crate::rtti::vfunc::<F>(record.instance, 0)(record.instance) }
}

/// `recordsByType.Get(type)`'s first record's id (under `mutex01`), or 0.
///
/// # Safety
/// `db` is the live TweakDB.
pub unsafe fn first_record_of_type(db: *mut c_void, ty: *const c_void) -> u64 {
    // SAFETY: the database's lock and map; HashMapHash<IType*> is FNV1a32 of the pointer's bytes.
    unsafe {
        let _held = SharedSpinLock::of(db, RECORDS_LOCK).shared();
        let key = ty as u64;
        let map = &*(db.cast::<u8>().add(RECORDS_BY_TYPE) as *const HashMap);
        let records = map.get(crate::fnv::fnv1a32(&key.to_le_bytes()), |other| other == key);
        if records.is_null() {
            return 0;
        }
        let entries = *(records as *const *const crate::handle::Handle);
        let size = *(records.add(0xC) as *const u32);
        if size == 0 {
            return 0;
        }
        *((*entries).instance.cast::<u8>().add(RECORD_ID) as *const u64)
    }
}

static DYNARRAY_REALLOC: Reloc = Reloc::new(hashes::DynArray_Realloc);
static CREATE_RECORD: Reloc = Reloc::new(hashes::TweakDB_CreateRecord);

/// The sorted flats sorted first when flagged (LowerBound's own step), then the lower bound of `id`.
///
/// # Safety
/// `flats` is a SortedArray<TweakDBID>.
unsafe fn flats_lower_bound(flats: *mut u8, id: u64) -> usize {
    // SAFETY: as documented.
    unsafe {
        let entries = *(flats as *const *mut u64);
        let size = *(flats.add(0xC) as *const u32) as usize;
        if size == 0 || entries.is_null() {
            return 0;
        }
        let ids = core::slice::from_raw_parts_mut(entries, size);
        let flags = flats.add(0x10) as *mut i32;
        if *flags & NOT_SORTED != 0 {
            ids.sort_unstable_by(|a, b| {
                if id_less(*a, *b) {
                    core::cmp::Ordering::Less
                } else if id_less(*b, *a) {
                    core::cmp::Ordering::Greater
                } else {
                    core::cmp::Ordering::Equal
                }
            });
            *flags &= !NOT_SORTED;
        }
        ids.partition_point(|entry| id_less(*entry, id))
    }
}

/// The SDK's `SortedUniqueArray<TweakDBID>` of `ids`, inserted in order (`InsertOrAssign` when `assign`, else
/// `Insert`): sorted by hash then length, one entry per name.
pub fn unique_flats(ids: &[u64], assign: bool) -> Vec<u64> {
    let mut set: Vec<u64> = Vec::with_capacity(ids.len());
    for &id in ids {
        let at = set.partition_point(|entry| id_less(*entry, id));
        if at < set.len() && same_id(set[at], id) {
            if assign {
                set[at] = id;
            }
        } else {
            set.insert(at, id);
        }
    }
    set
}

/// `db->flats.InsertOrAssign(set)` under `mutex00` held exclusively (SortedArray::Merge): an id already there takes
/// the new value; the others are inserted at their lower bounds, from the back, after the array grows through
/// DynArray_Realloc as Reserve does (geometric 1.5x, alignment 8). False when the array must grow and cannot.
///
/// # Safety
/// `db` is the live TweakDB; `set` sorted and unique (`unique_flats`).
pub unsafe fn merge_flats(db: *mut c_void, set: &[u64]) -> bool {
    // SAFETY: as documented.
    unsafe {
        let _held = SharedSpinLock::of(db, FLATS_LOCK).exclusive();
        let flats = db.cast::<u8>().add(FLATS);
        let mut insertions: Vec<(usize, u64)> = Vec::with_capacity(set.len());
        for &item in set {
            let at = flats_lower_bound(flats, item);
            let entries = *(flats as *const *mut u64);
            let size = *(flats.add(0xC) as *const u32) as usize;
            if at < size && same_id(*entries.add(at), item) {
                *entries.add(at) = item;
                continue;
            }
            insertions.push((at, item));
        }
        if insertions.is_empty() {
            return true;
        }
        let size = *(flats.add(0xC) as *const u32);
        let capacity = *(flats.add(8) as *const u32);
        let new_size = size + insertions.len() as u32;
        if new_size > capacity {
            type Realloc = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void);
            let Some(realloc) = DYNARRAY_REALLOC.func::<Realloc>() else { return false };
            let geometric = capacity + capacity / 2;
            realloc(flats.cast(), new_size.max(geometric), 8, 8, core::ptr::null());
            if *(flats.add(8) as *const u32) < new_size {
                return false;
            }
        }
        let entries = *(flats as *const *mut u64);
        let mut upper = size as usize;
        for (d, &(lower, item)) in insertions.iter().enumerate().rev() {
            let final_position = lower + d;
            let span = upper - lower;
            if span > 0 {
                core::ptr::copy(entries.add(lower), entries.add(final_position + 1), span);
            }
            *entries.add(final_position) = item;
            upper = lower;
        }
        *(flats.add(0xC) as *mut u32) = new_size;
        true
    }
}

/// `CreateRecord(id, type)`: the type's first record's tweak base hash (TweakDBRecord::GetTweakBaseHash, +0x110; the
/// record's handle held to the end), then -- for a valid id no record has -- the engine's CreateTDBRecord over the
/// database. False without a record of the type, for an invalid id, an existing record, or no CreateTDBRecord.
///
/// # Safety
/// `db` is the live TweakDB; `ty` an RTTI class.
pub unsafe fn create_record(db: *mut c_void, id: u64, ty: *const c_void) -> bool {
    // SAFETY: as documented; the handle copied under mutex01.
    unsafe {
        let record = {
            let _held = SharedSpinLock::of(db, RECORDS_LOCK).shared();
            let key = ty as u64;
            let map = &*(db.cast::<u8>().add(RECORDS_BY_TYPE) as *const HashMap);
            let records = map.get(crate::fnv::fnv1a32(&key.to_le_bytes()), |other| other == key);
            if records.is_null() || *(records.add(0xC) as *const u32) == 0 {
                return false;
            }
            (*(*(records as *const *const crate::handle::Handle))).clone()
        };
        type BaseHash = unsafe extern "C" fn(*mut c_void) -> u32;
        let base = crate::rtti::vfunc::<BaseHash>(record.instance, 0x110)(record.instance);
        if !crate::tweakdbid::is_valid(id) || record_exists_held(db, id) {
            return false;
        }
        type Create = unsafe extern "C" fn(*mut c_void, u32, u64);
        let Some(create) = CREATE_RECORD.func::<Create>() else { return false };
        create(db, base, id);
        true
    }
}
/// `GetRecord(id)`'s truth: the id's record handle there and holding an instance (under `mutex01`).
///
/// # Safety
/// `db` is the live TweakDB.
unsafe fn record_exists_held(db: *mut c_void, id: u64) -> bool {
    // SAFETY: as documented.
    unsafe {
        let _held = SharedSpinLock::of(db, RECORDS_LOCK).shared();
        record_slot(db, id).as_ref().is_some_and(|handle| !handle.instance.is_null())
    }
}

/// `recordsByID.Get(id) != nullptr` under `mutex01`.
///
/// # Safety
/// `db` is the live TweakDB.
pub unsafe fn record_exists(db: *mut c_void, id: u64) -> bool {
    // SAFETY: the database's lock and map.
    unsafe {
        let _held = SharedSpinLock::of(db, RECORDS_LOCK).shared();
        !record_slot(db, id).is_null()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_and_writer() {
        let mut db = [0u8; 0x30];
        let p = db.as_mut_ptr().cast::<c_void>();
        // SAFETY: a fake database with the lock byte in place.
        let lock = unsafe { SharedSpinLock::of(p, FLATS_LOCK) };
        assert!(lock.try_lock_shared());
        assert!(lock.try_lock_shared());
        assert_eq!(lock.0.load(SeqCst), 2);
        lock.unlock_shared();
        lock.unlock_shared();
        assert_eq!(lock.0.load(SeqCst), 0);
        lock.0.store(-1, SeqCst);
        assert!(!lock.try_lock_shared());
        // SAFETY: as above; the other lock is the next byte.
        assert_eq!(unsafe { SharedSpinLock::of(p, RECORDS_LOCK) }.0.load(SeqCst), 0);
    }

    /// A TweakDB-shaped buffer: the locks, the flats, the two maps and the flat buffer at their offsets.
    #[repr(C, align(8))]
    struct Db([u8; 0x170]);
    fn put<T>(db: &mut Db, at: usize, value: T) {
        // SAFETY: inside the buffer.
        unsafe { core::ptr::write_unaligned(db.0.as_mut_ptr().add(at) as *mut T, value) }
    }
    fn id(hash: u32, length: u8) -> u64 {
        u64::from(hash) | (u64::from(length) << 32)
    }

    #[test]
    fn flats_by_offset_and_by_search() {
        let mut db = Db([0; 0x170]);
        let buffer = [0u8; 0x100];
        put(&mut db, FLAT_DATA_BUFFER, buffer.as_ptr() as usize);
        // Unsorted, flagged: the search sorts first, as LowerBound does.
        let mut flats = [
            crate::tweakdbid::with_offset(id(30, 4), 0x30),
            crate::tweakdbid::with_offset(id(10, 4), 0x10),
            crate::tweakdbid::with_offset(id(10, 2), 0x20),
        ];
        put(&mut db, FLATS, flats.as_mut_ptr());
        put(&mut db, FLATS + 0xC, 3u32);
        put(&mut db, FLATS + 0x10, NOT_SORTED);
        let p = db.0.as_mut_ptr().cast::<c_void>();
        // SAFETY: the fake database.
        unsafe {
            assert_eq!(flat_value(p, id(10, 4)) as usize, buffer.as_ptr() as usize + 0x10);
            assert_eq!(flat_value(p, id(10, 2)) as usize, buffer.as_ptr() as usize + 0x20);
            assert!(flat_value(p, id(10, 3)).is_null());
            assert!(flat_value(p, 0).is_null());
            // An id with its own offset is not searched.
            assert_eq!(flat_value(p, crate::tweakdbid::with_offset(id(99, 1), 0x44)) as usize, buffer.as_ptr() as usize + 0x44);
        }
        assert_eq!(flats.map(|f| f as u32), [10, 10, 30]);
        assert_eq!(flats[0] >> 32 & 0xFF, 2);
        assert_eq!(db.0[FLATS + 0x10], 0);
        assert_eq!(db.0[FLATS_LOCK], 0);
    }

    #[test]
    fn flats_inserted_and_assigned() {
        // The set: inserted in order, a repeated name assigned only with `assign`.
        let a1 = crate::tweakdbid::with_offset(id(5, 2), 0x11);
        let a2 = crate::tweakdbid::with_offset(id(5, 2), 0x22);
        assert_eq!(unique_flats(&[id(9, 1), a1, id(5, 1), a2], true), vec![id(5, 1), a2, id(9, 1)]);
        assert_eq!(unique_flats(&[a1, a2], false), vec![a1]);
        // The merge: an existing name takes the new value, the others land at their bounds; the room is already there.
        let mut db = Db([0; 0x170]);
        let mut flats = [id(3, 1), crate::tweakdbid::with_offset(id(5, 2), 0x99), id(8, 1), 0, 0, 0];
        put(&mut db, FLATS, flats.as_mut_ptr());
        put(&mut db, FLATS + 8, 6u32);
        put(&mut db, FLATS + 0xC, 3u32);
        let p = db.0.as_mut_ptr().cast::<c_void>();
        // SAFETY: the fake database.
        assert!(unsafe { merge_flats(p, &[id(1, 1), a2, id(6, 1), id(9, 1)]) });
        assert_eq!(flats, [id(1, 1), id(3, 1), a2, id(6, 1), id(8, 1), id(9, 1)]);
        assert_eq!(db.0[FLATS + 0xC], 6);
        assert_eq!(db.0[FLATS_LOCK], 0);
    }

    #[test]
    fn maps_by_hashed_key() {
        let mut db = Db([0; 0x170]);
        // recordsByID: bucket 7 % 2 = 1 chains node 1 (the other length), then node 0 (the wanted id, found by hash and
        // length, not by its offset); bucket 0 is empty.
        let mut index = [INVALID_INDEX, 1u32];
        let mut nodes = [0u8; 2 * NODE_BYTES];
        let wanted = id(7, 3);
        let other = id(7, 5);
        nodes[0..4].copy_from_slice(&INVALID_INDEX.to_le_bytes());
        nodes[4..8].copy_from_slice(&7u32.to_le_bytes());
        nodes[8..16].copy_from_slice(&crate::tweakdbid::with_offset(wanted, 0x99).to_le_bytes());
        nodes[NODE_BYTES..NODE_BYTES + 4].copy_from_slice(&0u32.to_le_bytes());
        nodes[NODE_BYTES + 4..NODE_BYTES + 8].copy_from_slice(&7u32.to_le_bytes());
        nodes[NODE_BYTES + 8..NODE_BYTES + 16].copy_from_slice(&other.to_le_bytes());
        put(&mut db, RECORDS_BY_ID, index.as_mut_ptr());
        put(&mut db, RECORDS_BY_ID + 8, 2u32);
        put(&mut db, RECORDS_BY_ID + 0xC, 2u32);
        put(&mut db, RECORDS_BY_ID + 0x10, nodes.as_mut_ptr());
        let p = db.0.as_mut_ptr().cast::<c_void>();
        // SAFETY: the fake database.
        unsafe {
            assert!(record_exists(p, wanted));
            assert!(record_exists(p, other));
            assert!(!record_exists(p, id(7, 4)));
            assert!(!record_exists(p, id(8, 3)));
        }
        assert_eq!(db.0[RECORDS_LOCK], 0);
    }
}
