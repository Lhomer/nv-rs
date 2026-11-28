# The hit reactions' self-test

Two layers test New Physics' hit reactions and the Delirium integration without anyone aiming,
shooting or recording (Patrick's ask of 2026-11-27, 5:47 PM).

## Layer 1: the game's self-test, on the player's PC (real Delirium)

Run:

```
Play.exe --physics-test
```

(`--physics-test` is the physics test room, `--room`, with `NV_PHYSICS_SELFTEST=1`; the host's
`crate::physics_selftest`). The room loads as it always does, its Powder Ganger is placed, and then
with no input the self-test strikes it with a fixed list of hits, watches each outcome every frame,
judges it, writes its results and ends the game by itself (always with status 0: the results say
how it went, so Play.exe never takes it for a crash). A run takes about three minutes (at most 10).

The hits go into New Physics' own hit path as a bullet's do (the body struck, its impulse, its
direction from the player or from behind the target): a 9mm's 25.9 N s (Patrick's logs) and a hit
past the knockdown threshold, 160 N s. They are pushes only: no damage is dealt (the kill is the
console's `kill`, the target's essential flag cleared first).

| Scenario | What is done | Expected |
| --- | --- | --- |
| `light-standing` | a 9mm on the chest | stays standing |
| `head-shot` | a 9mm on the head | stays standing |
| `staggering-hit` | a 9mm, and another 0.3 s later | stays standing |
| `downed-face-down` | a heavy hit from behind, then a 9mm once it is down | rises |
| `downed-face-up` | a heavy hit from the front, then a 9mm on the forearm once it is down | rises |
| `mid-rise-hit` | a heavy hit, then a 9mm half a second into its rise | rises |
| `kill-mid-rise` | a heavy hit, then the console's kill half a second into its rise | dies, a Delirium corpse |
| `past-threshold` | a heavy hit | rises |
| `light-standing-slope`, `past-threshold-slope` | the same on a slope of 1:8 to 1:2 found within 20 m of the room (SKIP when there is none) | stays / rises |
| `trench` | on uneven ground as a trench's dirt (bumps of 12 to 60 units within 1.4 m, found within 30 m of the room; SKIP when there is none): a 9mm standing, 4 s later a heavy hit, then three 9mm hits while it is down (forearm, chest, calf) | the 9mm doesn't down it; it rises |

### Unattended: `Launcher.exe --update-and-physics-test`

One command does it all with no window and no question (scripts/updater, `physics_test`): it
looks at Play's `latest.txt` and the tester's `windows-latest.txt`, waits (at most 30 minutes)
for the tester's build to catch up with Play's newest, updates the physics tester's folder to it
(only its programs: its `userdata`, saves and INIs are never touched), runs its
`Play.exe --physics-test` with no launcher window (`NV_RS_NO_WINDOWS=1`) until the game ends by
itself (at most 20 minutes, then it is ended), and reads the results. If the game or Play.exe is
already running it does nothing.

**Started from a shell with no desktop** (a remote or background PowerShell, Windows' session
0, where the game can't open its window): it starts itself again in the logged-on player's own
desktop session (a scheduled task, `nv-rs physics test`, interactive only, removed afterwards)
and waits for that run, passing on its exit code. The player has to be logged on at the PC
(the screen may be locked or the window minimized).

Exit code: **0** every check passed, **1** a check failed, **2** it could not run (no tester and
no release, the game wrote no results, the self-test ran out of time), **3** the game or Play.exe
was running (nothing done).

From PowerShell, `<tester>` the physics tester's folder (`nv-rs-physics-tester`, beside the Play
install). Launcher.exe is a windowed program: `Start-Process -Wait` waits for it and gives its
code:

```powershell
$p = Start-Process -FilePath '<tester>\Launcher.exe' -ArgumentList '--update-and-physics-test' -Wait -PassThru; $p.ExitCode
```

**Once, the first time:** a tester installed before this command has a Launcher.exe that doesn't
know it (it would ask its questions instead). Run the newest tester's own Launcher.exe from a
temporary folder and point it at the tester; it updates the tester (its Launcher.exe with it), and
from then on the command above works:

```powershell
$t = '<tester>'; $b = Join-Path $env:TEMP 'nv-rs-boot'; Remove-Item $b -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $b | Out-Null; curl.exe -fL -o "$b\room.zip" 'https://github.com/Lhomer/nv-rs/releases/download/windows-test/nv-rs-windows-room.zip'; tar.exe -xf "$b\room.zip" -C $b; $l = (Get-ChildItem $b -Recurse -Filter Launcher.exe | Select-Object -First 1).FullName; $p = Start-Process -FilePath $l -ArgumentList '--update-and-physics-test','--install',$t -Wait -PassThru; $p.ExitCode
```

Files, in the tester's `userdata\logs`:

- `physics-selftest-latest.txt`: the newest results, always here (a `NOT RUN (exit N)` one with
  the reason when it could not run). While the game runs it says where the self-test is
  (`RUNNING: <scenario> (n of N), <stage>`). If the game stops mid-run, the results are written
  all the same: the scenarios done, `FAIL <scenario>: the game stopped N s into it (<stage>):
  <the game's stop line>`, and `FAILED: ... (the game stopped in <scenario>)` last (exit 1).
- `launcher.log`: what the launcher did, its first line the moment it starts (its session and
  whether it has a desktop, the builds it saw, the update, the wait, Play.exe's end).
- `physics-test-exit.txt`: `exit N`, the run's last step.
- `physics-selftest-<UTC stamp>.txt` and `-replay.txt`: each run's own (below).
- `game-<stamp>.log`: the run's game log (its path is in the results' header).

**What it writes**, beside the game's log (the folder of `NV_REPORT_LOG`, the launcher's
`userdata` log folder):

- `physics-selftest-YYYYMMDD-HHMMSS.txt` (UTC; ASCII), also copied to
  `physics-selftest-latest.txt`: a header (`build:` the commit, `run:`, `Delirium: on|off`,
  `replay:` and `game log:` their paths), then a line per check, `PASS|FAIL|SKIP <scenario>: <check> (<where it counts>): <figures>`, and at the
  end `ALL PASS` or `FAILED: <scenarios>`.
- `physics-selftest-YYYYMMDD-HHMMSS-replay.txt`: what was done, frame by frame (each frame's
  seconds, the hits, the death's frame, the slope, the bumps and the uneven spot's ground as cast,
  5 by 5 heights 0.35 m apart, the standing height): nothing of GTA IV's and no
  game file's contents, safe to paste. Layer 2 runs it again.

The game's log has the self-test's last line: `physics self-test: all pass` or `N failed (...)`,
and where the two files went. The Record button and F9 still record clips while it runs.

## Layer 2: the replay in CI (no GTA IV, no game files)

`cargo test -p host --lib replay_` runs the self-test's scenarios through New Physics' own path
(`new_physics::apply`, the main thread's pass, the drawing's read-back) on the tests' mannequin
(`crate::rig_fixture`: FNV's tree laid out, no game file), with a stand-in for Delirium's body
(`crate::delirium_stand_in`: a mechanical double; a light hit shoves it and, on its own balance,
gives way about 2 s later as Delirium's did on Patrick's PC; a heavy hit or its death tips it over
its feet; its muscles on, its parts go where it is told). Everything between the two
is the real integration: `crate::delirium_np`'s frame, the put-on, the hand-overs and retakes, the
joint limits drawn, the ground cast and fed, the rise drawn as New Physics' with Delirium's parts put on it, the hand-back to the animation.
The CI also runs Patrick's PC runs on 5f52c0e (`crates/host/src/replays/pc-5f52c0e.txt`, its open
failures listed and explained in `KNOWN_OPEN_PC_5F52C0E`) and on 8a9b4a4 (`pc-8a9b4a4.txt`, none
open). A PC replay's scenario is acted by the self-test's plan of its name (until down, until
rising, its waits), its hits as recorded, so each hit and the kill land where they did in the fall
and the rise; a synthetic one at its frames. The CI runs `physics_replay::synthetic()` (the same scenarios at 120 frames a second, flat, on
a slope and in a trench's bumps, 6 units high every 140). A replay from the player's PC runs the same way:

```
NV_PHYSICS_REPLAY=physics-selftest-...-replay.txt cargo test -p host --lib replay_from_the_file -- --nocapture
```

## Which checks count where

Each check is in `crate::physics_checks` and is the same in both layers.

| Check | PC (real Delirium) | CI (stand-in) |
| --- | --- | --- |
| light hit stays standing (the pelvis never under 85% of standing) | counts | INFO only |
| recovers upright (torso under 15 degrees, pelvis at 90%) | counts | INFO only |
| rise reaches upright within 9 s of going down (3 s settle, 5 s get-up, the fall) | counts | INFO only |
| the rise ends with no snap (no bone over 10 units a frame within a second of upright) | counts | counts |
| the AI has it again (New Physics lets it go to its animation within 6 s of upright) | counts | counts |
| the corpse is Delirium's (and the game's ragdoll held on its pose: lootable, draggable) | counts | counts (the hold not asked) |
| no pose jumps (no bone over 15 units a frame) | counts | counts |
| no launch (no bone over 9 m/s, the pelvis never 25 units over standing, a wall under 20000 N, the floor under 25 body weights) | counts | counts |
| no joint past its limit while down or dead (under 5 degrees, from 0.25 s lying) | counts | counts |
| never under the ground (no node over 5 units under the ground under it, each body's own) | counts | counts |
| no limb stretched (no bone more than 12% and 3 units off its length) | counts | counts |
| a light hit never downs it (no 9mm-class hit, under 100 N s, followed by lying within 3 s) | counts | counts |
| never down again of its own (lying again with no hit in the 1.5 s before) | counts | counts |
| never sliding while limp (from 1 s lying, no 0.5 s with the pelvis 15 units across and no bone 4 units about it) | counts | counts |
| every rise with its hands planted (rising with the pelvis under 60% of standing, a wrist within 15 units of the ground) | counts | counts |
| never the game's ragdoll (no hand-over to it, no frame waiting for one) | counts | counts |

What only real Delirium can show (its stagger keeps it up, its muscles lift it) counts only on the
PC; over the stand-in those lines say whether the integration let it happen (INFO). "The AI has it
again" is New Physics letting the actor go back to the game's AI and animation; whether it then
walks or fights is the game's own AI, seen in the clip. "Lootable, draggable" is checked as the
game's corpse with its ragdoll held where Delirium keeps it (what looting, dragging and VATS
read); actually dragging it needs a player.
