//! CyberpunkVRPort_Menu/modules/jobs.lua: background reads for the menu. A producer is a factory that returns a step
//! function: each call does a small piece of work and returns More, or Done with the value and its stamp. `step` runs
//! queued producers a step at a time inside a per-frame time budget, so a journal or inventory read never stalls one
//! frame. Results outlive the producer (and a menu close): a panel draws the last result at once and redraws when a
//! newer one differs (its stamp).
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

pub(super) type JobValue = Rc<dyn Any>;
pub(super) enum Step {
    More,
    Done(JobValue, Option<String>),
}
pub(super) type StepFn = Box<dyn FnMut() -> Step>;
pub(super) type Factory = Rc<dyn Fn() -> StepFn>;
/// A producer done in one step.
pub(super) fn once<T: 'static>(f: impl Fn() -> (T, Option<String>) + 'static) -> Factory {
    let f = Rc::new(f);
    Rc::new(move || {
        let f = f.clone();
        Box::new(move || {
            let (v, s) = f();
            Step::Done(Rc::new(v), s)
        })
    })
}
pub(super) struct Rec {
    pub value: JobValue,
    pub stamp: Option<String>,
    pub at: Instant,
    pub seq: u64,
}
struct Running {
    factory: Factory,
    step: Option<StepFn>,
}
#[derive(Default)]
pub(super) struct Jobs {
    results: HashMap<String, Rec>,
    running: HashMap<String, Running>,
    queue: Vec<String>,
}
impl Jobs {
    pub(super) fn get<T: 'static>(&self, key: &str) -> Option<Rc<T>> {
        self.results.get(key).and_then(|r| r.value.clone().downcast::<T>().ok())
    }
    pub(super) fn seq(&self, key: &str) -> Option<u64> {
        self.results.get(key).map(|r| r.seq)
    }
    pub(super) fn has(&self, key: &str) -> bool {
        self.results.contains_key(key)
    }
    pub(super) fn busy(&self, key: &str) -> bool {
        self.running.contains_key(key)
    }
    pub(super) fn drop_key(&mut self, key: &str) {
        self.results.remove(key);
    }
    pub(super) fn drop_prefix(&mut self, prefix: &str) {
        self.results.retain(|k, _| !k.starts_with(prefix));
    }
    /// Replace a result in place (a local edit the next read would only confirm).
    pub(super) fn put<T: 'static>(&mut self, key: &str, value: T, stamp: Option<String>) {
        let seq = self.results.get(key).map_or(0, |r| r.seq) + 1;
        self.results.insert(key.to_string(), Rec { value: Rc::new(value), stamp, at: Instant::now(), seq });
    }
    /// Run a producer to its end now.
    pub(super) fn now(factory: &Factory) -> (JobValue, Option<String>) {
        let mut step = factory();
        loop {
            if let Step::Done(v, s) = step() {
                return (v, s);
            }
        }
    }
    /// Queue `factory` under `key` unless it is already running; `front` puts it ahead of the queue.
    pub(super) fn run(&mut self, key: &str, factory: Factory, front: bool) {
        if self.running.contains_key(key) {
            if front {
                if let Some(i) = self.queue.iter().position(|k| k == key) {
                    let k = self.queue.remove(i);
                    self.queue.insert(0, k);
                }
            }
            return;
        }
        self.running.insert(key.to_string(), Running { factory, step: None });
        if front {
            self.queue.insert(0, key.to_string());
        } else {
            self.queue.push(key.to_string());
        }
    }
    fn finish(&mut self, key: &str, value: JobValue, stamp: Option<String>) {
        self.running.remove(key);
        self.queue.retain(|k| k != key);
        if let Some(old) = self.results.get_mut(key) {
            if stamp.is_some() && old.stamp == stamp {
                old.at = Instant::now();
                return;
            }
        }
        let seq = self.results.get(key).map_or(0, |r| r.seq) + 1;
        self.results.insert(key.to_string(), Rec { value, stamp, at: Instant::now(), seq });
    }
    /// One step of `key`; true when it has finished.
    fn advance(&mut self, key: &str) -> bool {
        super::menu_main::trace(&format!("job {key}"));
        let Some(job) = self.running.get_mut(key) else { return true };
        if job.step.is_none() {
            job.step = Some((job.factory)());
            return false;
        }
        let r = job.step.as_mut().map_or(Step::More, |s| s());
        match r {
            Step::Done(v, s) => {
                self.finish(key, v, s);
                true
            }
            Step::More => false,
        }
    }
    /// Run queued producers until `budget` seconds of this frame are spent.
    pub(super) fn step(&mut self, budget: f64) {
        let t0 = Instant::now();
        while let Some(k) = self.queue.first().cloned() {
            if t0.elapsed().as_secs_f64() >= budget {
                break;
            }
            self.advance(&k);
        }
    }
    /// Finish `key` now if it takes no more than `budget` seconds; true when its result is in.
    pub(super) fn finish_now(&mut self, key: &str, budget: f64) -> bool {
        let t0 = Instant::now();
        while self.running.contains_key(key) {
            if self.advance(key) {
                break;
            }
            if t0.elapsed().as_secs_f64() >= budget {
                return false;
            }
        }
        self.results.contains_key(key)
    }
    /// Seconds since `key`'s result was last confirmed (infinity without one).
    pub(super) fn age(&self, key: &str) -> f64 {
        self.results.get(key).map_or(f64::INFINITY, |r| r.at.elapsed().as_secs_f64())
    }
}
