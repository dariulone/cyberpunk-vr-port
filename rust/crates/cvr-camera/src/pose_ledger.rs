//! The pose address ledger (formerly include/Camera/PoseAddressLedger.hpp): which tracked pose a camera's bytes were
//! composed from, keyed by the bytes' address and checked against their fingerprint.
//!
//! Fingerprints validate a KNOWN object. They are never used to search for a similar pose: object address plus
//! publication generation establish ancestry.
use cvr_host::fast::FastMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// The pose bytes at an address: position (3 x i32 as the engine stores them) and rotation (4 x f32 bits).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fingerprint {
    pub position: [i32; 3],
    pub rotation: [u32; 4],
}

/// A capture: where, which publication, the bytes then, and what they were composed from. `generation == 0` is none.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Receipt<P> {
    pub source: usize,
    pub generation: u64,
    pub fingerprint: Fingerprint,
    pub payload: P,
}
impl<P> Receipt<P> {
    pub fn valid(&self) -> bool {
        self.generation != 0
    }
}

/// One input of a blend: its receipt, its bytes now, its weight.
#[derive(Clone, Copy, Debug)]
pub struct BlendInput<P> {
    pub source: Receipt<P>,
    pub current: Fingerprint,
    pub weight: f32,
}

struct Record<P> {
    generation: u64,
    /// Written by concurrent readers under the shared lock.
    last_use: AtomicU64,
    fingerprint: Fingerprint,
    payload: P,
}
struct Inner<P> {
    records: FastMap<usize, Record<P>>,
    generation: u64,
}

pub struct PoseAddressLedger<P> {
    inner: RwLock<Inner<P>>,
    clock: AtomicU64,
    capacity: usize,
}

impl<P: Copy> PoseAddressLedger<P> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        PoseAddressLedger { inner: RwLock::new(Inner { records: cvr_host::fast::fast_map(), generation: 0 }), clock: AtomicU64::new(0), capacity }
    }
    pub fn publish(&self, address: usize, fingerprint: Fingerprint, payload: P) {
        if address == 0 {
            return;
        }
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        self.store(&mut inner, address, fingerprint, payload);
    }
    pub fn invalidate(&self, address: usize) {
        self.inner.write().unwrap_or_else(|p| p.into_inner()).records.remove(&address);
    }
    /// Every node dispatch of both eyes reads its camera here, from all render job threads at once: a shared lock
    /// lets those reads run side by side. The use stamp is the only thing a read writes -- one past the latest store,
    /// so a camera read since then still outranks every record stored before it.
    pub fn capture(&self, address: usize, fingerprint: Fingerprint) -> Option<Receipt<P>> {
        let inner = self.inner.read().unwrap_or_else(|p| p.into_inner());
        let r = inner.records.get(&address)?;
        if r.fingerprint != fingerprint {
            return None;
        }
        let used = self.clock.load(Ordering::Relaxed) + 1;
        if r.last_use.load(Ordering::Relaxed) != used {
            r.last_use.store(used, Ordering::Relaxed);
        }
        Some(Receipt { source: address, generation: r.generation, fingerprint: r.fingerprint, payload: r.payload })
    }
    /// Capture before the engine copy; commit only after it returned. Reusing the source buffer, even for identical
    /// coordinates, revokes the receipt.
    pub fn transfer(&self, receipt: &Receipt<P>, source_now: Fingerprint, destination: usize, destination_now: Fingerprint) -> bool {
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        let ok = receipt.valid()
            && inner.records.get(&receipt.source).is_some_and(|s| s.generation == receipt.generation && s.fingerprint == source_now);
        if !ok {
            inner.records.remove(&destination);
            return false;
        }
        self.store(&mut inner, destination, destination_now, receipt.payload);
        true
    }
    /// CameraDirector blends serialized setups without calling the copy hook. Right-multiplication by a common head
    /// rotation commutes with its nlerp; only inputs carrying exactly the same head sample can keep that label.
    pub fn transfer_blend(&self, inputs: &[BlendInput<P>], destination: usize, output: Fingerprint, same_sample: impl Fn(&P, &P) -> bool) -> bool {
        let mut inner = self.inner.write().unwrap_or_else(|p| p.into_inner());
        let mut selected: Option<P> = None;
        let mut total_weight = 0.0f64;
        for input in inputs {
            if !input.weight.is_finite() || input.weight < 0.0 {
                inner.records.remove(&destination);
                return false;
            }
            if input.weight == 0.0 {
                continue;
            }
            total_weight += input.weight as f64;
            let receipt = &input.source;
            let matches = receipt.valid()
                && inner.records.get(&receipt.source).is_some_and(|s| s.generation == receipt.generation && s.fingerprint == input.current)
                && selected.as_ref().is_none_or(|s| same_sample(s, &receipt.payload));
            if !matches {
                inner.records.remove(&destination);
                return false;
            }
            selected = Some(receipt.payload);
        }
        let Some(payload) = selected else {
            inner.records.remove(&destination);
            return false;
        };
        if destination == 0 || (inputs.len() > 1 && (total_weight - 1.0).abs() > 1.0e-6) {
            inner.records.remove(&destination);
            return false;
        }
        self.store(&mut inner, destination, output, payload);
        true
    }
    pub fn len(&self) -> usize {
        self.inner.read().unwrap_or_else(|p| p.into_inner()).records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn store(&self, inner: &mut Inner<P>, address: usize, fingerprint: Fingerprint, payload: P) {
        // A cached VRCAM descriptor can remain alive while thousands of MAIN copies update a few other objects. Bound
        // distinct objects, not the number of publications: activity elsewhere must not expire its ID. Records read
        // in the same interval share a use stamp; the older publication goes first.
        if !inner.records.contains_key(&address) && inner.records.len() >= self.capacity {
            let oldest = inner
                .records
                .iter()
                .min_by_key(|(_, r)| (r.last_use.load(Ordering::Relaxed), r.generation))
                .map(|(&a, _)| a);
            if let Some(a) = oldest {
                inner.records.remove(&a);
            }
        }
        // Stores step the clock by two, so the reads in between (clock + 1) rank above this store and below the next.
        let stamp = self.clock.load(Ordering::Relaxed) + 2;
        self.clock.store(stamp, Ordering::Relaxed);
        inner.generation += 1;
        let generation = inner.generation;
        inner.records.insert(address, Record { generation, last_use: AtomicU64::new(stamp), fingerprint, payload });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fp(k: i32) -> Fingerprint {
        Fingerprint { position: [k, k + 1, k + 2], rotation: [1, 2, 3, k as u32] }
    }
    #[test]
    fn capture_checks_the_bytes() {
        let l = PoseAddressLedger::<u32>::new(8);
        l.publish(0x100, fp(1), 7);
        assert_eq!(l.capture(0x100, fp(1)).map(|r| r.payload), Some(7));
        assert!(l.capture(0x100, fp(2)).is_none(), "other bytes at the address are not the published pose");
        l.publish(0, fp(1), 9);
        assert!(l.capture(0, fp(1)).is_none(), "address 0 is never recorded");
    }
    #[test]
    fn transfer_needs_the_same_publication() {
        let l = PoseAddressLedger::<u32>::new(8);
        l.publish(0x100, fp(1), 7);
        let r = l.capture(0x100, fp(1)).unwrap();
        assert!(l.transfer(&r, fp(1), 0x200, fp(5)));
        assert_eq!(l.capture(0x200, fp(5)).map(|r| r.payload), Some(7));
        l.publish(0x100, fp(1), 8);
        assert!(!l.transfer(&r, fp(1), 0x200, fp(5)), "a republished source revokes the old receipt");
        assert!(l.capture(0x200, fp(5)).is_none(), "and the failed copy erases the destination");
    }
    #[test]
    fn blend_keeps_one_sample() {
        let l = PoseAddressLedger::<u32>::new(8);
        l.publish(0x100, fp(1), 7);
        l.publish(0x200, fp(2), 7);
        let a = BlendInput { source: l.capture(0x100, fp(1)).unwrap(), current: fp(1), weight: 0.25 };
        let b = BlendInput { source: l.capture(0x200, fp(2)).unwrap(), current: fp(2), weight: 0.75 };
        assert!(l.transfer_blend(&[a, b], 0x300, fp(3), |x, y| x == y));
        assert_eq!(l.capture(0x300, fp(3)).map(|r| r.payload), Some(7));
        let c = BlendInput { weight: 0.5, ..b };
        assert!(!l.transfer_blend(&[a, c], 0x300, fp(3), |x, y| x == y), "weights must sum to one");
        assert!(l.capture(0x300, fp(3)).is_none());
    }
    #[test]
    fn eviction_spares_the_recently_read() {
        let l = PoseAddressLedger::<u32>::new(3);
        l.publish(0x100, fp(1), 1);
        l.publish(0x200, fp(2), 2);
        l.publish(0x300, fp(3), 3);
        assert!(l.capture(0x100, fp(1)).is_some());
        l.publish(0x400, fp(4), 4);
        assert!(l.capture(0x100, fp(1)).is_some(), "read since the last store");
        assert!(l.capture(0x200, fp(2)).is_none(), "the oldest unread one goes");
        assert_eq!(l.len(), 3);
    }
}
