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
judges it, writes its results and ends the game by itself (exit status 0 when every check passed,
4 when one failed). A run takes a few minutes (at most 10).

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

**What it writes**, beside the game's log (the folder of `NV_REPORT_LOG`, the launcher's
`userdata` log folder):

- `physics-selftest-YYYYMMDD-HHMMSS.txt` (UTC; ASCII): the build and whether Delirium ran, then a
  line per check, `PASS|FAIL|SKIP <scenario>: <check> (<where it counts>): <figures>`, and at the
  end `ALL PASS` or `FAILED: <scenarios>`.
- `physics-selftest-YYYYMMDD-HHMMSS-replay.txt`: what was done, frame by frame (each frame's
  seconds, the hits, the death's frame, the slope, the standing height): nothing of GTA IV's and no
  game file's contents, safe to paste. Layer 2 runs it again.

The game's log has the self-test's last line: `physics self-test: all pass` or `N failed (...)`,
and where the two files went. The Record button and F9 still record clips while it runs.

## Layer 2: the replay in CI (no GTA IV, no game files)

`cargo test -p host --lib replay_` runs the self-test's scenarios through New Physics' own path
(`new_physics::apply`, the main thread's pass, the drawing's read-back) on the tests' mannequin
(`crate::rig_fixture`: FNV's tree laid out, no game file), with a stand-in for Delirium's body
(`crate::delirium_stand_in`: a mechanical double; a light hit shoves it, a heavy hit or its death
tips it over its feet, its muscles on, its parts go where it is told). Everything between the two
is the real integration: `crate::delirium_np`'s frame, the put-on, the hand-overs and retakes, the
joint limits held, the ground cast and fed, the rise's sequencing, the hand-back to the animation.
The CI runs `physics_replay::synthetic()` (the same scenarios at 120 frames a second, flat and on
a slope). A replay from the player's PC runs the same way:

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
| no pose jumps (no bone over 25 units a frame) | counts | counts |
| no launch (no bone over 9 m/s, a wall under 20000 N, the floor under 25 body weights) | counts | counts |
| no joint past its limit while down or dead (under 5 degrees, from 0.25 s lying) | counts | counts |
| never under the floor (no node over 5 units under it) | counts | counts |
| never the game's ragdoll (no hand-over to it, no frame waiting for one) | counts | counts |

What only real Delirium can show (its stagger keeps it up, its muscles lift it) counts only on the
PC; over the stand-in those lines say whether the integration let it happen (INFO). "The AI has it
again" is New Physics letting the actor go back to the game's AI and animation; whether it then
walks or fights is the game's own AI, seen in the clip. "Lootable, draggable" is checked as the
game's corpse with its ragdoll held where Delirium keeps it (what looting, dragging and VATS
read); actually dragging it needs a player.
