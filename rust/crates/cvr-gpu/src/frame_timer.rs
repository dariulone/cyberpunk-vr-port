//! The game's GPU frame time for framegen's statistics (formerly src/Framegen/GpuTimer.cpp; the header keeps
//! `BeforeGameCommands` / `FinishGpuFrame`, which forward here with framegen's metrics switch and its frame sink).
//!
//! A timestamp before the game's first command list of a frame and one after its last, on the game's queue, each in a
//! command list of its own; eight slots in flight, resolved into a readback buffer and read once the queue's fence
//! passes the slot. Production callers hold the image-submission lock before either entry point; the markers submit
//! through the (hooked) queue recursively, which the thread-local guard turns away. When the metrics go off, a begin
//! marker still open gets the fence signalled and the timer is dropped once the GPU passed it.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering::*};
use std::cell::Cell;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// Framegen's metrics switch (src/Framegen/Framegen.cpp MetricsEnabled).
pub type MetricsEnabled = unsafe extern "C" fn() -> bool;
/// Framegen's sink for one GPU frame time in milliseconds (OnGpuFrame).
pub type OnGpuFrame = unsafe extern "C" fn(f64);

const SLOTS: usize = 8;

struct Slot {
    begin_allocator: ID3D12CommandAllocator,
    end_allocator: ID3D12CommandAllocator,
    begin: ID3D12GraphicsCommandList,
    end: ID3D12GraphicsCommandList,
    value: u64,
    pending: bool,
}
struct Timer {
    queue: ID3D12CommandQueue,
    fence: ID3D12Fence,
    heap: ID3D12QueryHeap,
    readback: ID3D12Resource,
    slots: Vec<Slot>,
    value: u64,
    frequency: u64,
    cursor: usize,
    opened: bool,
    stopped: bool,
}
// SAFETY: D3D12 objects are free-threaded; the timer is used under TIMER's lock.
unsafe impl Send for Timer {}

impl Timer {
    unsafe fn create(queue: &ID3D12CommandQueue, metrics: MetricsEnabled) -> Option<Timer> {
        let mut device: Option<ID3D12Device> = None;
        queue.GetDevice(&mut device).ok()?;
        let device = device?;
        crate::hardware_telemetry::cvr_gpu_hw_telemetry_configure(device.as_raw(), metrics);
        let query = D3D12_QUERY_HEAP_DESC {
            Count: 16,
            Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
            NodeMask: 0,
        };
        let read = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 128,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).ok()?;
        let mut heap: Option<ID3D12QueryHeap> = None;
        device.CreateQueryHeap(&query, &mut heap).ok()?;
        let mut readback: Option<ID3D12Resource> = None;
        device
            .CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &read,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut readback,
            )
            .ok()?;
        let frequency = queue.GetTimestampFrequency().ok()?;
        let mut slots = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            let begin_allocator: ID3D12CommandAllocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .ok()?;
            let end_allocator: ID3D12CommandAllocator = device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .ok()?;
            let begin: ID3D12GraphicsCommandList = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &begin_allocator, None)
                .ok()?;
            let end: ID3D12GraphicsCommandList = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &end_allocator, None)
                .ok()?;
            let _ = begin.Close();
            let _ = end.Close();
            slots.push(Slot {
                begin_allocator,
                end_allocator,
                begin,
                end,
                value: 0,
                pending: false,
            });
        }
        Some(Timer {
            queue: queue.clone(),
            fence,
            heap: heap?,
            readback: readback?,
            slots,
            value: 0,
            frequency,
            cursor: 0,
            opened: false,
            stopped: false,
        })
    }
    unsafe fn poll(&mut self, on_frame: OnGpuFrame) {
        let complete = self.fence.GetCompletedValue();
        for i in 0..self.slots.len() {
            if !(self.slots[i].pending && complete >= self.slots[i].value) {
                continue;
            }
            let mut data: *mut c_void = core::ptr::null_mut();
            let range = D3D12_RANGE {
                Begin: i * 16,
                End: i * 16 + 16,
            };
            if self.readback.Map(0, Some(&range), Some(&mut data)).is_ok() {
                let ticks = (data as *const u8).add(i * 16) as *const u64;
                let (t0, t1) = (*ticks, *ticks.add(1));
                if t1 >= t0 {
                    on_frame((t1 - t0) as f64 * 1000.0 / self.frequency as f64);
                }
                let none = D3D12_RANGE { Begin: 0, End: 0 };
                self.readback.Unmap(0, Some(&none));
            }
            self.slots[i].pending = false;
        }
    }
    unsafe fn begin(&mut self, on_frame: OnGpuFrame) {
        if self.opened {
            return;
        }
        self.poll(on_frame);
        let cursor = self.cursor;
        let slot = &self.slots[cursor];
        if slot.pending {
            return;
        }
        if slot.begin_allocator.Reset().is_err()
            || slot.begin.Reset(&slot.begin_allocator, None).is_err()
        {
            return;
        }
        slot.begin
            .EndQuery(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, (cursor * 2) as u32);
        if slot.begin.Close().is_err() {
            return;
        }
        self.queue
            .ExecuteCommandLists(&[Some(slot.begin.cast().expect("a command list"))]);
        self.opened = true;
    }
    unsafe fn end(&mut self, on_frame: OnGpuFrame) {
        if !self.opened {
            self.poll(on_frame);
            return;
        }
        let cursor = self.cursor;
        let slot = &self.slots[cursor];
        if slot.end_allocator.Reset().is_err() || slot.end.Reset(&slot.end_allocator, None).is_err()
        {
            return;
        }
        slot.end.EndQuery(
            &self.heap,
            D3D12_QUERY_TYPE_TIMESTAMP,
            (cursor * 2 + 1) as u32,
        );
        slot.end.ResolveQueryData(
            &self.heap,
            D3D12_QUERY_TYPE_TIMESTAMP,
            (cursor * 2) as u32,
            2,
            &self.readback,
            (cursor * 16) as u64,
        );
        if slot.end.Close().is_err() {
            return;
        }
        self.queue
            .ExecuteCommandLists(&[Some(slot.end.cast().expect("a command list"))]);
        self.value += 1;
        let _ = self.queue.Signal(&self.fence, self.value);
        let slot = &mut self.slots[cursor];
        slot.value = self.value;
        slot.pending = true;
        self.opened = false;
        self.cursor = (cursor + 1) % SLOTS;
        self.poll(on_frame);
    }
    unsafe fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        // A begin marker without its end marker yet is guarded; no resolve is needed with the statistics off.
        if self.opened {
            self.value += 1;
            let _ = self.queue.Signal(&self.fence, self.value);
            self.opened = false;
        }
    }
}

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}
static TIMER: Mutex<Option<Timer>> = Mutex::new(None);
static TIMER_EXISTS: AtomicBool = AtomicBool::new(false);

/// The begin marker, before the game's commands of a frame on `queue`.
///
/// # Safety
/// `queue` is the game's live command queue; the callbacks are framegen's.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_frame_timer_before(
    queue: *mut c_void,
    metrics: MetricsEnabled,
    on_frame: OnGpuFrame,
) {
    if INSIDE.get() || !metrics() {
        return;
    }
    let mut timer = TIMER.lock().unwrap_or_else(|p| p.into_inner());
    if !metrics() {
        return;
    }
    INSIDE.set(true);
    if let Some(t) = timer.as_mut() {
        if t.queue.as_raw() != queue {
            t.stop();
        }
        if t.stopped {
            if t.fence.GetCompletedValue() < t.value {
                INSIDE.set(false);
                return;
            }
            *timer = None;
            TIMER_EXISTS.store(false, Relaxed);
        }
    }
    let Some(queue) = ID3D12CommandQueue::from_raw_borrowed(&queue) else {
        INSIDE.set(false);
        return;
    };
    if timer.is_none() {
        if let Some(t) = Timer::create(queue, metrics) {
            *timer = Some(t);
            TIMER_EXISTS.store(true, Relaxed);
        }
    }
    if let Some(t) = timer.as_mut() {
        if t.queue.as_raw() == queue.as_raw() {
            t.begin(on_frame);
        }
    }
    INSIDE.set(false);
}

/// The end marker after the game's last commands of a frame (or the timer stopped when the metrics went off).
///
/// # Safety
/// The callbacks are framegen's.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_frame_timer_finish(
    queue: *mut c_void,
    metrics: MetricsEnabled,
    on_frame: OnGpuFrame,
) {
    if INSIDE.get() || !TIMER_EXISTS.load(Relaxed) {
        return;
    }
    let mut timer = TIMER.lock().unwrap_or_else(|p| p.into_inner());
    let Some(t) = timer.as_mut() else {
        return;
    };
    INSIDE.set(true);
    if metrics() && !t.stopped && t.queue.as_raw() == queue {
        t.end(on_frame);
    } else {
        t.stop();
    }
    if t.stopped && t.fence.GetCompletedValue() >= t.value {
        *timer = None;
        TIMER_EXISTS.store(false, Relaxed);
    }
    INSIDE.set(false);
}
