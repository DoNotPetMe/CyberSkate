//! RED4ext plugin: Skate 3's skating as global natives for Cyberpunk 2077.
//!
//! The Cyber Engine Tweaks mod in `cyberpunk/mod` drives these every frame
//! (`Game.CyberSkate_Step(dt)` and friends); redscript can declare and call
//! them as `native func CyberSkate_Step(dt: Float) -> Bool` too. Every native
//! returns immediately: the simulation runs on its own threads.
#![cfg(windows)]
#![allow(non_snake_case)]

use bevy::math::Vec3;
use cyberskate::keyboard::Keys;
use cyberskate::{InputSource, Runtime, Scan, Status};
use red4ext_rs::{
    Exportable, GlobalExport, Plugin, PluginOps, SemVer, U16CStr, export_plugin_symbols, exports,
    global, wcstr,
};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

pub struct CyberSkate;

impl Plugin for CyberSkate {
    const AUTHOR: &'static U16CStr = wcstr!("CyberSkate");
    const NAME: &'static U16CStr = wcstr!("CyberSkate");
    const VERSION: SemVer = SemVer::new(0, 1, 0);

    fn exports() -> impl Exportable {
        exports![
            GlobalExport(global!(c"CyberSkate_Version", version)),
            GlobalExport(global!(c"CyberSkate_Start", start)),
            GlobalExport(global!(c"CyberSkate_Status", status)),
            GlobalExport(global!(c"CyberSkate_AssetsPath", assets_path)),
            GlobalExport(global!(c"CyberSkate_Notice", notice)),
            GlobalExport(global!(c"CyberSkate_ScanBreaks", scan_breaks)),
            GlobalExport(global!(c"CyberSkate_SubmitScan", submit_scan)),
            GlobalExport(global!(c"CyberSkate_Activate", activate)),
            GlobalExport(global!(c"CyberSkate_Deactivate", deactivate)),
            GlobalExport(global!(c"CyberSkate_Step", step)),
            GlobalExport(global!(c"CyberSkate_Frame", frame)),
            GlobalExport(global!(c"CyberSkate_State", state)),
            GlobalExport(global!(c"CyberSkate_Controller", controller)),
            GlobalExport(global!(c"CyberSkate_Collision", collision)),
            GlobalExport(global!(c"CyberSkate_PadButtons", pad_buttons)),
            GlobalExport(global!(c"CyberSkate_InputSource", input_source)),
            GlobalExport(global!(c"CyberSkate_SetKeyboard", set_keyboard)),
            GlobalExport(global!(c"CyberSkate_Score", score)),
            GlobalExport(global!(c"CyberSkate_Trick", trick)),
        ]
    }

    fn on_load(env: &red4ext_rs::SdkEnv) {
        env.info(format!(
            "CyberSkate {} loaded; Skate 3 data expected in {}",
            env!("CARGO_PKG_VERSION"),
            default_assets().display()
        ));
    }
}

export_plugin_symbols!(CyberSkate);

static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

fn runtime() -> MutexGuard<'static, Option<Runtime>> {
    RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A panic must never unwind into the game.
fn guarded<T>(fallback: T, name: &str, body: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            CyberSkate::env().error(format!("CyberSkate: {name} panicked"));
            fallback
        }
    }
}

unsafe extern "system" {
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut isize) -> i32;
    fn GetModuleFileNameW(module: isize, path: *mut u16, size: u32) -> u32;
}

#[link(name = "xinput")]
unsafe extern "system" {
    fn XInputGetState(index: u32, state: *mut [u32; 4]) -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(window: isize, process: *mut u32) -> u32;
}

/// The keyboard layout of the virtual pad, while the game has focus.
fn read_keys() -> Option<Keys> {
    // SAFETY: plain Win32 queries with no retained pointers.
    let focused = unsafe {
        let window = GetForegroundWindow();
        let mut process = 0;
        window != 0
            && GetWindowThreadProcessId(window, &mut process) != 0
            && process == std::process::id()
    };
    if !focused {
        return None;
    }
    let down = |key: i32| unsafe { GetAsyncKeyState(key) } as u16 & 0x8000 != 0;
    Some(Keys {
        left_up: down(0x57),       // W
        left_down: down(0x53),     // S
        left_left: down(0x41),     // A
        left_right: down(0x44),    // D
        flick_up: down(0x26),      // Up
        flick_down: down(0x28),    // Down
        flick_left: down(0x25),    // Left
        flick_right: down(0x27),   // Right
        a: down(0x20),             // Space
        b: down(0xA2),             // Left Ctrl
        x: down(0xA0),             // Left Shift
        y: down(0x46),             // F
        left_trigger: down(0x51),  // Q
        right_trigger: down(0x45), // E
        left_bumper: down(0x5A),   // Z
        right_bumper: down(0x43),  // C
        start: down(0x0D),         // Enter
        back: down(0x08),          // Backspace
    })
}

/// `skate-data/assets` beside this DLL, where the setup script converts the
/// player's Skate 3 into.
fn default_assets() -> PathBuf {
    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    let mut module = 0;
    let mut buffer = [0u16; 1024];
    // SAFETY: the address is a function in this module; the buffer is ours.
    let length = unsafe {
        if GetModuleHandleExW(
            FROM_ADDRESS | UNCHANGED_REFCOUNT,
            default_assets as *const u16,
            &mut module,
        ) == 0
        {
            0
        } else {
            GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32)
        }
    };
    let dll = PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
    dll.parent()
        .map(|dir| dir.join("skate-data").join("assets"))
        .unwrap_or_else(|| PathBuf::from("skate-data/assets"))
}

fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Starts loading the converted Skate 3 data. An empty `assets` uses the
/// folder beside the plugin. Restarts a runtime that failed or that was
/// started on another folder; otherwise leaves the running one alone.
fn start(assets: String) -> bool {
    guarded(false, "Start", || {
        let root = if assets.trim().is_empty() {
            default_assets()
        } else {
            PathBuf::from(assets.trim())
        };
        let mut slot = runtime();
        if let Some(running) = slot.as_ref()
            && running.root() == root
            && !matches!(running.status(), Status::Failed(_))
        {
            return true;
        }
        if !root.join("private").join("game.json").is_file() {
            CyberSkate::env().warn(format!(
                "CyberSkate: no converted Skate 3 data in {}",
                root.display()
            ));
        }
        match Runtime::start(&root, Some(Box::new(read_keys))) {
            Ok(started) => {
                CyberSkate::env().info(format!("CyberSkate: loading {}", root.display()));
                *slot = Some(started);
                true
            }
            Err(e) => {
                CyberSkate::env().error(format!("CyberSkate: could not start: {e}"));
                false
            }
        }
    })
}

/// `idle`, `loading`, `ready`, `active` or `error: <why>`.
fn status() -> String {
    guarded(String::from("error: panic"), "Status", || {
        runtime()
            .as_ref()
            .map_or_else(|| "idle".into(), |r| r.status().describe())
    })
}

fn assets_path() -> String {
    guarded(String::new(), "AssetsPath", || {
        runtime()
            .as_ref()
            .map_or_else(default_assets, |r| r.root().to_owned())
            .display()
            .to_string()
    })
}

fn notice() -> String {
    guarded(String::new(), "Notice", || {
        runtime()
            .as_ref()
            .and_then(Runtime::take_notice)
            .unwrap_or_default()
    })
}

/// The grid edges whose step positions the mod should bisect before
/// submitting: (sample index, axis) pairs.
fn scan_breaks(header: Vec<f32>, ground: Vec<f32>) -> Vec<f32> {
    guarded(Vec::new(), "ScanBreaks", || {
        Scan::breaks(&header, &ground).unwrap_or_else(|e| {
            CyberSkate::env().warn(format!("CyberSkate: scan refused: {e}"));
            Vec::new()
        })
    })
}

fn submit_scan(
    header: Vec<f32>,
    ground: Vec<f32>,
    edges: Vec<f32>,
    walls: Vec<f32>,
    posts: Vec<f32>,
) -> bool {
    guarded(false, "SubmitScan", || {
        let scan = match Scan::parse(&header, &ground, &edges, &walls, &posts) {
            Ok(scan) => scan,
            Err(e) => {
                CyberSkate::env().warn(format!("CyberSkate: scan refused: {e}"));
                return false;
            }
        };
        runtime().as_ref().is_some_and(|r| r.submit(scan))
    })
}

fn activate(x: f32, y: f32, z: f32, forward_x: f32, forward_y: f32) -> bool {
    guarded(false, "Activate", || {
        let position = Vec3::new(x, y, z);
        let forward = Vec3::new(forward_x, forward_y, 0.);
        if !position.is_finite() || !forward.is_finite() {
            return false;
        }
        runtime().as_mut().is_some_and(|r| {
            matches!(r.status(), Status::Ready | Status::Active) && r.activate(position, forward)
        })
    })
}

fn deactivate() -> bool {
    guarded(false, "Deactivate", || {
        runtime().as_mut().is_some_and(Runtime::suspend)
    })
}

fn step(dt: f32) -> bool {
    guarded(false, "Step", || {
        runtime()
            .as_ref()
            .is_some_and(|r| !matches!(r.status(), Status::Failed(_)) && r.step(dt))
    })
}

/// The latest skater, deck and camera; empty before the first frame. Layout
/// in `cyberskate::SkateFrame::to_floats`.
fn frame() -> Vec<f32> {
    guarded(Vec::new(), "Frame", || {
        runtime()
            .as_ref()
            .and_then(Runtime::frame)
            .map(|f| f.to_floats())
            .unwrap_or_default()
    })
}

/// The skater's physical state, such as `PhysicsGround` or `GrindFiftyFifty`.
fn state() -> String {
    guarded(String::new(), "State", || {
        runtime()
            .as_ref()
            .and_then(Runtime::frame)
            .map(|f| f.state)
            .unwrap_or_default()
    })
}

/// Whether anything (a controller or the keyboard) drove the last step.
fn controller() -> bool {
    guarded(false, "Controller", || {
        runtime()
            .as_ref()
            .is_some_and(|r| r.input() != InputSource::None)
    })
}

/// 0 nothing, 1 an XInput controller, 2 the keyboard.
fn input_source() -> i32 {
    guarded(0, "InputSource", || {
        runtime().as_ref().map_or(0, |r| r.input() as i32)
    })
}

/// Lets keys stand in for a missing controller, or stops them.
fn set_keyboard(enabled: bool) -> bool {
    guarded(false, "SetKeyboard", || {
        runtime().as_ref().is_some_and(|r| {
            r.set_keyboard(enabled);
            true
        })
    })
}

/// Skate 3's scoring: active (1/0), sequence points, multiplier, banked
/// total, tricks announced, landings, last landing's points, bails. The
/// three counters only grow; the HUD watches them for new events.
fn score() -> Vec<f32> {
    guarded(Vec::new(), "Score", || {
        runtime().as_ref().map_or_else(Vec::new, |r| {
            let s = r.score();
            vec![
                if s.active { 1. } else { 0. },
                s.sequence,
                s.multiplier,
                s.total,
                s.tricks as f32,
                s.landings as f32,
                s.last_landing,
                s.bails as f32,
            ]
        })
    })
}

/// The trick the running sequence was last announced as.
fn trick() -> String {
    guarded(String::new(), "Trick", || {
        runtime()
            .as_ref()
            .map(|r| r.score().trick)
            .unwrap_or_default()
    })
}

/// Installed collision: generation, triangles, rails.
fn collision() -> Vec<f32> {
    guarded(Vec::new(), "Collision", || {
        runtime().as_ref().map_or_else(Vec::new, |r| {
            let c = r.collision();
            vec![c.generation as f32, c.triangles as f32, c.rails as f32]
        })
    })
}

struct Pad {
    slot: Option<u32>,
    searched: Option<std::time::Instant>,
}

static PAD: Mutex<Pad> = Mutex::new(Pad {
    slot: None,
    searched: None,
});

/// The XInput button bits of the first connected controller, or 0. Reads one
/// slot per call once a controller is found: an empty slot is slow to ask,
/// so the others are searched at most once a second.
fn pad_buttons() -> i32 {
    guarded(0, "PadButtons", || {
        let mut pad = PAD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let read = |slot: u32| {
            let mut state = [0u32; 4];
            // SAFETY: XINPUT_STATE is 16 bytes, 4-aligned: the packet number,
            // then the gamepad whose first field is the u16 button mask.
            (unsafe { XInputGetState(slot, &mut state) } == 0).then(|| (state[1] & 0xffff) as i32)
        };
        if let Some(slot) = pad.slot {
            if let Some(buttons) = read(slot) {
                return buttons;
            }
            pad.slot = None;
        }
        let now = std::time::Instant::now();
        if pad
            .searched
            .is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(1))
        {
            return 0;
        }
        pad.searched = Some(now);
        for slot in 0..4 {
            if let Some(buttons) = read(slot) {
                pad.slot = Some(slot);
                return buttons;
            }
        }
        0
    })
}
