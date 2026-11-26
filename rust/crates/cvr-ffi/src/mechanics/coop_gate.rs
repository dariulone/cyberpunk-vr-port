//! NightCoop (co-op) and the port's script hooks: whose goes on a function first.
//!
//! NightCoop wraps a scripted function by patching its bytecode (a prologue that calls its native slot before the
//! game's body) and refuses one that is native already. The port's hooks (script_hooks.rs) swap the engine's function
//! object for a native one and keep the original in an object of their own. Both act in their first Running frame:
//! ours first, NightCoop found 18 functions co-op relies on native ("a CET mod took it over before NightCoop") --
//! PlayerPuppet.OnGameAttached / OnDetach (partners' bodies are PlayerPuppets here), ScriptedPuppet.OnDamageReceived,
//! the damage system, NPC reactions, the main / pause / load menus and saves -- and left them unwrapped.
//!
//! So with NightCoop in the process the port's hooks wait until its wraps are placed: then the function ours swap out
//! is NightCoop's patched body, and ours run it as the original (NightCoop's prologue included). Without NightCoop
//! nothing waits.
//!
//! Placed is what NightCoop's SDK answers: hookCoopUses ("does co-op rely on this function now") gives an area mask for
//! a function it wraps in that frame and the port never hooks (an error code, -2, the frame before). A bound keeps the
//! port working should it never answer: SETTLE_MS after the first ask.
use crate::script_hooks::{HookFn, Kind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Functions NightCoop wraps in its first Running frame (logs/nightcoop-hooks.txt) that no hook of the port's takes.
const SENTINELS: [(&core::ffi::CStr, &core::ffi::CStr); 3] = [
    (c"PauseMenuGameController", c"OnInitialize"),
    (c"LoadGameMenuGameController", c"OnInitialize"),
    (c"Door", c"MoveDoor"),
];
const SETTLE_MS: u128 = 5000;
/// Functions NightCoop wraps later than that frame (its cyberware look, a few seconds into the main menu): ours on them
/// wait one by one, until hookCoopUses answers for the function or LATE_MS passed.
const LATE: [&str; 1] = ["CyberwareCustomizationSystem.RestoreCyberwareCustomization"];
const LATE_MS: u128 = 30000;

static SETTLED: AtomicBool = AtomicBool::new(false);
static WAIT: Mutex<(Option<Instant>, u32)> = Mutex::new((None, 0));
struct Late {
    class: String,
    name: String,
    kind: Kind,
    hook: HookFn,
    since: Instant,
}
static PENDING: Mutex<Vec<Late>> = Mutex::new(Vec::new());

/// Whether a hook of the port's on `class.name` waits for NightCoop's late wrap (and is kept for [`tick`]) instead of
/// being installed now.
pub(super) fn defer(class: &str, name: &str, kind: Kind, hook: HookFn) -> bool {
    let full = format!("{class}.{name}");
    if !LATE.contains(&full.as_str()) || !cvr_rig::vrik::nightcoop::loaded() {
        return false;
    }
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    p.push(Late { class: class.to_owned(), name: name.to_owned(), kind, hook, since: Instant::now() });
    cvr_host::log!("[nightcoop] {full}: the port's hook waits for NightCoop's late wrap\n");
    true
}

/// Each frame: the deferred hooks whose function NightCoop wrapped (or whose wait ran out) installed.
pub(super) fn tick() {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if p.is_empty() {
        return;
    }
    p.retain(|l| {
        let (Ok(class), Ok(name)) = (std::ffi::CString::new(l.class.as_str()), std::ffi::CString::new(l.name.as_str())) else {
            return false;
        };
        let uses = cvr_rig::vrik::nightcoop::coop_uses(&class, &name);
        let waited = l.since.elapsed().as_millis();
        if !uses.is_some_and(|u| u > 0) && waited < LATE_MS {
            return true;
        }
        let ok = crate::script_hooks::install(&l.class, &l.name, l.kind, l.hook);
        cvr_host::log!(
            "[nightcoop] {}.{}: the port's hook installed {ok} after {waited} ms ({})\n",
            l.class,
            l.name,
            if waited < LATE_MS { "over NightCoop's wrap" } else { "NightCoop never answered" }
        );
        false
    });
}

/// The port's script hooks may be installed now: no NightCoop, or its wraps placed (or the wait's bound passed).
/// Main thread, from the Running state's update.
pub(super) fn hooks_may_install() -> bool {
    if SETTLED.load(Ordering::Acquire) {
        return true;
    }
    if !cvr_rig::vrik::nightcoop::loaded() {
        SETTLED.store(true, Ordering::Release);
        return true;
    }
    let mut w = WAIT.lock().unwrap_or_else(|p| p.into_inner());
    let since = *w.0.get_or_insert_with(Instant::now);
    w.1 += 1;
    let uses = SENTINELS.map(|(class, name)| cvr_rig::vrik::nightcoop::coop_uses(class, name));
    let waited = since.elapsed().as_millis();
    let why = if let Some(i) = uses.iter().position(|u| u.is_some_and(|v| v > 0)) {
        format!(
            "hookCoopUses answers {:#x} for {}.{}",
            uses[i].unwrap_or(0),
            SENTINELS[i].0.to_string_lossy(),
            SENTINELS[i].1.to_string_lossy()
        )
    } else if waited >= SETTLE_MS {
        format!("no answer in {SETTLE_MS} ms (hookCoopUses {uses:?}): installed anyway")
    } else {
        if w.1 == 1 {
            cvr_host::log!("[nightcoop] NightCoop is loaded: the port's script hooks wait for its wraps (hookCoopUses {uses:?})\n");
        }
        return false;
    };
    cvr_host::log!("[nightcoop] NightCoop's wraps are placed ({why}; frame {}, {waited} ms): the port's script hooks go over them\n", w.1);
    SETTLED.store(true, Ordering::Release);
    true
}
