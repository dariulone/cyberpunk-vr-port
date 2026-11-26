//! The FBT calibration view's font choice (src/Fbt/Game/GameFont.cpp, TextLanguage, kChoices and ViewTexts): the text
//! language -- "/language", then "OnScreen", then its "value" in %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\
//! UserSettings.json, found in that order as the .cpp found them, a five-letter `xx-yy` code or en-us; the font for it
//! -- Rajdhani Medium for every Latin-script language, the game's Cyrillic Rajdhani for Russian and Ukrainian, the
//! game's own font for Japanese, Korean, both Chinese, Thai and Arabic; and the view's texts in that language -- the
//! values of the `  ["Step ...` and `  ["Not taken: ...` entries of the menu's translation table
//! (lua/CyberpunkVRPort_Menu/modules/lang/settings/<language>.lua), none for en-us.
//!
//! In the Rust build the .cpp asks `cvr_fbt_text_language`, `cvr_fbt_font_path` and `cvr_fbt_view_texts`; loading
//! the font through the game's resource loader stays there.

use core::ffi::{c_char, c_void};

/// Rajdhani Medium, the VR menu's text face.
pub const RAJDHANI: &str = "base\\gameplay\\gui\\fonts\\raj\\raj-medium.fnt";
const CHOICES: [(&str, &core::ffi::CStr); 8] = [
    ("ru-ru", c"base\\gameplay\\gui\\fonts\\foreign\\russian\\rajdhani_medium_neu.fnt"),
    ("ua-ua", c"base\\gameplay\\gui\\fonts\\foreign\\russian\\rajdhani_medium_neu.fnt"),
    ("jp-jp", c"base\\gameplay\\gui\\fonts\\foreign\\japanese\\mgenplus\\mgenplus-1cp-medium.fnt"),
    ("kr-kr", c"base\\gameplay\\gui\\fonts\\foreign\\korean\\kbiz_go\\kbizgom.fnt"),
    ("zh-cn", c"base\\gameplay\\gui\\fonts\\foreign\\chinese\\jing_xi_heig\\arudjingxiheig30_db.fnt"),
    ("zh-tw", c"base\\gameplay\\gui\\fonts\\foreign\\chinese_traditional\\jing_xi_heig_b5\\arudjingxiheib5_db.fnt"),
    ("th-th", c"base\\gameplay\\gui\\fonts\\foreign\\thai\\th_sarabun_new\\thsarabunnew_bold.fnt"),
    ("ar-ar", c"base\\gameplay\\gui\\fonts\\foreign\\arabic\\ara_es_nawar\\araesnawar-regular.fnt"),
];
const RAJDHANI_C: &core::ffi::CStr = c"base\\gameplay\\gui\\fonts\\raj\\raj-medium.fnt";

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|at| at + from)
}
/// The text language of a UserSettings.json's bytes (en-us when it names none).
pub fn text_language_of(json: &[u8]) -> String {
    let at = find(json, b"\"/language\"", 0)
        .and_then(|at| find(json, b"\"OnScreen\"", at))
        .and_then(|at| find(json, b"\"value\"", at))
        .and_then(|at| find(json, b":", at))
        .and_then(|at| find(json, b"\"", at));
    let Some(at) = at else { return "en-us".into() };
    let code = match find(json, b"\"", at + 1) {
        Some(end) => &json[at + 1..end],
        None => &[][..],
    };
    if code.len() == 5 && code[2] == b'-' {
        String::from_utf8_lossy(code).into_owned()
    } else {
        "en-us".into()
    }
}
/// The player's text language.
pub fn text_language() -> String {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return "en-us".into() };
    let path = std::path::PathBuf::from(local).join("CD Projekt Red").join("Cyberpunk 2077").join("UserSettings.json");
    text_language_of(&std::fs::read(path).unwrap_or_default())
}
/// The font for `language`.
pub fn font_path(language: &str) -> &'static core::ffi::CStr {
    CHOICES.iter().find(|(code, _)| *code == language).map_or(RAJDHANI_C, |(_, path)| path)
}
/// The view's texts of a translation table's bytes, in order.
pub fn view_texts_of(table: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in table.split(|&b| b == b'\n') {
        if !line.starts_with(b"  [\"Step ") && !line.starts_with(b"  [\"Not taken: ") {
            continue;
        }
        let eq = find(line, b"\"] = \"", 0);
        let end = line.windows(2).rposition(|w| w == b"\",");
        if let (Some(eq), Some(end)) = (eq, end) {
            if end > eq + 6 {
                out.extend_from_slice(&line[eq + 6..end]);
            }
        }
    }
    out
}
/// The view's texts in `language` (none for en-us).
pub fn view_texts(language: &str) -> Vec<u8> {
    if language == "en-us" {
        return Vec::new();
    }
    view_texts_of(settings_words(language))
}
/// The VR settings pages' words by language (mods/cet/CyberpunkVRPort_Menu/modules/lang/settings/<code>.lua), built
/// in: the package carries no Lua mod folders any more.
fn settings_words(language: &str) -> &'static [u8] {
    macro_rules! words {
        ($($code:literal),*) => {
            match language {
                $($code => include_bytes!(concat!("../../../../mods/cet/CyberpunkVRPort_Menu/modules/lang/settings/", $code, ".lua")),)*
                _ => b"",
            }
        };
    }
    words!(
        "ar-ar", "cz-cz", "de-de", "es-es", "es-mx", "fr-fr", "hu-hu", "it-it", "jp-jp", "kr-kr", "pl-pl", "pt-br", "ru-ru",
        "th-th", "tr-tr", "ua-ua", "zh-cn", "zh-tw"
    )
}

/// The text language into `out` (at most `cap` bytes); its length.
///
/// # Safety
/// `out` holds `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_text_language(out: *mut u8, cap: usize) -> usize {
    let language = text_language();
    let n = language.len().min(cap);
    // SAFETY: as documented.
    unsafe { core::ptr::copy_nonoverlapping(language.as_ptr(), out, n) };
    n
}
/// The font for the language (a static C string).
///
/// # Safety
/// `language` holds `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_font_path(language: *const u8, len: usize) -> *const c_char {
    // SAFETY: as documented.
    let bytes = if language.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(language, len) } };
    font_path(core::str::from_utf8(bytes).unwrap_or("")).as_ptr()
}
/// The view's texts in the language, appended through `append`.
///
/// # Safety
/// `language` holds `len` bytes; `append` takes the caller's `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_view_texts(language: *const u8, len: usize, user: *mut c_void, append: unsafe extern "C" fn(*mut c_void, *const c_char, usize)) {
    // SAFETY: as documented.
    let bytes = if language.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(language, len) } };
    let texts = view_texts(core::str::from_utf8(bytes).unwrap_or(""));
    if !texts.is_empty() {
        // SAFETY: as documented.
        unsafe { append(user, texts.as_ptr().cast(), texts.len()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_font_and_texts() {
        let json = br#"{"data":[{"name":"/language","options":[{"name":"VoiceOver","value":"en-us"},{"name":"OnScreen","type":"name","value":"ru-ru"}]}]}"#;
        assert_eq!(text_language_of(json), "ru-ru", "OnScreen's value, after /language");
        assert_eq!(text_language_of(br#"{"/language":{"OnScreen":{"value":"english"}}}"#), "en-us", "not a five-letter code");
        assert_eq!(text_language_of(b"{}"), "en-us");
        assert_eq!(text_language_of(br#""OnScreen":{"value":"jp-jp"} "/language""#), "en-us", "OnScreen only after /language");
        assert_eq!(font_path("ru-ru").to_bytes(), b"base\\gameplay\\gui\\fonts\\foreign\\russian\\rajdhani_medium_neu.fnt");
        assert_eq!(font_path("de-de").to_bytes(), RAJDHANI.as_bytes(), "a Latin-script language");
        let table = b"return {\r\n  [\"Step 1\"] = \"Shag odin\",\r\n  [\"Other\"] = \"no\",\r\n  [\"Not taken: arms\"] = \"Ruki\",\n  [\"Step 2\"] = \"\",\n}\n";
        assert_eq!(view_texts_of(table), b"Shag odinRuki".to_vec(), "the Step and Not taken entries, CRLF lines too");
        assert!(view_texts("en-us").is_empty());
    }
}
