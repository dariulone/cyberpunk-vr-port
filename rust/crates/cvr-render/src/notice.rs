//! THE NOTICE, IN THE GAME'S LANGUAGE (formerly src/Lobby/Notice.cpp; Lobby/Notice.hpp keeps the C++ face).
//!
//! The intro's first stage is the game's legal notice -- the bink the boot splash would have played -- in the language
//! the game shows its text in, and under it the port's word: that it is an experimental fan-made mod, not CD PROJEKT
//! RED's. The boot's loading screen plays the bink: the port's archive names the English one where the game's ink
//! settings have their logo loop (initLoadingScreenLogoLoopVideo), and the plugin the player's own as those settings
//! load. The language is the player's settings file's, /language OnScreen: the game takes its own from the same file
//! as it starts, and its settings system answers only its defaults while the game boots.
use core::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use red4ext_abi::cname::CName;
use red4ext_abi::handle::WeakHandle;
use red4ext_abi::rtti::{vfunc, Class, RttiKind};

/// A language the game is translated into: its name in the settings, the suffix of its legal notice's bink (the boot
/// splash's own table, inkSplashScreenLoadingScreenLogicController introMessageBink), the locale its letters are set
/// in, whether it reads right to left, and the port's word in it.
struct Language {
    name: &'static str,
    bink: &'static str,
    locale: &'static str,
    right_to_left: bool,
    lines: [&'static str; 2],
}

const fn lang(
    name: &'static str,
    bink: &'static str,
    locale: &'static str,
    right_to_left: bool,
    lines: [&'static str; 2],
) -> Language {
    Language {
        name,
        bink,
        locale,
        right_to_left,
        lines,
    }
}

/// The first is the one the game falls back to.
static LANGUAGES: [Language; 19] = [
    lang(
        "en-us",
        "en",
        "en-us",
        false,
        [
            "cyberpunkvrport is an experimental fan-made mod.",
            "An unofficial project, not affiliated with CD PROJEKT RED.",
        ],
    ),
    lang(
        "ru-ru",
        "ru",
        "ru-ru",
        false,
        [
            "cyberpunkvrport — экспериментальный фанатский мод.",
            "Неофициальный проект, не связанный с CD PROJEKT RED.",
        ],
    ),
    lang(
        "ua-ua",
        "ua",
        "uk-ua",
        false,
        [
            "cyberpunkvrport — експериментальний фанатський мод.",
            "Неофіційний проєкт, не пов'язаний із CD PROJEKT RED.",
        ],
    ),
    lang(
        "pl-pl",
        "pl",
        "pl-pl",
        false,
        [
            "cyberpunkvrport to eksperymentalny mod stworzony przez fanów.",
            "Nieoficjalny projekt, niezwiązany z CD PROJEKT RED.",
        ],
    ),
    lang(
        "de-de",
        "de",
        "de-de",
        false,
        [
            "cyberpunkvrport ist eine experimentelle Fan-Mod.",
            "Ein inoffizielles Projekt ohne Verbindung zu CD PROJEKT RED.",
        ],
    ),
    lang(
        "fr-fr",
        "fr",
        "fr-fr",
        false,
        [
            "cyberpunkvrport est un mod expérimental créé par des fans.",
            "Un projet non officiel, sans lien avec CD PROJEKT RED.",
        ],
    ),
    lang(
        "es-es",
        "es",
        "es-es",
        false,
        [
            "cyberpunkvrport es un mod experimental creado por fans.",
            "Un proyecto no oficial, sin relación con CD PROJEKT RED.",
        ],
    ),
    lang(
        "es-mx",
        "mx",
        "es-mx",
        false,
        [
            "cyberpunkvrport es un mod experimental creado por fans.",
            "Un proyecto no oficial, sin relación con CD PROJEKT RED.",
        ],
    ),
    lang(
        "it-it",
        "it",
        "it-it",
        false,
        [
            "cyberpunkvrport è una mod sperimentale creata dai fan.",
            "Un progetto non ufficiale, non affiliato a CD PROJEKT RED.",
        ],
    ),
    lang(
        "pt-br",
        "pr",
        "pt-br",
        false,
        [
            "cyberpunkvrport é um mod experimental feito por fãs.",
            "Um projeto não oficial, sem vínculo com a CD PROJEKT RED.",
        ],
    ),
    lang(
        "cz-cz",
        "cz",
        "cs-cz",
        false,
        [
            "cyberpunkvrport je experimentální fanouškovský mód.",
            "Neoficiální projekt bez vazby na CD PROJEKT RED.",
        ],
    ),
    lang(
        "hu-hu",
        "hu",
        "hu-hu",
        false,
        [
            "A cyberpunkvrport egy kísérleti rajongói mod.",
            "Nem hivatalos projekt, nem kapcsolódik a CD PROJEKT RED-hez.",
        ],
    ),
    lang(
        "tr-tr",
        "tr",
        "tr-tr",
        false,
        [
            "cyberpunkvrport, hayranlar tarafından yapılmış deneysel bir moddur.",
            "CD PROJEKT RED ile bağlantısı olmayan resmî olmayan bir projedir.",
        ],
    ),
    lang(
        "jp-jp",
        "jp",
        "ja-jp",
        false,
        [
            "cyberpunkvrport はファンによる実験的なMODです。",
            "CD PROJEKT REDとは関係のない非公式プロジェクトです。",
        ],
    ),
    lang(
        "kr-kr",
        "kr",
        "ko-kr",
        false,
        [
            "cyberpunkvrport는 팬이 만든 실험적인 모드입니다.",
            "CD PROJEKT RED와 관련 없는 비공식 프로젝트입니다.",
        ],
    ),
    lang(
        "zh-cn",
        "zh_cn",
        "zh-cn",
        false,
        [
            "cyberpunkvrport 是一个由玩家制作的实验性模组。",
            "这是一个与 CD PROJEKT RED 无关的非官方项目。",
        ],
    ),
    lang(
        "zh-tw",
        "tw",
        "zh-tw",
        false,
        [
            "cyberpunkvrport 是一個由玩家製作的實驗性模組。",
            "這是一個與 CD PROJEKT RED 無關的非官方專案。",
        ],
    ),
    lang(
        "th-th",
        "th",
        "th-th",
        false,
        [
            "cyberpunkvrport เป็นม็อดทดลองที่สร้างโดยแฟน ๆ",
            "เป็นโปรเจกต์ไม่เป็นทางการ ไม่เกี่ยวข้องกับ CD PROJEKT RED",
        ],
    ),
    lang(
        "ar-ar",
        "ar",
        "ar-sa",
        true,
        [
            "cyberpunkvrport تعديل تجريبي من صنع المعجبين.",
            "مشروع غير رسمي لا يرتبط بشركة CD PROJEKT RED.",
        ],
    ),
];

/// `std::atof`: the longest number at the start of the text (after white space), 0 without one.
fn atof(text: &[u8]) -> f64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) {
        i += 1;
    }
    let start = i;
    if i < text.len() && matches!(text[i], b'+' | b'-') {
        i += 1;
    }
    let digits = |mut j: usize| {
        while j < text.len() && text[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let whole = digits(i);
    let mut end = whole;
    if end < text.len() && text[end] == b'.' {
        end = digits(end + 1);
    }
    if end == i || (end == i + 1 && whole == i) {
        return 0.0;
    }
    if end < text.len() && matches!(text[end], b'e' | b'E') {
        let mut k = end + 1;
        if k < text.len() && matches!(text[k], b'+' | b'-') {
            k += 1;
        }
        let exponent = digits(k);
        if exponent > k {
            end = exponent;
        }
    }
    core::str::from_utf8(&text[start..end])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}

/// What the player's settings file says, read once.
struct Player {
    language: &'static Language,
    effects: f32,
}

fn slider(file: &[u16], name: &[u8]) -> f32 {
    let value = cvr_settings::game_settings::get(file, b"/audio/volume", name);
    if value.is_empty() {
        return 1.0;
    }
    (atof(&value) as f32 / 100.0).clamp(0.0, 1.0)
}

fn player() -> &'static Player {
    static PLAYER: OnceLock<Player> = OnceLock::new();
    PLAYER.get_or_init(|| {
        let file = cvr_settings::game_settings::player_settings_file();
        let on_screen = if file.is_empty() {
            Vec::new()
        } else {
            cvr_settings::game_settings::get(&file, b"/language", b"OnScreen")
        };
        let mut player = Player {
            language: &LANGUAGES[0],
            effects: 1.0,
        };
        for language in &LANGUAGES {
            if on_screen == language.name.as_bytes() {
                player.language = language;
            }
        }
        if !file.is_empty() {
            player.effects = slider(&file, b"MasterVolume") * slider(&file, b"SfxVolume");
        }
        cvr_host::log!(
            "[notice] the game's language {} ({}), effects at {:.2}\n",
            player.language.name,
            if on_screen.is_empty() {
                "not in the settings: the game's own"
            } else {
                "the settings'"
            },
            player.effects as f64
        );
        player
    })
}

/// The port's word as Lobby/Notice.cpp's Word() lays it out: two lines (UTF-16) 250 and 333 px of the notice's 2160
/// under its middle, 51 px tall.
#[repr(C)]
pub struct Word {
    pub lines: [*const u16; 2],
    pub lengths: [u32; 2],
    pub rows: [f32; 2],
    pub size: f32,
    pub light: f32,
    pub locale: *const u16,
    pub locale_length: u32,
    pub right_to_left: bool,
}

struct WordText {
    lines: [Vec<u16>; 2],
    locale: Vec<u16>,
}

/// Fills `out` with the port's word in the player's language; the text lives for the process.
///
/// # Safety
/// `out` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_notice_word(out: *mut Word) {
    static TEXT: OnceLock<WordText> = OnceLock::new();
    let language = player().language;
    let text = TEXT.get_or_init(|| WordText {
        lines: [
            language.lines[0].encode_utf16().collect(),
            language.lines[1].encode_utf16().collect(),
        ],
        locale: language.locale.encode_utf16().collect(),
    });
    // SAFETY: as the caller vouches.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return;
    };
    *out = Word {
        lines: [text.lines[0].as_ptr(), text.lines[1].as_ptr()],
        lengths: [text.lines[0].len() as u32, text.lines[1].len() as u32],
        rows: [250.0, 333.0],
        size: 51.0,
        light: 0.35,
        locale: text.locale.as_ptr(),
        locale_length: text.locale.len() as u32,
        right_to_left: language.right_to_left,
    };
}

/// The game's sliders for effects, master times effects (0..1), as the player's settings file has them.
#[no_mangle]
pub extern "C" fn cvr_notice_effects_volume() -> f32 {
    player().effects
}

const LOGO_FIELD: &str = "initLoadingScreenLogoLoopVideo";

/// The ink settings as they loaded and the logo loop they named, till the game has booted.
struct Boot {
    ink_settings: WeakHandle,
    logo: u64,
    booted: bool,
}
// SAFETY: the weak reference is only touched under the mutex; its counts are the engine's atomics.
unsafe impl Send for Boot {}

static BOOT: Mutex<Boot> = Mutex::new(Boot {
    ink_settings: WeakHandle::null(),
    logo: 0,
    booted: false,
});

/// The logo loop's field of an ink settings resource: `GetNativeType()->GetProperty(...)`, when it is an async
/// resource reference (`require_kind`) or any.
///
/// # Safety
/// `settings` is a live ISerializable.
unsafe fn logo_field(
    settings: *mut c_void,
    require_kind: bool,
) -> Option<&'static red4ext_abi::function::Property> {
    type GetNativeType = unsafe extern "C" fn(*mut c_void) -> *const Class;
    // SAFETY: ISerializable::GetNativeType, vtable +0x00.
    let class = unsafe { vfunc::<GetNativeType>(settings, 0x00)(settings).as_ref() }?;
    let field = class.property(CName::hashed(LOGO_FIELD))?;
    if require_kind {
        // SAFETY: a property's type is the engine's.
        let ty = unsafe { field.ty.as_ref() }?;
        if ty.kind() != RttiKind::ResourceAsyncReference as u8 {
            return None;
        }
    }
    Some(field)
}

/// The ink settings' boot video, their logo loop, as they load: the legal notice's bink in the player's language
/// (Core/ArchiveExtensions AddFixup for inkGameSettingsResource).
///
/// # Safety
/// `settings` is a live inkGameSettingsResource, as the loader hands it over.
#[no_mangle]
pub unsafe extern "C" fn cvr_notice_set_boot_video(settings: *mut c_void) {
    if settings.is_null() {
        return;
    }
    // SAFETY: as the caller vouches.
    let Some(field) = (unsafe { logo_field(settings, true) }) else {
        cvr_host::log!(
            "[notice] the ink settings have no {}: the boot plays what they name\n",
            LOGO_FIELD
        );
        return;
    };
    let path = format!(
        "base\\movies\\fullscreen\\common\\cyberpunk2077_game_intro_message_{}.bk2",
        player().language.bink
    );
    // SAFETY: the field's value, a ResourceAsyncReference<> (its path hash alone).
    let video = unsafe { field.value_ptr::<u64>(settings) };
    {
        let mut boot = BOOT.lock().unwrap_or_else(|e| e.into_inner());
        if boot.booted {
            return;
        }
        // SAFETY: ISerializable's own weak reference to itself, at +0x08.
        boot.ink_settings =
            unsafe { &*((settings as *const u8).add(8) as *const WeakHandle) }.clone();
        // SAFETY: as above.
        unsafe {
            boot.logo = *video;
            *video = red4ext_abi::resource::resource_path(&path);
        }
    }
    cvr_host::log!("[notice] the boot's loading screen plays {}\n", path);
}

/// The game has booted (its main menu is up): the loading screens after the boot loop the game's own logo again.
#[no_mangle]
pub extern "C" fn cvr_notice_boot_over() {
    let mut boot = BOOT.lock().unwrap_or_else(|e| e.into_inner());
    if boot.booted {
        return;
    }
    boot.booted = true;
    let settings = boot.ink_settings.lock();
    boot.ink_settings = WeakHandle::null();
    if settings.is_null() {
        return;
    }
    // SAFETY: a strong reference to the live resource.
    let Some(field) = (unsafe { logo_field(settings.instance, false) }) else {
        return;
    };
    // An aligned 8-byte write: a loading screen starting meanwhile reads the one or the other.
    // SAFETY: as above.
    unsafe { *field.value_ptr::<u64>(settings.instance) = boot.logo };
    cvr_host::log!("[notice] booted: the loading screens loop the game's logo again\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cvr-settings' live controls name the C++ subsystems' parser; the unit tests' stand-in takes no line.
    #[no_mangle]
    extern "C" fn cvr_cpp_settings_parse_subsystem(_which: i32, _line: *const core::ffi::c_char, _s: *mut c_void) -> bool {
        false
    }

    #[test]
    fn atof_reads_the_leading_number() {
        assert_eq!(atof(b"100"), 100.0);
        assert_eq!(atof(b"  42.5abc"), 42.5);
        assert_eq!(atof(b"-3e1"), -30.0);
        assert_eq!(atof(b"1e"), 1.0);
        assert_eq!(atof(b".5"), 0.5);
        assert_eq!(atof(b"abc"), 0.0);
        assert_eq!(atof(b"."), 0.0);
        assert_eq!(atof(b""), 0.0);
    }

    #[test]
    fn languages_match_the_table() {
        assert_eq!(LANGUAGES[0].name, "en-us");
        assert!(LANGUAGES
            .iter()
            .filter(|l| l.right_to_left)
            .all(|l| l.name == "ar-ar"));
        assert_eq!(
            LANGUAGES.iter().find(|l| l.name == "zh-cn").unwrap().bink,
            "zh_cn"
        );
    }
}
