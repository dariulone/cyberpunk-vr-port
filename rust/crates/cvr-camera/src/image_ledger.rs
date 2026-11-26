//! The image pose ledger (formerly include/Camera/ImagePoseLedger.hpp): which pose each image a command list wrote was
//! rendered from, published when the list is submitted.
//!
//! Command-list recording and submission are separate publications. A reset discards unsubmitted metadata, just as it
//! discards the recorded GPU commands. A resource's lifetime number keeps a recycled address from inheriting a label
//! written before the object behind it was replaced (its owner forgets the resource when the object dies).
use cvr_host::fast::FastMap;
use std::sync::Mutex;

/// An image's label: the resource, the queue it was submitted on, the publication's generation (0: none), when, and
/// the pose.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Image<P> {
    pub resource: usize,
    pub queue: usize,
    pub generation: u64,
    pub stamp_ms: u64,
    pub pose: P,
}
impl<P> Image<P> {
    pub fn valid(&self) -> bool {
        self.generation != 0
    }
}

#[derive(Clone, Copy)]
struct Write<P> {
    resource: usize,
    pose: P,
    stable: bool,
    lifetime: u64,
}
struct Inner<P> {
    lists: FastMap<usize, Vec<Write<P>>>,
    images: FastMap<usize, Image<P>>,
    lifetimes: FastMap<usize, u64>,
    latest_stable: Image<P>,
    generation: u64,
    lifetime: u64,
}

pub struct ImagePoseLedger<P> {
    inner: Mutex<Inner<P>>,
}
impl<P: Copy + Default> Default for ImagePoseLedger<P> {
    fn default() -> Self {
        Self::new()
    }
}
impl<P: Copy + Default> ImagePoseLedger<P> {
    pub fn new() -> Self {
        ImagePoseLedger {
            inner: Mutex::new(Inner {
                lists: cvr_host::fast::fast_map(),
                images: cvr_host::fast::fast_map(),
                lifetimes: cvr_host::fast::fast_map(),
                latest_stable: Image::default(),
                generation: 0,
                lifetime: 0,
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<P>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// A list reset: what it recorded and was not submitted is gone.
    pub fn reset(&self, list: usize) {
        self.lock().lists.entry(list).or_default().clear();
    }
    /// A write to `resource` recorded on `list`. The last write to each target in a list is kept; an unknown pose is
    /// explicit too: it revokes an earlier label when the pixels are replaced.
    pub fn record(&self, list: usize, resource: usize, pose: P, stable: bool) {
        if list == 0 || resource == 0 {
            return;
        }
        let mut s = self.lock();
        let s = &mut *s;
        let lifetime = match s.lifetimes.get(&resource) {
            Some(&l) => l,
            None => {
                s.lifetime += 1;
                s.lifetimes.insert(resource, s.lifetime);
                s.lifetime
            }
        };
        let writes = s.lists.entry(list).or_default();
        if let Some(at) = writes.iter().position(|w| w.resource == resource) {
            writes.remove(at);
        }
        writes.push(Write {
            resource,
            pose,
            stable,
            lifetime,
        });
    }
    /// `list` submitted on `queue`: its writes published (those whose resource is still the object they were written
    /// to), the stable ones as the latest stable image.
    pub fn submit(&self, queue: usize, list: usize, stamp_ms: u64) {
        let mut s = self.lock();
        let s = &mut *s;
        let Some(writes) = s.lists.get(&list) else {
            return;
        };
        for w in writes {
            if s.lifetimes.get(&w.resource) != Some(&w.lifetime) {
                continue;
            }
            s.generation += 1;
            let image = Image {
                resource: w.resource,
                queue,
                generation: s.generation,
                stamp_ms,
                pose: w.pose,
            };
            s.images.insert(w.resource, image);
            if w.stable {
                s.latest_stable = image;
            }
        }
    }
    pub fn read(&self, resource: usize) -> Image<P> {
        self.lock()
            .images
            .get(&resource)
            .copied()
            .unwrap_or_default()
    }
    pub fn latest_stable(&self) -> Image<P> {
        self.lock().latest_stable
    }
    /// The object behind `resource` died.
    pub fn forget_resource(&self, resource: usize) {
        let mut s = self.lock();
        s.images.remove(&resource);
        s.lifetimes.remove(&resource);
        if s.latest_stable.resource == resource {
            s.latest_stable = Image::default();
        }
    }
    /// The list object died.
    pub fn forget_list(&self, list: usize) {
        self.lock().lists.remove(&list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_is_published_by_its_submission_only() {
        let l = ImagePoseLedger::<u32>::new();
        l.record(1, 100, 7, false);
        assert!(!l.read(100).valid());
        l.submit(9, 2, 5);
        assert!(!l.read(100).valid());
        l.submit(9, 1, 5);
        let i = l.read(100);
        assert!(i.valid() && i.pose == 7 && i.queue == 9 && i.stamp_ms == 5 && i.generation == 1);
    }
    #[test]
    fn the_last_write_to_a_target_wins_and_a_reset_discards() {
        let l = ImagePoseLedger::<u32>::new();
        l.record(1, 100, 7, false);
        l.record(1, 100, 8, false);
        l.submit(1, 1, 0);
        assert_eq!(l.read(100).pose, 8);
        l.record(2, 200, 3, false);
        l.reset(2);
        l.submit(1, 2, 0);
        assert!(!l.read(200).valid());
    }
    #[test]
    fn a_recycled_address_does_not_inherit_a_label() {
        let l = ImagePoseLedger::<u32>::new();
        l.record(1, 100, 7, true);
        l.forget_resource(100);
        l.submit(1, 1, 0);
        assert!(!l.read(100).valid());
        l.record(1, 100, 9, true);
        l.submit(1, 1, 0);
        // The new write replaced the old one in the list; the address has a new lifetime.
        let i = l.read(100);
        assert!(i.valid() && i.pose == 9);
        assert_eq!(l.latest_stable().pose, 9);
        l.forget_resource(100);
        assert!(!l.latest_stable().valid());
    }
    #[test]
    fn generations_count_every_publication() {
        let l = ImagePoseLedger::<u32>::new();
        l.record(1, 100, 1, false);
        l.record(1, 101, 2, false);
        l.submit(1, 1, 0);
        l.submit(1, 1, 0);
        assert_eq!(l.read(100).generation, 3);
        assert_eq!(l.read(101).generation, 4);
        l.forget_list(1);
        l.submit(1, 1, 0);
        assert_eq!(l.read(101).generation, 4);
    }
}
