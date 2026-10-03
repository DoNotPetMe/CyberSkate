# Skate 3 in Cyberpunk 2077 (CyberSkate)

The same Skate 3 engine the MW2 skate mode runs, inside Cyberpunk 2077:
press your toggle, drop onto a board wherever V stands, and ride Night City
with Skate 3's flick-it controls, physics, tricks and grinds.

No game files ship with this repository or its builds. You bring your own
Cyberpunk 2077 (PC) and Skate 3 (Xbox 360, extracted).

## What you need

- **Cyberpunk 2077** on PC with **[RED4ext](https://github.com/WopsS/RED4ext)**
  and **[Cyber Engine Tweaks](https://github.com/maximegmd/CyberEngineTweaks)**
  installed and working.
- **Skate 3 for Xbox 360, extracted**: `default.xex` with the game's `data`
  folder beside it (see [SKATE.md](SKATE.md#where-does-defaultxex-come-from)).
  The `default.xex` alone is not enough: the skater, animations and physics
  settings are read from `data/`.
- An **Xbox / XInput controller**. Skate 3 is played on the sticks.

## Install

1. Download the `CyberSkate` artifact from the latest successful
   [CyberSkate workflow run](../../../actions/workflows/cyberskate.yml) and
   extract it **into your Cyberpunk 2077 folder** (the one with `bin`, `r6`
   and `red4ext`). It adds:
   - `red4ext/plugins/CyberSkate/CyberSkate.dll`, the plugin with the
     Skate 3 engine
   - `red4ext/plugins/CyberSkate/converter/`, the Skate 3 converter
   - `bin/x64/plugins/cyber_engine_tweaks/mods/CyberSkate/`, the CET mod
2. Run `red4ext/plugins/CyberSkate/Setup-CyberSkate.bat` and select your
   Skate 3 `default.xex`. It writes what skating needs into
   `red4ext/plugins/CyberSkate/skate-data/`. Your game folders are only read.
3. Start the game. In the CET overlay, **Bindings → CyberSkate → Toggle
   skateboard** (J matches the MW2 mode), or click **both sticks in**.

## Riding

| | |
|---|---|
| toggle key / both sticks clicked | get on or off the board |
| controller | Skate 3's own controls |
| CET overlay → CyberSkate | status, camera, scan settings, reload |

While riding, V's walking, jumping, weapons, scanner, phone and camera
control are held off so the controller only skates. The holds are copies of
the game's own restrictions with saving turned off, so a crash never leaves
them in a save.

The default view is first person, turned to where Skate 3's camera looks,
dipping as the skater crouches. **Skate 3 chase** moves the view to Skate 3's
camera behind the skater (experimental: V is drawn as the first-person body).

## How it works

| piece | where | what |
|---|---|---|
| skate engine | [`skate/crates`](../skate/crates) | unchanged Skate 3 physics, animation graph and grind code, as in the MW2 mode |
| adapter | [`cyberpunk/crates/cyberskate`](../cyberpunk/crates/cyberskate) | runs the session on its own threads, turns scans into collision and rails, frames in Night City coordinates |
| plugin | [`cyberpunk/crates/cyberskate-red4ext`](../cyberpunk/crates/cyberskate-red4ext) | `CyberSkate.dll`: `CyberSkate_*` global natives for CET and redscript, XInput read directly |
| CET mod | [`cyberpunk/mod`](../cyberpunk/mod) | toggling, scanning, stepping, moving V and the view, restrictions, overlay |
| setup | [`Setup-CyberSkate.ps1`](../cyberpunk/mod/red4ext/plugins/CyberSkate/Setup-CyberSkate.ps1) | runs the converter on the player's `default.xex` |

Night City has no collision Skate can read, so the mod **scans** it: a grid
of downward ray casts around the skater (33 × 33 at 0.5 m by default), each
step between samples bisected to a few centimetres, and a ring of horizontal
casts for walls. The adapter cuts every grid cell with marching squares
where the ground is not one surface, keeps each side's height, joins them
with vertical faces, and runs the MW2 mode's lip finder over the result, so
curbs, ledges, benches and drop-offs become grind rails. Scans are spread
over frames and rebuilt off the simulation thread as the skater moves; a
grind in progress is never given new geometry.

## Known limits

- V is moved with the skater but does not play Skate 3's animations, and no
  board is drawn: skating is felt through the view, the HUD and the physics.
- Collision comes from scans: thin things (poles, railings) can be missed,
  NPCs and cars are not solid, and walls are only found where the ring of
  casts reaches them.
- Built and tested here without the game: the adapter by unit tests, the
  CET mod by a mocked run, the plugin by the Windows CI build. The first
  in-game runs are the real test.

## Building

`cargo test --release -p cyberskate` and
`cargo build --release -p cyberskate-red4ext` in `cyberpunk/` (the plugin
needs Windows with MSVC and LLVM). [`.github/workflows/cyberskate.yml`](../.github/workflows/cyberskate.yml)
builds the plugin, the converter and the folder `scripts/package.ps1` lays
out.

Unofficial fan project, not affiliated with CD PROJEKT RED or EA.
