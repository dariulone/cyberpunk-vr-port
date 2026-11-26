//! config/physhands.ini, read again by the worker whenever it changes.
//!
//! TWO STRENGTHS. In the air the hand follows the controller as fast as `move_accel` lets it, so it never trails; once
//! something has stopped it (the solver changed the velocity it was given) it pushes with `force` alone until it has
//! been free for a moment. So it swings quick and presses soft: a cup slides, a wall does not shake it.
use cvr_host::log;
use cvr_host::paths::{file, Folder};

/// What each field holds is said where main's Settings says it (src/Natives/PhysHands.cpp); the numbers are its.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub enabled: i32,
    /// the hands keep their bodies with a weapon out (and with cyberware arms, which are one)
    pub with_weapon: i32,
    /// the bodies move with the player's entity each step (steer); 0: carried by its pace
    pub ride: i32,
    /// the fingers bend on the world
    pub fingers: i32,
    /// kg
    pub mass: f32,
    /// N: the most a hand presses the world with...
    pub force: f32,
    /// ...and pushes a loose thing with (Bonelab's PhysHand.newtonDamp)
    pub push_force: f32,
    pub finger_shapes: i32,
    pub forearm: i32,
    /// 1: the drawn hand IS the body (carried forward by its velocity to the frame); 0: tracking plus the body's offset
    pub drawn: i32,
    /// m/s^2: the most it speeds up in the air
    pub move_accel: f32,
    /// rad/s^2: the most it turns with in the air...
    pub spin_accel: f32,
    /// ...and touching something
    pub spin_contact: f32,
    /// m/s: touching, the most it leans into what it touches...
    pub press_speed: f32,
    /// rad/s: ...and turns toward tracking
    pub press_spin: f32,
    /// the share of the gap it closes a step while it presses the world
    pub press_gain: f32,
    /// m/s
    pub max_speed: f32,
    /// m/s: the most PhysX pushes it out of what it got into
    pub depenetration: f32,
    /// m off its target: put there, not steered
    pub teleport: f32,
    /// m of offset the drawn hand ignores
    pub deadband: f32,
    /// rad of turn it ignores...
    pub turn_deadband: f32,
    /// ...and the most of it shown
    pub max_turn: f32,
    /// m: on a surface the drawn hand stays put while its body stirs within this...
    pub stick_hold: f32,
    /// rad: ...and keeps its turn while the body rocks within this
    pub stick_turn: f32,
    /// the drawn hand's offset filter: its cutoff at rest...
    pub smooth_hz: f32,
    /// ...and how much faster it gets per m/s the offset moves
    pub smooth_beta: f32,
    /// the hand's box, m: along the fingers, across, through the palm
    pub half: [f32; 3],
    /// m from the wrist to the box's middle, along the fingers
    pub along: f32,
    /// friction as Bonelab has it: an open hand's, a closed one's
    pub friction: f32,
    pub friction_grip: f32,
    /// m/s: the most PhysX pushes a loose thing near the hands out of what it got into; 0 the game's
    pub prop_depenetration: f32,
    /// m/s: the same for a thing standing in another (nested plates)
    pub stack_depenetration: f32,
    /// s after the grip opens before the palm meets loose things again
    pub loose_after_grip: f32,
    /// the gun in the hand is boxes on that hand's body
    pub weapon: i32,
    /// rad/s and rad/s^2: a gun's hand keeps its turn this firmly while it touches something
    pub weapon_spin: f32,
    pub weapon_spin_accel: f32,
    /// m/s, rad/s: the gap the body was left behind by is closed at no more than this on top of tracking's own speed
    pub catch_speed: f32,
    pub catch_spin: f32,
    /// 1: every drawn frame written to log/physhands_trace.csv
    pub trace: i32,
    /// the fingers lie on what the palm is on
    pub conform: i32,
    /// a thing taken off a stack: what stood on it comes down
    pub stack_drop: i32,
    /// a held thing meets no loose thing while held, only the world
    pub held_alone: i32,
    /// the hand pushes the body (Bonelab's -F)
    pub body_push: i32,
    /// 1/s
    pub body_push_gain: f32,
    /// m
    pub body_push_dead: f32,
    /// m/s
    pub body_push_most: f32,
    /// the body is not sent farther from the shoulder than V's arm reaches
    pub arm_reach: i32,
    /// m
    pub arm_reach_spare: f32,
    /// a living person near the hands is a few capsules on their bones
    pub people: i32,
    /// V's own hand: the shapes fitted to the skin of V's arms
    pub skin: i32,
    /// m past the skin
    pub skin_margin: f32,
    /// continuous collision: 1 swept, 2 speculative
    pub ccd: i32,
    /// the hand's shapes' contact offset, m (0: PhysX's own)
    pub contact_offset: f32,
    /// on the world as Bonelab's hand is
    pub settle: i32,
    pub press_catch: f32,
    pub press_catch_spin: f32,
    /// the fingers' joints as hinges while they lie on something
    pub finger_hinge_only: i32,
    /// the hand that holds on (climb)
    pub climb: i32,
    pub climb_most: f32,
    /// V's own body and the other hand
    pub self_: i32,
    pub hands_meet: i32,
    /// a person taken hold of
    pub grab_people: i32,
    pub grab_release: f32,
    pub grab_give: f32,
    /// 0: the census decides
    pub group_override: u64,
    pub mask_override: u64,
    /// s between statistics
    pub log_every: i32,
}
impl Default for Settings {
    fn default() -> Settings {
        Settings::DEFAULT
    }
}
impl Settings {
    pub const DEFAULT: Settings = Settings {
            enabled: 1,
            with_weapon: 1,
            ride: 1,
            fingers: 1,
            mass: 0.4,
            force: 20.0,
            push_force: 50.0,
            finger_shapes: 1,
            forearm: 1,
            drawn: 1,
            move_accel: 600.0,
            spin_accel: 2000.0,
            spin_contact: 60.0,
            press_speed: 0.15,
            press_spin: 1.0,
            press_gain: 0.25,
            max_speed: 20.0,
            depenetration: 0.8,
            teleport: 1.0,
            deadband: 0.004,
            turn_deadband: 0.03,
            max_turn: 0.35,
            stick_hold: 0.008,
            stick_turn: 0.05,
            smooth_hz: 6.0,
            smooth_beta: 40.0,
            half: [0.065, 0.045, 0.016],
            along: 0.065,
            friction: 0.05,
            friction_grip: 0.6,
            prop_depenetration: 1.0,
            stack_depenetration: 0.01,
            loose_after_grip: 0.25,
            weapon: 1,
            weapon_spin: 12.0,
            weapon_spin_accel: 1500.0,
            catch_speed: 2.5,
            catch_spin: 10.0,
            trace: 0,
            conform: 1,
            stack_drop: 1,
            held_alone: 1,
            body_push: 1,
            body_push_gain: 12.0,
            body_push_dead: 0.015,
            body_push_most: 2.0,
            arm_reach: 1,
            arm_reach_spare: 0.03,
            people: 1,
            skin: 1,
            skin_margin: 0.002,
            ccd: 1,
            contact_offset: 0.05,
            settle: 1,
            press_catch: 0.5,
            press_catch_spin: 2.0,
            finger_hinge_only: 1,
            climb: 1,
            climb_most: 3.0,
            self_: 1,
            hands_meet: 1,
            grab_people: 1,
            grab_release: 0.30,
            grab_give: 0.04,
            group_override: 0,
            mask_override: 0,
            log_every: 10,
        };
}
pub const SETTINGS_VERSION: i32 = 26;
/// The worker's last reading (Seq<Settings> settings): the physics thread and the animation thread read it.
pub static SETTINGS: super::seq::Seq<Settings> = super::seq::Seq::new(DEFAULT);
const DEFAULT: Settings = Settings::DEFAULT;
/// `Settings s{}; settings.Read(s);`: the last reading, or the defaults before the first.
pub fn current() -> Settings {
    SETTINGS.read().unwrap_or_default()
}
/// settings.Read(s) as a test: the reading, when there is one.
pub fn read() -> Option<Settings> {
    SETTINGS.read()
}

/// The ini's text for these numbers, as main writes it.
pub fn ini_text(d: &Settings) -> String {
    format!(
        "; Physics hands (prototype). Read again while the game runs, within a second of a save; mass and\n\
         ; depenetration at once, the box and friction when the hands are next made (a load).\n\
         version={}\nenabled={}\nwith_weapon={}\ndrawn={}\nfingers={}\nfinger_shapes={}\nforearm={}\nmass={:.2}\n\
         ; N: pressing the world, pushing a loose thing; m/s^2: how quick it is in the air\n\
         force={:.0}\npush_force={:.0}\nmove_accel={:.0}\nspin_accel={:.0}\nspin_contact={:.0}\npress_speed={:.2}\npress_spin={:.1}\npress_gain={:.2}\n\
         max_speed={:.1}\ndepenetration={:.2}\n\
         teleport={:.2}\ndeadband={:.3}\nturn_deadband={:.3}\nmax_turn={:.2}\nstick_hold={:.3}\nstick_turn={:.3}\nsmooth_hz={:.1}\nsmooth_beta={:.1}\n\
         half_along={:.3}\nhalf_across={:.3}\nhalf_through={:.3}\nalong={:.3}\nfriction={:.2}\nfriction_grip={:.2}\n\
         prop_depenetration={:.2}\nstack_depenetration={:.3}\nloose_after_grip={:.2}\nweapon={}\nweapon_spin={:.1}\nweapon_spin_accel={:.0}\n\
         catch_speed={:.2}\ncatch_spin={:.1}\nconform={}\nstack_drop={}\nheld_alone={}\ntrace=0\n\
         ; the hand pressed into the world pushes the body away (1/s per metre past the dead zone, m, m/s)\n\
         body_push={}\nbody_push_gain={:.1}\nbody_push_dead={:.3}\nbody_push_most={:.2}\n\
         ; the hand's body is held within V's arm's reach of the shoulder (m spare past upper arm + forearm)\n\
         arm_reach={}\narm_reach_spare={:.3}\n\
         ; people near the hands are capsules on their bones: the hands stop on them, the fingers lie on them\n\
         people={}\n\
         ; the hand's shapes fitted to the skin of V's arms (0: the box above)\n\
         skin={}\n\
         ; the shapes grown past the skin by this much, m: drawn a hair off a surface, not in it\n\
         skin_margin={:.4}\n\
         ; continuous collision: 1 swept, 2 speculative, 0 none; the shapes' contact offset, m (0: PhysX's own)\n\
         ccd={}\ncontact_offset={:.3}\n\
         ; on the world: tracking's move in full, the gap closed at press_catch m/s / press_catch_spin rad/s,\n\
         ; Bonelab's settling of the hand's push and turn (settle=0: the old soft press)\n\
         settle={}\npress_catch={:.2}\npress_catch_spin={:.2}\n\
         ; the fingers' joints as hinges on a surface (the animation's lean and twist dropped)\nfinger_hinge_only={}\n\
         ; the grip closed on a top or an edge holds on and the player is pulled after it (m/s)\nclimb={}\nclimb_most={:.1}\n\
         ; 0: from the census of the shapes round the player\ngroup=0\nmask=0\nlog_every={}\n\
         ; the palm and fingers meet V's own body (self) and the other hand (hands_meet); the grip closed on a\n\
         ; person holds that part, grab_give m toward the controller, and lets go at grab_release m\n\
         self={}\nhands_meet={}\ngrab_people={}\ngrab_release={:.2}\ngrab_give={:.3}\n",
        SETTINGS_VERSION,
        d.enabled,
        d.with_weapon,
        d.drawn,
        d.fingers,
        d.finger_shapes,
        d.forearm,
        d.mass,
        d.force,
        d.push_force,
        d.move_accel,
        d.spin_accel,
        d.spin_contact,
        d.press_speed,
        d.press_spin,
        d.press_gain,
        d.max_speed,
        d.depenetration,
        d.teleport,
        d.deadband,
        d.turn_deadband,
        d.max_turn,
        d.stick_hold,
        d.stick_turn,
        d.smooth_hz,
        d.smooth_beta,
        d.half[0],
        d.half[1],
        d.half[2],
        d.along,
        d.friction,
        d.friction_grip,
        d.prop_depenetration,
        d.stack_depenetration,
        d.loose_after_grip,
        d.weapon,
        d.weapon_spin,
        d.weapon_spin_accel,
        d.catch_speed,
        d.catch_spin,
        d.conform,
        d.stack_drop,
        d.held_alone,
        d.body_push,
        d.body_push_gain,
        d.body_push_dead,
        d.body_push_most,
        d.arm_reach,
        d.arm_reach_spare,
        d.people,
        d.skin,
        d.skin_margin,
        d.ccd,
        d.contact_offset,
        d.settle,
        d.press_catch,
        d.press_catch_spin,
        d.finger_hinge_only,
        d.climb,
        d.climb_most,
        d.log_every,
        d.self_,
        d.hands_meet,
        d.grab_people,
        d.grab_release,
        d.grab_give,
    )
}
/// sscanf's "%f" / "%d" / "%llx": the longest number at the start of the value (leading blanks skipped).
fn lead(v: &str, ok: impl Fn(usize, char) -> bool) -> &str {
    let v = v.trim_start();
    let end = v.char_indices().find(|&(i, ch)| !ok(i, ch)).map_or(v.len(), |(i, _)| i);
    &v[..end]
}
fn scan_f(v: &str) -> Option<f32> {
    let s = lead(v, |i, ch| ch.is_ascii_digit() || ch == '.' || ((ch == '-' || ch == '+') && i == 0) || ch == 'e' || ch == 'E');
    let mut s = s.to_string();
    // a trailing exponent marker without digits is not part of the number
    while s.ends_with(['e', 'E', '-', '+']) && s.len() > 1 {
        s.pop();
    }
    s.parse().ok()
}
fn scan_i(v: &str) -> Option<i32> {
    lead(v, |i, ch| ch.is_ascii_digit() || ((ch == '-' || ch == '+') && i == 0)).parse().ok()
}
fn scan_x(v: &str) -> Option<u64> {
    let v = v.trim_start();
    let v = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")).unwrap_or(v);
    u64::from_str_radix(lead(v, |_, ch| ch.is_ascii_hexdigit()), 16).ok()
}

/// The ini's numbers over `s` (one line each, `key=value`): main's sscanf chain, its ranges kept.
pub fn parse(text: &str, s: &Settings) -> (Settings, i32) {
    let mut r = *s;
    let mut version = 0;
    for line in text.lines() {
        let Some((key, v)) = line.split_once('=') else { continue };
        let f = || scan_f(v);
        let i = || scan_i(v);
        match key {
            "version" => {
                if let Some(x) = i() {
                    version = x
                }
            }
            "enabled" => i().map_or((), |x| r.enabled = x),
            "with_weapon" => i().map_or((), |x| r.with_weapon = x),
            "ride" => i().map_or((), |x| r.ride = x),
            "fingers" => i().map_or((), |x| r.fingers = x),
            "mass" => f().filter(|v| *v > 0.05 && *v < 50.0).map_or((), |x| r.mass = x),
            "force" => f().filter(|v| *v > 1.0 && *v < 10000.0).map_or((), |x| r.force = x),
            "push_force" => f().filter(|v| *v > 1.0 && *v < 10000.0).map_or((), |x| r.push_force = x),
            "finger_shapes" => i().map_or((), |x| r.finger_shapes = x),
            "forearm" => i().map_or((), |x| r.forearm = x),
            "drawn" => i().map_or((), |x| r.drawn = x),
            "move_accel" => f().filter(|v| *v > 1.0).map_or((), |x| r.move_accel = x),
            "spin_accel" => f().filter(|v| *v > 1.0).map_or((), |x| r.spin_accel = x),
            "spin_contact" => f().filter(|v| *v > 1.0).map_or((), |x| r.spin_contact = x),
            "max_speed" => f().filter(|v| *v > 0.5).map_or((), |x| r.max_speed = x),
            "press_speed" => f().filter(|v| *v > 0.01).map_or((), |x| r.press_speed = x),
            "press_spin" => f().filter(|v| *v > 0.01).map_or((), |x| r.press_spin = x),
            "press_gain" => f().filter(|v| *v > 0.01 && *v <= 1.0).map_or((), |x| r.press_gain = x),
            "depenetration" => f().filter(|v| *v > 0.01).map_or((), |x| r.depenetration = x),
            "teleport" => f().filter(|v| *v > 0.05).map_or((), |x| r.teleport = x),
            "deadband" => f().filter(|v| *v >= 0.0).map_or((), |x| r.deadband = x),
            "turn_deadband" => f().filter(|v| *v >= 0.0).map_or((), |x| r.turn_deadband = x),
            "max_turn" => f().filter(|v| *v >= 0.0).map_or((), |x| r.max_turn = x),
            "stick_hold" => f().filter(|v| *v >= 0.0).map_or((), |x| r.stick_hold = x),
            "stick_turn" => f().filter(|v| *v >= 0.0).map_or((), |x| r.stick_turn = x),
            "smooth_hz" => f().filter(|v| *v > 0.1).map_or((), |x| r.smooth_hz = x),
            "smooth_beta" => f().filter(|v| *v >= 0.0).map_or((), |x| r.smooth_beta = x),
            "half_along" => f().filter(|v| *v > 0.005).map_or((), |x| r.half[0] = x),
            "half_across" => f().filter(|v| *v > 0.005).map_or((), |x| r.half[1] = x),
            "half_through" => f().filter(|v| *v > 0.003).map_or((), |x| r.half[2] = x),
            "along" => f().map_or((), |x| r.along = x),
            "friction" => f().filter(|v| *v >= 0.0).map_or((), |x| r.friction = x),
            "friction_grip" => f().filter(|v| *v >= 0.0).map_or((), |x| r.friction_grip = x),
            "prop_depenetration" => f().filter(|v| *v >= 0.0).map_or((), |x| r.prop_depenetration = x),
            "stack_depenetration" => f().filter(|v| *v >= 0.0).map_or((), |x| r.stack_depenetration = x),
            "loose_after_grip" => f().filter(|v| *v >= 0.0).map_or((), |x| r.loose_after_grip = x),
            "weapon" => i().map_or((), |x| r.weapon = x),
            "weapon_spin" => f().filter(|v| *v > 0.1).map_or((), |x| r.weapon_spin = x),
            "weapon_spin_accel" => f().filter(|v| *v > 1.0).map_or((), |x| r.weapon_spin_accel = x),
            "catch_speed" => f().filter(|v| *v > 0.05).map_or((), |x| r.catch_speed = x),
            "catch_spin" => f().filter(|v| *v > 0.1).map_or((), |x| r.catch_spin = x),
            "trace" => i().map_or((), |x| r.trace = x),
            "conform" => i().map_or((), |x| r.conform = x),
            "stack_drop" => i().map_or((), |x| r.stack_drop = x),
            "held_alone" => i().map_or((), |x| r.held_alone = x),
            "body_push" => i().map_or((), |x| r.body_push = x),
            "body_push_gain" => f().filter(|v| *v >= 0.0 && *v < 100.0).map_or((), |x| r.body_push_gain = x),
            "body_push_dead" => f().filter(|v| *v >= 0.0 && *v < 1.0).map_or((), |x| r.body_push_dead = x),
            "body_push_most" => f().filter(|v| *v >= 0.0 && *v < 10.0).map_or((), |x| r.body_push_most = x),
            "arm_reach" => i().map_or((), |x| r.arm_reach = x),
            "people" => i().map_or((), |x| r.people = x),
            "skin" => i().map_or((), |x| r.skin = x),
            "skin_margin" => f().filter(|v| *v >= 0.0 && *v <= 0.01).map_or((), |x| r.skin_margin = x),
            "ccd" => i().filter(|v| (0..=2).contains(v)).map_or((), |x| r.ccd = x),
            "settle" => i().map_or((), |x| r.settle = x),
            "press_catch" => f().filter(|v| *v > 0.01).map_or((), |x| r.press_catch = x),
            "press_catch_spin" => f().filter(|v| *v > 0.05).map_or((), |x| r.press_catch_spin = x),
            "finger_hinge_only" => i().map_or((), |x| r.finger_hinge_only = x),
            "climb" => i().map_or((), |x| r.climb = x),
            "climb_most" => f().filter(|v| *v > 0.1 && *v < 10.0).map_or((), |x| r.climb_most = x),
            "contact_offset" => f().filter(|v| *v >= 0.0 && *v <= 0.2).map_or((), |x| r.contact_offset = x),
            "arm_reach_spare" => f().filter(|v| *v >= 0.0 && *v < 0.5).map_or((), |x| r.arm_reach_spare = x),
            "self" => i().map_or((), |x| r.self_ = x),
            "hands_meet" => i().map_or((), |x| r.hands_meet = x),
            "grab_people" => i().map_or((), |x| r.grab_people = x),
            "grab_release" => f().filter(|v| *v > 0.05 && *v < 2.0).map_or((), |x| r.grab_release = x),
            "grab_give" => f().filter(|v| *v >= 0.0 && *v < 0.5).map_or((), |x| r.grab_give = x),
            "group" => scan_x(v).map_or((), |x| r.group_override = x),
            "mask" => scan_x(v).map_or((), |x| r.mask_override = x),
            "log_every" => i().filter(|v| *v > 0).map_or((), |x| r.log_every = x),
            _ => {}
        }
    }
    (r, version)
}

fn write(d: &Settings) {
    // fopen "w": text mode, each line ending CRLF
    let _ = std::fs::write(file(Folder::Config, "physhands.ini"), ini_text(d).replace('\n', "\r\n"));
}
/// ReadSettings: the ini over `s`; written with today's numbers when it is missing or an older version's.
pub fn read_settings(s: &mut Settings) {
    let Ok(bytes) = std::fs::read(file(Folder::Config, "physhands.ini")) else {
        write(s); // written once, so the numbers are there to change
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    let (r, version) = parse(&text, s);
    if version < SETTINGS_VERSION {
        // an older prototype's numbers (a 1 kg hand at 250 N, a box the length of the fingers): written again with today's
        write(s);
        log!("[physhands] config/physhands.ini was version {}: written again with version {}'s numbers\n", version, SETTINGS_VERSION);
        return;
    }
    *s = r;
}
