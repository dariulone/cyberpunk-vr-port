//! THE CAR'S SOUND IN THE HANDS (main's src/Runtimes/OpenXRVehicleAudioHaptics.cpp): the Windows default render
//! endpoint captured in loopback mode, its low band and its transients turned into a small per-hand amplitude envelope
//! that the haptics' tick plays under everything else while driving (xr_openxr_vehicle_haptic_gain; 0, the default,
//! leaves it off -- the loopback hears all the output, the car radio included). One thread, started at the end of
//! OpenXR's init and stopped at its shutdown; on foot or muted it only drains the stream.
use core::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Mutex;
use std::thread::JoinHandle;

use cvr_settings::haptic_route;
use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

#[allow(non_upper_case_globals)]
extern "C" {
    #[link_name = "?g_isDriving@@3U?$atomic@_N@std@@A"]
    static g_isDriving: AtomicBool;
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

fn log(text: &str) {
    cvr_host::log::write(text);
}
fn driving() -> bool {
    // SAFETY: the C++ half's atomic.
    unsafe { g_isDriving.load(Relaxed) }
}

/// The mix format's sample kind: (float, pcm).
///
/// # Safety
/// `format` is the endpoint's WAVEFORMATEX(TENSIBLE).
unsafe fn kind(format: *const WAVEFORMATEX) -> (bool, bool) {
    // SAFETY: as documented; packed fields read unaligned.
    unsafe {
        let tag = core::ptr::addr_of!((*format).wFormatTag).read_unaligned();
        let extra = core::ptr::addr_of!((*format).cbSize).read_unaligned();
        if tag == WAVE_FORMAT_EXTENSIBLE && extra >= 22 {
            let sub = core::ptr::addr_of!((*(format as *const WAVEFORMATEXTENSIBLE)).SubFormat).read_unaligned();
            return (sub == SUBTYPE_IEEE_FLOAT, sub == SUBTYPE_PCM);
        }
        (tag == WAVE_FORMAT_IEEE_FLOAT, tag == WAVE_FORMAT_PCM)
    }
}

#[derive(Clone, Copy)]
struct Format {
    rate: u32,
    channels: u16,
    bits: u16,
    block_align: u16,
    float: bool,
    pcm: bool,
}

/// One channel of one frame as -1..1.
///
/// # Safety
/// `frame` holds a whole frame of `f`.
unsafe fn read_sample(frame: *const u8, channel: u16, f: &Format) -> f32 {
    let bytes = (f.bits / 8) as usize;
    // SAFETY: inside the caller's frame.
    unsafe {
        let s = frame.add(channel as usize * bytes);
        if f.float && f.bits == 32 {
            let v = (s as *const f32).read_unaligned();
            return if 1.0 < v { 1.0 } else if v < -1.0 { -1.0 } else { v };
        }
        if !f.pcm {
            return 0.0;
        }
        match f.bits {
            8 => (*s as i32 - 128) as f32 / 128.0,
            16 => (s as *const i16).read_unaligned() as f32 / 32768.0,
            24 => {
                let mut v = *s as i32 | (*s.add(1) as i32) << 8 | (*s.add(2) as i32) << 16;
                if v & 0x80_0000 != 0 {
                    v |= !0xFF_FFFF;
                }
                v as f32 / 8_388_608.0
            }
            32 => ((s as *const i32).read_unaligned() as f64 / 2_147_483_648.0) as f32,
            _ => 0.0,
        }
    }
}

fn clampd(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < v {
        hi
    } else if v < lo {
        lo
    } else {
        v
    }
}

/// The filters' and envelopes' state between packets.
#[derive(Default)]
struct Dsp {
    previous_input: [f64; 2],
    high_passed: [f64; 2],
    low_passed: [f64; 2],
    smoothed: [f64; 2],
    fast_envelope: f64,
    slow_envelope: f64,
}

fn capture(client: &IAudioClient, capture: &IAudioCaptureClient, f: Format) -> windows::core::Result<()> {
    let rate = f64::from(f.rate);
    let hp_alpha = (-2.0 * core::f64::consts::PI * 28.0 / rate).exp();
    let lp_alpha = 1.0 - (-2.0 * core::f64::consts::PI * 320.0 / rate).exp();
    let mut d = Dsp::default();
    let mut was_driving = false;
    let mut envelope_confirmed = false;
    while RUNNING.load(Acquire) {
        // SAFETY (whole loop): the live capture client; each buffer released after it is read.
        let mut packet = unsafe { capture.GetNextPacketSize()? };
        if packet == 0 {
            std::thread::sleep(std::time::Duration::from_millis(3));
            continue;
        }
        while packet > 0 && RUNNING.load(Acquire) {
            let mut data: *mut u8 = core::ptr::null_mut();
            let (mut frames, mut flags) = (0u32, 0u32);
            unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)? };
            // Drain without filtering while on foot or muted: never charge the render thread for audio work, and do not
            // carry an old engine envelope into a car.
            if !driving() || haptic_route::vehicle_gain() <= 0.0 {
                unsafe { capture.ReleaseBuffer(frames)? };
                d = Dsp::default();
                if was_driving {
                    haptic_route::set_vehicle_haptics(0.0, 0.0);
                }
                was_driving = false;
                packet = unsafe { capture.GetNextPacketSize()? };
                continue;
            }
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            let mut energy = [0.0f64; 2];
            for index in 0..frames as usize {
                let mut input = [0.0f64; 2];
                if !silent {
                    // SAFETY: frame `index` of the packet.
                    unsafe {
                        let frame = data.add(index * f.block_align as usize);
                        input[0] = read_sample(frame, 0, &f) as f64;
                        input[1] = if f.channels > 1 { read_sample(frame, 1, &f) as f64 } else { input[0] };
                        // Centre / LFE / surround content carries much of the engine and the shift's transient.
                        for channel in 2..f.channels {
                            let extra = read_sample(frame, channel, &f) as f64 * 0.12;
                            input[0] += extra;
                            input[1] += extra;
                        }
                    }
                }
                for side in 0..2 {
                    d.high_passed[side] = hp_alpha * (d.high_passed[side] + input[side] - d.previous_input[side]);
                    d.previous_input[side] = input[side];
                    d.low_passed[side] += lp_alpha * (d.high_passed[side] - d.low_passed[side]);
                    energy[side] += d.low_passed[side] * d.low_passed[side];
                }
                let full = input[0].abs().max(input[1].abs());
                d.fast_envelope += 0.08 * (full - d.fast_envelope);
                d.slow_envelope += 0.0012 * (full - d.slow_envelope);
            }
            unsafe { capture.ReleaseBuffer(frames)? };
            let gain = haptic_route::vehicle_gain().clamp(0.0, 2.0);
            if driving() && gain > 0.0 && frames > 0 {
                // The low band's RMS is the sustained engine and road; fast minus slow is the shift's or an impact's
                // edge -- the PSVR2 bridge's evidence, reduced to an amplitude envelope (Touch has no PCM).
                let transient = clampd((d.fast_envelope - d.slow_envelope * 1.12) * 12.0, 0.0, 1.0);
                let mut out = [0.0f32; 2];
                for side in 0..2 {
                    let rms = (energy[side] / frames as f64).sqrt();
                    let engine = clampd((rms - 0.0012) / 0.032, 0.0, 1.0);
                    let mut target = clampd(engine * 0.42 + transient * 0.68, 0.0, 0.90) * gain as f64;
                    if target > 1.0 {
                        target = 1.0;
                    }
                    let smoothing = if target > d.smoothed[side] { 0.48 } else { 0.10 };
                    d.smoothed[side] += smoothing * (target - d.smoothed[side]);
                    out[side] = d.smoothed[side] as f32;
                }
                haptic_route::set_vehicle_haptics(out[0], out[1]);
                if !envelope_confirmed && (out[0] > 0.03 || out[1] > 0.03) {
                    envelope_confirmed = true;
                    log(&format!(
                        "OpenXRManager[VehicleHaptics]: driving audio envelope confirmed left={:.2} right={:.2}.\n",
                        out[0], out[1]
                    ));
                }
                was_driving = true;
            } else {
                d.smoothed = [0.0; 2];
                if was_driving {
                    haptic_route::set_vehicle_haptics(0.0, 0.0);
                    was_driving = false;
                }
            }
            packet = unsafe { capture.GetNextPacketSize()? };
        }
    }
    let _ = client;
    Ok(())
}

fn capture_thread() {
    // SAFETY: COM on this thread, uninitialised at its end; the endpoint's objects released by their wrappers.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let opened = (|| -> windows::core::Result<(IAudioClient, IAudioCaptureClient, Format, *mut WAVEFORMATEX)> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let format = client.GetMixFormat()?;
            let (float, pcm) = kind(format);
            let f = Format {
                rate: core::ptr::addr_of!((*format).nSamplesPerSec).read_unaligned(),
                channels: core::ptr::addr_of!((*format).nChannels).read_unaligned(),
                bits: core::ptr::addr_of!((*format).wBitsPerSample).read_unaligned(),
                block_align: core::ptr::addr_of!((*format).nBlockAlign).read_unaligned(),
                float,
                pcm,
            };
            if (!f.float && !f.pcm) || f.rate == 0 || f.channels == 0 || f.block_align < f.channels * (f.bits / 8) {
                CoTaskMemFree(Some(format as *const core::ffi::c_void));
                return Err(AUDCLNT_E_UNSUPPORTED_FORMAT.into());
            }
            if let Err(e) = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 0, 0, format, None) {
                CoTaskMemFree(Some(format as *const core::ffi::c_void));
                return Err(e);
            }
            let capture: IAudioCaptureClient = match client.GetService() {
                Ok(c) => c,
                Err(e) => {
                    CoTaskMemFree(Some(format as *const core::ffi::c_void));
                    return Err(e);
                }
            };
            if let Err(e) = client.Start() {
                CoTaskMemFree(Some(format as *const core::ffi::c_void));
                return Err(e);
            }
            Ok((client, capture, f, format))
        }
    })();
    match opened {
        Err(e) => log(&format!(
            "OpenXRManager[VehicleHaptics]: default-output loopback unavailable result=0x{:08X}.\n",
            e.code().0 as u32
        )),
        Ok((client, capturer, f, format)) => {
            log(&format!(
                "OpenXRManager[VehicleHaptics]: audio-derived engine/gear capture active endpoint={}Hz channels={}.\n",
                f.rate, f.channels
            ));
            let result = capture(&client, &capturer, f);
            // SAFETY: the started client; the mix format the client gave.
            unsafe {
                let _ = client.Stop();
                CoTaskMemFree(Some(format as *const core::ffi::c_void));
            }
            if let Err(e) = result {
                if RUNNING.load(Relaxed) {
                    log(&format!(
                        "OpenXRManager[VehicleHaptics]: loopback capture stopped result=0x{:08X}.\n",
                        e.code().0 as u32
                    ));
                }
            }
        }
    }
    haptic_route::set_vehicle_haptics(0.0, 0.0);
    if com.is_ok() {
        // SAFETY: paired with the CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
}

/// StartVehicleAudioCapture: the capture's thread (once).
pub fn start() {
    if RUNNING.swap(true, AcqRel) {
        return;
    }
    let handle = std::thread::Builder::new().name("cvr-vehicle-audio".into()).spawn(capture_thread).ok();
    *THREAD.lock().unwrap_or_else(|e| e.into_inner()) = handle;
}

/// StopVehicleAudioCapture: the thread asked to stop and joined, the levels zeroed.
pub fn stop() {
    if !RUNNING.swap(false, AcqRel) {
        return;
    }
    if let Some(handle) = THREAD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = handle.join();
    }
    haptic_route::set_vehicle_haptics(0.0, 0.0);
}
