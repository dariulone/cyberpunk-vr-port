//! The port's own sounds (formerly src/Audio/PortSound.cpp; include/Audio/PortSound.hpp forwards): 16-bit WAV and Ogg
//! Vorbis files (stb_vorbis, the third-party C library src/Audio/StbVorbis.cpp compiles into the plugin) played through
//! XAudio2 -- one-shot sounds, and named channels whose sound, volume and low-pass filter fade to what they are told,
//! with a loop's seam crossfaded into its samples (equal power) so the loop is one continuous sound. A relative file is
//! under the plugin's Lua folder. Decoded once per file and loop.
#![allow(clippy::missing_safety_doc)]
use core::ffi::{c_char, c_void, CStr};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Media::Audio::XAudio2::{
    IXAudio2, IXAudio2MasteringVoice, IXAudio2SourceVoice, LowPassFilter, XAUDIO2_BUFFER, XAUDIO2_FILTER_PARAMETERS, XAUDIO2_MAX_FILTER_FREQUENCY,
    XAUDIO2_VOICE_STATE,
};
use windows::Win32::Media::Audio::{AudioCategory_GameEffects, WAVEFORMATEX};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

extern "C" {
    /// stb_vorbis (externals/stb), compiled into the plugin by src/Audio/StbVorbis.cpp.
    fn stb_vorbis_decode_memory(mem: *const u8, len: i32, channels: *mut i32, sample_rate: *mut i32, output: *mut *mut i16) -> i32;
    fn free(p: *mut c_void);
}

const WAVE_FORMAT_PCM: u16 = 1;
const XAUDIO2_END_OF_STREAM: u32 = 0x40;
const XAUDIO2_LOOP_INFINITE: u32 = 255;
const XAUDIO2_VOICE_USEFILTER: u32 = 0x8;
const XAUDIO2_VOICE_NOSAMPLESPLAYED: u32 = 0x100;
const XAUDIO2_DEFAULT_PROCESSOR: u32 = 1;
const XAUDIO2_DEFAULT_FREQ_RATIO: f32 = 2.0;
/// NTDDI_VERSION the C++ build passed (WDK_NTDDI_VERSION, NTDDI_WIN11_GE).
const NTDDI_VERSION: u32 = 0x0A00_0010;

fn log(text: &str) {
    super::env::log(text);
}

/// A sound as XAudio2 plays it: 16-bit interleaved PCM, and the region it loops (frames; length 0 = no loop).
#[derive(Default)]
struct Pcm {
    format: WAVEFORMATEX,
    samples: Vec<i16>,
    loop_begin: u32,
    loop_length: u32,
}
impl Pcm {
    fn frames(&self) -> u32 {
        if self.format.nChannels != 0 {
            (self.samples.len() / self.format.nChannels as usize) as u32
        } else {
            0
        }
    }
}

/// RIFF / WAVE, 16-bit PCM only: the 'fmt ' and 'data' chunks, whatever else sits between them.
fn decode_wav(b: &[u8], pcm: &mut Pcm) -> bool {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return false;
    }
    let mut have_format = false;
    let mut data: Option<(usize, usize)> = None;
    let mut at = 12;
    while at + 8 <= b.len() {
        let size = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]) as usize;
        let body = at + 8;
        if body + size > b.len() {
            break;
        }
        if &b[at..at + 4] == b"fmt " && size >= 16 {
            let f = &b[body..body + 16];
            pcm.format = WAVEFORMATEX {
                wFormatTag: u16::from_le_bytes([f[0], f[1]]),
                nChannels: u16::from_le_bytes([f[2], f[3]]),
                nSamplesPerSec: u32::from_le_bytes([f[4], f[5], f[6], f[7]]),
                nAvgBytesPerSec: u32::from_le_bytes([f[8], f[9], f[10], f[11]]),
                nBlockAlign: u16::from_le_bytes([f[12], f[13]]),
                wBitsPerSample: u16::from_le_bytes([f[14], f[15]]),
                cbSize: pcm.format.cbSize,
            };
            have_format = pcm.format.wFormatTag == WAVE_FORMAT_PCM && pcm.format.wBitsPerSample == 16;
        } else if &b[at..at + 4] == b"data" {
            data = Some((body, size));
        }
        at += 8 + size + (size & 1);
    }
    let Some((start, size)) = data else { return false };
    if !have_format || pcm.format.nChannels == 0 {
        return false;
    }
    pcm.samples = b[start..start + size / 2 * 2]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    pcm.format.cbSize = 0;
    !pcm.samples.is_empty()
}
fn decode_ogg(b: &[u8], pcm: &mut Pcm) -> bool {
    let (mut channels, mut rate) = (0i32, 0i32);
    let mut out: *mut i16 = core::ptr::null_mut();
    // SAFETY: stb_vorbis decodes the buffer into one it allocates with malloc.
    let frames = unsafe { stb_vorbis_decode_memory(b.as_ptr(), b.len() as i32, &mut channels, &mut rate, &mut out) };
    if frames <= 0 || out.is_null() || channels <= 0 {
        // SAFETY: free(NULL) is allowed; otherwise stb's own allocation.
        unsafe { free(out as *mut c_void) };
        return false;
    }
    // SAFETY: frames * channels samples, as stb_vorbis reports.
    pcm.samples = unsafe { core::slice::from_raw_parts(out, frames as usize * channels as usize) }.to_vec();
    unsafe { free(out as *mut c_void) };
    let block = (channels * 2) as u16;
    pcm.format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM,
        nChannels: channels as u16,
        nSamplesPerSec: rate as u32,
        nAvgBytesPerSec: rate as u32 * block as u32,
        nBlockAlign: block,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    true
}
/// std::lround on a double, to i64.
fn lround(v: f64) -> i64 {
    v.round() as i64
}
/// The loop from `from` to `to` (the file's end when not after `from`), its seam blended (equal power).
fn make_loop(pcm: &mut Pcm, from: f32, to: f32, crossfade: f32) {
    let (frames, channels, rate) = (pcm.frames(), pcm.format.nChannels as u32, pcm.format.nSamplesPerSec);
    let begin = (from as f64 * rate as f64).clamp(0.0, frames as f64) as u32;
    let mut end = (to as f64 * rate as f64).clamp(0.0, frames as f64) as u32;
    if end <= begin {
        end = frames;
    }
    if begin + 1 >= end {
        return;
    }
    let blend = (0.0f64.max(crossfade as f64 * rate as f64) as u32).min(begin).min(end - begin);
    for i in 0..blend {
        let t = (i as f64 + 0.5) / blend as f64;
        let (fade_in, fade_out) = ((t * 1.5707963267948966).sin(), (t * 1.5707963267948966).cos());
        for c in 0..channels as usize {
            let tail = (end - blend + i) as usize * channels as usize + c;
            let lead = (begin - blend + i) as usize * channels as usize + c;
            let v = lround(pcm.samples[tail] as f64 * fade_out + pcm.samples[lead] as f64 * fade_in);
            pcm.samples[tail] = v.clamp(-32768, 32767) as i16;
        }
    }
    pcm.loop_begin = begin;
    pcm.loop_length = end - begin;
}
fn sound_path(file: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(file);
    if p.is_absolute() {
        p
    } else {
        cvr_host::paths::dir(cvr_host::paths::Folder::Lua).join(p)
    }
}
/// Its key in the cache: the file and its loop.
fn key(file: &str, from: f32, to: f32, crossfade: f32) -> String {
    let f = |v: f32| cvr_crt::format(c"%.4f", &[cvr_crt::Arg::F(v as f64)]).to_str().unwrap_or("").to_string();
    format!("{file}|{}|{}|{}", f(from), f(to), f(crossfade))
}

/// A voice playing one sound, with its volume and filter on their way to where they are told to go.
#[derive(Default)]
struct Voice {
    voice: Option<IXAudio2SourceVoice>,
    pcm: Option<Arc<Pcm>>,
    key: String,
    volume: f32,
    volume_target: f32,
    volume_rate: f32,
    lowpass: f32,
    lowpass_target: f32,
    lowpass_rate: f32,
    ending: bool,
}
struct Audio {
    audio: Option<IXAudio2>,
    master: Option<IXAudio2MasteringVoice>,
    failed: bool,
    cache: HashMap<String, Arc<Pcm>>,
    once: Vec<IXAudio2SourceVoice>,
    channels: HashMap<String, Voice>,
    fading: Vec<Voice>,
    last_tick: u64,
}
// SAFETY: XAudio2 is free-threaded; every access is under the mutex.
unsafe impl Send for Audio {}
static AUDIO: Mutex<Option<Audio>> = Mutex::new(None);
fn with_audio<R>(f: impl FnOnce(&mut Audio) -> R) -> R {
    let mut guard = AUDIO.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(|| Audio {
        audio: None,
        master: None,
        failed: false,
        cache: HashMap::new(),
        once: Vec::new(),
        channels: HashMap::new(),
        fading: Vec::new(),
        last_tick: 0,
    }))
}

impl Audio {
    fn load(&mut self, file: &str, from: f32, to: f32, crossfade: f32) -> Option<Arc<Pcm>> {
        let k = key(file, from, to, crossfade);
        if let Some(pcm) = self.cache.get(&k) {
            return Some(pcm.clone());
        }
        let path = sound_path(file);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let mut decoded = Pcm::default();
        let ogg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ogg"));
        let ok = if ogg {
            decode_ogg(&bytes, &mut decoded)
        } else {
            decode_wav(&bytes, &mut decoded)
        };
        if !ok {
            log(&format!("[sound] {file}: not a 16-bit WAV or an Ogg Vorbis to play\n"));
            return None;
        }
        if from >= 0.0 {
            make_loop(&mut decoded, from, to, crossfade);
        }
        let pcm = Arc::new(decoded);
        self.cache.insert(k, pcm.clone());
        Some(pcm)
    }
    /// XAudio2 on xaudio2_9.dll (as the SDK's XAudio2Create loads it) and a mastering voice; once, or never again.
    unsafe fn start(&mut self) -> bool {
        if self.audio.is_some() {
            return true;
        }
        if self.failed {
            return false;
        }
        // The calling thread has COM already; this only adds a reference (or answers another apartment).
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut audio: Option<IXAudio2> = None;
        let created = (|| -> bool {
            let Ok(dll) = LoadLibraryExW(windows::core::w!("xaudio2_9.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else {
                return false;
            };
            let mut out: *mut c_void = core::ptr::null_mut();
            let hr = if let Some(f) = GetProcAddress(dll, PCSTR(c"XAudio2CreateWithVersionInfo".as_ptr() as *const u8)) {
                type F = unsafe extern "system" fn(*mut *mut c_void, u32, u32, u32) -> windows::core::HRESULT;
                core::mem::transmute::<_, F>(f)(&mut out, 0, XAUDIO2_DEFAULT_PROCESSOR, NTDDI_VERSION)
            } else if let Some(f) = GetProcAddress(dll, PCSTR(c"XAudio2Create".as_ptr() as *const u8)) {
                type F = unsafe extern "system" fn(*mut *mut c_void, u32, u32) -> windows::core::HRESULT;
                core::mem::transmute::<_, F>(f)(&mut out, 0, XAUDIO2_DEFAULT_PROCESSOR)
            } else {
                return false;
            };
            if hr.is_err() || out.is_null() {
                return false;
            }
            audio = Some(<IXAudio2 as windows::core::Interface>::from_raw(out));
            true
        })();
        let mut master: Option<IXAudio2MasteringVoice> = None;
        let mastered = created
            && audio.as_ref().is_some_and(|a| {
                a.CreateMasteringVoice(&mut master, 0, 0, 0, PCWSTR::null(), None, AudioCategory_GameEffects)
                    .is_ok()
            });
        if !mastered {
            self.failed = true;
            log("[sound] no XAudio2 device: the port's own sounds are off\n");
            return false;
        }
        self.audio = audio;
        self.master = master;
        true
    }
}
/// The filter's cutoff as XAudio2 takes it (2 sin(pi f / rate), at most 1: none).
unsafe fn apply_filter(v: &Voice) {
    let Some(voice) = v.voice.as_ref() else { return };
    let mut p = XAUDIO2_FILTER_PARAMETERS {
        Type: LowPassFilter,
        Frequency: XAUDIO2_MAX_FILTER_FREQUENCY,
        OneOverQ: 1.0,
    };
    if let (true, Some(pcm)) = (v.lowpass > 0.0, v.pcm.as_ref()) {
        p.Frequency = XAUDIO2_MAX_FILTER_FREQUENCY.min((2.0 * (3.141592653589793 * v.lowpass as f64 / pcm.format.nSamplesPerSec as f64).sin()) as f32);
    }
    let _ = voice.SetFilterParameters(&p, 0);
}
fn buffer(pcm: &Pcm) -> XAUDIO2_BUFFER {
    XAUDIO2_BUFFER {
        Flags: XAUDIO2_END_OF_STREAM,
        AudioBytes: (pcm.samples.len() * 2) as u32,
        pAudioData: pcm.samples.as_ptr() as *const u8,
        ..Default::default()
    }
}
unsafe fn start_voice(audio: &IXAudio2, v: &mut Voice, pcm: &Arc<Pcm>) -> bool {
    let mut voice: Option<IXAudio2SourceVoice> = None;
    if audio
        .CreateSourceVoice(&mut voice, &pcm.format, XAUDIO2_VOICE_USEFILTER, XAUDIO2_DEFAULT_FREQ_RATIO, None, None, None)
        .is_err()
    {
        return false;
    }
    let Some(voice) = voice else { return false };
    v.pcm = Some(pcm.clone());
    let mut buf = buffer(pcm);
    if pcm.loop_length != 0 {
        buf.LoopBegin = pcm.loop_begin;
        buf.LoopLength = pcm.loop_length;
        buf.LoopCount = XAUDIO2_LOOP_INFINITE;
    }
    let _ = voice.SetVolume(v.volume, 0);
    v.voice = Some(voice);
    apply_filter(v);
    let voice = v.voice.as_ref().unwrap();
    if voice.SubmitSourceBuffer(&buf, None).is_err() || voice.Start(0, 0).is_err() {
        voice.DestroyVoice();
        v.voice = None;
        return false;
    }
    true
}
/// Toward `to` over `fade` seconds from where it is (at once when no fade).
fn aim(value: &mut f32, target: &mut f32, rate: &mut f32, to: f32, fade: f32) {
    *target = to;
    if fade <= 0.0 {
        *value = to;
        *rate = 0.0;
    } else {
        *rate = (to - *value).abs() / fade;
    }
}
unsafe fn finished(voice: &IXAudio2SourceVoice) -> bool {
    let mut st = XAUDIO2_VOICE_STATE::default();
    voice.GetState(&mut st, XAUDIO2_VOICE_NOSAMPLESPLAYED);
    st.BuffersQueued == 0
}
fn step(value: &mut f32, target: f32, rate: f32, dt: f32) {
    if *value == target {
        return;
    }
    let s = rate * dt;
    *value = if rate <= 0.0 || (target - *value).abs() <= s {
        target
    } else if target > *value {
        *value + s
    } else {
        *value - s
    };
}
/// Advances a voice; false when it is done with (let go).
unsafe fn advance(v: &mut Voice, dt: f32) -> bool {
    let Some(voice) = v.voice.as_ref() else { return false };
    if finished(voice) || (v.ending && v.volume <= 0.0) {
        voice.DestroyVoice();
        v.voice = None;
        return false;
    }
    let (volume, lowpass) = (v.volume, v.lowpass);
    step(&mut v.volume, v.volume_target, v.volume_rate, dt);
    step(&mut v.lowpass, v.lowpass_target, v.lowpass_rate, dt);
    if v.volume != volume {
        let _ = voice.SetVolume(v.volume, 0);
    }
    if v.lowpass != lowpass {
        apply_filter(v);
    }
    true
}
unsafe fn fade_out(v: &mut Voice, fade: f32) {
    v.ending = true;
    aim(&mut v.volume, &mut v.volume_target, &mut v.volume_rate, 0.0, fade);
    if let Some(voice) = v.voice.as_ref() {
        let _ = voice.SetVolume(v.volume, 0);
    }
}
unsafe fn text<'a>(p: *const c_char) -> Option<&'a str> {
    (!p.is_null()).then(|| CStr::from_ptr(p).to_str().unwrap_or(""))
}

/// cvr::sound::PlayOnce: a sound played once (at most 16 at a time).
#[no_mangle]
pub unsafe extern "C" fn cvr_sound_play_once(file: *const c_char, volume: f32) -> bool {
    with_audio(|a| {
        let Some(file) = text(file) else { return false };
        if !a.start() {
            return false;
        }
        let mut i = 0;
        while i < a.once.len() {
            if finished(&a.once[i]) {
                a.once[i].DestroyVoice();
                a.once.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let Some(pcm) = a.load(file, -1.0, 0.0, 0.0) else { return false };
        if a.once.len() >= 16 {
            return false;
        }
        let audio = a.audio.as_ref().unwrap();
        let mut voice: Option<IXAudio2SourceVoice> = None;
        if audio
            .CreateSourceVoice(&mut voice, &pcm.format, 0, XAUDIO2_DEFAULT_FREQ_RATIO, None, None, None)
            .is_err()
        {
            return false;
        }
        let Some(voice) = voice else { return false };
        let buf = buffer(&pcm);
        let _ = voice.SetVolume(volume.clamp(0.0, 1.0), 0);
        if voice.SubmitSourceBuffer(&buf, None).is_err() || voice.Start(0, 0).is_err() {
            voice.DestroyVoice();
            return false;
        }
        a.once.push(voice);
        true
    })
}

/// cvr::sound::Sound.
#[repr(C)]
pub struct Sound {
    pub file: *const c_char,
    pub volume: f32,
    pub lowpass: f32,
    pub loop_from: f32,
    pub loop_to: f32,
    pub crossfade: f32,
}
/// cvr::sound::Channel: the named channel's sound, volume and filter, fading over `fade` seconds; an empty file fades
/// it out.
#[no_mangle]
pub unsafe extern "C" fn cvr_sound_channel(name: *const c_char, sound: *const Sound, fade: f32) -> bool {
    with_audio(|a| {
        let (Some(name), Some(sound)) = (text(name), sound.as_ref()) else {
            return false;
        };
        if !a.start() {
            return false;
        }
        let volume = sound.volume.clamp(0.0, 1.0);
        let lowpass = 0.0f32.max(sound.lowpass);
        let file = text(sound.file).unwrap_or("");
        if file.is_empty() {
            if let Some(mut v) = a.channels.remove(name) {
                fade_out(&mut v, fade);
                a.fading.push(v);
            }
            return true;
        }
        let id = key(file, sound.loop_from, sound.loop_to, sound.crossfade);
        if let Some(v) = a.channels.get_mut(name) {
            if v.key == id && v.voice.is_some() {
                v.ending = false;
                aim(&mut v.volume, &mut v.volume_target, &mut v.volume_rate, volume, fade);
                aim(&mut v.lowpass, &mut v.lowpass_target, &mut v.lowpass_rate, lowpass, fade);
                let _ = v.voice.as_ref().unwrap().SetVolume(v.volume, 0);
                apply_filter(v);
                return true;
            }
        }
        if let Some(mut v) = a.channels.remove(name) {
            fade_out(&mut v, fade);
            a.fading.push(v);
        }
        let Some(pcm) = a.load(file, sound.loop_from, sound.loop_to, sound.crossfade) else {
            return false;
        };
        let mut v = Voice {
            key: id,
            volume: if fade > 0.0 { 0.0 } else { volume },
            lowpass,
            ..Voice::default()
        };
        aim(&mut v.volume, &mut v.volume_target, &mut v.volume_rate, volume, fade);
        v.lowpass_target = lowpass;
        let audio = a.audio.as_ref().unwrap();
        if !start_voice(audio, &mut v, &pcm) {
            return false;
        }
        a.channels.insert(name.to_string(), v);
        true
    })
}
/// cvr::sound::Tick (each Present): the channels' and the fading voices' volumes and filters stepped.
#[no_mangle]
pub unsafe extern "C" fn cvr_sound_tick() {
    with_audio(|a| {
        let now = super::env::GetTickCount64();
        let dt = if a.last_tick != 0 {
            0.25f32.min(now.wrapping_sub(a.last_tick) as f32 / 1000.0)
        } else {
            0.0
        };
        a.last_tick = now;
        if a.audio.is_none() {
            return;
        }
        a.channels.retain(|_, v| advance(v, dt));
        let mut i = 0;
        while i < a.fading.len() {
            if advance(&mut a.fading[i], dt) {
                i += 1;
            } else {
                a.fading.swap_remove(i);
            }
        }
    });
}
