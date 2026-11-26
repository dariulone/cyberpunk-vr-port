//! vrport.ini's first_launch, rewritten in place (the plugin's first launch, rust/crates/cvr-ffi/src/first_launch.rs).

const KEY: &[u8] = b"first_launch";
/// The C++ copy reads at most this much of vrport.ini.
const READ_LIMIT: usize = 16384 - 1;

/// vrport.ini with first_launch rewritten in place (only at the start of a line, so the word inside a comment cannot be
/// mistaken for the key), or appended when it is not there; nothing else of the file changes.
pub fn rewrite_flag(raw: &[u8], value: i32) -> Vec<u8> {
    let len = raw.len().min(READ_LIMIT);
    let raw = &raw[..len];
    // The C++ copy treats what it read as a C string.
    let buf = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
    let at = (0..buf.len()).find(|&p| (p == 0 || buf[p - 1] == b'\n') && buf[p..].starts_with(KEY));
    let mut out = Vec::with_capacity(buf.len() + 32);
    match at {
        Some(at) => {
            let tail = buf[at..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(buf.len(), |i| at + i);
            out.extend_from_slice(&buf[..at]);
            out.extend_from_slice(format!("first_launch={value}").as_bytes());
            out.extend_from_slice(&buf[tail..]);
        }
        None => {
            out.extend_from_slice(buf);
            if len != 0 && raw[len - 1] != b'\n' {
                out.push(b'\n');
            }
            out.extend_from_slice(format!("first_launch={value}\n").as_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::rewrite_flag;

    #[test]
    fn rewrites_the_key_at_a_line_start() {
        assert_eq!(
            rewrite_flag(b"a=1\nfirst_launch=1\nb=2\n", 0),
            b"a=1\nfirst_launch=0\nb=2\n"
        );
        assert_eq!(rewrite_flag(b"first_launch=1", 0), b"first_launch=0");
        assert_eq!(
            rewrite_flag(b"# first_launch=1\nfirst_launch = 1\r\nx", 0),
            b"# first_launch=1\nfirst_launch=0\nx"
        );
    }

    #[test]
    fn appends_the_key_when_missing() {
        assert_eq!(rewrite_flag(b"a=1", 0), b"a=1\nfirst_launch=0\n");
        assert_eq!(rewrite_flag(b"a=1\n", 0), b"a=1\nfirst_launch=0\n");
        assert_eq!(rewrite_flag(b"", 0), b"first_launch=0\n");
        assert_eq!(
            rewrite_flag(b"# first_launch\n", 0),
            b"# first_launch\nfirst_launch=0\n"
        );
    }
}
