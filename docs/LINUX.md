# The Linux test room

A native Linux x86_64 build of the real engine that boots straight into the same Test Range
room as the Android test build (docs/ANDROID.md; the room's code is shared,
`crates/host/src/phone_range.rs`): one invincible target, a railing, a half-wall, a table and a
low obstacle behind it, a short loadout (9mm pistol, .44 Magnum revolver, 10mm pistol,
anti-materiel rifle, 10mm SMG, assault carbine, light machine gun, minigun, frag grenades, grenade
launcher, missile launcher) with ammo kept topped up. It is for testing the active ragdoll's
physics in the real engine on a Linux machine, by hand or by a script with no one at the keyboard.

It is a test build: there is no sound and no way to reach the rest of the game from it.

**Not run with game files yet.** It is built and smoke-tested by CI and where it was made, but
no game files exist there: the first runs will tell what still stops it. Send the log (the
standard error) and, for a scripted run, the JSON file.

## Get it

The tarball, rebuilt from the branch by CI (`.github/workflows/linux-test.yml`):

https://github.com/Lhomer/nv-rs/releases/download/linux-test/nv-rs-linux-room.tar.gz

```bash
curl -LO https://github.com/Lhomer/nv-rs/releases/download/linux-test/nv-rs-linux-room.tar.gz
```

```bash
tar -xzf nv-rs-linux-room.tar.gz
```

It holds `nvrs-room` (the room), `sdp-convert` (the shader check), this file as `README.md`,
`default-script.txt` (the built-in scripted run) and `BUILD.txt` (the commit).
`linux-latest.txt` beside the tarball in the release names the commit it was built from.

It needs a Vulkan driver: a graphics card's, or Mesa's software one (lavapipe) on a machine with
none (Debian and Ubuntu: `sudo apt install mesa-vulkan-drivers`). With no graphics card it uses
lavapipe by itself; `--software` asks for it anyway.

It also needs ALSA's library, `libasound.so.2` (Debian and Ubuntu: `libasound2` or
`libasound2t64`, on almost every desktop already), since the merge of upstream's Linux sound
(cpal, `engine-platform` `sound_out.rs`): the program links it and does not start without it.
The room itself still plays no sound unless a sound table is given.

## The game files

Your own Fallout: New Vegas 1.4.0.525 files (the Windows install's, the same `FalloutNV.exe` the
Windows play build uses). None are in the tarball or the repository. Give the room the folder
holding `FalloutNV.exe` and `Data` (the whole Windows install folder works). It needs these (the
same as the phone, docs/ANDROID.md):

```
FalloutNV/
  FalloutNV.exe
  Fallout_default.ini
  Data/
    FalloutNV.esm
    Update.bsa
    Fallout - Meshes.bsa
    Fallout - Textures.bsa
    Fallout - Textures2.bsa
    Fallout - Misc.bsa
    Shaders/
```

Nothing is written into the game folder, and no game is ever saved: the room refuses every
save, and the game's own writes stay in memory. It keeps one snapshot of its own, for a fast
start: the first run builds the room and saves it in `~/.local/share/nv-rs/testrange`
(`NV_TEST_RANGE_CACHE` names another folder), later runs load it from the main menu; a new
build, another loadout or another load order makes it again (`NV_TEST_RANGE_REBUILD=1` too). A folder of
saves next to it is never read or changed.

The add-ons (Dead Money and the rest, and the four packs) are not needed. With only some of their
archives there, or an add-on's `.esm` without its archive, the room goes on: the search for one
add-on's archives no longer stops on another's (`engine_platform::files`' short names); an
add-on's things simply have no meshes or textures then.

### The machine's tables

The game asks Direct3D 9 about the graphics card, and the room answers from a table made on a
Windows PC: the one the Windows Play package makes on its first start
(`userdata\machine\d3d9_adapter.tsv`, or `%LOCALAPPDATA%\nv-rs\d3d9_adapter.tsv` from
`scripts/d3d9-capture`). Copy it once beside `nvrs-room` (or into its `machine` folder, or into
`~/.local/share/nv-rs/`); `NV_D3D9_ADAPTER` is only needed for a table elsewhere. Without one the
room says where it looked and stops.

The sound table is not needed: the room has no sound, and with no `dsound.tsv` in those places
(or `NV_DSOUND_TABLE`) the game is told there is no sound device and goes on.

### What it needs of the machine

- Linux x86_64 with glibc 2.39 or newer (Ubuntu 24.04, Debian 13; CI builds it on Ubuntu 24.04
  and checks this).
- A Vulkan driver (above).

## Play it (keyboard and mouse)

```bash
./nvrs-room /path/to/FalloutNV
```

The game starts by itself: the main menu (the start-up movies are skipped), then the room, which
takes a minute or two to load (several on lavapipe). The target stands 6 m ahead, the obstacles
behind it.

It runs the fixes preset (the Windows play build's), scripted and filmed runs too;
`NV_PRESET=vanilla` runs the original game's code instead.

- Click in the window to take the mouse (the look); **F10** gives it back (so does leaving the
  window).
- The game's own keys: W, A, S, D to move, the left button to fire, the right to aim, R to reload,
  Space to jump, 1 to 8 for the hotkeys, Tab for the Pip-Boy, Esc for the pause menu.
- **F6**: the next weapon (the loadout in turn). **F7**: the target put back where it first stood.
  **Home**: the range menu (weapons, ammo, hostile or passive targets, back to the main menu).
- `--size 1600x900` sets the window's size (1280x720 when not given).

## Headless and scripted runs

`--headless` draws the frames off screen: no window and no display needed (no X or Wayland).
`--test` runs the built-in script; `--script FILE` runs your own. The run ends by itself when
the script does (status 0). If the room is not set up within `NV_ROOM_TIMEOUT` seconds (900 when
not set) it ends with status 3.

```bash
./nvrs-room /path/to/FalloutNV --headless --test --out results.jsonl --shots shots
```

- `--out FILE`: the results, one JSON object a line (without it, on the standard output, each
  line starting `NVRS-ROOM `).
- `--shots DIR`: a picture (BMP) for each `shot` step; with `--frames-every MS`, one every MS
  milliseconds too (a frame sequence).
- `--software`: lavapipe even with a graphics card. `NV_WGPU_BACKEND=vulkan` forces the card.
- `--print-script`: the built-in script (copy it to start your own).

### The script

One step a line; `#` starts a comment.

| Step | Does |
| --- | --- |
| `test NAME` | Starts a test: the results after it carry its name; its summary is written at the next `test` or at the end |
| `weapon N` | The Nth weapon of the loadout in hand: 0 the 9mm pistol, 1 the .44 Magnum revolver, 2 the 10mm pistol, 3 the anti-materiel rifle, 4 the 10mm SMG, 5 the assault carbine, 6 the light machine gun, 7 the minigun, 8 frag grenades, 9 the grenade launcher, 10 the missile launcher (or an editor ID, e.g. `weapon WeapMinigun`) |
| `stand D [A] [H]` | The player D metres from where the target first stood, A degrees around it from where the player started (clockwise seen from above; the push goes away from the player: 0 into the railing 1 m behind the target, 68 the half-wall, -68 the table, 65 the low obstacle), aiming at H units above the target's feet (85, the chest, when not given; 70 the belly, 110 the head) |
| `fire [N] [GAP] [HOLD]` | N shots (1), GAP frames apart (60), the trigger held HOLD frames each (4; longer for a melee swing) |
| `wait F` | F frames |
| `reset` | The target put back where it first stood (a fresh one, standing) |
| `state` | The target's state now |
| `shot [NAME]` | A picture of the frame (with `--shots`) |
| `look DX DY` | The view turned by mouse counts |
| `quit` | Ends the run (the end of the script does too) |

For example, three anti-materiel rifle shots into the railing from 10 m, one into the table:

```
test amr-railing
weapon 3
stand 10 0
wait 60
fire 3 240
wait 300
state
shot
reset
wait 120
test amr-table
stand 10 -68
wait 60
fire 1
wait 300
state
shot
```

### The results

Each line has `test` (the test's name), `frame` (frames since the script began) and `event`:

| `event` | Fields |
| --- | --- |
| `ready` | The room set up: `target` (its reference), `spot` and `facing` (where it stands), `guns` (editor IDs found), `obstacles` (kind and form id) |
| `step` | The step begun (`step`, `index`) |
| `hit` | A hit's reaction: `momentum` (N s), `stacked`, `speed` (m/s) against `topple`, `mass`, `pain` (stage), `decision` (`None`: flinch, `Stagger`, `Knockdown`), `point`, `direction` |
| `steps` | The catch steps: `speed`, `need` (m past the feet), `count`, `outcome` (`Caught`, `Overrun`, `Ledge`, `Wall`, `Trip`), `said` |
| `trip` | A foot caught on a low obstacle: `height`, `on`, `falls` (pitched over, or stumbled and stepped over) |
| `brace` | A brace against an obstacle: `on` (`low obstacle`, `waist-high obstacle`, `wall`), `arms`, `wall` (distance), `top` (its height), `speed` against `limit`, `falls`, `fold` (folded over it) |
| `fall` | Gone over into the ragdoll: `speed`, `fold` |
| `note`, `knockdown_note` | The reaction's log lines (`text`), as in the game's log |
| `state` | `position`, `knock_state` (0 standing), `life_state`, `health`, `falling`, `getting_up` |
| `shot` | A picture asked for: `name`, `dir` (the file is the next `wgpu-frame-NNNNNN.bmp` there) |
| `summary` | Per test: `shots`, `hits`, `flinch_only`, `staggers`, `knockdowns`, `catch_steps`, `braces`, `folds`, `trips`, `trip_falls`, `falls` |
| `done` | The end |

A quick look at the summaries:

```bash
grep '"summary"' results.jsonl
```

## The shaders on lavapipe

Under Wine the Windows build's Direct3D 9 went through Wine's own renderer, whose shaders failed
to compile on lavapipe (black screens). This build never uses it: the game's shaders are
converted to WGSL and compiled by wgpu for Vulkan, the renderer the Windows build uses too, and
the software adapter is picked by itself when there is no graphics card. To check every game
shader on this machine's Vulkan driver before a run:

```bash
./nvrs-room /path/to/FalloutNV --check-shaders
```

It runs `sdp-convert` on `Data/Shaders` with `--gpu`: each package's shaders converted, then
built into a pipeline on the adapter; it prints the count of each package's shaders the converter
or the driver refused, and the first errors.

## When it does not work

The standard error is the log: it says the adapter it draws with, the game's start, the room's
set-up (`Test Range:` lines, `Phone range:` for the obstacles), and the reactions. Save it with
`2> room-log.txt`.

While the room's cell loads, the log has a `Test Range:` line for each call of the load, and a
watchdog writes where it stands every 10 s with nothing new. It ends the run after 90 s without a
step inside the load (150 s after it), ten times that when the frames are drawn in software
(lavapipe: every loading-screen frame and texture costs the processor). `NV_RANGE_STUCK=SECONDS`
sets the load's limit by hand (the wait after it gets a minute more). The range's own timeout,
which goes back to the main menu when play does not begin after the load, stretches with it.

## How it is built

- `linux/`: the app (`nvrs-room`, winit for the window), its own workspace so the main workspace
  and the Windows play build are untouched. The game runs on a thread of its own
  (`host::phone_range::run`, which the Android app runs too); the window's thread hands over the
  window (`d3d9_wgpu::native`) and the keyboard and mouse (`engine_platform::d3d9real::inject_*`).
- `crates/engine-platform`: `cfg(wgpu_window)` (build.rs: Windows, Android and Linux) forwards the
  game's Direct3D 9 calls to wgpu; `NV_WGPU_OFFSCREEN=1` (`--headless`) draws without a window.
- `crates/host/src/room_script.rs`: the scripted runs; `room_events.rs`: the active ragdoll's
  events as JSON (`active_ragdoll.rs`, `knockdown.rs` record them; off unless a script runs).
- CI builds it on Ubuntu, packs the tarball and publishes it at the link above on each
  successful build of a `claude/**` branch.
