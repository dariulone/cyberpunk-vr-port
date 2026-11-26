//! vrcam.json's scanners and the names derived from it (include/Stereo/VrcamConfig.hpp's helpers, for
//! the selection in vrcam_selection.rs): a scanner, not a parser -- the file is a handful of fields and a comment block,
//! and a ':' is required between a key and its value, so a key name quoted inside a comment string is not the field.

/// The engine's CName hash (FNV-1a 64).
pub fn cname_hash(s: &[u8]) -> u64 {
    s.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}
pub fn find(text: &[u8], byte: u8, from: usize) -> Option<usize> {
    text.get(from..)?
        .iter()
        .position(|&b| b == byte)
        .map(|p| p + from)
}
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// The span of `"key": "value"` in a flat JSON object.
pub fn json_string_span(text: &[u8], key: &str) -> Option<(usize, usize)> {
    let key = key.as_bytes();
    let mut i = 0;
    while let Some(quote) = find(text, b'"', i) {
        let name = quote + 1;
        let name_end = find(text, b'"', name)?;
        i = name_end + 1;
        if &text[name..name_end] != key {
            continue;
        }
        let mut q = i;
        while q < text.len() && matches!(text[q], b' ' | b'\t' | b'\r' | b'\n') {
            q += 1;
        }
        if q >= text.len() || text[q] != b':' {
            continue; // not "key": ... -> keep looking
        }
        q += 1;
        while q < text.len() && matches!(text[q], b' ' | b'\t' | b'\r' | b'\n') {
            q += 1;
        }
        if q >= text.len() || text[q] != b'"' {
            return None; // the value is not a string
        }
        let value = q + 1;
        let value_end = find(text, b'"', value)?;
        return Some((value, value_end - value));
    }
    None
}
pub fn json_string(text: &[u8], key: &str) -> Option<Vec<u8>> {
    let (at, len) = json_string_span(text, key)?;
    (len != 0).then(|| text[at..at + len].to_vec())
}
/// The strings of a `"key": [ "a", "b" ]` array (None when there is no such array).
pub fn json_string_array(text: &[u8], key: &str) -> Option<Vec<Vec<u8>>> {
    Some(
        json_string_array_spans(text, key)?
            .into_iter()
            .map(|(at, len)| text[at..at + len].to_vec())
            .collect(),
    )
}
/// json_string_array's strings as (start, length) spans in `text`.
pub fn json_string_array_spans(text: &[u8], key: &str) -> Option<Vec<(usize, usize)>> {
    let key = key.as_bytes();
    let mut i = 0;
    while let Some(quote) = find(text, b'"', i) {
        let name = quote + 1;
        let name_end = find(text, b'"', name)?;
        i = name_end + 1;
        if &text[name..name_end] != key {
            continue;
        }
        let mut q = i;
        while q < text.len() && is_space(text[q]) {
            q += 1;
        }
        if q >= text.len() || text[q] != b':' {
            continue;
        }
        q += 1;
        while q < text.len() && is_space(text[q]) {
            q += 1;
        }
        if q >= text.len() || text[q] != b'[' {
            return None;
        }
        let end = find(text, b']', q)?;
        let mut out = Vec::new();
        let mut p = q + 1;
        while p < end {
            let Some(s) = find(text, b'"', p).filter(|&s| s <= end) else {
                break;
            };
            let Some(e) = find(text, b'"', s + 1).filter(|&e| e <= end) else {
                break;
            };
            if e > s + 1 {
                out.push((s + 1, e - s - 1));
            }
            p = e + 1;
        }
        return Some(out);
    }
    None
}

/// vrcam_feed_<suffix> from vrcam_<suffix>, the authored naming convention.
pub fn derive_camera(component: &[u8]) -> Option<Vec<u8>> {
    let suffix = component.strip_prefix(b"vrcam_")?;
    if 11 + suffix.len() > 127 {
        return None; // the C++'s 128-byte buffer: _snprintf_s refused the truncation
    }
    let mut camera = b"vrcam_feed_".to_vec();
    camera.extend_from_slice(suffix);
    Some(camera)
}
/// `%d` as sscanf reads it: optional leading whitespace, a sign, at least one digit.
pub fn scan_int(s: &[u8], i: &mut usize) -> Option<i32> {
    while *i < s.len() && is_space(s[*i]) {
        *i += 1;
    }
    let mut negative = false;
    if *i < s.len() && (s[*i] == b'+' || s[*i] == b'-') {
        negative = s[*i] == b'-';
        *i += 1;
    }
    let start = *i;
    let mut v: i64 = 0;
    while *i < s.len() && s[*i].is_ascii_digit() {
        v = (v * 10 + (s[*i] - b'0') as i64).min(i64::from(i32::MAX) + 1);
        *i += 1;
    }
    if *i == start {
        return None;
    }
    let v = if negative { -v } else { v };
    Some(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}
/// `%d` as the UCRT's sscanf stores it: optional leading whitespace, a sign, at least one digit, parsed as a 64-bit
/// signed value (LLONG_MAX / LLONG_MIN past its range) and stored into the int by truncation.
pub fn scanf_int(s: &[u8], i: &mut usize) -> Option<i32> {
    while *i < s.len() && is_space(s[*i]) {
        *i += 1;
    }
    let mut negative = false;
    if *i < s.len() && (s[*i] == b'+' || s[*i] == b'-') {
        negative = s[*i] == b'-';
        *i += 1;
    }
    let start = *i;
    let limit: u64 = if negative { 1 << 63 } else { (1 << 63) - 1 };
    let mut value: u64 = 0;
    let mut overflow = false;
    while *i < s.len() && s[*i].is_ascii_digit() {
        let digit = (s[*i] - b'0') as u64;
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(next) if next <= limit => value = next,
            _ => overflow = true,
        }
        *i += 1;
    }
    if *i == start {
        return None;
    }
    let wide: i64 = if overflow {
        if negative { i64::MIN } else { i64::MAX }
    } else if negative {
        (value as i64).wrapping_neg()
    } else {
        value as i64
    };
    Some(wide as i32)
}
/// The "<W>x<H>" tail of a component name; None for names that do not carry one.
pub fn parse_resolution(component: &[u8]) -> Option<(i32, i32)> {
    let underscore = component.iter().rposition(|&b| b == b'_')?;
    let s = &component[underscore + 1..];
    let mut i = 0;
    let a = scanf_int(s, &mut i)?;
    if i >= s.len() || s[i] != b'x' {
        return None;
    }
    i += 1;
    let b = scanf_int(s, &mut i)?;
    if i < s.len() {
        return None; // "%dx%d%c" read a third field
    }
    (a > 0 && b > 0).then_some((a, b))
}
/// `atoi`: leading whitespace, a sign, the digits; 0 when there are none.
pub fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    scan_int(s, &mut i).unwrap_or(0)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanners_match_the_header() {
        let text = br#"{ "_comment": "the \"component\": field", "component" : "vrcam_2560x2560", "virtualCamera":"x",
            "components": [ "vrcam_2560x2560", "", "vrcam_3072x3072" ] }"#;
        assert_eq!(json_string(text, "component").unwrap(), b"vrcam_2560x2560");
        assert_eq!(json_string(text, "virtualCamera").unwrap(), b"x");
        let list = json_string_array(text, "components").unwrap();
        assert_eq!(
            list,
            vec![b"vrcam_2560x2560".to_vec(), b"vrcam_3072x3072".to_vec()]
        );
        assert_eq!(
            derive_camera(b"vrcam_2560x2560").unwrap(),
            b"vrcam_feed_2560x2560"
        );
        assert!(derive_camera(b"cam").is_none());
        assert_eq!(
            parse_resolution(b"vrcam_feed_2648x2864"),
            Some((2648, 2864))
        );
        assert_eq!(parse_resolution(b"vrcam_2648x2864a"), None);
        assert_eq!(parse_resolution(b"vrcam_feed"), None);
        assert_eq!(parse_resolution(b"vrcam_0x10"), None);
        assert_eq!(cname_hash(b"vrcam_feed"), 0x8D23_967F_656E_A945);
        assert_eq!(atoi(b" 2560\r\nheight=1"), 2560);
    }
}
