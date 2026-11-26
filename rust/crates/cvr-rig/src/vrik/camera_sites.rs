//! The camera pose sites (include/Hooks/CameraPoseSites.hpp, the same table): CP2077 2.5, SHA256
//! d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4, call sites re-found by hand (2.31 values in git
//! history). Each is an RVA and the bytes the port was written against; a hook checks them before it patches.
use cvr_engine::code::read_bytes;

pub struct Site {
    pub rva: usize,
    pub bytes: &'static str,
}

pub const SERIALIZE: Site = Site { rva: 0x5FC318, bytes: "48895c2408488974" };
pub const GENERIC_SERIALIZE: Site = Site { rva: 0x7E8530, bytes: "48895c24084889742410574883ec40" };
pub const RTT_DESCRIPTOR_CALL: Site = Site { rva: 0xD29C71, bytes: "488d9590000000f30f1185d8000000488d8d40080000e860994eff" };
pub const BLEND: Site = Site { rva: 0x5FB8F0, bytes: "488bc4488958205556574156" };
pub const BLEND_FINALIZE: Site = Site { rva: 0x5FBE64, bytes: "4883ec2883790800" };
pub const BLEND_FINALIZE_CALL: Site = Site { rva: 0x5FBE21, bytes: "488d4c2430e839000000" };
pub const WORLD_BLEND_SOURCE_READ: Site = Site { rva: 0xEBE4C3, bytes: "488b8fc0000000" };
pub const WORLD_BLEND_STORE: Site = Site { rva: 0xEBE7C2, bytes: "f20f1006488d9fe0feffff" };
pub const WORLD_BLEND_NOTIFY_CALL: Site = Site { rva: 0xEBE7F6, bytes: "ff9040020000" };
pub const SETUP_COPY: Site = Site { rva: 0x2A5C80, bytes: "f20f1002f20f1101" };
pub const SETUP_DEFAULT: Site = Site { rva: 0x2A5D40, bytes: "33c048c7411c0000" };
pub const MAIN_READ: Site = Site { rva: 0x2A59B8, bytes: "488bc44889580848" };
pub const RTT_BUILD: Site = Site { rva: 0xD29AA8, bytes: "48895c2410555657" };
pub const RTT_REFRESH: Site = Site { rva: 0x490300, bytes: "48895c24084889742410574883ec40" };
pub const PANZER_AIM_UPDATE: Site = Site { rva: 0x2BB99F8, bytes: "48895c240855488bec4881ec80000000" };
pub const PANZER_AIM_CALL: Site = Site { rva: 0x2BB9839, bytes: "e8ba010000" };
pub const PANZER_WEAPON_AIM: Site = Site { rva: 0x2B97BA8, bytes: "488bc4488958084889781055488da828" };
pub const VEHICLE_CAMERA_POSITION: Site = Site { rva: 0x2BAE290, bytes: "40534883ec404c8bc1488bda8b89a000" };
pub const VEHICLE_CAMERA_ROTATION: Site = Site { rva: 0x2BAE35C, bytes: "40534883ec30488bda488bd18b89a000" };
pub const PANZER_WEAPON_POSITION_CALL: Site = Site { rva: 0x2B97CC8, bytes: "e8c3650100" };
pub const PANZER_WEAPON_ROTATION_CALL: Site = Site { rva: 0x2B97CFD, bytes: "e85a660100" };
pub const VEHICLE_WEAPON_LAUNCH: Site = Site { rva: 0x2B96D44, bytes: "488bc448895808488970104889781855" };
pub const VEHICLE_WEAPON_TARGETED_LAUNCH_CALL: Site = Site { rva: 0x24CC50C, bytes: "e833a86c00" };
pub const DESCRIPTOR_BUILD: Site = Site { rva: 0x2135EC, bytes: "488bc44889580848" };
pub const CAMERA_CLONE: Site = Site { rva: 0x2130D0, bytes: "48895c2410488974" };
pub const RENDER_BUILD: Site = Site { rva: 0x42579C, bytes: "48895c2408488974" };
pub const CAMERA_COPY: Site = Site { rva: 0x21297C, bytes: "48895c2410574883" };
pub const RENDER_CALL: Site = Site { rva: 0x42493F, bytes: "4c8d8680020000488d55f0488d8d90000000e8460e0000" };

/// The site's bytes in the image at `exe` are the ones the port was written against.
pub fn site_matches(exe: usize, site: &Site) -> bool {
    let hex = site.bytes.as_bytes();
    let mut expected = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks(2) {
        let Some(b) = core::str::from_utf8(pair).ok().and_then(|s| u8::from_str_radix(s, 16).ok()) else { return false };
        expected.push(b);
    }
    let mut actual = vec![0u8; expected.len()];
    read_bytes(exe + site.rva, &mut actual) && actual == expected
}
