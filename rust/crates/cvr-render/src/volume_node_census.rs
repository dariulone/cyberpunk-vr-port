//! The cubic-dispatch (volume/LUT) probe (src/Stereo/CommandListCensus.cpp, `volume_node_note`): each distinct
//! (node RVA, n) of an n x n x n compute dispatch, at most 16, with the views that dispatched it (M = MAIN, V = the
//! second view); every first sighting of a pair for a view logs the whole table as one `[volnode]` line
//! (`<rva hex>:<n>x<n>x<n>(MV) ` per entry, kept to the .cpp's 600-byte buffer: an entry is written only while fewer
//! than 560 bytes are used). The node RVA is 0 when the dispatch arrived outside any node.
//!
//! The C++ half's `volume_node_note` forwards to `cvr_cl_volume_node_note`; the unit tests below check the table and
//! the line.

/// One (node, shape) seen, with the views that dispatched it (bit 1 MAIN, bit 2 the second view).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Seen {
    rva: u32,
    n: u32,
    views: u8,
}
/// The probe's table.
#[derive(Debug, Default)]
pub struct VolumeNodes {
    seen: Vec<Seen>,
}
impl VolumeNodes {
    pub const CAPACITY: usize = 16;
    /// A cubic dispatch noted: the table's line when it is a pair's first sighting for this view, None otherwise
    /// (already reported for this view, or the table full).
    pub fn note(&mut self, rva: u32, n: u32, vrcam: bool) -> Option<String> {
        let bit = if vrcam { 2 } else { 1 };
        match self.seen.iter().position(|s| s.rva == rva && s.n == n) {
            Some(i) if self.seen[i].views & bit != 0 => return None,
            Some(i) => self.seen[i].views |= bit,
            None if self.seen.len() >= Self::CAPACITY => return None,
            None => self.seen.push(Seen { rva, n, views: bit }),
        }
        Some(self.line())
    }
    /// The entries part of the line, as the .cpp's snprintf loop wrote it into its 600-byte buffer.
    fn line(&self) -> String {
        let mut line = String::new();
        for s in &self.seen {
            if line.len() >= 600 - 40 {
                break;
            }
            let m = if s.views & 1 != 0 { "M" } else { "-" };
            let v = if s.views & 2 != 0 { "V" } else { "-" };
            line += &format!("{:X}:{}x{}x{}({m}{v}) ", s.rva, s.n, s.n, s.n);
        }
        line.truncate(599);
        line
    }
}

#[cfg(feature = "rust-build")]
mod abi {
    use super::VolumeNodes;
    use core::ffi::c_char;
    use std::sync::Mutex;

    extern "C" {
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    static TABLE: Mutex<VolumeNodes> = Mutex::new(VolumeNodes { seen: Vec::new() });

    #[no_mangle]
    pub extern "C-unwind" fn cvr_cl_volume_node_note(rva: u32, n: u32, vrcam: bool) {
        let line = TABLE.lock().unwrap_or_else(|p| p.into_inner()).note(rva, n, vrcam);
        let Some(line) = line else { return };
        let text = std::ffi::CString::new(format!("[volnode] cubic dispatches, node:shape(views): {line}"))
            .unwrap_or_default();
        // SAFETY: a C string for the C++ half's log.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    //! The table and the line.
    use super::*;

    #[test]
    fn table() {
        let mut t = VolumeNodes::default();
        assert_eq!(t.note(0x61EE78, 6, false).as_deref(), Some("61EE78:6x6x6(M-) "));
        // The same pair for the same view is reported once.
        assert_eq!(t.note(0x61EE78, 6, false), None);
        // The other view adds its bit and reports again.
        assert_eq!(t.note(0x61EE78, 6, true).as_deref(), Some("61EE78:6x6x6(MV) "));
        assert_eq!(t.note(0x61EE78, 6, true), None);
        // A new shape of the same node, and an unattributed one, are their own entries, in order.
        assert_eq!(t.note(0x61EE78, 16, true).as_deref(), Some("61EE78:6x6x6(MV) 61EE78:16x16x16(-V) "));
        assert_eq!(t.note(0, 2, false).as_deref(), Some("61EE78:6x6x6(MV) 61EE78:16x16x16(-V) 0:2x2x2(M-) "));
    }
    #[test]
    fn capacity() {
        let mut t = VolumeNodes::default();
        for i in 0..16 {
            assert!(t.note(0x100 + i, 4, false).is_some());
        }
        // A full table takes no new pair, but an existing one still gains its other view.
        assert_eq!(t.note(0x200, 4, false), None);
        let line = t.note(0x100, 4, true).expect("the other view");
        assert!(line.starts_with("100:4x4x4(MV) 101:4x4x4(M-) "));
        assert_eq!(line.matches("x4x4(").count(), 16);
        assert!(!line.contains("200:"));
    }
}
