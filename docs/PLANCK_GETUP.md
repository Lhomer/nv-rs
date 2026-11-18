# PLANCK's ragdoll-to-get-up handoff: what its source does

Reference for the get-up and fly-up work (Patrick, 2026-11-18). PLANCK is the Skyrim VR active
ragdoll by adamhynek, GPLv3 like nv-rs: <https://github.com/adamhynek/activeragdoll>. Read at
branch `master`, commit `aed524a5e398f04f10f8f95469153362a88c3681` (2026-10-14). Line numbers below
are that commit's. Defaults are the field initialisers in `include/config.h`; the released mod's
ini may set others. The two Reddit write-ups refused automated reads (HTTP 403): nothing here is
from them. Code ported from PLANCK carries a credit comment naming PLANCK by adamhynek and the
repository URL.

## What is in the source

- **The get-up blend** (`src/blender.cpp`, `include/blender.h`; triggered in `src/main.cpp`
  5320-5326): on the knock state's step from BeginGetUp (5) to GetUp (6), the local pose track is
  blended from the last frame's ragdoll pose to the pose the graph now outputs
  (`RagdollToCurrentRagdoll`), linearly over `getUpBlendInTime` (0.3 s), with the root bone left out
  (`ignoreRoot`). The blend is the game's `hkbBlendPoses` per bone. A blend started while one runs
  starts from the current blended pose. Off by default (`blendWhenGettingUp = false`).
- **The world-from-model fade** (`main.cpp` 5029-5097, 6206-6222): when the behaviour graph's
  powered-ragdoll world-from-model mode switches into COMPUTE, both reference frames
  (`hkaPoseMatchingUtility::computeReferenceFrame`'s outputs, the game's own) are lerped from the
  previous world-from-model over `computeWorldFromModelFadeInTime` (0.5 s); out of COMPUTE, the
  world-from-model is lerped from the last one over `computeWorldFromModelFadeOutTime` (0.3 s).
  Translation and scale lerped, rotation slerped (`math_utils.cpp` 462-469). Both on by default.
- **The get-up motors** (`main.cpp` 5351-5373): while computing world-from-model or in the ragdoll
  state, every powered constraint motor gets `getUpMaxForce` (**0**: "TODO: make it non-zero for
  non-humans"), tau 0.8, damping 1.0, proportional recovery velocity 5, constant recovery velocity
  0.2; otherwise the powered values (max force 500, the same tau, damping and recovery). Bones'
  velocity caps when added to the world: 500 linear, 500 angular.
- **The bugged get-up** (`main.cpp` 5266-5292): only the powered controller on, every motor at 0
  force, the root bone used as world-from-model, and the actor not in the ragdoll state: logged
  and knocked down again (`PushActorAway` from the player's position at force 0;
  `knockDownAfterBuggedGetUp = true`). A powered-only frame with every motor at 0 force is not
  driven.
- **Around the get-up**: the game's zeroing of pitch and roll at the get-up's start is skipped
  (`0x74D65B` patched out) and the orientation update forced while pitch or roll remain; get-up
  waits at least `getUpMinTimeRagdolled` (0.1 s) ragdolled and never while grabbed; the warp check
  (root 3 units off) is off while getting up; collision sounds are muted for 0.5 s from the get-up's
  start; the constraint loosening ramp restarts.
- **Velocity zeroing**: only when a ragdoll is first added to the world (every body put on the
  animation pose, linear and angular velocity zeroed, the driver reset), not at the get-up.

## What the task named that is not in PLANCK's source

- No placement of the character controller's capsule at the feet or the ground: PLANCK leaves it
  to the game.
- No penetration recovery speed or time-sliced depenetration (nothing in the history either).
- No yaw-only reference frame for the get-up: its one yaw-only fit (`SolvePlanarYaw`) is for a
  grabbed actor's movement.

These, and the zeroing of the bodies' velocities before the controller takes over, are nv-rs's
own to design (FNV's `bhkCharacterController` and Havok 4's structures), with Havok's
`hkaPoseMatchingUtility` (`computeReferenceFrame`) as the reference for the yaw-only frame.

## How it maps to nv-rs

- FNV's get-up: knock states 1/2 (ragdoll), 3 and 5 (queued), 6 (the get-up animation);
  `crate::knockdown::get_ups` follows them, `active_ragdoll::begin_get_up_blend` blends the drawn
  pose from the ragdoll's at state 6 (as PLANCK's blend, but in world space and with the root);
  `fit_spot`/`place` set the controller, `watch_get_up` clears stored pushes and caps the rise.
- New Physics' rise (`ragdoll_core::np_down`) is its own: emergent from the balance and the arms'
  push, handed to the game's ragdoll only on a give-up.
