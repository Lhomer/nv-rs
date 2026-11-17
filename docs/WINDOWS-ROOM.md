# The Windows test room

A Windows build of the barebones physics test room: the same room as the Android and Linux
builds (`crates/host/src/phone_range.rs`), so the active ragdoll can be tried and filmed on a PC.
The room has one invincible target, a railing, a half-wall, a table and a low obstacle behind it,
and a short loadout (the 9mm pistol, the .44 Magnum revolver, the 10mm pistol, the anti-materiel
rifle, the 10mm SMG, the assault carbine, the light machine gun, the minigun, frag grenades, the
grenade launcher and the missile launcher) with ammo kept topped up. It uses the game's own mouse and keyboard, and has a Record button and F9
for video clips.

It is the Play package built as the room's (`NV_RS_ROOM=1`). It finds the game the same way, makes
the same machine tables on its first start, keeps everything it writes in its own `userdata`
folder, and shows the same window when the game stops. It never saves a game: the room refuses
every save. It keeps one snapshot of its own (below), in its own folder.

**Not run with game files yet.** CI builds it, but no game files or Windows PC exist where it was
made. The first runs will tell what still stops it. Send the log (in `userdata\logs`) and, for a
clip, its `.json`.

## Get it

The zip, rebuilt from the branch by CI (`.github/workflows/windows-test.yml`) on every push that
touches the engine:

https://github.com/Lhomer/nv-rs/releases/download/windows-test/nv-rs-windows-room.zip

`windows-latest.txt` beside it names the commit it was built from (and `BUILD.txt` in the
package). Unzip it anywhere outside `Program Files` (a short path, such as `C:\nv-rs-room`).

## Start it

- **From the Play package**: its Launcher.exe has a **Launch physics tester** button. It reads
  `windows-test`'s `windows-latest.txt`, installs or updates the room in a folder of its own beside
  the Play install (`<Play's folder>\..\nv-rs-physics-tester`: its own programs, `userdata` and
  saves; Play's are not touched), and starts it. Play's own updates (`latest.txt`) are as before.
  Without internet it starts the installed room, if there is one.
  - The room is built after Play's package, about 15 minutes later. When `windows-latest.txt` is
    not yet the commit of Play's newest build (Play's `latest.txt`), the launcher waits for it
    (looking every 20 s, at most 30 minutes) instead of starting the older one as the newest
    (Patrick, 0c1fbbf: f9eac18's tester was started). Closing its window starts the newest one
    there is then. The room is built for every push now, as Play is, so it always comes.
- **Launcher.exe** (in the room's own zip): looks for a newer build on the `windows-test` release. If there
  is one it offers to update: the download is checked against its SHA-256, and `userdata` is never
  touched. Then it starts the room. It updates the folder it is in.
- **Play.exe** starts the room straight away, without the update check.

The first start finds Fallout: New Vegas through Steam (the registry and every Steam library in
`libraryfolders.vdf`), GOG, Epic or the game's installer. Without one it asks for the folder: the
one holding `FalloutNV.exe` and `Data`. It needs version 1.4.0.525 and the game's files as
installed; nothing is copied or changed there. The first start also makes the machine tables
(a few seconds).

The game opens at the main menu (the intro movies are skipped), then goes into the room by itself.
The target stands 6 m ahead and the obstacles are behind it, so shots push it into them.

**The snapshot.** The first start builds the room (the cell's load, then the target, the
obstacles and the loadout) and, a second and a half later, saves a snapshot of it: the Test
Range's prepared save, in the tester's own `userdata\testrange` (never in your FalloutNV saves,
and never in git or CI). Later starts load the snapshot straight from the main menu. A new build
(its commit), another loadout or another load order makes it again; so does
`NV_TEST_RANGE_REBUILD=1`. The room used to give every weapon, armour and aid item of the game too:
thousands of console lines in one frame after the load, the long black screen before the room
appeared. It gives only its loadout now, and the log says how long the set-up took
(`set up in N ms`).

In the regular Play package, `Play.exe --room` starts the same room.

## Controls

The game's own: mouse to look, left button to fire, right button to aim, W A S D to move, R to
reload, 1 to 8 for the hotkeys, Tab for the Pip-Boy, Esc for the pause menu. Home opens the range
menu (weapons, ammo, hostile or passive targets, back to the main menu). The room's own keys, kept
from the game: **F6** the next weapon (the loadout in turn), **F7** the target put back where it
first stood, **F9** a clip (below). The loadout is in the inventory, with 500 rounds of each ammo
and 50 frag grenades, topped up every five seconds. The target is essential and has 100,000 health.

## Clips

- **F9** starts and stops a clip, anywhere in the room. The game never sees F9 there (nor F6 and
  F7); outside the room F9 is its Quickload.
- **The Record button** is the small red dot in the window's top right corner, under the
  frame-rate counter. It shows faintly while idle and blinks while recording. Clicking it does the
  same as F9 whenever the mouse cursor shows (in the range menu or the pause menu; in play the game
  holds the mouse for the look). It is never in the clip. The regular Play package has the same
  recorder (docs/PLAYTESTING.md, "Record a bug").

A clip:

- is saved in `%USERPROFILE%\Videos\nv-rs-test-clips` (`NV_ROOM_CLIPS` set before the start picks
  another folder);
- is named `<date>_<time>_<commit>_<weapon>_<scenario>.mp4`, for example
  `2026-11-16_14-03-09_992aab1_WeapNVHuntingShotgun_brace-waist.mp4`. The weapon is the one in
  your hand when the clip starts. The scenario is the most telling thing that happened in the clip:
  `fold`, `trip-fall`, `fall`, `brace-waist` / `brace-wall` / `brace-low`, `trip`, `steps`,
  `knockdown`, `stagger`, `flinch`, or `room` for nothing;
- is H.264 in an MP4 at 30 frames a second. A window larger than 1920 by 1080 is scaled down.
  Windows' own Media Foundation encodes it, on the graphics card's encoder where there is one;
- starts 1.5 s before the press (the pre-roll), so a shot fired just before it is in it;
- stops by itself after 2 minutes, or when the game ends (the file is finished first).

Beside each clip a `.json` sidecar of the same name holds:

- `clip`, `commit`, `weapon`, `scenario`, `fps`, `preroll_s`;
- `video`: the file, its frames and size (or the error if the video could not be written);
- `events`: every room event during the clip, each with `t` (seconds from the clip's first frame).
  These are each hit's reaction (`hit`: momentum, speed against topple, pain, the decision), the
  catch steps (`steps`), trips (`trip`), braces (`brace`, and `brace_hand` for each hand planting or
  letting go), falls (`fall`), and the reaction's log lines (`note`, `knockdown_note`).
  docs/LINUX.md's results table describes the same events;
- `samples`: the target's state ten times a second, and every frame while it reacts
  (`position`, `knock_state`, `life_state`, `health`, `falling`, `getting_up`), each with `t`, and
  `body`: the reaction's `active` (the physical body on), `pelvis_offset` (game units written over
  the animation), `pelvis_jump` (the shown pelvis's move in the frame beyond the actor's own,
  game units), `yaw_rate` (its turn against the animation, rad/s), `spin` and `spin_bone` (the
  fastest any bone's shown offset from its animation changed, rad/s, and which bone),
  `settling` (being handed back: it only comes closer), `yielded` (it yields to its animation:
  the AI moved or turned the actor, or its animation jumped), `limited` (how many offsets were
  held to the push since the body was made), `ai_speed` (how fast the AI moves the actor,
  units/s), `drawn_offset` and `drawn_turn` (what the frame was drawn with, read as the world pass
  begins from the bones the actor's skinned geometry lists: the most the pelvis, the spine's top
  or the head was drawn off its animation, game units and degrees; 0 while the body is shown off
  it means the write never reached the drawing), `drawn_skin` (of those three, how many the skin
  lists, how many of them are the rig's own nodes, and how many bones the skin lists), `spread` (the body's farthest bone from the pelvis, m; the body is dropped past
  2.5), `drift` and `drift_most` (reads a later game pass moved, and the most, in tolerances),
  `jumps` (animation jumps the reaction yielded to), `fade` (how much of a dropped body's way back
  is left), `heights` (the root's, `Bip01 NonAccum`'s and the pelvis's world height over the
  actor's reference, as drawn, game units) and `feet_over_floor` (each foot's height over the
  collision floor under it, `null` where the cast found none), `drawn_delta` (the most a drawn
  bone moved this frame beyond its animation's own move, game units: a pop at the hit shows here),
  `drawn_length` (the most a drawn bone's length is off its animation's, game units: 0 unless a
  bone is stretched) and `rebased` (leg-only animation jumps at a hit re-based instead of yielded
  to); `null` when it is not reacting.

The clips need the Vulkan renderer (the default; Settings.cmd's Renderer). With "Direct3D 9" the
game is drawn by Windows' Direct3D, whose frames the recorder does not see: the clip comes out
empty and its sidecar says so.

## When it does not work

Each run's log is in `userdata\logs` (`game-<date>.log`), and the window shown when the game stops
names it. The room's lines start with `Test Range:` and `Phone range:`, the clips' with
`Test room clips:`.

## How it is built

- The game is the Play package's `app\FalloutNV.exe` (`viewer/`) with `NV_TEST_RANGE_PHONE=1`:
  the shared room (`crates/host/src/phone_range.rs`, `test_range.rs`).
- Clips (`crates/host/src/room_record.rs`, `NV_ROOM_RECORD=1`): the wgpu backend hands over the
  game's frames without waiting for the GPU (`crates/d3d9-wgpu/src/capture.rs`). A thread sizes
  them, turns them into NV12, keeps the pre-roll and writes them with Media Foundation
  (`crates/engine-platform/src/record.rs`). The sidecar comes from `crate::room_events`. F6, F7 and
  F9 are kept from the game's keyboard in the room (`crate::player_move`).
- Play.exe (`scripts/launcher`) built with `NV_RS_ROOM=1` (or run with `--room`) starts the game
  with both variables. Launcher.exe (`scripts/updater`) built with `NV_RS_UPDATE_TAG=windows-test`,
  `NV_RS_UPDATE_LATEST=windows-latest.txt` and `NV_RS_ROOM=1` updates from the release and starts
  Play.exe without asking.
- CI (`.github/workflows/windows-test.yml`) runs `scripts/package-playtest.ps1 -TestBuild` with
  those variables, adds this file as `ROOM README.md`, and publishes `nv-rs-windows-room.zip`, its
  SHA-256, `BUILD.txt` and `windows-latest.txt` on the `windows-test` release for each successful
  build of a `claude/**` branch.
