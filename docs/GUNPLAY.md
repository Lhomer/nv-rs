# Gunplay: weighty combat

Patrick, 2026-11-13: guns and melee that feel weighty, as in the newer first-person games of the
series: recoil, kick, inertia, impact and reach; hits that push the body by calibre or weapon
weight; limbs severed when the force is enough; then a Test Range reached from the main menu.
Patrick, after playing it (2026-11-13): recoil works, but no sway showed; and the hit-stop's slow
motion looked bad and added no weight, so game time is never changed now (an impact punch instead).

This is **not 1:1 with FalloutNV.exe**. It is an nv-rs addition, a row of the `fixes` preset:
`[nv-rs Fixes] bWeightyCombat`, `NV_WEIGHTY_COMBAT`, on by default. The `vanilla` preset and
`bWeightyCombat=0` keep the original's combat exactly. Code: `crates/host/src/gunplay.rs`. The
FIXES.md row is "Weighty combat".

## Assets and licensing

Nothing is added to the game's data. Every effect is **procedural**: the code turns the view,
turns and offsets the weapon, punches the view and the weapon on impacts, scales the strength of
the game's own impulse, and
sets the game's own dismember and explode outcome flags. No animation, model, texture or sound is
shipped or needed, and no game asset is committed to git.

Nothing is taken from Fallout 4 or any other commercial game: no animation, curve, value table or
asset. The feel is described in words and built from the game's own weapon records (damage,
projectiles, weight, type and flags). Fallout: New Vegas's own animations stay as they are.

If animation work is wanted later, these sources are allowed:
- the game's own New Vegas animations;
- community packs made for New Vegas whose licence permits redistribution, with the licence
  recorded here before use;
- freely licensed work, with the licence recorded here;
- new procedural motion.

None is used now.

## What the original already does

- **Spread and kick.** `TESObjectWEAP::Fire` (`00523150`) builds each shot's aim from the
  shooter's angles. The spread comes from the weapon (`fMinSpread`, `fSpread`), the actor's skill
  and the ammo. The controller rumbles for the player. The view itself never moves on a shot: the
  only "recoil" is the first-person fire animation (and the iron-sights animation).
- **Limb damage and outcomes.** `HitData::DetermineOutcome` (`009b73d0`) sets the hit's flags:
  - `0x10` lethal;
  - `0x20` dismember;
  - `0x40` explode;
  - `0x80` cripple, when the limb's actor value falls to 0;
  - `0x100`/`0x200` the target's weapon breaks.

  The dismember and explode roll (`00646e50`) runs only for a killing hit on a limb. It rolls
  `rand() % 100` against:
  - the part's explode chance and `iCombatExplodePartChance` (`011ce2fc`);
  - `iCombatDismemberPartChance` (`011cf2ec`).

  A weapon can override the outcome (`008a16b0`: sever only, explode only, neither). Whether a limb
  can be severed or explode at all is the body part data's (`BGSBodyPartData` and its parts). The
  dismemberment then plays from those flags with the game's own severed-limb models and effects.
- **Impulses.** On a death with an aggressor, the kill impulse (`008ae000`) pushes the ragdoll from
  the hit's point. Its strength is the hit's health damage (`+0x14`), through `006466e0`, times 2.5
  times the weapon's kill impulse (`+0x1a8`), with a falloff past the impulse distance (`+0x1b8`).
  It is applied by `TESHavokUtilities::AddExplosionImpulse` (`0062b660`). A melee swing that hits a
  loose physics body pushes it by the weapon's or armour's weight (`008ae660`). Living actors are
  keyframed: there is no ragdoll impulse on a non-lethal hit.
- **Melee reach.** The actor's reach comes from `CalcWeaponReach` (`006463e0`: the weapon's reach
  times `fCombatDistance`), times its scale. The reach search (`009a60e0`, from `008990f0`) finds
  what it can hit, and `Actor::CombatHit` (`00899cb0`) lands it.
- **Time.** `BSTimer::fGlobalTimeMultiplier` (`011ac3a0`, target `011ac3a4`) slows the game.
  V.A.T.S. and scripts use it, and the ramp `00aa4e40` moves it to its target (nv-rs's slow-motion
  fix runs the ramp by time).

## Slice 1: what is added, and where

| Feel | How | Hook |
| --- | --- | --- |
| View kick | Each player shot of a gun (types 3 to 9) kicks the view up, with a little random sideways kick. Degrees = (0.35 + 0.055 × force), capped at 9. A one-handed pistol kicks ×1.15, a launcher ×1.4, a held-trigger weapon ×0.25, automatic fire ×0.45, iron sights ×0.7, then `iRecoil`%. Nothing in V.A.T.S. | `TESObjectWEAP::Fire` `00523150` (wrapped) |
| Kick in and recovery | Turned in over about 35 ms with the game's own clamped look functions (`00931e50` pitch, `00931d30` heading). Then `iRecoilRecovery`% comes back over about 160 ms, less what the player pulled down. | The player's look handling `009445b0` (wrapped, once a frame) |
| Sway and inertia | The weapon turns a little behind fast look turns and settles back: a lag of 0.014 rad per rad/s of heading turn (up to 3.4°), 0.012 of pitch (up to 2.6°) and a slight roll, through a spring of about 2.4 swings a second just under critical damping; plus an offset lag (0.45 units per rad/s sideways, up to 1.4), a walking bob and a shot's push back. All times `iSway`%, times 0.8 to 1.5 by weapon weight, a fifth in iron sights; `iSway=0` turns it off. Off in third person, the Pip-Boy and V.A.T.S. The springs run in the look handling; the pose goes into the first-person root in `Main::DrawWorld_MTSetup1stPerson` (below). | `009445b0` (springs), `00874c10` (placed) |
| Impact punch | A player melee hit that connects, and a heavy shot (force 25 or more: a shotgun, a magnum, a rifle), punches the viewmodel (pushed back up to 2.4 or 3 units, muzzle up 1° to 3.5°, a little twist) and the camera (up 0.2° to 0.9°, a little sideways). It rises over the first fifth and eases back by the end: `iHitStopMs` (90) × 0.85 to 1.3 by weight or calibre, so about 75 to 120 ms. The camera's turns over a punch add up to nothing, so the view ends where it started. Game time is never touched (a test holds that the module never names the time multiplier). `bHitStop=0` or `iHitStopMs=0` turns it off. Not in V.A.T.S. | `Actor::CombatHit` `00899cb0`, `Fire` `00523150` |
| Melee reach | The player's reach in the reach search times `iMeleeReach`%. | Reach search `009a60e0` |
| Kill impulse | The impulse strength (the hit's health damage, restored afterwards) times `iImpulse`%. Melee also scales by weight: 0.75 + weight/12, from 0.75 to 2.5. A gun's push already grows with its damage. | Kill impulse `008ae000` |
| Dismemberment by force | After the roll, if it said neither: a weapon whose force reaches `iDismemberForce` severs the limb. A gun whose force reaches `iExplodeForce` explodes a limb with an explode chance above 0 (melee only severs). The weapon's override still applies. The game's own body part data then decides whether the limb can be severed. | Roll `00646e50` |

**Force** is a gun's damage times its projectiles (up to 12), so its calibre including pellets. A
melee weapon's force is its damage × (1 + weight/10). With the defaults:
- a 10mm pistol (20) never forces a sever;
- a .44 Magnum (48) or a hunting rifle (45) severs on a killing hit;
- an anti-materiel rifle (110) explodes an explodable limb;
- a sledgehammer (35, weight 12: 77) severs;
- a knife does not.

The weapon form's offsets used:
- weight `+0x90`;
- damage `+0xA0` (16 bits);
- `DATA` at `+0xF4`, holding the type `+0`, flags `+0x0C` (automatic `2`) and projectiles `+0x2A`.

### Where the sway goes in

`Main::DrawWorld_MTSetup1stPerson` (`00874c10`, each frame, just before the first-person
accumulation is queued) is where the arms and the weapon get their place for the draw:
1. It reads the first-person root's world translation (`+0x8c`) and places the first-person camera
   (`Main +0xa0`) at the camera node (`011e07d0`) less that translation, with the world camera's
   rotation.
2. It saves the root's local translation, sets it to zero, updates the root and everything under
   it (`00a59c60`), and puts the translation back. So the first person is drawn near the origin,
   and anything written into the root's local translation earlier in the frame never shows.

The wrap ([`place_pose`] in gunplay.rs) runs before it with the frame's pose:
- the lag rotation goes into the root's local rotation (in the parent's frame if it has one),
  turned about the eye (the camera node less the root), so the game's own update carries it to the
  arms and the weapon after every animation of the frame;
- the offset, and the translation that makes the turn pivot on the eye, go into the root's world
  translation that the camera is placed from, so the camera moves the other way.

Afterwards ([`unplace_pose`]) the local rotation is put back, so nothing builds up, and the world
translation too when the game did not update the root. Recoil, the impulse, the reach and the
dismemberment are unchanged.

### Ragdoll hit force (`crates/host/src/hit_force.rs`)

Patrick, 2026-11-14: a body that dies, or is knocked down into its ragdoll, carries the
momentum of what hit it. Independent of dismemberment; no new animations, no slow motion, the
kill camera untouched.

- **The game's own push does the work.** The kill impulse (`008ae000`) finds the body under the
  hit and pushes every rigid body of the ragdoll from the impact point along the hit's direction
  (the projectile's path; from the weapon to the hit for a swing), more for the parts near the
  impact within the weapon's impulse radius, each part by its own multiplier
  (`TESHavokUtilities::AddExplosionImpulse`, `0062b660`), radially with a falloff for a blast.
  Its strength (the hit's health damage) is replaced for the call by the hit's momentum.
- **The momentum, in damage points:**
  - a gun: its damage per projectile less the target's damage threshold (actor value `0x4c`;
    never below 20 % of it), times `projectiles^0.75` (a shotgun's pellets share their push);
    thrown and placed weapons by their damage;
  - a swing: the weapon's damage x `(1 + weight / 10)` x reach (0.5 to 2) x Strength
    (`0.6 + 0.08 x STR`: 0.68 to 1.4) x skill (`0.8 + skill / 250`: 0.8 to 1.2) x 1.5 for a power
    attack (recorded where the game makes the melee hit, `HitData::InitializeHitData_ov3`,
    `009b5170`), less the threshold the same way; unarmed: the hit's total damage, reach 1;
  - a blast: the game's own (its health damage, the threshold already taken) with its falloff;
  - then x `iHitForce` % x `iImpulse` %, at most 160 (bodies are thrown, never launched).
- **Knock-downs** by a blast (`MiddleHighProcess::KnockExplosion`, `0091fee0`): their push x
  `iHitForce` %, raised no further than 160.
- **For a later leg-loss fall:** `hit_force::push_body` takes any hit and pushes the body the
  same way.
- `iHitForce=0` (Hit Force: Off) gives the previous behaviour: the game's push scaled by
  `iImpulse` and, for melee, by weight.

| Constant | Value |
| --- | --- |
| `MIN_THROUGH` (share of damage through armour) | 0.2 |
| `PELLET_EXPONENT` | 0.75 |
| `WEIGHT_PER_DOUBLING` | 10 lb |
| `POWER_ATTACK` | 1.5 |
| `REACH_RANGE` | 0.5 to 2.0 |
| Strength factor | 0.6 + 0.08 x STR |
| Skill factor | 0.8 + skill / 250 |
| `MAX_MOMENTUM` | 160 |

### Knockdowns of living actors (`crates/host/src/knockdown.rs`)

Patrick, 2026-11-14: Skyrim-style knockdowns, on the hit force. Never the player; companions only
with `bKnockdownCompanions=1`.

- **When:** over `Actor::HitMe` (`0089a760`), after the game applied the hit, for a living
  character not already knocked down: the hit's momentum as the hit force works it out (damage
  less the target's damage threshold, projectiles, weight, reach, Strength, skill, power attack).
  Blasts too (2026-11-14 fix): the blast's health damage (the game took the threshold), pushed
  from the blast's centre through the chest; a blast the game knocks down itself (its knock state
  already set) is left to it. A process below middle-high (level `+0x28` above 1) has no knock
  state and is skipped.
- **The 2026-11-14 fix:** a hit was taken for a blast when its source answered virtual `+0x224`,
  which is `IsProjectile`: every bullet answered it, so every gunshot counted as a blast and was
  skipped, and a grenade's hit (an explosion, which does not) counted as a thrown weapon of the
  grenade's own damage (about 1). `IsExplosion` is `+0x220` (the process lists hand such a
  reference to `explosion.cpp`).
- **Log:** the run's settings at start (`Knockdown: bKnockdowns=...`), then a `Knockdown:` line
  per decision on a living character (form id and name, kind, momentum, DT, resistance, body,
  scale, threshold, the decision; why one was left standing), the game's `CanKnockDown` answer
  with each of its questions when it refuses, and after `KnockExplosion` whether the actor has a
  ragdoll (slot `0x38c`) and its knock state; at most 20 lines a second.
- **Physics (Patrick, 2026-11-14, replacing the per-body points):** the hit puts a momentum p
  (N s) into the body, added to the actor's **stack**: every hit that lands (each landed pellet
  of a shotgun, each round of a burst, each swing) adds its own p, and the stack decays
  exponentially, by half every `iImpactStacking` ms (Settings > Gameplay "Impact Stacking",
  0.10 to 1.00 s in 0.05 s steps, default 0.30 s): stack = stack x 0.5^(t / half-life) + p. Its
  direction is the momentum-weighted sum of the hits' directions (each p x its direction, decayed
  the same). The decisions use the stack, the launch is the stack over the mass along that
  direction, and a knockdown empties it. Each decision's log line shows the hit's p, the stack
  left of earlier hits, the total, the hits in it, the half-life and the direction.
  The stack's speed is v = p_stack / mass x `iHitForce` %, at most 12 m/s; it topples at the speed that lifts its
  centre of mass (height h) over the edge of its stance (half-width b), times a recovery factor 3
  (a standing body steps to catch itself below that), times `iKnockdownThreshold` %:
  v_topple = 3 x sqrt(2 g (sqrt(h^2 + b^2) - h)), lengths x scale, mass x scale^3. Knockdown at
  v_topple, stagger at 40 % of it (the game's own hit reaction only, logged, no push: 2026-11-15).
  The launch is v along
  the hit (lifted 0.3).
  - A round: p = mass x speed at impact x the share it leaves in the body x 15 (game feel: a .50
    round's real 38 N s moves 80 kg only 0.5 m/s) x armour 1 / (1 + DT x load factor / 40). The
    share: pellets and slugs 1 (they stop), a standard round 0.6 (it mostly goes through), times
    the load's transfer (HP 0.84, JSP 0.72, AP 0.42). Speed: the tracked bullet's (less after a
    pass), else the loaded ammo's calibre, load and weapon mods (Classic hitscan too).
  - A swing: p = (weapon weight in lb x 0.4536 + 3 kg of arm) x 8 m/s x Strength (0.68-1.4) x
    skill (0.8-1.2) x reach x 1.5 for a power attack x 0.8 x armour.
  - A blast: p = 3.5 N s x the game's damage to the body at its distance (its falloff).
  - No known calibre: 0.3 N s per damage point as a standard round.
  - Mass: the ragdoll's own Havok masses (`hkpMotion::getMass`, `00c86430`, summed over its rigid
    bodies once `KnockExplosion` has made them dynamic; keyframed bones weigh 0), measured at each
    kind's first knockdown and used from then on (within a fifth to five times the figure),
    else by body x scale^3.

  | Body | Mass kg | h m | b m | v_topple m/s |
  | --- | --- | --- | --- | --- |
  | Human | 80 | 1.0 | 0.15 | 1.41 |
  | Feral ghoul | 55 | 0.9 | 0.15 | 1.48 |
  | Super mutant, nightkin | 250 | 1.2 | 0.25 | 2.13 |
  | Robot | 200 | 1.0 | 0.3 | 2.79 |
  | Animal (dogs, coyotes) | 30 | 0.4 | 0.2 | 2.89 |
  | Deathclaw (abomination) | 400 | 1.4 | 0.5 | 3.91 |
  | Mutated animal | 150 | 0.8 | 0.4 | 4.08 |
  | Giant | 1500 | 2.0 | 0.8 | 5.22 |
  | Mutated insect | 25 | 0.3 | 0.4 | 5.94 |

  | Hit | p N s | human | dog | super mutant | deathclaw (m/s) |
  | --- | --- | --- | --- | --- | --- |
  | .50 MG | 340 | 4.25 | 11.3 | 1.36 | 0.85 |
  | .45-70 | 112 | 1.40 | 3.74 | 0.45 | 0.28 |
  | .308 | 86 | 1.08 | 2.88 | 0.35 | 0.22 |
  | .44 Magnum | 63 | 0.79 | 2.11 | 0.25 | 0.16 |
  | 5.56mm (HP) | 36 (48) | 0.45 (0.60) | 1.19 | 0.14 | 0.09 |
  | 9mm | 26 | 0.32 | 0.86 | 0.10 | 0.06 |
  | 12 ga, 8 pellets landed at once / slug | 168 / 208 | 2.10 / 2.60 | 5.6 / 6.9 | 0.67 / 0.83 | 0.42 / 0.52 |
  | Sledgehammer / power | 77 / 116 | 0.97 / 1.45 | 2.6 / 3.9 | 0.31 / 0.46 | 0.19 / 0.29 |
  | Knife / fists | 22 / 19 | 0.28 / 0.24 | | | |
  | Frag blast, 100 damage | 350 | 4.38 | 11.7 | 1.40 | 0.88 |

- **Knockdown** (momentum at or past the resistance): the game's own blast knock-down
  (`MiddleHighProcess::KnockExplosion`, `0091fee0`): the actor leaves its furniture, goes into its
  ragdoll and gets the game's push from a point 40 units behind the impact along the hit's
  direction (the hit data's impact point and direction; for a swing without them, from the
  attacker through the chest), so the part hit is pushed hardest and the body spins or flies from
  there; strength = momentum x 0.35 x `iHitForce` %, between 8 and 40. The game gets it up again
  through its get-up idle (`UpdateKnockState`).
- **Fourth 2026-11-14 fix (Patrick on a5644a3: still thrown on getting up, mostly in cramped
  places and on uneven ground; the log ended at "get-up allowed ... set at ..."):**
  - Causes: the get-up spot was a navmesh point up to 200 units from the body (one 58 units from
    another actor set there 4 s before, one 450 units away) and kept the higher of the bones' and
    the navmesh's height (about 20 units over the floor): overlapping capsules and a capsule in a
    slope were pushed out violently. And the stored push grew every frame while the body lay
    (1, 5, 7, 11, 16): `bhkCharacterController::characterInteractionCallback` (`00c6f500`) adds
    to the knocked-down actor's standing capsule a push (`PushDelta` `+0x580`, `iPushCount`
    `+0x590`, and `SetVelocityModifier` with a 0.01 s time, a hundredfold velocity) from every
    character that walks into it; only the stored velocity was being cleared.
  - Now the actor stands where the ragdoll ended up: its pelvis (`Bip01 Pelvis`, the node's
    `m_kName` `+0x08`) on the ground under it (the navmesh floor within 24 units across and 40
    under the lowest bone, else the lowest bone less 6). Moved only when a capsule (22 units
    times its scale, 120 tall) does not fit there: to the nearest of 16, 32, 48 and 64 units
    (eight ways) clear of walls and statics (`IsValidLocation`'s line of sight, the navmesh point
    within 12 units) and of other standing actors' capsules (the high and middle-high process
    lists, JIP's walk); else left at the body, never far.
  - The pushes are cleared with the stored velocity while it lies and for 0.6 s after the get-up
    starts; for 2 s the controller's rising speed and the bones' are held to 60 units/s, and a
    `get-up +t s` line every 0.2 s (or at once on a clamp or overlap) gives the position, how far
    it was moved off its spot across and up (depenetration), the controller's velocity, the
    clamps and any actor overlapping.
- **Third 2026-11-14 fix (Patrick on 535c53a: a crash after leaving the range, still no
  momentum, still thrown on getting up):**
  - The crash (`read of unmapped memory at 3e2aabe7 in 009445b0`): `3e2aabe7` is `0x3e2aaaab`
    (the float 1/6) plus `0x13c`, the knock state's offset: a knocked-down actor kept in the
    get-up list across the return to the main menu was read after the game freed it (its process
    word now held a float). Every knock-down list now keeps the actor's form id and checks each
    frame that the game's form map (`LookupFormByID`, `004839c0`) still gives the same reference;
    all of it (and the ballistics' bullets and continuations) is dropped at a load, a new game and
    the main menu (the Test Range's end seams, which fire in every game).
  - No momentum: the launch waited for the frame after knock state 2, but the game turns 2 into
    1 (lying) within a frame (`UpdateKnockState`: once the blend value is 0), so the log said
    "launch dropped: the ragdoll never went limp (knock state 1)" every time. It is now given
    right after `KnockExplosion` returns, where the game gives its own blast's push (the bones were
    just set moving under physics), and checked the next frames in knock state 2 or 1.
  - Thrown on getting up: `Actor::HitMe` stores a knockback in the character controller
    (`SetVelocityModifier`: `VelocityMod` `+0x500`, `fVelocityTime` `+0x524`, the larger one kept)
    on every hit, also on a body that is already down, and the controller only spends it when it
    moves again: at the get-up, which threw the actor. While it lies it is now cleared each frame
    (logged), and at the get-up the bones' velocities are zeroed too. And 274ad9c/535c53a damped
    every slow bone each frame, which woke the ragdoll every frame (`hkpRigidBody::setLinearVelocity`
    activates the body), so it never slept and trembled: only outliers are touched now, during the
    first 1.5 s.
- **The launch and the get-up** (second 2026-11-14 fix: bodies dropped straight down, and flew up
  and shook when getting up, upside down against a wall in a small interior):
  - Cause of the drop: the previous build held every bone to 12 Havok units a second (about
    1.2 m/s; a Havok unit is 7 game units, the game's float at `011b1864`) for 1.5 s, which took the
    push and even the fall away; and the push itself ran inside `KnockExplosion`, before the
    ragdoll was fully set up.
  - Now `KnockExplosion` only puts the body into its ragdoll (strength 2), and the frame after it is
    limp (knock state 2) every bone gets the same velocity added along the hit, lifted 0.3:
    momentum x 6 game units/s x Hit Force %, 150 to 700 (about 2 to 10 m/s; a human's threshold 45
    gives about 4 m/s, an anti-materiel round about 8.6). The next frame checks that at least 40 %
    is left and gives it again (twice at most). Limits are in game units now: bones at most 1,400
    units/s and spin 20 rad/s while flying (outliers only), settling below 12 units/s.
  - Cause of the get-up: the game gets up on a timer (`UpdateKnockState`, `00920150`: state 1 a
    moment after the ragdoll is limp, then the get-up idle), whether the body is still or lying, so
    a body wedged upside down against a wall was stood up inside it. Now states 1 and 3 wait (the
    hook skips the game's update) until every bone is slower than 30 units/s for 20 frames and the
    bones lie within 70 units of height, at most 8 s; then the actor and its character controller
    are set, with no velocity, at a clear spot by the body: the game's `PathingLocation` of the
    body's centre (`006dce10`), moved onto the closest navmesh triangle (`006ddc00`), within
    200 units and valid 25 units clear four ways (`IsValidLocation`, `006ded70`); else, and for a
    body that never lay down, where it was knocked from (`SetLocationOnReference` `00575830`,
    `bhkCharacterController::SetPosition` `005620e0`, `bhkCharacterProxy::SetLinearVelocity`
    `00926980`). The log has `impulse applied`, `the launch holds`, `get-up held` and `get-up
    allowed ... set at ...` lines.
- **The strength is a speed** (first 2026-11-14 fix of bodies flying off and shaking):
  `AddExplosionImpulse` (`0062b660`) adds to each bone's velocity its own direction away from the
  push point times the strength times the bone's layer multiplier, with no falloff and no division
  by mass; the game's `PushActorAway` uses `GetKnockbackSpeed` (tens). The first build gave 135
  to 600 from a point 40 units behind the impact, so bones flew and were pulled apart radially
  (the joints fighting it: the shaking). Now 8 to 40, from 400 units behind (all bones the same
  way). While knocked down (knock state 2) every bone's speed is held to 12 Havok units/s and its
  spin to 15 rad/s for 1.5 s; afterwards a bone slower than 1.5 has its sideways motion and spin
  damped (6 a second; the fall is left to gravity). Guards: no second knock-down within 2.5 s of
  one or 1.5 s after getting up; the game's own `KnockExplosion` (blasts, `PushActorAway`, a
  knock-out) adds no second push to an actor knocked here within 2.5 s; a push stored in the
  character controller is cleared before the ragdoll. Each clamp and guard is a `Knockdown:` log
  line.
- **Stagger** (at or past 40 % of the topple speed): the game's own hit reaction only, which
  `Actor::HitMe` plays; nothing pushes the standing actor (2026-11-15: the stagger's JIP shove,
  from the stacked speed, fired on every rapid hit below topple; the shoves added up in the
  character controller and slid or launched the standing actor). A `Knockdown: ... stagger` line
  gives this hit's speed and momentum and the stack's. Only a knockdown moves the body.
- **Falls:** the game's fall damage runs from the character controller, which a ragdoll does not
  use, so each knocked-down actor is followed (its highest point) until it is down, getting up or
  12 s have passed; a drop past `fJumpFallHeightMin` takes the game's formula
  (`(drop - min)^exponent x mult`) through `Actor` slot `0x338` with the attacker (a kill is the
  attacker's: the player's), and each leg takes the game's landing roll
  (`iJumpFallLimbDamageChance`, `fJumpFallLimbDamageMult`, slot `0x3ac`).
- **For a later leg-loss fall:** `knockdown::knock_down(actor, attacker, point, direction,
  momentum)`.
- No slow motion; the kill camera is not involved.

| Constant | Value |
| --- | --- |
| Size | mass x scale^3, lengths x scale (scale 0.25 to 4) |
| Stagger share | 0.4 |
| Knock-down push (ragdoll only) | 2 |
| Launch | v = p / mass x Hit Force %, at most 12 m/s, lift 0.3 |
| Stagger force | none (the game's hit reaction only) |
| Push origin behind the impact | 400 units |
| Bone speed / spin while flying | 1,400 units/s / 20 rad/s, for 1.5 s |
| Settling damping | 6 a second below 12 units/s (sideways and spin) |
| Get-up | bones under 30 units/s for 20 frames and within 70 units of height, at most 8 s; a clear navmesh spot within 200 units (25 clear), else where it stood |
| Cooldowns | 2.5 s after a knock-down, 1.5 s after getting up |
| Fall followed for | 12 s |

### Active ragdoll, stage 1: the flinch (`crates/host/src/active_ragdoll.rs`)

Patrick, 2026-11-15: a GTA IV / Red Dead style active ragdoll, in stages (2: balance and
procedural stepping; 3: bracing and falling into the ragdoll, later). Stage 1: a hit below the
knockdown's topple speed makes the living body flinch. `[nv-rs Gunplay] iHitReactions` (0-200 %,
0 off; Settings > Gameplay "Hit Reactions").
- **The kick:** per hit (never the stack), the knockdowns' momentum and mass: v = p / mass x
  Hit Reactions %. The struck bone is the rig's bone whose segment the impact is on
  (`flinch::struck_bone`). Each bone of the rig
  (`Bip01` pelvis, spine 0-2, neck, head, clavicles, upper arms, forearms, thighs, calves; a
  creature's that has them) gets an angular kick of 16 rad/s per m/s x its weight, about the axis
  across its lever to the impact and the hit's direction; weight = the role's lightness (pelvis
  0.35, spine 0.7, neck 0.8, head 0.9, clavicle 1.0, upper arm 1.2, forearm 1.1, thigh 0.6, calf 0.7)
  x e^(-distance / spread), spread = 0.25 m + 0.4 m per m/s, at most 1.5 m (a pistol round stays at
  the shoulder and upper torso, a .50 or a sledgehammer reaches the whole body; a low torso hit is
  nearest the pelvis and turns the hips more). The head is also kicked 0.6 of the spine's kick
  against it (it lags, then snaps). A leg hit buckles the knee (thigh and calf of that side bend,
  6 rad/s per m/s) and drops the pelvis (0.6 m/s per m/s, at most 12 cm).
- **The spring back:** each bone a spring-damper toward its animated pose, under-damped (an
  overshoot): pelvis 3.2 Hz / 0.5, spine 3.0 / 0.45, neck 2.6 / 0.4, head 2.2 / 0.35, clavicle 2.4 /
  0.4, upper arm 1.6 / 0.3, forearm 1.8 / 0.3, thigh 3.0 / 0.5, calf 3.2 / 0.5; its muscle (stiffness)
  lowered by up to 60 % near the hit, back in a quarter second; held within each role's limit
  (pelvis 0.2 rad ... upper arm 0.9). Hits layer (each adds to the bones' motion).
- **How it shows:** not through the Havok joints: a living actor's ragdoll bodies are keyframed to
  its animation, and switching a standing actor's ragdoll into a powered blend is what made the
  knockdowns' get-ups unstable. A pose layer instead: at the draw's set-up
  (`Main::DrawWorld_MTSetup1stPerson`, `00874c10`, after the animation posed the actors) each bone's
  local rotation is turned by its spring's world-space offset carried into its frame (P^T R P L;
  the pelvis also dropped), and the actor's 3D is updated by the game's `NiAVObject::Update`
  (`00a59c60`, an update data of no time, as `KnockExplosion` updates a body). A bone the
  animation did not write again (a pause, a menu) is put back to its pose first; at rest the pose
  is put back. The actor's reference and character controller are never moved.
- **Never:** the player, the dead, an actor knocked down or getting up (or in the get-up's watch),
  a companion unless `bKnockdownCompanions`, an actor below the high process. Everything is
  dropped at a load, a new game or the main menu.
- **Log:** `Hit reaction: <id>: flinch at <bone>: <p> N s on <mass> kg -> <v> m/s (Hit Reactions
  <n>%), spread <m> m, <k> of <n> bones`.

### Active ragdoll, stage 2: balance and stepping (`crates/host/src/balance.rs`)

Patrick, 2026-11-15. `[nv-rs Gunplay] iBalance` (0-200 %, 0 off; Settings > Gameplay "Balance").
- **When:** a hit below the topple speed gives the centre of mass (the pelvis) v = p / mass x
  Balance % (this hit's momentum and the ragdoll's mass, as the flinch). Its capture point lies
  v sqrt(h / g) along the push (h the pelvis's height over the feet). Within the feet and 0.15 m
  around them: no step (the flinch alone). Past them: catch steps toward it, up to 4, each at most
  0.6 m (x scale), the feet alternating from the one on the push's side and 0.12 m apart across it:
  bigger hits take more and longer steps. A new hit while stepping re-plans from the planted feet,
  half the old speed carried.
- **How:** the legs by two-bone IK in the pose layer (thigh and calf turned so the ankle reaches
  its target, the knee bending the way it bends, the foot kept level), the swinging foot on an arc
  (0.12 m high) over 0.16-0.3 s, the pelvis carried over the feet and 8 cm past them along the
  push (a 6 Hz spring), 5 cm lower; the arms raised out 0.9 rad (blend in 0.08 s); after the last
  step the pose blends back over 0.35 s. The flinch's springs stay on top. When a foot lands the
  reference and its character controller are set to the feet's middle (no velocity: the actor's
  position follows the feet only), its height changed as the feet's ground changed; the pelvis's
  offset takes the move back. If its AI moves it more than 30 units otherwise, the steps give way.
- **The ground:** each target is put on the navmesh under it (the foot's ankle height kept):
  slopes and stairs within 40 units up or down. No walkable ground within 30 units across: a wall
  or obstacle; the step is shortened to two thirds, then a third, and with no room the body
  catches itself where it is. Ground more than 60 units lower: a ledge; the foot goes over it and,
  halfway through that step, the body falls into its ragdoll (`knock_down` with the push's speed;
  its fall's damage as the knockdowns count it).
- **Log:** `Hit reaction: <id>: balance: pushed <v> m/s ..., capture point <m> m past its feet:
  <n> step(s) (<lengths> m) ...; <outcome>`, a line per landed foot (where, and how far the actor
  followed), `steps off a ledge: falls`, `steps done`.
- **For dismemberment:** `balance::Limbs` per rig (a body with one leg does not step; an arm lost
  does not go out or, in stage 3, brace).

### Active ragdoll, stage 3: bracing and falling (`crates/host/src/active_ragdoll.rs`)

Patrick, 2026-11-15. Rides on Hit Reactions and Balance (either on).
- **Into the ragdoll, smoothly** (replacing the hard switch): a knockdown first kicks the posed
  body (the flinch, full strength) and lets it go over for 0.12 s (the torso leaning 0.4 rad into
  the fall, the knees giving 0.3 rad, the pelvis following the fall), then the ragdoll takes it
  with the fall's speed (`knock_down`). From then the ragdoll owns the bones: the pose layer
  writes nothing back (it would fight it; the rig just goes, also for the dead). Hits arriving
  while it goes over wait for the ragdoll.
- **Walls:** a step with no room (no walkable ground ahead) braces: both hands (the arms it has)
  reach to the wall ahead at the shoulders (two-bone IK of upper arm and forearm), in 0.12 s, held
  0.5 s, released over 0.4 s, the steps that fit still taken. A push of more than 2.5 m/s (x Balance
  %), or no arm to brace with, crumples it against the wall into its ragdoll at half the speed.
- **Ledges:** the foot goes over, then 0.4 s of windmilling (the arms swinging 1.6 rad at 2.2 Hz,
  the torso leaning back), then the ragdoll at the push's speed; the fall's damage as before.
- **Too much to catch:** more distance than 4 steps can cover: the steps are taken, then it goes
  over at 0.6 of the speed. On one leg (dismemberment later) it cannot step: it goes over at once.
- **Bracing on the ground:** for 1.2 s after the ragdoll takes it, while the pelvis is more than
  25 units over its lowest bone and dropping, the hands and forearms (the arms it has) are steered a
  quarter a frame toward 1.5 m/s along the fall and 2.5 m/s down: the arms go out for the landing.
- **The get-up fixes stay** (the get-up waits for a still, lying body; stands where it lies; the
  watch).
- **For dismemberment:** `balance::set_limbs(actor, Limbs)` (a lost leg: no steps, it falls; a
  lost arm: no brace on that side), `balance::limbs_of`; the rig keeps per-role spring, gain and
  limit tables for masses and muscles.
- **Log:** `braces against a wall`, `lets go of the wall`, `steps off a ledge: windmills, then
  falls`, `the steps could not catch it: falls`, `into its ragdoll after <t> s of going over`,
  `braces for the ground with its arms`.

### Active ragdoll: the stretching and the freeze (Patrick, 2026-11-15, f36ddda)

An armed Powder Ganger in the Test Range flinched, took its catch step, then stuttered,
stretched and grew, and the game froze. Two causes, both fixed:
- **The pelvis's follow ran away.** Stage 2's `Stepping::follow` (the pelvis carried over the
  feet) integrated a 6 Hz critically damped spring explicitly in two halves of the frame: at a
  slow frame (50 ms: a hitch, a slow machine) it diverged right after a foot landed and the shift
  jumped by the actor's move; the body was pulled after it, growing every frame. It is now
  integrated implicitly in steps of at most 4 ms, stable at any frame time; the lab reproduced
  the blow-up (armed, two .357s, a .44, one catch step, 50 ms frames) and now passes it.
- **The writes could add up.** A bone the game's ragdoll-controller pass wrote back a hair off
  was taken for fresh animation and got the offset again, and the animated world pose was read
  back from bones that held the last write. Now the base is remembered while a bone still holds
  what was written (within tolerances), the animated world pose is worked out from the
  animation's own locals, every rotation is made orthonormal (no scale ever written), and what
  is written is the absolute local the shown pose needs (`ragdoll_core::pose`), so the pass's
  wrapper writing it again changes nothing (and it no longer updates the scene graph on the AI
  thread).
- **Guards:** the body's steps are never longer than 2 ms whatever the frame; every frame the
  body is checked (finite, speeds held to 12 m/s and 30 rad/s, no bone 2.5 m from the pelvis)
  and the pose (offsets rotations, the pelvis moved at most 80 units, the animated joints where
  the actor is); anything wrong drops the body back to the animation, logged `the active ragdoll
  is dropped back to the animation: <why>`.
- **Lab tests:** an armed body hit five times while it steps at 240, 144, 60, 30 and 20 frames a
  second stays believable and returns to its animation; the game's write path over the lab's
  poses (the pass writing back three times a frame, a hair off, the animation skipping frames)
  makes exactly the shown pose and never adds up.

### Crash fix: a rigged terminal's shot (Patrick, 2026-11-17, REPCONN's basement)

Activating a booby-trapped terminal stopped the game: `read of unmapped memory at 0048385c (in
00525980 < 009bca60 < 00523150 < 0087b990 < 0087a790 < 0086e650)`: the main loop's task queue
ran `TESObjectWEAP::Fire` (`00523150`) for the trap, which made its projectile
(`CreateProjectile`, `009bca60`), and under it `TESObjectWEAP::GetCurrentAmmo` (`00525980`) read
an actor's process (`+0x68`) of what was no actor. The game's own fire asks it only for an actor;
the ballistics' hook over `CreateProjectile` (`crate::ballistics`, Projectile mode) asked it for
every shot's shooter as it was: the terminal's reference. Now `ballistics::current_ammo` asks it
with no holder (the weapon's default ammo) for anything but an actor (`jip::commands::is_actor`),
for the shot, the hit's round and the breakdown; a knocked-down actor's fall damage names no
attacker but an actor. Test: `ballistics::a_trap_shot_asks_for_no_holders_ammo`. The merged JIP
patches (#624) and the knockdown's companion refusal were not on the path.

### New Physics (Patrick approved the design 2026-11-17)

One balance controller in place of the layered reactions (the drawn flinch, catch steps, knockdown
thresholds), on FNV's own skeleton's bodies and joints: recoil, stagger steps, gather steps when
running, a fall into the game's ragdoll, and (M6) downed but still fighting, every pose emerging
from the physics and the AI, never staged. A separate option, off by default: Settings > Gameplay
"New Physics", `[nv-rs Gunplay] bNewPhysics` (`NV_GUNPLAY_NEW_PHYSICS`). Off, the current reactions
run as they do, and their fixes keep landing. Milestones: M0 the joint limits and the switch; M1
the body and drive (`ragdoll-core::np_body`, `np_drive`) with the lab's recoil tests and videos; M2
balance and steps; M3 the host behind the switch, the game's stagger animation suppressed; M4 the
fall and get-up; M5 in-game metrics and tuning; M6 downed but active.

**M0.** The switch (nothing reads it yet: 0 and 1 play the same; the run's first `Knockdown:` line
says `bNewPhysics=`). The joint limits, supplied by Patrick from `skeleton.nif` (not read from the
game's `bhk` constraints): `crates/ragdoll-core/data/fnv_skeleton.txt` (moved from the host's
tests-data, one file for the host's skeleton fixture, the lab and New Physics), its CONSTRAINTS
section: the 17 joints (14 `bhkMalleableConstraint`s at strength 0.9 wrapping 8 ragdoll and 6
limited hinge constraints, the spine's 3 plain ragdoll ones), child and parent body, pivots and
axes in each body's frame, limits in radians as stored. `ragdoll_core::np_skeleton` parses the
nodes, the 18 bodies (81 kg) and the joints, and measures a joint against its limits from its
bodies' world rotations: a ragdoll joint's cone (the twist axes apart), plane (A's twist axis out
of B's plane) and twist; a hinge's angle (B's zero reference to A's about A's axle). Checked on the
data's bind pose and on the host's fixture (the tree laid out by the engine's update), within 0.1
degrees of each other and of Patrick's figures: the knees 69.9, the ankles 2.5, the elbows -4.5,
the wrists -2.5 and 1.5; the cones of the thighs 3.2, the spine 3.9, 3.7 and 10.0 (Spine2 at its
plane limit), the head 0, the upper arms 3.6. What the data has that New Physics must allow for:
- the neck lies 20 degrees out of its 6.8 degree cone and below its plane's -3.3 (Havok pulls it
  in at the switch to the ragdoll);
- the legs', spine's, neck's and head's pivots meet within 0.2 game units in the bind pose, the
  arms' do not: the shoulder's 1.1 apart, the elbow's 2.2 (its pivot 19.6 units along the upper arm,
  the elbow node 17.4), the wrist's 2.0; and the arms' hinge axles are 15 degrees apart. The
  bodies themselves sit on their nodes (within 0.12, the data's rounding).

**M1, the body and its muscles** (`ragdoll-core`: `np_body`, `np_drive`, `np`; the lab's
`np_lab`). The 18 bodies are rigid capsules with the data's masses and centres; the 17 joints are
pinned at the child's node (the parent's anchor taken from the bind pose, so the drawn bones keep
the skeleton's lengths) and held inside the data's limits, each widened to take in the animated
pose's own value plus 3 degrees (the neck's bind pose lies outside its cone). Solver: extended
position based dynamics (XPBD) with 16 substeps a frame: hard joints and limits, the muscles as
compliant springs, floor contacts with friction. It replaces the plan's "Featherstone with 4
substeps": hard limits and stiff springs stay stable at any frame time without a force integrator's
step limits. Muscles: the design's table, the spine 600 N·m/rad (the table's 400 bent the torso 1.2
times the targets), each damper critical for what its joint moves; a muscle hold that carries the
weight below each joint (gravity compensation: the springs answer only to hits); a hit softens its
chain by up to 0.8 (0.6 per joint further), held 0.05 s and eased back over 0.55 s. Each body's turn
off its animation's also decays at 90 per second (tracking damping, half-softened by a hit): FNV's
root body is an 8 kg ball of 4 cm and the spine body a 9 kg ball under 30 kg of chest, arms and
head, and a joint's damper alone, capped by the lighter body, let the upper body swing for a second
and a half whatever its size. M1 holds the root up by a capped spring toward the animated root (M2's
balance replaces it). Lab results (FNV's own bodies and limits; the animation its bind pose): a chest
hit of 0.3, 0.8 and 2 m/s turns the spine's top 6.2, 14.8 and 24.0 degrees (targets 6, 14, 25), the
head 1.08, 1.08 and 0.92 times that, back under 2 degrees in 0.38, 0.67 and 0.68 s; nothing under
the floor, the joints within 0.22 units. FNV's spine limits (each spine joint's plane at most 10
degrees, Spine2 already at its limit in the bind pose) let the torso bend about 18 degrees over the
pelvis; the rest of a big hit's 24 is the body leaning on its feet. At 2 m/s one foot is shoved and
stays (M2's steps re-plant it). Videos: `tools/ragdoll-lab/scripts/np_video.py` renders
`examples/np_frames.rs`'s frames.

**M2, balance and steps** (`ragdoll-core::np_balance`). Each frame: the whole body's centre of mass
and its velocity against the animation's, the capture point (`com + v/ω₀`), the support (the hull of
the planted feet's soles). Standing, the root's support holds the body across only while the
capture point is inside the feet (the ankles' and hips' reach: 900 N; 15% of that once it is out),
so a push past the feet makes it drift and step: the foot that reaches past the capture point
without crossing the other, landing 8 cm beyond it, at most 65 cm, over 0.3 s on an arc 8 cm high
(its leg's muscles relaxed under the foot's pull), pressed onto its spot until it stands; a landed
foot is held where it stands (a friction-strong pull: the leg's muscles pulled it back under the
hip) and no other step starts until it does. While the balance is engaged the legs' targets are the
animation's bent onto the footing (`flinch::leg_ik`): the hips lowered as the feet spread, each
ankle where its foot stands or lands (the animation's straight stance leg held the hips up and the
stepped foot hung in the air). Past three catching steps still outside, it falls (M4 hands it to
the game's ragdoll). Caught, one slow stance step puts the feet back on the animation's stance (the
foot landing off its animated place by what the other foot is), and the body's move against its
animation (the shift: the planted feet's middle against the animated feet's) is handed to the actor
(`Np::take_move`: the game moves its reference; the lab moves its animation). Moving (the animation
walks or runs it): its feet go where the walk puts them (the floor holds them up without gripping),
its legs at 4 times the standing springs (at the table's values the hips trailed a walk by 20
degrees); knocked off its path, the path follows the body for a second (the walk's own steps gather
it, the support's damper against the walk's velocity brings it back in line), and knocked past what
steps can take (its capture point 0.7 m off), it falls. Lab results on FNV's ragdoll: a 0.3 m/s chest
hit takes no step; 0.8 m/s one catching step back (caught by 0.55 s, the actor moved 28 units back,
standing again with its feet on the animation's stance by 2.4 s); 2 m/s one long step (45 units),
standing by 2.8 s; never a fall; a side push of 0.8 m/s steps out with the near foot, 18 units, the
legs never crossing; hit while walking (101 units a second) at 0.8 and 2 m/s it recoils 14.2 and 19.5
degrees and the walk carries on, every node within 9.2 units of the walk 1.5 s after the hit. The
walk is a procedural gait on FNV's skeleton standing in for the game's walk cycle.

**M3, in the game** (`crates/host/src/new_physics.rs`). With `bNewPhysics=1`, `Actor::HitMe`'s
wrapper (`crate::knockdown`) offers the hit's target to New Physics before the game's hit runs: a
living character other than the player, middle-high process or higher, standing, whose 3D has FNV's
18 bodies' nodes (by name) is taken (`[np take]`); anything else keeps the current reactions. For
an actor it has, the game's own reaction is not played: `Actor::HitMe_ov2` (`008a4af0`, the
physical hit reaction) is skipped, the crippled-limb idle's setting (`011df760`) cleared around
`HitMe`, `KnockExplosion` from `HitMe` refused (`crate::hit_force`), and the controller's stored
push cleared; the hit's momentum (the knockdowns' measure, Hit Force applied) goes to the body
nearest the impact (`[np hit]`), with no knockdown, stagger or flinch of the current reactions (and
whatever `bKnockdowns` says). Each frame at the draw's start (`crate::active_ragdoll::apply`, the same
write point): the body nodes' worlds as the game's update left them are the animation; the
controller steps on them (the floor at the actor's reference); each body is written as its node's
local, solved from its parent's world as written (the nodes between bodies, NonAccum's parent and
the clavicles, kept as animated), the tree updated; read back (drawn against simulated); after the
draw the game's locals are put back. A step's move is the actor's: its reference and controller set
on it (`knockdown::place`). Standing calm 1.5 s after its last hit and within 1.5 units of its
animation, it is let go (`[np reaction]`: the spine top's peak, catching steps, the move, the
read-back); dead or knocked down (the game's ragdoll), it is let go at once. Fixture tests on FNV's
skeleton through the real write, update, read-back and put-back
(`new_physics_fixture_tests.rs`): a 0.8 m/s chest hit draws exactly what it simulates (0.000
degrees, 0.000 units), puts the animation back exactly, peaks the spine's top at 14.8 degrees, takes
one catching step and moves the actor 28 units back, nothing under the floor, the current reactions
never touching it; 0.3 m/s takes no step (6.8 degrees); off, nothing is taken. Not run in the game.
There is no stagger animation group in the game to suppress (the research found none: the visible
stagger is `HitMe_ov2`'s physical reaction and the crippled-limb idle).

**M4, the fall** (`new_physics.rs`, `knockdown::hand_over`). A hit past any step (the capture
point still outside the feet after 3 catching steps, or more than 0.9 m outside them at once) is a
fall: New Physics hands the body to the game's own ragdoll that frame. Through the game's call,
`KnockExplosion` at strength 0 (the ragdoll with no push of its own; `crate::hit_force` lets this
one by), knock state 2 checked; each Havok body is set on New Physics' pose (the knockdowns'
`start_from`: the node's world turn and move from the animated pose the bodies were keyframed on
to the pose handed, put on the body) and given its body's linear and angular velocity; the
knockdowns' get-up watch and down log take it (aggressor none, the fall straight down), and the
get-up is theirs. `[np fall]` logs the bodies handed and the root body's and the centre's speeds.
Fixture test on FNV's skeleton (`np_falls_into_the_games_ragdoll_on_fnvs_skeleton`, the game's
calls doubled, the bodies keyframed on the animation each frame): a 4.35 m/s chest hit (an
anti-materiel round) falls in its first frame with no step, knock state 2, strength 0; the centre
moves at 305 units a second for a 304 hit, the pelvis at 282 (the body turns too), the game's
bodies within 2% of New Physics' (exactly, in the fixture), on the pose handed within 0.00 units
and at most 6.9 units past their own motion from the pose last drawn (a 24 ms debug frame; the arms
slow behind the chest within it), the centre not rising (-16 units a second; a hand at most 212),
the get-up watch on. The fixture's bodies were spaced 0x100 apart, so each body's position (+0x120)
fell on the next one's angular velocity: now 0x200. Not run in the game.

**M5, what the game says** (`new_physics.rs`). The log's lines for an actor New Physics has, each
named by its form id: `[np take]` (taken at its first hit), `[np hit]` (the momentum, the body
struck, the speed it gives the whole body), `[np step]` as each step lands (catching or stance,
the foot, where, the capture point still outside the feet, the hips lowered for reach), `[np fall]`
(the hand-over to the ragdoll: bodies, the root body's and the centre's speeds), `[np let go]`
(dead, knocked down by something else, or a bad pose) and `[np reaction]` when it is let go (the
frames and the seconds since its last hit, the spine top's most off its animation, the joints'
most softening, the capture point's most outside the feet, the catching steps, the actor's move,
the read-back's most). The clips (`crate::room_record`) sample each actor New Physics has every
frame, after `"body"` and `"down"`, as `"np":{"phase":"standing"|"stepping"|"recovering"|
"falling","spine":degrees,"outside":m,"soft":0..0.8,"centre_speed":units/s,"lower":m,
"swinging":0|1|null,"steps":n,"moved":units,"readback":[degrees,units],"since_hit":s,"dt":s}`
(`"np":null` for one it does not have). The fixture test checks the samples against what it
measures itself (the 0.8 m/s hit: 96 samples, spine 14.7 degrees as measured, the capture point
0.15 m out, 1 step, a swing shown; off: `"np":null`). How Patrick tests it: docs/PLAYTESTING.md
"Testing New Physics".

The tuning, all in `ragdoll-core` (a change is a rebuild; none is a player setting yet):

| What | Where | Value |
|---|---|---|
| Joint springs and dampers by body (N m/rad, N m s/rad), at least critically damped | `np_drive::GAINS`, `DAMPING` | thigh 500/50 ... hand 30/3; 1 |
| A hit's softening: most, the hit that gives it (N s), per joint further, hold, ease back | `np_drive::SOFT_MOST`, `SOFT_FULL`, `SPREAD`, `SOFT_HOLD`, `RECOVER` | 0.8, 162, 0.6, 0.05 s, 0.55 s |
| The bodies' tracking of the animation (1/s) | `np::TRACKING` | 90 |
| A catching step: past the feet (m), longest (m), time (s), lift (m) | `np_balance::STEP_PAST`, `STEP_MOST`, `STEP_TIME`, `STEP_LIFT` | 0.08, 0.65, 0.3, 0.08 |
| Steps before a fall; a fall at once past (m) | `np_balance::MAX_STEPS`, `UNREACHABLE` | 3, 0.9 |
| Settling after a step (s); a stance step past (m) | `np_balance::SETTLE`, `STANCE_OFF` | 0.3, 0.1 |
| Let go: calm for (s), within (units) of the animation | `new_physics::LET_GO_AFTER`, `LET_GO_OFF` | 1.5, 1.5 |

**M6, downed but active: the body (lab).** Nothing is staged: no seated, propped or lying state,
no get-up animation. With `Np::keep_down` set (the host does not set it yet: a fall still goes
to the game's ragdoll, M4), a hit past any step keeps the body: it falls with its muscles let go,
and from then on the controller holds it on whatever touches it (`np::Np::down_frame`). The
joints still aim at the animation, with springs that hold rather than stand
(`np_down::DOWN_GAINS`: legs 0.15, spine 0.35 with some of its weight held, the head kept up, the
arms 0.4). The floor and the walls are one contact pass
(`np_body::Model::step_world`, `Plane`): each push out of a surface is reported per body
(`StepReport::floor_load`, `wall_load`), the contact leaves no speed away from the surface (no
bounce) and slides by Coulomb's friction of its own push (the standing feet keep the weight's
grip, `Tracking::grip`). The pose is read from that (`np_down::read`: the loads on the feet, the
seat, the hands, the back on a wall, the weapon arm; the pelvis's height, the torso's tilt, the
centre of mass against the feet's soles and against everything touching) and named for the AI
and the log only (`np_down::classify`: lying, seated, propped, crouched, rising, standing, held
0.25 s before a change, `Labeler`). Nothing moves the body toward a label. The AI's reads
(`np_down::reads`): the weapon arm free (it carries under 40 N), the target within 100 degrees of
the torso's facing, a push up possible (something under it, the legs' push over the weight), a
crawl possible.

Rising is the controller pursuing balance when the AI asks (`Np::rise`). Its goals each frame come from
the pose it has: the torso turned up and leaning toward the feet (by how far the centre of mass is
behind them), each foot drawn along the floor in front of its hip (pulled there as the leg's
muscles drag a heel), the legs folded onto it by the legs' IK; the arms' push turns the torso up
while a hand can reach the floor or a wall (`RISE_ARMS`, capped by strength); once the centre of
mass is within 0.35 m of a foot that is down (a kneel counts), the legs' push lifts the pelvis
toward its standing height over the feet (`RISE_FORCE` 1500 N times strength and effort: the
standing balance's root support is the same kind of abstraction). Each joint's goal lies at most
0.3 rad from where it is (`RISE_REACH`). A rise whose pelvis gets no higher for 1.5 s gives up and
settles back (`RISE_STALL`); one with nothing to push from, or a push under the body's weight,
gives up in 0.5 s. Standing again (the pelvis at 88% of its height, the torso within 35 degrees,
the feet carrying it, still) for 0.4 s, the balance has it again.

**The joint limits while downed.** The skeleton's limits are a dead ragdoll's (the hip a 45 degree
cone and 30 degrees forward, the knee to 87 degrees, the spine 18 degrees a joint): no one gets
off the floor inside them. A downed body's limits take in, besides the data's and the
animation's, a living body's reach (`np_down::reach_pose`: the bind pose with each leg folded by
the legs' IK into a deep squat and each spine joint curled 26 degrees). The dead ragdoll, a fall
and the standing balance keep the data's limits as before. The limits never widen to a rise's own
goals: widening by each frame's target let the goals ratchet the joints 120 degrees past them.
For 0.3 s after a fall, and after standing up, a joint outside the limits is brought back at most
0.01 rad a substep, never snapped (`LIMIT_STEP`); otherwise the limits are hard. **For Patrick to
confirm:** the reach goes past his skeleton data on purpose; it is in one place
(`np_down::REACH_FOLD`, `REACH_CURL`).

Lab tests (`np_lab.rs`, FNV's skeleton): a 4.35 m/s hit with a wall 45 units behind slides down
the wall and stays propped (the wall carrying at least 420 N, the label "propped" over the last
2 s, never in the wall, still); without the wall it ends lying, still. Asked to stand it stands by
its balance 3.45 s after the ask on its back, 4.15 s propped on the wall, the centre of mass over
its feet at the end of the rise, no node moving more than 4.6 units or 10.2 degrees a frame.
Whatever pose it lands in, it gets up: of 18 landings (3.8, 4.35 and 5 m/s from six directions,
the AI asking again a second after a rise gives up) 17 stand, in 3.4 to 7.8 s. Too weak
(effort 0.4: the legs' 600 N under its 795 N) it tries, gives up and lies still. Hit again
(1.5 m/s) half way up, it ends standing. Nothing under the floor, the solver's speed guard (30 m/s,
200 rad/s, `np_body::MAX_SPEED`, `MAX_SPIN`) never used after a fall's impact. Videos:
`np_m6_fall_rise`, `np_m6_wall`, `np_m6_weak`, `np_m6_hit_rising` (`examples/np_frames.rs m6`).
The solver's other changes (the floor's Coulomb friction, no bounce, the walls) leave the M1 and M2
tests as they were.

**M6, downed but active: the game** (`new_physics.rs`). With `bNewPhysics=1` a hit past any step
now keeps the actor downed instead of handing it to the game's ragdoll (M4's hand-over is kept
for a collapse it cannot rise from). Each frame the downed body is drawn as simulated (the same
write, update, read-back and put-back), the actor's reference follows the pelvis, and the game's
path movement is held for it (`ActorMover::Update`, `009dc120`, wrapped in `crate::knockdown`:
run, its move delta zeroed for a downed actor). The AI's choice is made from the pose it has
(`downed`): settle (its first second down); **fire** from the ground (its weapon arm free and its
combat target, the actor's `0x42c`, within 100 degrees of the torso's facing: the game's own
attacks go ahead, each shot wider as a support hand 0.6 off a two-handed grip, `crate::one_hand`;
at most 4 s before it tries to stand); **stand** (the controller pursues balance, M6 lab); a
second's **wait** after a rise gives up. Whenever the choice is not fire, its shots are withheld
(`crate::gunplay`'s `TESObjectWEAP::Fire` hook skips them). After three rises give up it collapses
into the game's ragdoll and the knockdowns' get-up (M4's hand-over). **Dying** (downed or standing)
puts the game's ragdoll's bodies on the pose drawn, moving as New Physics' bodies were
(`knockdown::place_bodies`), so the death neither jumps back to the animation nor flings. The log
says `[np pose]` as the label changes (pelvis height, tilt, the loads on feet, seat, hands and a
wall, the weapon arm, the target's angle, the legs' push against the weight), `[np choice]` as
the choice changes (with the reads behind it), `[np death]`; the clips' `"np"` sample has
`"label"` and `"choice"`. Fixture tests on FNV's skeleton (`new_physics_fixture_tests.rs`): a
4.35 m/s hit is kept downed (knock state 0), drawn is simulated (0.000 degrees, 0.000 units) every
downed frame, the reference within 0.7 units of the pelvis, the shots withheld while it cannot aim
and not once it stands; it lands lying, sits up, rises and is standing by its balance 7.1 s after
the start (its second try; the fixture's frames run 24 ms), no bone more than 0.6 units under the
floor (the fingers and thumbs aside: the animation's, past the hand's capsule, a few units through
the floor when a palm presses it); dying 2.5 s in, its 18 bodies are placed on the pose drawn
(0.00 units off) and given its velocities, and New Physics lets go. The M4 fall test runs with
the downed keeping off (`set_keep_down`, tests only).

**Fix from Patrick's clip 2026-11-17_18-59-52_7825515** (`bNewPhysics=1`; 39 hits of 25.9 N s,
a 9mm at 0.32 m/s, on a Powder Ganger: each taken, fallen in 2 to 3 frames with no step,
"not handed over (on an AI thread): let go", drawn 44 to 75 degrees and 38 to 58 units off what
was written). Causes and fixes:
- **A spurious fall.** The animation's velocity was the pose at `HitMe` against the first frame's,
  over the wall clock's frame, then each frame's raw change over its time: at 250 frames a second
  a fraction of a unit of animation (and the root's unit between the hit and the first frame)
  was metres a second, and the capture point flew out. Now `Np` is made on the first frame's
  pose, still; the animation's velocity is its centre's move across the last 0.12 s of frames
  whose pose advanced (at least 0.06 s of them; `np::ANIM_V_WINDOW`), the actor's own moves taken
  out; the step is the game's frame time (`011dea30`, what its AI threads step the world by: 0 in
  a menu, slowed with the time), not the wall clock. The body is simulated about a local origin
  (the actor's first root, `Np::origin`): at (-72152, 8282) an f32's last digit is 0.06 mm, a
  substep's velocity from it 0.24 m/s. And the solver's substeps are a 960th of a second whatever
  the frame (`np_body::SUBSTEP`): cut 16 to a frame, the same hit recoiled 4.3 degrees at 250 frames
  a second and 7.9 at 30 (now 6.6 to 6.8).
- **The hand-over could not happen.** New Physics runs at the draw's set-up, while the game's AI
  threads use the task queue, where `KnockExplosion` is refused. A fall (and the downed AI's
  give-up, and a death's bodies) now waits for the main thread's update
  (`new_physics::main_thread`, from `crate::knockdown::frame`), the body simulated and drawn
  meanwhile; a fall is never let go: if the game refuses it, New Physics keeps it downed.
- **The read-back.** The 3D is now brought up to its locals before the animation is read and
  written against (a root moved since its worlds were updated was a unit off every bone). The
  game's 44 to 75 degrees are not reproduced in the fixture: the log now says, for a take's first
  3 frames, each body's read-back (`[np readback]`) and, at the world pass, each body as drawn
  against as written (`[np drawn]`: a bone the game's AI threads' ragdoll-controller pass wrote
  after us shows there), and for its first 6 frames the balance (`[np balance]`: the frame's
  time, the animation's speed, the body's against it, the capture point outside the feet, the
  phase); `[np fall]` says its reason (steps, unreachable, path).
Fixture tests on the game's path (`np_game_path_*`: the actor at (-72152, 8282, 223), claim and
hit in one frame, the root moved a unit before the first frame's draw, 0.3 units of jitter a
frame, `0x008c7aa0` true at the draw and false at the main thread's update, frame times 0.004 and
0.016 s), each failing on 7825515 but the switch's: 0.32 m/s stands throughout, no step, the spine
6.5 and 6.3 degrees, held, drawn as written (0.000 degrees, 0.008 units); 0.8 m/s one step, no
fall; 4.35 m/s (M4's hand-over) knocked down 1 frame after the hit on the main thread, the game's
bodies' centre 1.000 of New Physics', never let go; off, nothing taken.

**Fix from Patrick's clip 2026-11-17_21-16-42_d02bbd1** (`bNewPhysics=1`: the 9mm hits did
nothing visible, the rifle hits left him upright sliding about 270 units back; the read-back never
under the body's own angle off its animation, `[np drawn]` equal to `[np readback]`). Causes and
fixes:
- **The write was never drawn.** `NiAVObject::Update` (`00a59c60`) runs each node's
  `UpdateWorldData` (`00a68c60`), which hands a node with a collision object to it (its `+0x90`);
  FNV's 18 body nodes each have a `bhkBlendCollisionObject`, whose `UpdateWorldData` (`00c817d0`)
  does nothing while the thread block's word at `+0x2c0` is 3, the mode the no-controller update
  holds (`crate::active_ragdoll::update`). So the update left exactly the nodes New Physics writes
  as the animation had them (the old system met the same and sets the worlds directly:
  `hold_write_to_shown`). Now after the update each body tree's top has its world worked out
  from its parent's and its local, and every node under it from it (`refresh_bodies`, the old
  system's `set_subtree`): before the animation is read, after the write (when the update left a
  body off what was written: the count goes in the logs), and after the put-back. The fixture
  had hidden it twice: it doubled `00a59c60` with its own update, which always takes the locals,
  and its nodes had no collision objects. `rig_fixture::Fixture::game_update` now runs the
  game's own update, translated (`00a59c60`, `NiNode::UpdateDownwardPass`, `UpdateWorldData`,
  `00a68bf0`, the upward pass, the blend collision object's `UpdateWorldData`), on nodes laid out
  as `NiNode`s with a blend collision object on each body node; the old system's own fallback
  (`[target drawn] ... worlds set directly`) fires on it too.
- **The kick at the take.** The floor was the reference's height whatever the animation's feet:
  feet a few units under it were pushed out on the first frame (the centre went 0.32 to 1.04 m/s
  in the clip). The floor is now the animated feet's lowest point when that lies within 8 units
  of the reference's height.
- **The upright slide.** The actor was moved by the body's move even while the pose was not
  drawn. Now only while the frame's write is drawn (within 1 degree and 1 unit).
- **The logs.** `[np balance]`, `[np readback]` and `[np drawn]` count from each hit, not each
  take; a fall kept downed says `[np fall] <reason>: kept downed`; each hit ends in one
  `[np outcome]` line (`hit N s / m/s on <body> -> stood | stepped N | fell (reason) | downed
  (reason) | died, peak spine, drawn-vs-sim max, world set directly N bones`); the clips' phase
  says `downed` while New Physics keeps it down.
Fixture tests on the game's own update (`np_real_update_*`: no double for `00a59c60`, the blend
collision objects, downed kept, a non-bind pose with the feet 3 units under the reference's
height, dt 0.004 and 0.016, the AI threads busy at the draw), failing on d02bbd13 but the switch's:
the 9mm on the chest and on the left upper arm drawn 0.000 degrees and at most 0.001 units off
the body each frame, the centre against its animation at most 0.321 / 0.320 m/s (chest) and 0.340
/ 0.337 m/s (arm; the ground's reaction as the arm swings the body round its feet), no fall; the
rifle (4.83 m/s) drawn down (the pelvis under 30 units) 0.35 s after the hit, upright only its
first 0.19 / 0.18 s (63.6 / 53.7 units of the fall's own motion), drawn exactly as simulated;
off, nothing taken; the old system's fallback said. On d02bbd13 the same fixture drew the 9mm
11.5 degrees off with its centre at 0.393 m/s, and the rifle never down, 265 degrees off.

**Fix from Patrick's Play clip 2026-11-17_22-08-25_803260d_PLAY** (`bNewPhysics=1` from the
mods profile's `Fallout.ini`: 2 of 7 10mm hits at 0.52 m/s went "downed (unreachable)" in 2 to 4
frames; downed, he flickered between the animation and a headstand; the rifle at 4.29 m/s left him
tumbling about 2 m up). Causes and fixes:
- **The flicker.** The drawn pose hung on the bones' locals when the worlds were worked out
  again, and something (the AI threads' animation pass, most likely) set the animation's locals
  between the write and the draw: 446 of 1170 downed frames were drawn as the animation. Now at
  the world pass's first call, right before it culls and skins the actors, every body node's
  world is set from New Physics' body itself, parents first, and the nodes under each from it
  with their own locals (`new_physics::read_drawn`, `set_subtree`); it is checked straight after
  and again after the pass (`after_world_pass`). What wrote over the write is said for each
  hit's first frames and for up to 8 frames after them in which something did: `[np locals]` (the
  body locals changed since the write, how many are the animation's again, the worlds off the
  body before they were set) and `[np drawn]`. The put-back puts the game's locals back only over
  what was written: a local the game wrote since is its own and stays.
- **The instant falls and the launch.** The floor followed the animated feet each frame: a
  walk's feet took it off the body's (no foot down: the capture point counted 1 m outside, past
  "unreachable", a fall at 0.12 m/s), or up into them (`feet 40912 N`, the headstand and the
  launch). The floor is now taken once, at the take: the collision floor under the reference
  (the game's ray cast, else the reference's height), or the animated feet's lowest point when
  within 12 units of it, kept against the reference's height (`[np floor]`). A surface pushes out
  what already lay inside it as a substep began at 1 m/s at most (`np_body::MAX_OUT`); sinking in
  is pushed out whole. No foot down measures the support from the feet's soles as they are, never
  "unreachable" on its own (`Balance::supported`).
- **The outcome.** `[np outcome]` is said 2 s after the hit (or at the next hit, a let-go or the
  hand-over), not at the fall: the result is what it first came to (`fell`, `downed`, `died`),
  then drawn-vs-sim at the world pass, the frames drawn as the animation, the frames written
  over before the world pass (set back), and the most the floor carried (body weights).
Fixture tests on the game's own update (`np_walking_*`, downed kept): the animation walks at 0.15
m/s and turns at 10 degrees a second, each foot lifted 5 units in turn and the whole pose bobbing
2.5 units, the stance's feet 3 and 7 units under the reference; every other frame, between the
write and the world pass, a writer sets the animation's locals on every node and works the worlds
out from them; dt 0.016 and 0.033; the bodies measured where the world pass draws them. They fail
on 803260db (the 10mm drawn 31-33 degrees off the body and as the animation on every writer
frame; at the 7 unit stance it fell, the floor carrying up to 269 body weights; the rifle at the
7 unit stance launched, the pelvis 164 units up, down only after 0.82 s). Now: the 10mm on the
chest, the head and the right thigh drawn 0.000 degrees and 0.000 units off the body every frame,
never as the animation, the floor at most 2.68 body weights, no fall (one catching step); the
rifle down (the pelvis under 30 units) after 0.33 to 0.35 s, the pelvis at most 67 units over the
floor; off, nothing taken. Not run in the game: whether the AI threads write over the bodies
*during* the world pass shows only in the game's `[np drawn] ... written over during the world
pass` lines.

**Fix from the testers' b507e997 crashes (20:58, 23:01: "read of unmapped memory at 43f00030" in
the draw).** A 0.32 m/s hit on the chest "stood", then three catching steps with no new hit took
it out of steps into a fall, kept downed; the downed AI read its combat target from the actor's
`+0x42c` as a field (since 9ba82a3). `0x42c` is a vtable slot (`GetCombatTarget`, as the game's
own script function calls it); the field there held 480.0 (`0x43f00000`) and the target's position
`+0x30` was unmapped. Now the target is the vtable's answer, taken only if it is a mapped
reference the form table knows by its own form id (`combat_target`). The walk into the fall: once
standing again, the planted feet stayed where they had landed while the animation went on moving
slowly (under the balance's 0.3 m/s "moving"), so the capture point drifted out and the steps
came with no hit. Now, standing again, a planted foot the animation itself moves faster than 0.05
m/s is let go to go with it (`np_balance::ANIM_FOOT_MOVING`). `[np outcome]` waits, after its 2 s,
for the steps to settle (standing, no step under way), at most 6 s. Tests: the downed AI with
`+0x42c` holding `0x43f00000` and the vtable answering a target 300 units off reads that target
(on b507e997 it panicked with the testers' read at `43f00030`); a 0.32 m/s hit with the game's
frame seconds anywhere from 0.008 to 0.038, the writer of the animation's locals every frame, 4 s,
standing and walking: at most one catching step (b507e997: two while walking), no fall, the hips
lowered under 0.05 m, the floor under 3 body weights, the outcome said once. The testers' 10.1
body weights on the floor at 0.32 m/s did not show in the fixture (at most 2.7). Not run in the
game.

**Not done in M6 (for a later pass):** crawling to cover (the combat controller's cover point as
the body's travel goal: `FindCombatLocation` `0099ac20`); the shot's origin and direction from the
weapon as drawn (the game aims its NPC shots from the weapon node as animated, `fire_locate`, so a
downed actor's shot leaves from where its weapon would be standing; only the spread follows the
pose); a creature's downed body (New Physics takes human skeletons only). Not run in the game.

### bdf710a: the get-up hoisted upside down, the stagger's slide, the head's nod, the launch along the ground (Patrick, 2026-11-17, clip 1456)

On FNV's skeleton (`tests-data/fnv_skeleton.txt`) the get-up fixture now runs knock state 6 as the
game does (`knockdown_fixture_tests.rs`, `game_get_up`): knock state 1 runs on 4 frames after the
get-up is allowed, the animation arrives 2 frames into state 6, each body is driven onto its
animated joint (its velocity pulled toward what reaches it in 0.08 s) and the drawn nodes follow
the bodies (the animation's locals underneath), the actor nudged a unit a frame. It drives the real
`get_ups`, write, update, skin read-back and put-back each frame. On bdf710a it fails as the clip
did ("farthest bone 0 units off"; 9 to 14 bones "held to 400 units/s, rising to 50").
- **The blend's from-pose** is frozen when the get-up is allowed (ks1 noted it again after `place`),
  and its offsets wait for the animation: computed on the first frame the animated pose is more
  than 5 units off it (at most 0.75 s). Nothing is written meanwhile.
- **The blend is world to world**: from the ragdoll's last pose to what the game draws this frame
  (the nodes as the bodies left them), each bone's world rotation eased, the pelvis's and a spine
  root's joints likewise, and each other joint's distance from where its parent and length put it
  (the bodies' few units apart) faded in, so it ends exactly on the game's drawing. The nodes over
  and between the rig's bones (NonAccum, the root body's; `Bip01 Neck`) keep the worlds the game drew
  through the update. Measured: from 36.7 units, at most 253 units/s, the pelvis at most 2.0 over the
  animation's, a pop at its end of 0.48 units and 0.00 degrees beyond the animation.
- **The clamps** (ks3/5/6) no longer fight the game's drive: once the animation has the bones a body
  is held only where it moves away from its animated joint beyond the joint's own motion, or rises
  more than 100 units/s past it (`hold_toward`, never a push toward it); before, only a fling past
  400. None held in the fixture once the animation is in.
- **The actor stays on its spot** in ks3/5/6: moved off it by more than half a unit with no one to
  step out of, it is put back (`put back on its spot from ... across`).
- **The pose log** has the drawn get-up every frame an actor is down or getting up
  (`down_log.rs`): `"down":{"knock_state","pelvis_over_floor","head_over_floor","feet_over_floor",
  "torso_up","anim_pelvis_over_floor"}`, the clips sampling it each frame; at the end a
  `[target get-up frames]` note (the fixture: pelvis at most 1.2 over the animation's, 0 frames
  upside down, the lowest of pelvis, head and feet 7.1 over the floor, the pelvis's largest move
  beyond the animation outside the blend 0.1).
- **The stagger's slide** (`drawn_delta` 4.5): handed back, the body's share of the pelvis is held
  (`Settle`) while the steps' shift springs home at 6 Hz; it showed at once. The drawn pelvis offset
  is now eased (`active::ease_offset`: 3000 units/s², at most 1.2 units a frame's change, 80 units/s,
  braking onto it, no overshoot), the actor's follow of its feet passing through at once; the planted
  foot's weight is eased (smoothstep) as the steps end. A walking Ganger with a walk cycle and the
  game's stagger animation jumping its pelvis 8 units back: `drawn_delta` 1.22 (bdf710a 2.57), pop
  1.35, no foot under the floor, `drawn_length` 0.
- **The head's nod** is a tenth of the flinch (was 0.8): a head hit's head turns 1.1 times the
  flinch with the torso's bend it rides on (was 1.8; the target 0.8 to 1.2).
- **The launch** is held and said along the ground: the bodies' mean speed along the flat launch
  direction held to 1.05 times the hit's for 0.25 s (a falling body's drop had hidden 540 units/s
  for a 304 hit), and the 0.1 s line reads it along the ground with the fall apart: 307 units/s,
  1.01 times the hit's 4.35 m/s, while falling 358.

The fixture results are the fixture's (FNV's real node tree and bodies, the game's calls answered by
doubles, the game's physics and get-up animation modelled); not run in the game.

### a7a2d15: fly-up on the get-up, a walked actor's sink and pop, the launch (Patrick, 2026-11-17, 1330 and 1332 clips)

Tested on FNV's own skeleton: `crates/host/tests-data/fnv_skeleton.txt`, derived data of
`meshes\characters\_male\skeleton.nif` (node names, parents, local transforms, which node each of the
18 ragdoll bodies hangs from; no meshes, no game bytes). `rig_fixture.rs` lays it out in the engine's
memory exactly as the file has it and answers the game's calls; the tests run the real write
(`write9`/`write3` of the locals), `update`, the skin's read at the world pass (`read_drawn`),
`after_draw`, the knockdown's lying check, pelvis lookup, floor, get-up blend and its hold.

What FNV's skeleton has, and what assumed otherwise:
- No body on `Bip01 Pelvis`: the root body is on `Bip01 NonAccum` (the pelvis's parent, at its own
  place); `Bip01 Spine` is the pelvis's sibling under NonAccum. `pelvis_of` looked for a pelvis body:
  every get-up read "pelvis NaN" and was set on the lowest bone. Now `pelvis_pos`: the pelvis body,
  else the NonAccum body, else between the thighs, else the node by name.
- `fit_spot` took the navmesh point's height for the get-up's spot (several units under the
  collision floor): the actor was set 7 to 13 units into the ground, and the physics pushed the
  bodies out (the handstand). It keeps the floor's height now.
- The switch to the ragdoll moved each body onto the pose shown by its rig node; the NonAccum body
  has none and was left on the animation, its constraints torn. It moves with the pelvis now.
- The launch's own speed was added to each bone's (no pelvis body to take its motion from): a
  4.35 m/s hit held 470 units/s. The bodies are set to the launch, then held to the hit's speed
  for 0.25 s against the game's own push; the 0.1 s line reads the NonAccum body.
- The planted feet were not carried once the body was handed back (an AI walking it away left a foot
  31 units behind, the body dropped); the steps' lag counted the AI's walk (the pelvis 30 units
  back). Both fixed; the pelvis is held to 8 units while the AI moves it; a drop and the return to
  the animation fade instead of popping.
- The get-up's blend: its from-pose kept only in knock states 1 and 2 (it was the animation's
  already: "farthest bone 0"); the drawn body lifted so no joint goes under the floor; at most 400
  units a second along a bone's arc. Every body rises at most 50 units a second once landed,
  through states 1, 3, 5 and 6, the controller too.
- The flinch: on FNV's skeleton the write delivers it in full (6.3, 13.6, 25.2 degrees at 0.32,
  0.79, 2 m/s; 14 of twist on a clavicle hit; also armed and keeping its aim, and over a 30 degree
  jump of the game's hit animation). The tree and the write are not what lost it in the game. A
  write check holds each node's world after the update to the pose computed (re-solving a local
  from the actual parent node, else setting the subtree's worlds directly: tested with a mocked
  update that skips the spine) and says it (`[target drawn]`). The flinch starts a frame after the
  hit: the first frame drawn is at most a degree.

The pose log's proof lines (grep `[target`):
- `[target get-up]`: the pelvis (its source and z), the floor (which), lying or not, face up or
  down, where it was set.
- `[target get-up blend]`: the blend's farthest bone and the pelvis's distance, its length.
- `[target no-fling]`: each 0.1 s of the get-up, bones held to 400 units/s and rising to 50.
- `[target launch]`: the root body's velocity 0.1 s after the switch against the hit's.
- `[target drawn]`: a node the update left off the pose written.
- `[target pop]`: a frame's drawn turn more than a degree beyond the computed change.
- `[target reaction]`: each reaction's summary: drawn turn peak against the flinch (want 80% or
  more), the first frame (want a degree or less), `drawn_delta` and `drawn_length` most, the pelvis
  while its AI moved it (want 8 or less).

Not run in the game.

### a45deed: still no reactions, first-frame spikes, a slide, a fast launch, a handstand get-up (Patrick, 2026-11-17, 12-02-59 and 12-04-38 clips)

- **The flinch never reached the screen.** A host test on a mock engine
  (`active_ragdoll_mock_tests.rs`: a 20-bone biped node tree, the game's calls answered by doubles,
  `hit`, then `apply`, the nodes read back, `after_draw`, `unwrite` each frame) drew it in full over
  a still animation (6.5 degrees at 0.32 m/s, 14.3 at 0.79, 14 of twist on a clavicle hit). Over the
  game's own hit or stagger animation starting under the hit it drew 1.8: the physical body was
  made from the last frame's pose (`make_body(..., &before)`), so the animation's own change that
  frame became the body's motion, and `Body::carry` carried the animation's motion on top every
  frame after. The torso spun about 10 rad/s with its muscles slack from the hit, 140 degrees off
  its target in 8 frames; the kept quarter of it (`BODY_TORSO_SHARE`) cancelled the flinch, and its
  first frame was the 13 to 15 degree first sample. The body is now made still against its
  animation (the lab's too). Over the stagger: 14.5 degrees at 0.17 s, no spike. Tests:
  `the_drawn_flinch_reaches_the_spine_tops_node` (at least 5 and 12 degrees at 0.08 to 0.14 s, at
  most 1.2 times 6 and 14, under 2 by 0.6 s), `a_clavicle_hit_twists_the_drawn_torso` (at least 6),
  `the_drawn_flinch_shows_over_the_games_stagger`.
- **Pose log:** `flinch_env`, `flinch_deg` (the flinch's turn at the spine's top as computed),
  `computed_turn` (the pose as written; `drawn_turn` is what the drawing got).
- **Jumps:** the turn check is seeded from the animation (drawn 0, computed 0) on a reaction's first
  written frame. `no_drawn_frame_jumps_from_its_neighbours`: each frame's drawn turn moves at most
  1 degree beyond the computed pose's move, and the computed pose no faster than 1.25 times
  `SPIN_MOST`, the first frame included, also over the stagger.
- **The slide:** the actor follows its feet over `FEET_FOLLOW` (0.3 s, eased), a part each frame,
  the steps' pelvis shift taking each part back, instead of 8 to 11 units in one frame. The steps'
  own move counts in `drawn_delta` when the root shows it (a frame later), not when it is set.
  `drawn_delta` is now a slide: how much the drawn pelvis's or a foot's move this frame differs from
  its move the frame before (a 14 degree flinch moves the head about 2.5 units a frame and is the
  turn check's; a stepping leg's swing is left out). `catch_steps_follow_the_feet_without_a_slide`
  (1.1 m/s, a step and an 11 unit follow): `drawn_delta` at most 0.5, `drawn_length` 0, the pelvis
  at most 1.3 units a frame. The thigh's `drawn_length` 3.44 came in the follow's frame; not
  reproduced on the mock.
- **The anti-materiel launch:** the launch is the hit's own speed (`launch_v = min(stack, this
  hit)`), set as the bodies' velocity (`knock::launch_velocity`: the hit's speed along the launch
  and each bone's own motion about the pelvis), never added to the body's speed nor to
  `KnockExplosion`'s push (5.7 m/s for a 4.30 m/s hit, then 6.9 a frame later). New log line: the
  pelvis's velocity 0.1 s after the switch, against the hit's (at most 1.05 times).
- **The floor and the get-up:**
  - The get-up's floor is `knock::get_up_floor`: the collision floor whenever it is within 15 units
    of the navmesh (cast from the lowest bone and the pelvis), never under where it stood on the
    same floor (navmesh 8249, stood 8259: set 10 units in, lifted out sitting over 1.85 s).
  - The lying check uses that same floor. The allowed line says the pelvis over which floor, the
    spine-to-head tilt, lying or not ("held too long, not lying"), face up or down.
  - Through the whole get-up (knock states 3, 5 and 6), not only its first 0.3 s, no bone the physics
    drives goes faster than 400 units a second any way, and the controller does not slide.
  - The game takes the bones from the ragdoll at once in state 5 to 6 (`00c9bdf0` keyframes them to
    the get-up animation), the snap into a crouch. The drawn pose now blends from the ragdoll's last
    pose (kept each frame of states 1 to 5) into the animation over `get_up_blend_len` (at least
    0.3 s, longer for a far bone, so no bone is drawn faster than 400 units a second along its
    arc), written as the reactions are and put back after the draw.
  - The game picks its get-up animation itself (`GetIdleToPlay` in state 3); the blend's line says
    when its first frame faces the other way from the ragdoll ("the game chose a get-up of the
    other side").
  - Mock test `the_get_up_blends_from_the_ragdoll_without_a_snap` (a body on its back, the
    animation standing: starts on the ragdoll's pose, at most 400 units a second, ends on the
    animation).
  - Lab: `a_face_up_fold_gets_up_without_a_handstand` (let up lying, pelvis 8.8 over the floor; no
    bone over the pelvis by more than 42 units while rising; fastest bone 127 units/s; standing at
    2.90 s) and `a_seated_slump_gets_up_smoothly` (let up lying, 10.4; fastest 118 units/s;
    standing at 2.95 s).
- Not run in the game.

### 3a9805e: reactions too small to see, a flopping get-up (Patrick, 2026-11-17, 08-21-49 clip)

No pop and no yield (one re-base at 12.857), the reactions drawn (`drawn_offset` matching
`pelvis_offset`, `drawn_skin` [3,3,56]) but tiny: a .44 at 0.79 to 0.85 m/s drew the torso 4.2
degrees and the pelvis 4 to 5 units (mostly the catch step, away from the camera), a 9mm at 0.32
to 0.39 m/s 2.1 degrees and 0.7 units, with `offset_most` allowing 21 and 8.6. The lab's body
turns the torso 5 to 6 degrees for the same 9mm: the game's kick chain gives less, and why is not
measured here.

- **A drawn flinch** (`active::TorsoFlinch`, `draw_flinch`): each hit draws a turn of the torso
  on top of the physical body, the same in the game and the lab:
  - a bend along the push of 6 degrees at 0.3 m/s, 14 at 0.8, 25 at 2 (`torso_turn`), up the
    spine from 0.15 at the pelvis to all of it at its top, the neck and the head;
  - a twist about the vertical by the hit's lever (an arm's hit at its shoulder): the struck
    shoulder back 8 degrees at 0.3 m/s to 14 at 0.8 (`side_twist`; the body adds about one);
  - a head hit's nod, 0.8 of the bend on the head;
  - eased in over 0.1 to 0.12 s (as its size needs under the spin bound), back over 0.45 s; with
    the hand-back's weight; the arms ride on the chest turned their own way (turned with it an
    armed aim drifted 7 degrees and a regripping hand wobbled).
  - The body's own torso turn is drawn at 0.25 under it (`BODY_TORSO_SHARE`, eased by
    `torso_keep`), all of it while a brace holds; a brace clears the flinch it began with.
  - `offset_most` leaves room for it (the turn 1.25 times the bend and twist together, the yaw 1.25
    times the twist); `SPIN_MOST` is kept and holds every frame.
- **The first frame after a hit**: a new body's bounds started empty (`Limit::open`), so its
  first frame was never held; they now start from the offsets drawn the frame before
  (`Limit::seed`). The lab cannot make the game's stale base: the host now says once a reaction
  when the drawn turn changes more than a degree beyond the computed pose's in a frame.
- **`drawn_delta`** leaves out the steps' own move of the actor (it reported the reference
  following the feet as a pop); **`drawn_length_bone`** names the bone in the pose log, and the
  log says it once a reaction past 0.5 units. The cause of the clip's 3.35 at a foot landing is
  not found here (only the pelvis and a spine root have their translation written): the next
  clip's bone will say.
- **A brace behind the hips** is not begun or said (`brace::behind`), unless the push takes the
  body over it.
- **The launch** rises at most `LIFT` over the hit's own rise (none for a hit going down) and at
  most 0.45 after (`knock::launch_direction`): the anti-materiel hit at (-0.16, -0.93, 0.33)
  launched along (-0.11, -0.67, 0.73). (No ini key sets the lift: its 0.68 came on top of a
  rising direction.)
- **The get-up**: a still body lies only with its pelvis within 12 units of the collision floor and
  its spine-to-head line within 35 degrees of level (`knock::lying`; the 70-unit span let a
  kneeling slump up, pelvis 23 units over the floor). Still but not lying, it is nudged along its
  lean at 0.8 m/s (every 0.6 s, at most three times), else it waits up to the hold's most.
  Through the get-up's first 0.3 s (`GET_UP_BLEND`) no free bone goes faster than 400 units a
  second any way, and the controller does not slide along the ground. Nothing is written into
  the bones while knocked down.

Lab:

| Run | spine top at its peak (wanted) | 90% reached | back by |
|---|---|---|---|
| 9mm at the chest, 0.32 m/s, front and side, unarmed, pistol, rifle | 6.2 to 6.4 degrees (6.4) | 0.083 s | 0.42 to 0.43 s |
| .44 at the chest, 0.79 m/s, front and either side | 12.7 to 13.4 (13.8) | 0.083 s | 0.45 to 0.48 s |
| 9mm / .44 at a clavicle or upper arm | twist 8.3 to 8.5 / 14.3 to 14.6 degrees | | |
| 9mm / .44 at the head | head 12.2 / 24.0, torso 7.4 / 14.7 | | |

The fastest the spine's top turned in any of them: 3.5 rad/s. A body dropped kneeling (pelvis
lowered by the shins) toppled by itself and was let up lying, its pelvis 8.9 units over the floor
(10.6 at scale 1.2), the get-up's fastest bone 128 units a second.

- Tests: lab `scenario::visible` (the peaks, the rise, the way back and the spin, front and side
  at 0.32 and 0.79 m/s; the twist and the nod), `scenario::get_up` (a kneeling slump), the thin
  rail's behind runs begin no brace, `continuity` with hits 1.1 s apart; core `the_drawn_flinch`,
  `a_new_body_starts_from_what_was_drawn`, `the_launch_rises_little`, `only_a_body_on_the_ground_lies`.
- Not yet: an armed body's weapon hand swings up to 6.7 units a frame under four 9mm hits 1.1 s
  apart in the lab (3a9805e too); left out of the continuity runs. Not run in the game.

### 6b0cec2: a pop at the hit, an erratic railing brace, a creature drawn far off (Patrick, 2026-11-16, 21-16-09 clip)

Patrick's tester clip, the game log only (the pose log stayed on his laptop): a Powder Ganger hit
27 times, a Young Bighorner, four braces on a railing at his hips.

- **The pop at the hit.** 7 of 27 hits ended "its animation jumped under the reaction (Bip01 L
  Calf 117 to 155 tolerances in a frame): the reaction yields", always a frame or two after the
  hit: the game's hit animation starting. The jump was carried into the physical body
  (`Body::carry` moves each bone by its animation's motion), the bounds kept their offsets
  against the old animation, and the reaction yielded: the upper body popped up and sideways for
  1 to 3 frames, then no flinch.
  - A leg turning more than 0.15 rad in a frame (`active::leg_jumped`, whatever the frame rate:
    `pose::capped`'s bound is 0.35 rad at 60 fps) within 0.2 s of a hit, with nothing but legs
    past their frame's share, is re-based (`active::rebases`): no yield, the log names the
    actor's playing animation groups and idle.
  - Any jump, re-based or yielded to, is kept out of the body's carry, and the bounds' kept
    offsets move onto the new animation (`Limit::rebase`): the body keeps its world pose and its
    motors take it to the new one.
  - The feet's lift (0c1fbbf's `lift_feet`) solved a nearly straight leg against a fixed pole: a
    unit's lift swung the knee 6 units. It now keeps the knee's own bend plane, lifts only past
    1.5 units under the animated ankle (`LIFT_SLACK`), eased in and out at 40 units a second
    (`lift_amount`), and not while the steps place the legs.
- **The railing brace.** Four braces planted stably (hands within 3 units each time) on a railing
  72 units over the ankle, 0 to 7 units past the foot along the push: at his hips, behind him.
  The hands went behind the back, then the left arm was held straight through the railing for
  about 2 s (35 to 36 and 41 to 42 s) and snapped to idle.
  - Behind: no hand braces on a target behind the torso's coronal plane, nor on an obstacle
    within 20 units behind the pelvis (`brace::hand_may_brace`): the hand-less brace (the steps
    stop at it) leans the body on it.
  - The host read the brace's shoulder, elbow and wrist, and "the arm as shown a frame before"
    for the plant's IK, from the bone nodes, which since 00262dd hold the game's own pose between
    draws: the arm was solved from the animation's every frame, its plant pinned from the
    animated shoulder. Now from the pose last drawn (`active_ragdoll::drawn`,
    `Rig::shown_world`). The pelvis is held up before the arm is put on its plant.
  - A plant past 0.97 of the arm's reach lets go (`PUSH_OFF`; plants only to 0.95), and so does
    one the shoulder backs off 0.5 units from the closest it came (`PUSH_BACK`, a frame ahead):
    the body has taken the load (held on, the elbow swung with the torso).
  - A released hand is home within 0.6 s (`RETURN_MOST` 0.6, was 3: the 2 s straight arm), its
    hand-over 0.5 s; it turns onto its home's rotation at most 6 rad/s (`turn_toward`).
  - A new brace keeps a hand still planted (`brace::carried_hand`), never a new reach from it.
  - A released hand's home is carried by the chest, not the clavicle the arm itself moves (a loop:
    the home jumped 4 units a frame, the arm spun at 13 rad/s); the approach onto a plant takes
    0.12 to 0.2 s (was 0.04 to 0.16: a hand next to the wall swung onto it at 12 rad/s).
- **The creature.** A Young Bighorner matched 19 of the 20 biped names (the log's "1 of 19 bones"
  was the bones the hit touched), its body built as a biped's: "bone 17 is 2.67 m from the
  pelvis", a 0.719 m joint gap, drawn 1047.8 units off.
  - A rig only when at least 80% of the 20 bones map, each under the bone a biped's hangs from
    (`flinch::rig_maps`).
  - Any bone more than 30 units off its animation (a fall's and a bracing arm's own aside, the
    steps' move the root does not show yet allowed for) drops the body and writes nothing that
    frame (`active::too_far`).
  - Only the pelvis and a spine root have their translation written; no limb's (`moves_root`):
    a limb's length is the game's own.
- **`update(root)`** (after the write and after the put-back) ran no controller (time 0,
  `bUpdateControllers` false), but every collision object in the skeleton keyframed its body
  from the node, or set the node from its body, at whatever mode the game's
  `bhkNiCollisionObject::Synchronize` had left in the thread block's `+0x2c0`. It is now held at
  3 (the nodes' transforms only) through our updates and put back.
- **The pose log:** `drawn_delta` (the most a drawn bone moved this frame beyond its animation's
  own move: a pop shows there), `drawn_length` (the most a drawn bone's length is off its
  animation's), `rebased`.

Lab, 6b0cec2's handling against this:

| Run | before | after |
|---|---|---|
| A forced 150-tolerance L Calf jump two frames after a 9mm (carried, against re-based) | a foot 8.6 units and 17.2 degrees in a frame | 2.5 units, 5.7 degrees (the spin bound), no yield |
| Front braces, 108 runs (wall, railing, table, gap railing) | arm up to 13.6 rad/s, regrip up to 28 wobbles, hand jitter 0.029 m/s | arm 6.9 rad/s at most, 0 wobbles, 0.008 m/s |
| A 3-unit pipe at 80 units, in front (27 runs) | (no such test) | wrist within 0.00 units of its plant, no elbow swing, home in 0.42 to 0.52 s |
| The same pipe behind (27 runs) | hands behind the back | no hand braces |

- Tests:
  - lab `scenario::continuity`: a forced 150-tolerance L Calf jump on frame 2 (`legjump=2`, read
    back) is re-based, never yielded, and no drawn bone moves more than 3 units or turns more
    than 6 degrees in a frame beyond its animation's own (the animation without the forced jump;
    a catch step's swinging legs aside), nor is drawn more than 30 units off, nor changes length
    by more than 0.5; a hit and its yield too (a later jump yielded to as well);
  - lab `scenario::thin_rail` (`ground=pipe`): behind, no hand braces; in front, the wrist within a
    unit of its plant, no elbow swing at rest, home within 0.6 s;
  - lab: every built-in scenario keeps every bone's length within 0.5 of the bind pose and none
    drawn more than 30 units off;
  - core: `a_leg_jump_at_the_hit_is_re_based`, `the_lift_eases_in_and_out`,
    `hands_brace_in_front_only_and_keep_a_plant`, `only_a_biped_gets_a_rig`,
    `a_bone_drawn_far_off_is_found`;
  - host: `the_update_moves_the_nodes_only`, `no_limb_bone_is_moved`.
- Changed lab expectations, said: the brace scenes now face their obstacle (`face=180`, shot from
  behind into it: each pushed the body backward into an obstacle behind it, which no longer gets
  hands); the floor tests allow a foot 2.5 units under its animation (the lift's slack and ease;
  never under the floor); the plant's elbow is watched once the wrist is at rest on it; a brace
  hand's return is timed to its lock on its home. Not run in the game.

### 0c1fbbf: bodies into the floor, a heap, a plank get-up, a snap, too fast (Patrick, 2026-11-16, 19-43-08 clip)

The reactions showed in 0c1fbbf (`drawn_offset` non-zero on every hit), but the tuning was set
while they were invisible. Worst first, and what changed:

- **The sink.** The .44's catch step moved the Fiend's reference with the feet's middle height
  (`balance::Stepping::follow_feet`: 8157.3 down to 8154.3), and `knockdown::place` put the actor
  and its character controller there, into the floor; she sank upright to the waist. A foot
  planted from the navmesh (about 7 units under the collision floor) is enough to do it.
  - `follow_feet` keeps the reference's height: the steps move it across only; the controller
    keeps it on its floor (stairs and slopes are the game's).
  - No step is planted under the height its foot stood at; the ankle over the floor is measured
    by the collision cast (`active_ragdoll::floor_under`), never the navmesh.
  - The shown pose is held off the floor after it is composed:
    - the pelvis at most 4 units under its animation in a flinch, 8 while stepping
      (`active::hold_pelvis_up`);
    - each foot's target never under its animated place nor its collision floor plus its ankle
      (`active::feet_on_floor`);
    - a shown foot under that is lifted by the leg's IK from the hip (`active::lift_feet`).
  - `unwrite` brings the rigs' world transforms back to the game's own locals (`update(root)`),
    so nothing between the draw and the next animation update reads our pose.
  - The pose log's `heights` (root, `Bip01 NonAccum` and pelvis world height over the reference,
    as drawn) and `feet_over_floor` (each foot over the collision floor under it).
  - (a), the step's move counted twice: the host's `placed`/`lag` take-back is consistent with
    the root's world read the same frame; the lab test below checks the move is applied once.
- **The heap.** The game's ragdoll started from the pose its bodies were keyframed to, the
  animation's, while 0.14 s of going over had been drawn. At the switch
  (`knockdown::start_from_shown`) each ragdoll body is moved by its bone's turn and move from the
  animated pose to the one last drawn (`active_ragdoll::shown_world`), whatever its offset from
  the bone (`hkpRigidBody::setPositionAndRotation`, `00ca8960`); the launch goes on top. The
  ragdoll's bone clamp (`hold_bones`) leaves a bone under the collision floor to the physics.
- **The plank get-up.** The get-up watch held every free ragdoll body rising faster than 60
  units a second; the get-up drives its bodies (not only keyframes them), so the torso stayed
  down while the legs rose, then snapped upright. It now holds only a fling, faster than 400
  (`knockdown::BONE_FLING_RISE`); the controller's own rise is still held to 60. Nothing is
  written while the knock state is not 0 (`active_ragdoll::may_write`, tested).
- **The snap and the stiff bend.**
  - `Limit` pulled a bounded offset back at 3 rad/s even while the body's own offset was easing
    to nothing: at the yield it was left 42.6 degrees off, dropped in one frame. An offset
    shrinking toward its animation now follows as fast as it shrinks, up to the spin bound.
  - The body takes 0.6 of a hit's momentum (`active::KICK_SHARE`): at full, a 0.32 m/s 9mm bent a
    pistol-armed torso 30 degrees and the bounds held it 328 times in the lab (123 in the clip).
  - A two-handed weapon's support hand stays on its grip until the body's weight is down to 0.35,
    then eases off (`brace::grip_weight`; followed all the way, it came 5 mm off the grip).
- **Too fast.**
  - The per-hit turn (`active::offset_most`): 8 degrees at 0.3 m/s, 20 at 0.75, on a line
    between (4 to 75); the yaw never more than that.
  - The spin bound on the shown offsets (`active::SPIN_MOST`): 6 rad/s (was 20); the ragdoll's
    hand-over keeps 20 (`HANDOVER_SPIN`).
  - The onset (`active::HIT_TIME`): 0.12 s (was 0.04). 0.1 left a rifle brace's arm at 6.6 rad/s.

Lab, 0c1fbbf against this (the same runs; the floor numbers are the shown pose's):

| Run | torso turn (deg) | fastest arm (rad/s) | bounds held | foot under its animation | torso turn's most change in a frame (yield) |
|---|---|---|---|---|---|
| pistol, 9mm chest (0.32 m/s) | 30.2 → 5.0 | 10.7 → 1.5 | 328 → 0 | 2.4 → 0 | 2.8 → 1.1 |
| 10mm head, pistol (0.53) | 22.7 → 13.5 | 4.7 → 4.6 | 0 → 12 | 1.8 → 0 | 3.6 → 2.9 |
| .44 head (0.79) | 38.8 → 21.1 | 3.2 → 3.2 | 0 → 13 | 2.0 → 0 | 6.6 → 5.6 |
| .45-70 chest (1.40) | 37.4 → 22.3 | 3.6 → 2.9 | 0 → 18 | 1.9 → 0 | 7.7 → 5.7 |
| 10mm clavicle, then a walk and turn | 18.3 → 8.8 | 3.5 → 3.0 | 0 → 13 | 6.6 → 0 | 3.2 (3.1) → 2.7 (2.7) |
| .44 head, then a walk and turn | 38.8 → 21.1 | 3.2 → 3.2 | 0 → 15 | 4.7 → 0 | 6.6 (4.5) → 5.6 (3.2) |

The pelvis was at most 3.4 units under its animation (stepping, a .45-70), and no foot or pelvis
went under the lab's floor. The lab's floors and navmesh are the same surface, so the reference
never moved up or down in it before either: the game's mismatch is covered by
`balance::the_reference_keeps_its_height_when_a_foot_lands_low`. Not run in the game here.

- Tests:
  - lab `scenario::floor`: a flinch, a catch step (flat, slope, uneven) and a yield never put a
    foot or the pelvis under the floor, the pelvis never more than 4 or 8 under its animation, the
    reference's height never moved; a step's move is applied once; a yield's torso turn never
    changes more than 5 degrees in a frame;
  - lab `a_standing_flinch_is_held_to_its_push` (was `the_bounds_keep_a_standing_flinch`): the
    torso within the push's turn, no bone but a leg faster than the spin bound, the torso's turn
    never faster than it in a frame, still visible;
  - core: `the_reference_keeps_its_height_when_a_foot_lands_low`, `a_sunk_foot_is_lifted_onto_its_floor`;
  - host: `knockdown::the_ragdoll_starts_from_the_pose_shown` (a body with an offset from its
    bone ends where the shown bone puts it), `active_ragdoll::nothing_is_written_while_knocked_down`.
- Changed lab expectations, said: the plank test's least bends within 0.15 s (1, 1, 0.8 and 4
  degrees: less is in by then with the slower, smaller kick); the capture-point test counts a
  staggering hit's steps starting with the hit; a leg's swing is bounded at 12 rad/s (a step,
  or the feet lifted over a walk; 0c1fbbf swung them up to 11.5), not by the reaction's bound.

### 00262dd still drew no reactions; a spin in the air; a get-up shown lying (Patrick, 2026-11-16, 18-21-05 clip)

The 9mm and .44 hits showed only the plain idle, though the pose log had the body active in
2,027 of 2,125 samples and `drawn_offset` 5 to 49 on every hit. That number fooled us: it read
the bones' world transforms that our own update wrote at the first-person set-up, after the
world had been drawn.

The frame's draw sequence (`00870a00`, `00870bd0`) builds the frame (`00872f50`), then runs the
world pass (`00873200`, given the world accumulator), where the actors are culled, skinned and
drawn from the pose the bones then hold. The first-person set-up (`00874c10`) comes after.
- b44a964 left its write in the locals, so the next frame's world pass drew it one frame late.
- d0f4192 and 00262dd put the game's own locals back before that pass, so nothing ever showed.

- **Written at the start of the frame's drawing** (`active_ragdoll::around_draw`, from
  `fixes::install`'s joined `DRAW_FRAME`, `008706b0`). The write and the world update come first,
  before the smooth physics drawing's own write and before the world pass. The drawing follows,
  then the game's own locals are put back (`unwrite`), before the next animation update. The
  first-person set-up no longer writes them; `place_pose` stays there.
- **What is drawn is checked where it is drawn** (`active_ragdoll::world_pass`, at `00873200`'s
  first call of the frame). The actor's 3D is walked to its skinned geometries (`IsNode`,
  `IsGeometry`, the `NiSkinInstance` at `+0xbc`). The bones their skin instances list (`+0x14`,
  the count in the skin data's `+0x44`) are matched by name to the rig's pelvis, spine top and
  head, and their world transforms read against the animated pose:
  - `drawn_offset` and `drawn_turn` in the pose log, and `drawn_missing` (a reaction shown off its
    animation but drawn on it for 30 frames is logged) come from that;
  - `drawn_skin` (in the pose log) says how many of the three were found in the skin, how many of
    those were the rig's own nodes, and how many bones the skin lists.
- **`[nv-rs Gunplay] iReactionDraw`** (`NV_GUNPLAY_REACTION_DRAW`):
  - 0 (the default): the above;
  - 1: b44a964's way, written at the first-person set-up and left in the bones (drawn a frame
    late), the frame's drawing and the world pass not joined. It is the fallback, and the way to
    bisect: with 1, nothing of the join runs.
- **No spin in the air.**
  - Before the fall the body's forearm and hand turned 61 to 169 rad/s, and the ragdoll took their
    motion (`body_velocities`). It now takes it held (`active::handover_velocities`): the pelvis's
    speed at most the hit's (or the fall's), and each bone's own motion about the pelvis at most
    the spin bound (20 rad/s) times its distance from it.
  - `KnockExplosion`'s origin sat 290 units below the body and pushed it along (-0.03, -0.20,
    0.98), straight up. It is now 400 units behind the hit at the pelvis's height, its way rising
    at most 0.3 (`knockdown::explosion_origin`).
- **A get-up shown lying.** Knock states 1, 5 and 6 ran from 20.87 to 22.95 s; he stayed lying,
  then popped to idle at state 0 (23.37). The get-up watch held every ragdoll body rising faster
  than 60 units a second, 19 bones at state 5, while the get-up animation moves them. It now
  leaves a keyframed body (motion type 4: the animation's) alone and holds only free ones.
  - Not bisected in the game here. With `iReactionDraw=1` the join is off, for that bisect.
  - Whether the game keyframes the bodies through the whole get-up is not measured; if it
    doesn't, this does not change it.
- Tests:
  - `the_world_pass_draws_the_write_then_it_is_put_back` (the write, then `00873200` sees it,
    then put back);
  - `the_drawn_pose_is_read_from_the_skins_bones` (a scene in memory: a skin bone of its own, 6
    units and 0.2 rad off);
  - `the_explosion_pushes_along_the_ground`;
  - `active::the_handover_holds_a_flung_arm`.

  Not run in the game: no game here. The next clip's `drawn_offset` (now from the skin, at the
  world pass) and `drawn_skin` say whether the write reaches the screen.

### nv-rs's own perks (Patrick, 2026-11-17)

Five new perks, real perk records the player picks at level-up: nv-rs's own plugin,
`nv-rs Perks.esp`, made in code (`perks::plugin_bytes`; no file of it is kept in the repository),
written into `Data` and added to the player's `plugins.txt` at start-up, before the game reads
its plugin list (`perks::ensure_plugin`; the `[nv-rs Fixes]` row `bNewPerks`, on in the fixes
preset). The game loads five `PERK` records:
- each has its name, its description, a level in `DATA`, one rank, and is playable;
- each has its requirements as `GetPermanentActorValue` conditions, as the game's own perks
  have them.

The level-up menu offers them, and the Pip-Boy lists the ones taken. A save with one lists the
plugin as any mod's; nv-rs writes no save. Every effect needs the perk (`perks::has`: the player's
rank, as the level-up menu reads it, of the plugin's form). The numbers are `ragdoll_core::perks`.

| Perk | Needs | What it does |
|---|---|---|
| Stopping Power | Guns 45, level 12 | The player's hits push the active ragdoll (the flinch, the catch steps) 1.35 times as hard, and stagger (interrupt an attack) from 75% of the stagger share; the damage, the stack and the knock-down are the hit's own (`knockdown::after_hit`) |
| Hot Loads | Guns 50, level 14 | Every gun the player fires 10% faster at the muzzle, its own factor (`ballistics::hot_loads`): the flight (drop), the push and the penetration follow; not the damage. The K detailed stats show it: `x 1.10 Hot Loads +10%` |
| Bowling Ball | Strength 6, level 10 | A body the player knocked down, its pelvis flying at 1 m/s or more in its first 1.5 s, coming within 48 units of another living actor: that actor takes a push of 0.25 of the body's momentum, once, as a hit of no damage (`knockdown::bowling`, `bump`): steps, a stagger, a knock-down when fast |
| Light Touch+ | Sneak 50, Agility 6, level 8 | The pickpocket chance (`00643400`, which the roll and the pickpocket menu's line both read): the value's penalty halved, 10 added, the game's limits kept. The game's chance has no weight term: the value is all an item counts for |
| Scavenger's Instinct | Perception 6, level 6 | Looting (quick loot, and the container menu on a container or a corpse), an item that beats every equipped item it would replace is marked `^` and listed first. A weapon is scored by its force (its damage of all its projectiles, a melee weapon's with its weight; the equipped one with its weapon mods), apparel by DT, then DR; an empty slot is beaten by anything worth something |

Lab (`scenario::perk_pushes`):
- Stopping Power: a 9mm, a 10mm and a .44 at the chest push 25.9 → 35.0, 42.1 → 56.9 and 63.2 →
  85.3 N s. The 10mm (0.53 m/s against a stagger at 0.56) staggers and interrupts with the perk,
  and not without.
- Hot Loads: 9mm 25.9 → 28.5 N s, .308 90.8 → 99.8, 5.56 likewise 10% more.
- Bowling Ball, an 80 kg body (nothing without the perk):
  - 2 m/s: 40 N s → a push;
  - 4 m/s: 80 N s → a stagger with a catch step;
  - 8 m/s: 160 N s → a knock-down.

Unit tests:
- the numbers (`ragdoll_core::perks`);
- the plugin read back with the `esm` crate's reader (`perks::the_plugin_holds_the_five_perks`);
- `plugins.txt` (`the_plugin_is_listed_once`);
- Light Touch+ on a game in memory, the chance 20 without the perk, 40 with it
  (`light_touch_changes_the_chance_only_with_the_perk`);
- the quick loot rows (`a_better_row_is_marked_and_first`);
- the K stats' line.

Not run in the game: the condition function index (495, `GetPermanentActorValue`) and the actor
value numbers are the game's as known, not read from a running game here; the perks have no icon.

### d0f4192 drew no reactions at all (Patrick, 2026-11-16, the 15-16-34 Play and 15-18-15 tester clips)

The pose logs had the reactions as before, but the videos showed every hit actor in its plain
idle:
- the body was active in 678 of 708 and 1,709 of 1,815 samples;
- the offsets reached 9.3 and 5.7 units, and every flinch was logged.

A .44 knock-down still went into Havok's ragdoll and got up. Only our own pose write was lost.

**The cause.** d0f4192 put the game's locals back after the draw's first-person set-up
(`00874c10`). But the frame's draw sequence (`00870a00`, `00870bd0`) draws the world after it, in
`00874b50` and `00875110`, and what that drew was worked out from the locals as they then were:
the game's own, put back.

- **The write is put back after the whole frame's drawing** (`active_ragdoll::draw_then_put_back`).
  `crate::fixes::install` joins `DRAW_FRAME` (`008706b0`) when weighty combat is on: the frame
  drawn (blended by the smooth physics drawing when that is on, through
  `phys_blend::draw_frame`), then the drawn transforms read, then the game's locals put back,
  also when the drawing unwinds. That is still before the game's next animation update, so the
  next read is the game's own pose: the stale reads and the spin stay fixed. `008706b0` is now
  shared by the two rows (`fixes::rows_share_an_address_only_where_install_joins_them`,
  `overridable.tsv`).
- **What was drawn is checked** (`active_ragdoll::after_draw`). After the drawing, the pelvis's,
  the top spine bone's and the head's world transforms are read against their animated world:
  - the pose log has `drawn_offset` (game units) and `drawn_turn` (degrees);
  - "back in its animation" says the most of each over the reaction;
  - a reaction shown more than 1 unit off its animation but drawn on it for 30 frames logs
    `the reaction's write did not reach the drawing`.
- **The bounds never take a standing flinch's size away** (`scenario::runaway`,
  `the_bounds_keep_a_standing_flinch`, the lab's `Sim::limit_off`). 24 standing hits: 9mm to
  .45-70, at the chest, clavicle, head, side and pelvis; unarmed, pistol and rifle.
  - Each turns its bones as far with the bounds as without: 5.9 to 41.1 degrees.
  - The exception is an armed 9mm at the chest, where without them a bone flipped over to 125
    degrees; held, it is 30.
  - A bound letting go caught up at the spin bound (20 rad/s on a hit from the side); it now
    catches up at twice the pull (`Limit`'s `catching`): 8.4 rad/s.
- Tests: `active_ragdoll::tests::the_write_is_drawn_then_put_back` (the write is in the bones
  while the frame draws, put back after, also when the drawing unwinds).

The lab has no renderer and no game files are here, so the drawn offset in the game is not
measured yet: the next clip's `drawn_offset` and `drawn_turn` show it.

### A small flinch as the AI starts to walk: a spin and a snap (Patrick, 2026-11-16, b44a964's 14-16-02 Play clip)

A Powder Ganger in Goodsprings, a 10mm. Hit 1 (head, 0.53 m/s, one caught step) settled by 3.03
s. Hit 2 at 4.58 s was the same small flinch (left clavicle, 0.53 m/s, decision None). From
about 4.95 s its AI walked and turned it (41 units in 1.8 s), and the reaction went wild:
- the pelvis offset went from 0.1 to 48 units by 5.28 s, up to 14 units a frame, and the yaw
  rate reached 2 to 6.7 rad/s;
- on the video, the upper body folded and spun round the waist 3 to 4 times in under a second,
  the legs planted;
- at 5.31 s: "its animation jumped under the reaction (L Foot, 253 tolerances in a frame): eased
  in";
- from 6.01 s the hand-back held the pelvis frozen at 34.31 units while the yaw kept changing;
- at about 7 s it snapped from walking back to facing forward.

The head had been crippled by hit 1, which changes its animation set.

**The cause: the write left in the bones.** The host wrote the reaction over each bone's local,
and the write stayed there into the game's next frame. Each frame it read the bone back and had
to tell the animation's new pose from its own write (`pose::read_base`). A read near the write
was taken for "a later pass moved our write" and the remembered pose was kept.
- A walk moving a bone the way its offset went fell in that window. The animation was read stale
  until it was 12 units off (`DISTURBED_MOVE`; 253 tolerances × 0.05 = 12.65), then it jumped.
- The body chased a frozen animation while the actor walked and turned, and the stale pose
  showed until the reaction ended (the snap).
- A pass moving the write past that window was taken for the animation. The offset was put on
  top of itself every frame, and the body chased a target running away by its own lag: the spin.

The motors were not at fault: every error in the body is taken the short way (`qlog`).

- **The write is put back after the draw** (`active_ragdoll::unwrite`; d0f4192 did it after the
  draw's set-up, before the world was drawn, and drew no reactions: now after the whole frame's
  drawing, see "d0f4192 drew no reactions at all"). Every bone written gets back the game's
  own local as it was read. The draw shows the reaction: the world transforms the write updated
  stay until the game updates them. Nothing of it is left for the game's next frame: its
  animation and its later passes work on its own pose. The next read is the animation as it is,
  never stale and never our write. The reaction is an offset over the current animation every
  frame.
- **The body rides on the animation** (`active::Body::carry`). Each frame each physical bone is
  carried from where its animated frame was to where it is now (its AI's walk and turn, a gait,
  a new set), its motion turned with it, so the physics works only the offset. The steps' own
  moves of the actor are left out (`Rig::own_total`). The b44a964 origin shift went with it.
- **It yields to a moving animation** (`active::yields`, `YIELD_BACK` 0.35 s). Past 6 units of
  the AI's move or 0.25 rad of its turn since the last hit, or an animation jump (no longer eased
  in under it), the body is handed back over 0.35 s, a brace's hands let go and the steps end
  (logged `yields to its animation (...)`).
- **No catch steps while the AI walks it** (`active::ai_walking`: 15 units/s or 0.6 rad/s): its
  stride carries a small push. Steps planned from a stride and followed threw the lab's pelvis 7
  units sideways.
- **Held to the push** (`active::Limit`, `offset_most`).
  - Each bone's offset from its target is held to 0.3 + 0.7 v rad, its yaw to 0.06 + 0.1 v rad,
    and the pelvis's to 2 + 8 v units. For a 0.53 m/s flinch that is 0.67 rad, 6.5 degrees of yaw
    and 6.2 units.
  - No bone's offset changes faster than 20 rad/s (`SPIN_MOST`), kept on the animation's
    hemisphere.
  - A bound pulls an offset beyond it in at 3 rad/s and 40 units/s (`PULL_TURN`, `PULL_MOVE`),
    never cutting in one frame.
  - A fall's offsets are only followed.
- **Settling never freezes** (`Settle::hold_down`). Its ceilings come down with the body's weight
  each frame.
- **A hit during the hand-back starts a fresh body** from the pose shown, which is eased out over
  it. Its weight used to jump back to full over a body left behind: a hand turned 43 rad/s in one
  frame in the lab.
- **The pose log** adds `spin` and `spin_bone` (the fastest bone offset change, rad/s, and which
  bone), `yielded`, `limited` and `ai_speed`. `drift` stays 0 now (no read is ever our write).
  `jumps` counts the animation jumps it yielded to.
- **Play clips carry the real commit.** The Play package's CI sets `NV_RS_COMMIT`; a clip said
  `local` before.
- **Lab** (`scenario::locomotion`):
  - Its gait swings the legs, lifts the feet, bobs the pelvis and turns the spine into a turn.
  - Its `move` starts the AI's walk and turn that long after the last hit; `turnby` stops the
    turn.
  - `readback=1` reads the animation back from the bones as the host does. `readback=written`
    leaves the write in them, as b44a964 did. `aimpass=1` adds a later pass turning the upper
    spine while the AI moves it.

  48 runs: the 10mm hits (the clip's two, a clavicle, a chest, a head; pistol, unarmed, rifle) with
  the AI walking and turning 90 to 180 degrees after or as they land, each with and without the
  read-back and the pass. Worst run, b44a964 → now:

  | Measure | b44a964 | Now |
  |---|---|---|
  | Pelvis turn against the animation (rad/s) | 5.27 | 0.95 |
  | Runs turning faster than the animation turns | 9 | 0 |
  | Fastest bone offset change (rad/s) | 45.7 (a hand) | 11.5 (a stepping foot) |
  | Runs past 20 rad/s | 5 | 0 |
  | Stale reads (the most in a run) | 363 | 0 |
  | Pelvis offset (units) | 3.8 | 3.5 |
  | Pelvis move per frame (units) | 0.96 | 0.79 |
  | Back in the animation (s, slowest) | 4.8 | 3.0 |

  An earlier probe, where the AI stopped at hit 2 and started again 0.37 s after it, had b44a964's
  upper spine at 48 rad/s and the pelvis at 11.4 units (8.9 growth after release). That stop and
  start is not a spec key, so it is not among the tests.

  **The lab did not reach the clip's 48 units.** Its read-back is a model: the game's later
  passes and the cripple's new set are not known from here.

  The tests:
  - `a_small_hit_rides_on_the_walk` checks every run: pelvis at most 4 units, at most 2 units a
    frame, yaw rate under the animation's own turn rate, no bone past 20 rad/s, no stale read,
    over within 3.5 s;
  - `a_write_left_in_the_bones_reads_the_walk_stale`: hundreds of stale reads with the write left
    in the bones, none put back.

  The ragdoll-core tests cover the yield, the bounds, the settling and the carry. Not seen in the
  game yet.

### One hit, then a 5.5 s runaway, a spin and a snap (Patrick, 2026-11-16, c2a53c0's .44 fold clip)

A .44 into a Powder Ganger braced at a waist-high obstacle. The first reaction looked right (a
fold, the hands planted at 3.85 and 4.03 s). But after the hands let go ("recovered", 4.745 s),
with no further hit, the body kept moving for about 5.5 s:
- the pelvis offset went 3.5 → 23 → 10 → 32 units, then dropped from 18 to 1.3 in 0.25 s (a
  snap upright);
- the body tilted stiffly, an arm went behind the back, and it spun in circles.

The game's AI keeps turning and walking the actor while it reacts. The reaction stayed fixed in
the world:
- a planted hand held where it touched, so turning the actor twisted the body round it, and
  walking dragged the body behind it;
- the physical body trailed the moving actor on its pelvis spring, then snapped back to it as it
  was handed back.

The lab had hidden this: its mannequin always faced +Y, and the lab never turned or walked it.

- **The reaction goes with the actor** (`active_ragdoll::carry`). The game's own move of the actor
  each frame is carried by the physical body's origin, the feet where they stand and step
  (`balance::Stepping::carry`), and a brace's count of it. The game's own move is the reference's
  move less the steps' own (`Rig::own_moved`). A body going over is not carried.
- **The hands let go when the actor is turned or walked away** (`brace::let_go`):
  - turned 0.45 rad from where the brace began (`TURNED_AWAY`);
  - walked 10 units (`WALKED_AWAY`);
  - one hit's time is up (`TIME_UP`);
  - or recovered, as before.
- **One hit's reaction is capped** (`active::REACTION_MOST`, 2 s; `active::reaction_weight`). A
  reaction that is not going over starts handing its body back to the animation 2 s after its
  last hit, busy or not, eased out over `BLEND_BACK` (0.8 s). A hand let go then still goes home
  its own way: handing it over at once wobbled a rifle's support hand on its grip.
- **Handed back, it only settles** (`active::Settle`). While the body's weight goes down (no hit
  since, not going over), each bone's shown offset from its target and the pelvis's from its own
  may only shrink. A growing one is held where it was the frame before. A brace's arms are left to
  their own way home: held, they spun at 9 rad/s.
- **The pose log** (`active_ragdoll::sample_fields`) adds three fields to `body`:
  - `pelvis_jump`: the shown pelvis's move in the frame beyond the actor's own, in game units;
  - `yaw_rate`: its turn against the animation, in rad/s (`active::heading_off`);
  - `settling`: whether the hand-back holds it.
- **The double hit.** The two hits 12 ms apart (450 then 307 m/s, both "L Calf") were one round.
  The .44 went through the body (power 28, a body costs 15: sqrt(13 / 28) of 450 is 307). Its
  continuation, started 45 units past the impact (`BODY_DEPTH`), hit the same body again a frame
  later, so its momentum and damage counted twice. Now a continuation remembers the character it
  passed through (`Bullet::through`):
  - its impact on that character again does no damage (zeroed before the game processes it) and
    goes no further;
  - the knockdowns take no second hit from it (`ballistics::passed_through`; logged `the round
    that passed through it again: no second hit`).
- **The struck bone** is now the one whose segment the impact is on (`flinch::struck_bone`): from
  its joint to its children's; an end bone's on past its joint half its parent's length. Before it
  was the nearest joint, and "Calf" is the knee's joint: a hit on the thigh was named for the calf.
  The hurt limb (`condition_hit`) uses it too. Whether the game's impact point was really on the
  leg (Patrick aimed at the midsection) is not known from here.
- **Lab** (`scenario::runaway`; the lab's `turn` in degrees/s and `walk` in units/s). One .44 at the
  chest of an armed and an unarmed actor at a table, an armed one at a half wall and on flat
  ground; each standing, turned at 90 and -200 degrees/s, walked at 120 units/s, and turned and
  walked. Worst of the 20 runs, before → after:

  | Measure | Before | After |
  |---|---|---|
  | Pelvis offset growth after nothing holds it (units) | 30.2 | 1.5 |
  | Growth while handed back (units) | 0.47 | 0.013 |
  | Shown pelvis move beyond the actor's per frame while handed back (units) | 1.66 | 0.19 |
  | Turn against the animation (rad/s) | 5.14 | 1.15 |
  | Turn off the animation (rad) | 0.28 | 0.07 |
  | Back in its animation (s) | 6.9 | 2.8 |

  Walked, the pelvis had trailed the animation by 17 units for the whole reaction and dropped
  from 16.9 to 1.5 in 0.5 s at the end; now it is within 0.7. The tests:
  - `one_hit_then_nothing_only_settles`: no growth past 3 units once nothing holds it, none past
    0.05 while handed back, no move past 0.5 units a frame;
  - `one_hit_reaction_is_capped`: handed back by 2.85 s, everything home within a second more;
  - `heading_never_spins`: at most 2 rad/s and 0.2 rad off the animation.

  The lab's own facing was fixed with it: the brace, reach, legs and shoulders now use the
  animation's facing, not +Y. Not seen in the game yet.

### Arms through the body, a folded body snapping upright (Patrick, 2026-11-17, 454ebb2's 10:47 clip)

454ebb2 looked a lot better, but the arms flailed through the body, and a body folded over the
railing jittered and then snapped upright. (The tester's target never dies: that was a posed fall,
not Havok.)

- **Falls that do not happen.** `knockdown::knock_down` refuses (2.5 s after a knockdown, 1.5 s
  after a get-up, the game's `CanKnockDown`, a low process), but the fall pose was started without
  asking, and a refusal did not tell it. Now `knockdown::can_knock_now` is asked first: a big hit
  that cannot knock down now is a stagger (logged `no knockdown now (...): a stagger instead`), and
  `active_ragdoll::fall` asks it too. Every refusal calls `fall_failed`. A fall pose that does not
  go into the ragdoll (refused, or still standing `FALL_WAIT` after the switch was asked) eases its
  support and its pelvis offset back over 0.6 s (`FALL_BACK`; they went in one frame). While it
  goes over the support never drops under half (`FALL_SUPPORT`): the ragdoll takes it, and a posed
  body with none folded into what was behind it. A knocked-down or dead actor is never written:
  `apply` drops its rig before anything is written (knock state, life state), the fade included.
- **The shoulder's range** (`ragdoll_core::flinch::SHOULDER_RANGE`, `shoulder_range`): forward
  and out to the side up to 150 degrees, straight back 50, across the chest 30. The brace's
  targets, the reach IK (`reach_arm_ik`) and the windmill (`held_turn`) are held inside it; a brace
  hand is not sent behind the back (`brace::reaches`). The shown pose's unarmed arms are held to
  it too, allowed as far past it as the animation has them (`Body::hold_shoulders`).
- **Arms out of the torso.** An unarmed body's forearms and hands are pushed out of its pelvis and
  spine capsules in its step, never deeper than the pose it was made from had them
  (`Body::keep_limbs_out`; a planted brace hand is left to its pin), and the shown pose's arms are
  turned out at the shoulder where they go more than 0.35 units deeper than the animation has them
  (`Body::clear_arms`; not a planted hand, a grip, or an armed actor's arms: those are the weapon
  pose's and the aim's).
- **The head nods.** The neck's and head's motors go to 55% after a hit (`NECK_SOFT`; the chest
  10%, `TORSO_SOFT`; the waist and limbs 45%), a hit goes in over 40 ms (`HIT_TIME`), and an
  unarmed body's free arms (not bracing, not falling) turn at most 0.55 of the arms' bound
  (`UNARMED_ARM_SPIN`). The elbows' allowance past straight is 0.02 rad (was 0.03).
- **The elbow's little flex** off straight in the targets goes with the body's weight (it popped
  3.4 degrees in one frame as the body ended).
- **Lab tests** (`scenario::arms`; unarmed, FNV's tree): on flat ground, a railing, a table, a
  half-wall and a wall, a .44 at the left clavicle, the left upper arm from 40 degrees round, the
  head and the chest, at 30, 60 and 240 frames a second: no shoulder past its range, no elbow past
  straight by more than 0.03 rad, a free arm's bones at most 8 rad/s and 6 degrees a 60 Hz frame,
  no forearm or hand more than 1 cm deeper in the torso than the animation has it. A .44 at the
  head: the head 24 degrees against the chest (at most 25), the arms' ringing after 0.6 s 1.2
  degrees (at most 2), the chest 14.6 against the pelvis: **the 12 asked is not reached** (held to
  15). A big hit in the cooldown makes no fall pose; a fall pose whose knock-down is refused stands
  back up with no bone turning more than 6 degrees nor the pelvis moving more than 3 units in a
  frame (the old snap: 15.9 degrees). The lab's own refused fall had the same snap (its fall was
  taken before the refusal); fixed with it. The brace scenes' arm speed cap is 8 rad/s (was 6.6).
  Not seen in the game yet.

### The slow plank lean (Patrick, 2026-11-17, the 126915e clip)

The snapping was gone, but on most shots the NPC still leaned back slowly as one stiff plank, then
was dropped. In FNV's `skeleton.nif` `Bip01 Spine` hangs from `Bip01 NonAccum`, a sibling of
`Bip01 Pelvis` (the thighs are under the pelvis). The host's rig took its parents from the node
tree, so the spine reached no rig bone and the whole upper body (spine to head and the arms,
about 55% of the mass) was a second free root of the physical body: no joint to the pelvis, no
motor, no carry, no limits. A hit spun it as one plank (spine, chest and head turned alike, no head
snap) until a hand was 2.5 m from the pelvis (0.4 to 0.75 s), then the fade ran, and a body made
again with no hit drifted and was dropped again: about 2 s a reaction. The lab's mannequin put the
spine under the pelvis, so the lab never saw it. In his clip 8 of 9 flinches and all 6 staggers were
dropped (the first at a median 0.5 s); in cebde05's, 12 of 12 at 0.70 to 0.90 s.

- **Physics parents** (`ragdoll_core::active::physics_parents`): a rig bone that reaches no rig
  bone hangs from the pelvis in the body. The host keeps the node tree for the write
  (`Bone::parent`, `above`, `between`): the spine's animated pose comes from its own nodes, its
  offset is a root's (`S A^T`), and its translation is written at its shown joint (carried by the
  pelvis's move and turn), so the torso follows the pelvis. The body, its targets and the shown
  joints use `Bone::phys`.
- **A body only for a hit.** The end of a fade, or a rig left without one, never makes a body.
- **The torso bends.** The chest's, the neck's and the head's motors go to 10% right after a hit
  (`TORSO_SOFT`; the waist and the limbs keep 45%), and a hit's momentum goes in over 45 ms (was
  60): the struck region bends, the body tilts little.
- **The drop note** names the body's roots, any bone hung from another than its node parent, and
  the most joint gap.
- **The lab** runs on FNV's node tree (`mannequin::NODE_PARENT`; the body on its physics
  parents) through the host's write. Tests (`scenario::plank`): every bone but the pelvis has a
  physics parent and the joints stay within 1 cm; a 9mm at the chest, the head and the pelvis and
  a .44 at the head are never dropped in 3 s, keep within 1.1 m; the struck region bends within
  0.15 s (chest 3.0 degrees, 9mm head 25, .44 head 32; a pelvis hit's torso 2.6, short of the 3
  asked, held to 2.5); the spine bends more than 1.5 times the whole body's tilt; the tilt about
  the ankles stays under 4 degrees (3.5 at most) and is within 1 by 0.8 s; never a plank. When
  the body's capture point leaves its support, the catch steps have started (the steps are planned
  at the hit). The old tree (the spine a free root) in the same lab spreads to 2.46 m and is
  dropped. The write path runs through FNV's tree too, and fails if the spine is not carried
  by its shown joint. The brace scenes' arm speed cap is 6.6 rad/s (6.5: the chest's bend carries
  a bracing rifle's arms a little faster). Not seen in the game yet.

### The reactions leaned away, then snapped back (Patrick, 2026-11-16, 9:06 PM log)

In the game the NPCs slowly turned or leaned back, then snapped into place in one frame, nothing
like the lab. In his log 13 of about 35 flinches ended right after `steps done (Caught)` with the
body check failing (`bone N is 2.5x m from the pelvis`), and the drop wrote the animation pose in
one frame: the snap. What changed (`ragdoll_core::pose`, `ragdoll_core::active::Fade`,
`crates/host/src/active_ragdoll.rs`, mirrored in the lab):

- **No more ratchet.** A bone a later game pass moved past the tolerances, and the animation did
  not write again, was taken for a fresh animated pose with the offset already in it, so the
  offset went on top of itself every frame (the lab's repro drifted 0 to 168 units in 2 s, then
  snapped to 0 when the animation wrote). Now `pose::read_base` tells three reads apart: still
  holding the write (`Held`), the write moved by a later pass (`Disturbed`: nearer the write than
  the remembered pose, and at most `DISTURBED_MOST`, 20 tolerances, from it), and the animation's
  (`Animated`). A disturbed read keeps the remembered pose. The rotation is read whole, the
  translation axis by axis: the game's foot IK sets the pelvis's height outright and leaves the
  rest as written, and read whole that took the write's sideways offset into the base, so the
  pelvis offset stacked every frame. That is the likeliest reading of Patrick's first Windows
  tester clip (cebde05, 9mm, 21 hits): 16 drops `bone N is 2.5x m from the pelvis` about 0.7 s
  after even a 0.32 m/s flinch with no steps. A disturbance is at most 50 rotation tolerances
  (`DISTURBED_MOST`) or 12 units (`DISTURBED_MOVE`) off the write. While the body reacts, an
  animated local's jump is eased in over frames (`pose::capped`: 0.35 rad and 15 units a frame),
  said once.
- **Never a snap.** Any drop (the body's check, or the pose's sanity) eases the pose last shown
  back to the animation over 0.3 s (`active::Fade`: each bone's last world rotation over its
  animated one, and the pelvis's offset, eased out on top of whatever is shown after, so a new
  hit during it starts from there too). Only a fade that goes wrong with no body under it (the
  animation itself not believable) is put back at once.
- **The steps read the animation.** `balance_frame` takes the pelvis, hips, knees and ankles from
  the computed animated pose, as the lab does, not from the shown bones (which hold the last
  write: the steps chased their own output).
- **The steps' end and the reference's move.** When the steps are done their pelvis offset is
  eased out over 0.3 s instead of dropped to nothing. When a landed step moves the actor
  (`knockdown::place`), the root is brought up to date and the animated pose worked out again
  from it; a move the root does not show yet is carried by the pelvis (`Rig::placed`), so the
  pelvis's target no longer jumps back for a frame.
- **The body near its own origin.** The physical body runs in metres from where it was made (the
  animated pelvis then, `Rig::origin`), not from the world's origin a hundred thousand units away.
- **The log.** A drop says the pelvis offset (now and as last shown), the body's most spread, the
  base drift (reads a later pass moved, the most in tolerances) and the eased animation jumps.
  The Windows tester's clip sidecars sample the target every frame while it reacts, with `body`:
  the pelvis offset, the most spread, the drift, the jumps and the fade left
  (docs/WINDOWS-ROOM.md), so a divergence shows frame by frame.
- **Lab tests:** `one_write_path_for_every_actor` adds a later pass moving the written bones past
  the tolerance with two seconds of no animation write, and the same at the game's scale (the
  root 1.2e5 units out, walking and turning), with the foot IK setting the pelvis's height 4 units
  under the animation's; with the old rule it fails ("bone 2's animated world went off"), and with
  the translation read whole the pelvis drifts sideways from frame 31. `a_dropped_body_eases_back` drops the body mid-flinch: with the old drop a joint
  jumped 7.7 units in one frame, now no joint moves more than a fifth of that in a frame and the
  animation is shown after the fade. Core tests: `a_disturbed_write_is_no_new_animation`,
  `a_pass_setting_one_axis_takes_that_axis_alone`,
  `an_animation_along_the_offset_is_taken`, `an_animation_pop_is_capped`,
  `a_dropped_body_fades_back`. Not seen in the game yet.

### Crash fix: the world probes took a scene block for a reference (Patrick, 2026-11-16, 022ac96)

Patrick's Windows Play build (RTX 4090, Vulkan) stopped in the Test Range at the first bullet that
staggered a Powder Ganger: `open function 56625f6d: not translated yet (in 0089a760 < 00899cb0 <
009c1b70 ...)`, under `Actor::HitMe`. `56625f6d` is the bytes "m_bV": string data called as code.

- **Cause:** `jip::commands::world_ray_cast` (the active ragdoll's floor, ledge, wall and obstacle
  probes) answered the hit's scene block (an `NiAVObject`), and the host's `cast_world` made the
  `IsActor` virtual call (slot `0x100`) on it as if it were a reference. A node's vtable is shorter,
  so the slot read past it into other data. It came with the own controller (c87282a), where it ran
  only for a rare wall brace. Since the world interaction (c6a857bb) every push scans for
  obstacles, so the first staggering hit tripped it. Not the #604 merge, the regrip or the
  one-handed accuracy.
- **Fix:** `world_ray_cast` answers the hit block's reference (`block_parent_ref`, as JIP's own
  readers find it; 0 for the land). `cast_world` tells an actor with `jip::commands::is_actor`,
  which compares the vtable slot with the game's answer-true stub instead of calling it. The
  one-handed accuracy's Strength read now checks the shooter is an actor first.
- **Test:** `world_ray_cast_answers_the_hit_reference_not_its_block` (jip). The hit static's block
  answers its reference, an actor answers true, the block itself is no actor, the land answers
  none. It fails on the old code (checked by putting the block back).
- **The crash dialog** (`Play.exe`, `plain_end`) blamed d3dx9_38.dll whenever the log named it
  anywhere, and the log names it at every start. It now reads the cause from the stop line: an
  untranslated function or a stray call says so (a bug in nv-rs, not the PC or DirectX). D3DX is
  blamed only when the stop line names it.

### A two-handed weapon fired one-handed is far less accurate (Patrick, 2026-11-15)

While an actor's support hand is off its two-handed weapon's grip (bracing, or on its way back),
its shots go much wider. The penalty eases back to normal as the hand returns to the grip.
Heavier weapons are worse one-handed, and stronger actors steady them better
(`ragdoll_core::one_hand`, `crate::one_hand`):

- **The share:** how far off the grip the support hand is, from its wrist's distance to the grip
  in the shown pose. 0 within 1 unit, 1 from 12 units, eased between. The active ragdoll's frame
  sets it for each actor holding a weapon two-handed (`brace::grips`); it lapses after 0.25 s
  unsaid.
- **The spread:** the weapon's effective spread (the game's own spread getter `0x524be0`, with the
  spread mods: WMX and any other mod plugin) one-handed becomes `spread * (1 + k) + k * 1.8`
  degrees. `k` is heaviness (`1 + weight / 8 lb`, the effective weight with the weight mods) times
  unsteadiness (Strength 5: 1, each point 0.11 less, at least 0.45 for the strongest, at most
  1.5). It is held to 0.75..6, so even the strongest actor with the lightest two-hander is
  clearly worse one-handed. Strength is the actor value, which creatures have too.
- **Applied at the shot:** `Projectile::Create`'s yaw and pitch (`crate::ballistics`'s hook) are
  turned by a random amount within the extra cone, on top of the game's own spread. Each pellet is
  turned separately.
- **Examples** (the lab test): a 6 lb rifle (0.4 degrees) is 4.3 degrees one-handed at Strength 5,
  5.5 at Strength 2 and 2.1 at Strength 10. A 20 lb one (0.6 degrees) is 9.0, 11.8 and 4.4.
  After the regrip both are back to their own.
- **The player** never braces (the active ragdoll leaves the player to the game), so the penalty
  never applies to the player.
- **Tests:** `ragdoll_core::one_hand` (one-handed far worse, heavier worse, stronger better, the
  floor). The lab's `one_handed_fire_is_far_less_accurate_until_the_regrip` takes the share from
  the `rifle-wall-regrip` scene: fully off during the brace, only coming down after, back on the
  grip at the end, for a light and a heavy rifle and Strength 2, 5 and 10. Not run in game.

### Active ragdoll: the support hand regrips smoothly (Patrick, 2026-11-15, e2e5531 reel)

After a brace, a two-handed weapon's support hand wobbled as it went back to the gun. It went
back toward the physical free arm's wrist (in the lab still about 42 units from the grip). The
arm was then handed back far from the gun, and the soft, freshly hit arm swung onto it: about
5 cm past the grip, settling over about 2 s. Now (`ragdoll_core::brace`, the game and the lab):

- **Home is the grip.** A released hand goes back to its home. For an armed actor holding its
  weapon two-handed (`brace::grips`: the animation's support hand within 35 units of the weapon
  hand; this also covers two-handed pistol stances) the home is the grip, carried by the weapon
  hand (`brace::carried`). Otherwise it is the animation's hand, carried by the bone its arm
  hangs from.
- **A critically damped way back.** The path is kept as an offset from the home, so the weapon
  hand's motion carries it and only the offset shrinks: critically damped (2 Hz), at most
  1.5 m/s, at most 3.5 rad/s around the shoulder, never faster toward the grip than it can stop
  at it (no overshoot, no swing). Each frame the shown hand goes on the home as that frame has
  it (no lag behind the weapon hand), and the hand turns onto its grip orientation as it comes.
- **The arm's motors follow the same path,** so the physical arm comes back with the shown one.
- **Locks, then the grip hold.** Within 0.5 units of the grip it counts as locked. From then on
  the grip hold keeps the support hand on the grip (the shown arm and its motors) while the body
  is physical, fading with the body's weight as it blends back into the animation (where the hand
  is on the grip anyway). The brace hands the arm back only once the free arm's own wrist is on
  the grip (or after 3 s), so nothing moves at the hand-over.
- **Measured in the lab** (`watch_regrip`, the shown pose against the grip, for 3 s from the
  release): how far the hand goes past the grip along its way back, how often its distance to the
  grip grows again by more than 0.5 cm (a wobble), and how far it comes off the grip once within
  1 unit. Every scene and the 108-brace sweep fail on more than 0.5 cm past, any wobble, or more
  than 0.5 cm of drift. New scenes: `rifle-wall-regrip`, `rifle-railing-regrip` (expectations
  `regripped`, `regripover<=C`, `regripwobble<=N`). Before: about 5 cm past and 30 or more
  swings; now 0 cm, no wobble, no drift in all 35 regrips of the sweep.
- **Not run in game.** The 35-unit grip test reads the game's own animated hands; whether every
  two-handed weapon's stance falls within it is unchecked.

### Active ragdoll: hands plant on what they brace against; armed actors brace one-handed (Patrick, 2026-11-15, b7aaa5a reel)

Patrick saw the arms bounce around when bracing instead of meeting the wall, railing or table and
stopping. The cause was that the brace only gave the arm motors a reach target: the shown arm was
whatever the physical arm did, nothing held a hand at the surface, and the motors (made firmer
for the brace) fought the body, so the hand flailed (the lab measured up to 5.6 m/s, changing
direction every frame). Now (`ragdoll_core::brace`, the same code in the game and the lab):

- **The reach is a path.** Each bracing hand follows its own critically damped path from where
  it set off to the brace's target (3 Hz, at most 1.5 m/s, at most 4.5 rad/s around the
  shoulder, never further than the arm reaches, never past the surface). The path swings around
  the shoulder, so a reach behind goes down past the hip, not through the chest.
- **It plants on contact.** Where the path meets the surface (a wall's face, a waist-high
  obstacle's top or edge), it decelerates onto its rest (a hand's thickness off it) and stays
  there: no slide, no rebound. It plants only where the arm can hold it.
- **The shown arm is put on the hand.** The upper arm and forearm are solved onto the path or
  plant from the arm as it was shown the frame before (the elbow stays in its bending plane and
  flexes about its own hinge; the upper arm twists at most 2 rad/s), so the arm moves on from
  itself whatever the physical arm does. The hand bone moves with the forearm while reaching
  and keeps its rotation once planted.
- **The body takes the load through a contact.** The physical body's wrist is held at the plant
  as a one-sided contact (it pushes the wrist out of the surface and holds it from sliding while
  it presses, never pulls it in; only once the physical hand is within 10 cm of it). The arm's
  motors reach for the same point. As the body leans in, the elbow and shoulder bend: about
  8 degrees in the lab's wall brace, settling within about 1.5 degrees.
- **It lets go once.** A hand lets go when the body has recovered (its steps done, the brace
  held, the centre of mass slower than 0.25 m/s), pushes off (the shoulder further from the
  plant than the arm reaches), or after 3 s; also when the game turns or walks the actor away, or
  2 s after the hit (see "One hit, then a 5.5 s runaway"). It then goes back along a path to where the free
  arm's hand is, and the arm is handed back over 0.5 s. It never reaches again in the same
  brace.
- **The bracing arms' motors are no longer firmer** (`BRACE_FIRM` 0): firmer motors fought the
  path and the contact, and shook an armed body in the lab.
- **Armed actors brace one-handed** (`brace::bracing_arms`). Every Fallout weapon is held in the
  right hand. The left (free) hand braces: a pistol's off hand, a rifle's support hand letting
  go of the foregrip. The weapon arm keeps its aim (its motors track the aim's world rotation,
  firm). It joins only on a push past what one arm holds (60% of the brace's limit), when the
  body has no free arm, or when the free hand cannot reach the surface and the weapon hand can.
  It keeps the weapon in hand and is never tucked. Unarmed actors brace with both hands.
- **Measured in the lab** (`watch_hands`, on the shown pose): contact losses while a hand holds,
  a settled hand's speed and slide, and a planted elbow's swings back of more than 3 degrees.
  `the_built_in_scenarios_hold` fails any scene with a loss, jitter over 0.02 m/s, a slide over
  0.3 cm or more than two swings. `braces_hold_across_pushes_and_weapons` runs 108 braces (wall,
  railing, table, gapped railing; unarmed, pistol, rifle; .45-70, .44 and .308; from the front
  and 15 degrees either side). It also holds them to no backward bend, the 6.5 rad/s arm cap and
  30 degrees of aim drift. Worst there: 0.012 m/s, 0.02 cm, two swings, 6.49 rad/s, 18.8
  degrees. New scenes: `armed-pistol-wall-brace`, `armed-rifle-wall-brace`,
  `armed-pistol-railing-brace` and `armed-rifle-railing-brace` (`armed=pistol|rifle`,
  expectation `onehand`). The lab's reset now also clears the body's condition: earlier runs'
  hurt limbs had carried into later scenes.
- **Not yet:** the lab's table (74 cm) is below the standing body's reach, so its hands come
  within about 3 cm and go back without touching (a waist bend is not modelled). None of it has
  been run in game: the plant uses the game's own ray casts and bones, but its feel there is
  unchecked.

### Active ragdoll: the world around it, and what hits leave (Patrick, 2026-11-15)

All in `ragdoll_core`, so the lab and the game decide alike. The host answers through the game's
ray casts (past actors) and navmesh, the lab through Rapier (`obstacle::World`).
- **Obstacles** (`obstacle::scan`):
  - Level rays at seven heights from the shin to the shoulders, each at three points across the
    body (0.2 m apart), plus rays down from 2 m every 4 units ahead. So a railing or a table with
    gaps between its posts, rails or legs, and a top between two heights, are still found.
  - The nearest hit gives the distance, a ray down just past it gives the top.
  - By height: low (below the knee, 0.45 m: it trips a step), waist-high (a railing, a table's
    edge, a half-wall, up to 1.25 m) or a wall.
- **The catch steps** (`obstacle::resolve`, one copy now for the host and the lab):
  - Each step lands on its floor, or is stopped by a wall in its way (two rays at knee height),
    by a waist-high obstacle or wall the body meets along the push (a railing's gaps let a foot's
    ray through, not the body), or by a ledge.
  - A swinging foot that does not clear a low obstacle (a curb, debris, a low box) catches on it
    (`Outcome::Trip`). The body stumbles: with little push left (70% of it kept, under 0.9 m/s,
    less with a hurt leg) it steps over the obstacle, else it pitches over it into its ragdoll
    (`tripped over a low obstacle`).
- **Braces at the obstacle's height** (`brace_target(.., top)`): on a waist-high obstacle the
  hands go onto its top just past its edge, within the shoulder's reach (the elbow fix's IK).
  On a wall, as before.
- **Folding over** (`obstacle::fold_launch`): too big a push into a waist-high obstacle, or a
  knockdown with one within 0.8 m along the push, folds the body over it. The ragdoll is launched
  up enough that the hips clear its top, and pivots over it (`folded over a waist-high
  obstacle`).
- **What hits leave** (`condition::Condition`, ideas after blugart-dev's Kickback, no code of
  it), kept per actor across reactions (the host keeps the 64 most recently hit):
  - *Fatigue*: each hit tires the motors, up to half their strength, setting in over about a
    second and fading with an 8 s half-life.
  - *Pain*: each hit's push over the topple speed, times 1.5 and at most 0.6 a hit (a shot's
    pellets count as one hit), adds up with a 2.5 s half-life. At 1.2 a hit staggers at least,
    at 2.6 it knocks down (`condition::escalate`), whatever the hit alone would do. Two quick
    pistol rounds stay a flinch, about five stagger, fourteen in 1.4 s knock down.
  - *Injury*: a hit on an arm or a leg hurts that limb, healing with a 45 s half-life. A
    crippled limb (actor values 27 to 30, left/right attack and mobility condition, at 0) is
    fully hurt. A hurt arm's motors are weak (it dangles); a hurt leg's knee bends up to
    0.35 rad more and the pelvis drops over it, and its stumbles pitch over sooner.
  - *Counter-side bracing* (`Body::hit`): bones away from a hit stiffen and the hit side
    softens, by how far to either side they are.
- **Lab:** new grounds (railing, gap railing, half-wall, table, curb) and scenes `railing-brace`,
  `half-wall-fold`, `table-edge-brace`, `curb-trip`, `gap-railing` and `sustained-fire`, with
  the expectations `braced:waist`, `tripped` and `escalated`. All 26 scenarios pass, with the arm
  speed cap, no standing shoves and the elbow/knee limits.

### Active ragdoll: elbows and knees are one-way hinges (Patrick, 2026-11-15, c87282a reel)

Patrick saw the wall brace's arms bend backwards in the c87282a lab reel (.45-70, wall behind).
Two causes, both in the IK, and the body had no idea which way a joint bends:
- **The brace's elbow pole pointed down.** Reaching back toward a wall behind, an elbow below
  the shoulder-hand line *is* a backward bend. And the upper arm was turned by the smallest
  rotation, with no care for its twist, so the forearm bent about the wrong axis.
- **The catch steps' knee pole followed the push.** With a straight leg, a push backwards bent
  the knee backwards: with the hinges switched off, the lab's new measure saw knees up to 68°
  past straight during catch steps (magnum, leg shot, slopes, stairs, the wall brace's step).

Now, all in `ragdoll_core` (the lab and the game share it):
- **One-way hinges** (`active::Body`): the elbows flex 0 to 2.5 rad and the knees 0 to 2.4 rad,
  never more than 0.03 rad past straight, with a little lean out of the plane. Each hinge's axis
  comes from the plain animation's bend, or, for a straight limb, from the way the actor faces
  (`Targets::anim`, `Targets::forward`; the host reads the actor's 3D root). The limits act on:
  - what the motors track (`Body::hinged`): no target asks a joint past its range;
  - the body: near a stop the joint's motor is firmer, past it the spin that would take it
    further is taken out, shared by inertia, and it is turned back a small share each step;
  - what is shown (`Body::shown`): elbows and knees are blended against their parents, not in
    the world, which could take a joint between two good poses past straight.
- **Human reach** (`flinch::reach_arm_ik`, `human_reach`, `hinge_ik`):
  - A reach behind the body is turned down, keeping its length, to at most 45° back from
    straight down, and goes a little out to the side. The upper arm stays within about 55°
    back, so hands bracing on a wall behind go back, down and out, never up behind the shoulder.
  - The elbow points away from the upper arm's front, and the upper arm is twisted so that its
    hinge is the bend's.
  - The slower, lower brace and the arm speed cap are unchanged.
- **Knees forward** (`flinch::leg_ik`): the catch steps' knees point the way the actor faces,
  whichever way the push goes.
- **Lab:**
  - Every built-in scenario now fails if any elbow or knee shows bent backwards (`back_bend`,
    measured from the mannequin's own anatomy, not from the body's hinges), standing, in the
    ragdoll or getting up. The most now is 2.7° past the allowance, at the ledge.
  - The lab ragdoll's elbows and knees are Rapier hinge joints (its three-axis limits let a
    sledgehammer's spin fold an elbow past its stop and round), held to their stops the same way.
  - The get-up blends each bone against its parent.
  - The arm speed, smoothness and all other expectations still hold.
- **Margin (after 5aff676c's lab CI failed on the ledge at 3.2°, against 2.65° on another
  machine).** The motors' targets keep elbows and knees 0.06 rad off straight (`TARGET_FLEX`;
  people seldom lock them), so a limb flung straight is not driven onto its stop. A leg's foot pin
  is held within the leg's straight reach from its actual hip (`within_reach`), so stepping down a
  stair cannot pull the knee past straight. The lab ragdoll's stops are at straight, and a joint
  past one is turned back onto it before and after each step. The most any scenario shows now is
  1.2° past the allowance (stairs-down), against the test's 3°.

### Active ragdoll: our own controller, one write for every actor (Patrick, 2026-11-15)

- **No more writes through the game's ragdoll-controller pass.** The flinch, catch steps, brace
  and windmill used to be written in two places: once at the draw's set-up, and again inside the
  game's `bhkRagdollController` pass (`00888970`, wrapped by nv-rs) for actors that took it. That
  pass is where the stretching Powder Ganger crashed, and the gate was wrong too: the log said
  the pass was taken by "an armed or attacking actor", but any actor with foot IK, grab IK or
  feedback took it, armed or not. Patrick's Powder Ganger FF001322 was unarmed. The wrapper is
  gone (its `overridable.tsv` row too). Now every actor, unarmed, armed or attacking, is written
  the same way, once a frame after the game has animated it: absolute locals over the animated
  pose (`ragdoll_core::pose::local_for`). The game's pass still runs before that write, untouched,
  and our write comes after it. Armed actors keep their aim as before. The flinch log line now
  ends `posed over its animation by nv-rs's own write (every actor alike, no ragdoll-controller
  pass)`.
- **World awareness by the game's own ray casts** (`jip::commands::world_ray_cast`, JIP's
  `TES::Pick`; hits on actors, the reacting actor's own limbs included, are passed and the cast
  goes on):
  - The floor under each foot and each step's target is a cast straight down. The navmesh is the
    fallback.
  - A wall between a foot and its step's target is two casts at knee height (the second 10 units
    higher must hit about as far off, so a ramp or a stair the step goes up is not a wall).
  - The ledge test is unchanged in kind (the floor under the target too far down), now on the
    cast floor.
  - The brace casts from the shoulders along the push (90 units) when it starts. The hands stop
    a hand's thickness (5 units) short of the wall's face instead of reaching through it
    (`brace_target(shoulder, dir, wall)`).
- **Havok's ragdoll only for the fall**, as before: the hand-over starts Havok's ragdoll from the
  body's bone velocities (`knockdown::give_launch`), and corpses, saves, looting and dragging stay
  on Havok. The every-frame guards from the crash fix stay: the body is checked to be finite and
  bounded, rotations orthonormal, no scale, offsets clamped, and it drops back to the animation
  on anything wrong.
- **Lab:** the same model. Its brace casts for the wall with Rapier and the step check casts at
  knee height. The write-path test now runs unarmed, armed and attacking mannequins, with no
  later rewrite (the new model) and with three rewrites a frame after the write, a hair off; all
  hold the shown pose exactly. All 20 scenarios pass.

### Active ragdoll: arm speed and the wall brace (f36ddda lab review, 2026-11-15)

- **Standing arm speed is limited too** (it was only in the ragdoll: a .50 reached 13.7 rad/s,
  a sledgehammer 9.3, the brace 8.5). Each arm joint's turn against its parent is held to its
  role's speed times 1.1 (about 4.4 to 6.6 rad/s), smoothly: below 60% of the bound nothing
  changes, above it the speed saturates (`tanh`), on the velocities before and after each small
  step and on what the step's constraints moved (`ragdoll_core::active` `limit_spin`,
  `limit_turns`; the legs only on their velocities, as they follow the steps' pins). In the lab:
  every scenario now asserts `armspin<=6.5`; the most is 6.1 (.50 5.9, sledge 5.3, brace 5.7).
- **The wall brace** is slower, lower and deliberate: the hands reach 0.45 m ahead of the
  shoulders and 0.28 m below them (chest high; was 0.55 m ahead, 8 units below) over 0.7 s with
  a quintic ease in and out (release 0.6 s); the bracing arms' motors twice as firm and tracking
  half their world rotation (they no longer ride up with a torso pitched toward the wall).
  Measured in the lab against f36ddda's brace: the hand stays at least 15 units below the
  shoulder (it rose 21 above it), drifts 1.5 units a frame while holding (was 5.7) and its arm
  turns at most 5.7 rad/s (was 8.1); it swings back behind the shoulder a little more as the
  push takes the body (8.7 units at most, was 4.4).

### Active ragdoll: a physical body (Patrick, 2026-11-15: toward Euphoria)

The reactions were far off Euphoria's: the spring offsets per bone did not look smooth or fluid.
Now, for a second or two after a hit, the struck actor's rig is a physical articulated body
(`crates/ragdoll-core/src/active.rs`, the same code in the game and the lab):

- **The body.** Each Bip01 bone a capsule (its mass a share of the body's), its joint held to its
  parent by a positional constraint; small steps (8 a frame, XPBD); every velocity taken from the
  moves, so nothing jumps.
- **The motors.** Every joint a PD motor (a soft angular constraint plus implicit damping) tracking
  the animation's pose with the steps' leg IK and the brace's arms as targets: frequency each
  role's spring times `DRIVE_SCALE` 2.0, damping ratio 1.0, strength 0.45 right after a hit eased
  back to full over 0.7 s, weakened near the hit (the muscle's dip); each joint carries the weight
  of what hangs below it (gravity compensation), its turn against its parent held to the role's
  speed (x1.3) and its bend from its target to the role's room (x1.5, at most 1.2 rad).
- **The hit.** Its momentum (as the knockdowns measure it, times Hit Reactions) shared among the
  bones by mass, fading with distance over the flinch's spread, going in over 60 ms (eased: no jump
  in velocity or acceleration) and reaching each bone after its distance at 5 m/s: the struck part
  moves first, the push travels up and down the chain.
- **Balance.** The pelvis held by a support (1.4 Hz, critically damped, carrying the body's weight)
  that follows the capture-point steps; the feet pinned (4 Hz) where they stand or along the steps'
  arcs (now eased in lift too), eased back to the animation's feet over 0.4 s after the steps. No
  scripted arms-out or lean: going over is the support fading over the fall's lead; the legs keep
  the steps' last targets while it goes over. An armed body's arms track their world aim and are
  stronger; the torso takes the push.
- **Back to the animation.** 1.4 s after the last hit (or the last step, brace or fall) the body
  blends back to the animation over 0.8 s with `6t^5 - 15t^4 + 10t^3` (no jump in speed or
  acceleration). A fall into the ragdoll hands the body's bone velocities to Havok's bones (the
  launch on top).
- **The lab** measures smoothness while the body stands: the biggest change of a bone's velocity
  in one frame and the biggest jerk. A pistol hit: 0.22 m/s and about 840 m/s3; a .44 with a catch
  step: 1.35 m/s and about 6,100 m/s3. Its scenarios check them (`vjump<=`, `jerk<=`). Every
  value is a slider ("Active ragdoll").
- **Tuning found in the lab:** each motor's gain uses the inertia of everything it moves (a spine
  tuned for its own bone let the upper body sway ±38° for two seconds); the spine at 2.2 Hz, the
  neck and the head firmer (3.4 and 3.0 Hz) so a light hit nods them rather than whips them; a
  pistol hit now turns no bone more than about 11 degrees, and the body settles in about 0.6 s
  before its blend back.

### Knockdowns ragdoll again (779cd5b regression, Patrick, 2026-11-15)

In 779cd5b no knockdown ragdolled: the decision was made, but `KnockExplosion` left knock state 0
and no bone got the launch, then `duplicate guard: the game's KnockExplosion skipped`. Stage 3's
fall made the switch from the pose layer, at the draw's set-up. There the game's "task queue in
use" test (`008c7aa0`, which depends on how far the AI threads are in their frame; the new pass
hook on the AI thread made them later) is true, so `KnockExplosion` only queued the request and
returned. Its queued replay then met the knockdowns' own duplicate guard, and the failed
knockdown was still registered as a get-up, which blocked the actor's reactions
(`no flinch: getting up from a knockdown`).

- The fall into the ragdoll is now made at the main thread's next update
  (`knockdown::frame`, `knock_down_next_update`); the body stays going over until the knock state
  changes, at most 1 s.
- `knock_down` itself never calls `KnockExplosion` while the task queue is in use: it waits for
  the next update (at most 0.6 s, `knock_down_later`).
- After the switch the knock state is logged (`knock state after the switch: <n>`). At 0 nothing
  of a knockdown is kept (no cooldown, no get-up), and a ragdoll actor is tried once more the hard
  way; still 0, it is left standing and its fall pose lets go.
- No `StopAttack` before the knock-down (the ragdoll ends the attack); the stagger and the catch
  steps still interrupt attacks.

### Active ragdoll: human arms, armed and attacking actors (Patrick, 2026-11-15)

- **The arms** (they snapped and flailed in the lab and the game): softer, well-damped arm springs
  (clavicle 2.0 Hz / 0.6, upper arm 1.3 / 0.65, forearm 1.5 / 0.7, hand 1.7 / 0.75; kick shares
  0.6 / 0.75 / 0.65 / 0.55); every joint's turn held to a person's speed (rad/s: arms 4-6, spine
  7, head 9), before the kick moves the bone; the poses the arms are given (arms out while
  stepping, now 0.6 rad; the wall brace, reach 0.22 s, release 0.5 s; the ledge windmill, now
  1.1 turns a second, 1.1 rad) followed with a lag and a little follow-through (a 2.6 Hz, 0.7
  damping spring, at most 4.5 rad/s) instead of jumps. In the ragdoll the arm bones spin at most
  8 rad/s, while it flies and while it lies (only an arm over that is touched, so it still
  sleeps), and the ground brace steers them 0.12 a frame (was 0.25).
- **Armed and attacking actors got no reaction** because the game's ragdoll controller pass
  (`bhkRagdollController::DoRagdollAnim`, `00c7d630`, from `00888970`, run per actor by the AI
  thread's process-list step `0096cb50`) reads each bone's local pose and writes it back through
  the grab IK (the hands on the weapon), the foot IK and the ragdoll feedback; it runs for an
  actor with any of them on, which armed actors have, and its write came over the flinch before
  the draw. Over `00888970` the layer now writes its offsets first
  (`active_ragdoll::before_ragdoll_anim`); the pass reads them, keeps the hands on the weapon
  through them and writes them back; the draw set-up only steps the springs for those actors.
  The process-level gate is now middle-high, as the knockdowns' (it was high only).
- **Aim and attacks:** an armed actor (`Actor::IsWeaponDrawn`, `008a16d0`) keeps its aim through
  a flinch below the stagger (its arms' kick cut and the torso's turn under them taken back by
  0.8, `ragdoll_core::flinch::hold_aim`); a stagger, a knockdown, a catch step or a fall
  interrupts an attack (`Actor::StopAttack`, `008a8e50`, acting on actions 2 and 4).
- **Log:** every skipped reaction says why (`no flinch: <reason>`, `no catch steps: <reason>`,
  `no fall pose: <reason>; straight into the ragdoll`: no process, process level, knock state,
  life state, getting up, no 3D, no rig bones, Hit Reactions 0, no momentum); `attack interrupted
  (<why>; action <n>)`; `its bones go through the game's ragdoll controller pass`; the flinch line
  says `armed: keeps its aim` or `armed: the hit takes its aim`; the clamp line says `lying: arms
  only`.

### The shared crate and the ragdoll lab (`crates/ragdoll-core`, `tools/ragdoll-lab`)

Patrick, 2026-11-15: a light web app to try the active ragdoll quickly, on the game's own code so
tuning carries over.
- **`crates/ragdoll-core`** holds the pure parts the host used to keep in its modules (moved, not
  changed): the bone roles, springs, spread and stage 3 timings (`active_ragdoll`), the whole of
  `balance`, the bodies, momentum transfer, topple speed, stack fading and launch (`knockdown`,
  and `hit_force`'s swing factors), the calibres and loads (`ballistics`). The host modules
  re-export it (`pub use ragdoll_core::...::*`), so every path and value is as before. Two changes
  of shape, not of behaviour: a stepping keeps its start on a clock the caller gives
  (`active_ragdoll::clock`, a monotonic clock, where it read `Instant` itself), and the functions
  read their constants through `ragdoll_core::tuning::get`, which is the constants unless
  `tuning::set` is called (only the lab calls it).
- **`tools/ragdoll-lab`** (its README): Rust to WebAssembly, Rapier for the ragdoll, a 2D canvas.
  A capsule mannequin of the rig's 20 bones, the game's controller glue mirrored on it (`sim.rs`,
  function by function after `active_ragdoll` and `knockdown`), guns by calibre and load, melee by
  weight and reach, blasts by damage and distance, grounds (flat, slope, uneven, stairs, wall,
  ledge), every tuning value a slider, the values as ini text (`[nv-rs Gunplay]` for the game's
  settings, `[nv-rs Ragdoll Lab]` for the compiled constants), and URL-driven scenarios that
  report upright or fell, steps, distance and recovery time as JSON for an automated browser.
  Built and published by `.github/workflows/ragdoll-lab-pages.yml`; not part of the game or its
  package.

### Ballistic bullets (`crates/host/src/ballistics.rs`)

Patrick, 2026-11-14: Battlefield-style bullets. `bBallistics` (Settings > Gameplay "Ballistics":
Classic / Projectile), `bPenetration` ("Penetration"), `iBulletSpeed` ("Bullet Speed", 25-200 %).

- **Flight, the game's own.** A bullet's projectile form (`BGSProjectile`, `DATA` at `+0x60`) is
  hitscan with bit 1 of its flags. In Projectile mode every missile-type hitscan form a weapon of a
  known calibre uses (its `+0x118` and its ammo's override `+0xB4`) has that bit cleared, its speed
  set to the calibre's (0.9 of the muzzle velocity, the drag the game doesn't model, x 70 units/m x
  `iBulletSpeed` %, at most 120,000), its gravity to the calibre's drop factor and its range to at
  least the calibre's. The game then flies it (travel time, drop, its model as a tracer) and the
  combat AI leads it: `GetProjectedPointForTarget` (`009a8df0`) iterates on the target's velocity
  with the form's speed and gravity, where a hitscan form gets the target's point. Originals are
  kept; Classic (or the row off) puts them back. Converted when the main menu is made and when the
  Ballistics, Bullet Speed or Weighty Combat row changes. Energy, flame, launcher and thrown ammo is
  never matched.
- **Per shot** (over `CreateProjectile`, `009bca60`, every shot of `TESObjectWEAP::Fire`, players'
  and NPCs'): the loaded ammo (`GetCurrentAmmo`, `00525980`) gives the load by its name, else by its
  effects (DT x more than 1.5: hollow point; DT subtracted: armour piercing); the load's velocity
  and the weapon mods' (below) go into the projectile's speed multiplier (`+0xD0`).
- **Momentum at impact:** mass x speed (the calibre's muzzle velocity x the load's and mods'
  factors x the share of speed left after passes; `iBulletSpeed` changes the flight, not the hit)
  x the load's transfer, 6 damage points per N s (a 5.56 mm round: 24). The hit force and the
  knockdowns take it (`Shot::Bullet`); the damage threshold counts by the load's factor; pellets as
  for a gun (`n^0.75`).
- **Penetration** (over `Projectile::ProcessImpacts`, `009c1b70`): after the game processed a
  bullet's first impact, a body costs 15 power, a thin object its material's usual thickness x cost
  per cm (glass 0.6 cm x 3, wood 4 x 1.5, heavy wood 10 x 1.5, sheet metal 0.15 x 40, hollow metal
  and car bodies 0.4 x 40, chain fences 0.3 x 3, cloth 0.5 x 1, bottles and cans 0.3 x 1.5); stone,
  concrete, earth, heavy metal and water stop it. The thickness is the material's usual one, not
  measured: the host has no ray cast into the game's world it can call yet. A round goes on when at
  least a fifth of its power is left; it keeps sqrt(left / power) of its speed and that squared of
  its damage. The continuation is a new projectile of the same form, shooter and weapon, started
  6 units past a thin object or 45 past a body at the next frame, with the original's rotation; at
  most 2 passes a bullet and 24 continuations a second. Hollow-point and explosive rounds never
  over-penetrate bodies; pellets never penetrate (slugs do).
- **Cheap:** one table lookup a shot (shotguns and automatics included); per frame only the queued
  continuations.

| Calibre (ammo name has) | m/s | g | Drop | Penetration | Range m | Momentum pts |
| --- | --- | --- | --- | --- | --- | --- |
| .22LR (`.22`) | 330 | 2.6 | 1.9 | 8 | 120 | 5.1 |
| .32 | 280 | 5 | 1.8 | 10 | 100 | 8.4 |
| 9mm | 360 | 8 | 1.6 | 18 | 150 | 17.3 |
| 10mm | 400 | 11.7 | 1.5 | 22 | 200 | 28.1 |
| .45 Auto (`.45`) | 260 | 14.9 | 1.7 | 18 | 150 | 23.2 |
| .357 Magnum | 440 | 10.2 | 1.4 | 25 | 250 | 26.9 |
| .44 Magnum | 450 | 15.6 | 1.4 | 28 | 250 | 42.1 |
| 12.7mm | 450 | 20 | 1.3 | 30 | 300 | 54 |
| 5mm | 900 | 2.6 | 1.1 | 30 | 600 | 14 |
| 4.73mm caseless | 930 | 3.3 | 1.0 | 35 | 700 | 18.4 |
| 5.56mm | 990 | 4 | 1.0 | 40 | 800 | 23.8 |
| .308 / 7.62 | 850 | 11.3 | 1.0 | 60 | 1200 | 57.6 |
| .45-70 Gov't | 480 | 26 | 1.4 | 45 | 400 | 74.9 |
| .50 MG | 900 | 42 | 0.9 | 120 | 1800 | 226.8 (push clamped at 160) |
| 2mm EC | 1800 | 2 | 0.5 | 150 | 2000 | 21.6 |
| 12 gauge (per pellet) | 400 | 3.5 | 2.0 | 6 | 80 | 8.4 |
| 20 gauge (per pellet) | 400 | 2.6 | 2.0 | 5 | 70 | 6.2 |
| Railway spike | 300 | 150 | 1.5 | 40 | 150 | 270 |
| BB | 100 | 0.35 | 2.5 | 1 | 40 | 0.2 |
| Harpoon / spear | 120 | 300 | 2.0 | 20 | 60 | 216 |
| Unknown round (modded: `round`, `cartridge`, `bullet`) | 600 | 8 | 1.2 | 25 | 400 | 28.8 |

| Load (name has; else effects) | Velocity | Mass | Penetration | Transfer | DT counts |
| --- | --- | --- | --- | --- | --- |
| Standard | 1 | 1 | 1 | 1 | 1 |
| Hollow point (`HP`, `hollow point`; DT x > 1.5) | 0.97 | 1 | 0.25 | 1.4 | 1.6 |
| Armour piercing (`AP`, `armor piercing`; DT subtracted) | 1.03 | 0.95 | 2.5 | 0.7 | 0.4 |
| JSP / soft point | 1 | 1 | 0.8 | 1.2 | 1.1 |
| Surplus | 0.9 | 1 | 0.9 | 0.9 | 1.1 |
| Hand load | 1.1 | 1 | 1.1 | 1.05 | 1 |
| Match | 1.08 | 1 | 1.05 | 1 | 1 |
| Incendiary / Dragon's breath | 1 | 0.95 | 0.8 | 1 | 1 |
| Explosive | 0.95 | 1.05 | 0.3 | 1.3 | 1.2 |
| Slug (shotgun) | 1.1 | 8 | 4 | 1 | 0.9 |
| Magnum (shotgun shell) | 1.1 | 1.2 | 1.1 | 1.1 | 1 |

### Per-gun muzzle velocity (`crates/host/src/gun_velocity.rs`, `data/gun_velocity.tsv`)

Patrick, 2026-11-14: each gun's bullets leave at its calibre's velocity times its own multiplier
(1.0 by default), times the load, the barrel and suppressor mods and the game's projectile-speed
multiplier. Momentum, flight (so drop) and penetration follow it: the penetration power goes with
the square of the speed factor (load x mods x gun).
- The table, `crates/host/data/gun_velocity.tsv`: the vanilla, DLC and Gun Runners' Arsenal guns
  by editor ID (long or heavy barrels, pistol rounds from rifles and unique variants higher, snub,
  sawed-off and integrally suppressed guns lower; 0.85 to 1.20). The game keeps no editor IDs in
  memory: the official masters' `WEAP` records are read on a host thread when the main menu is
  made (each plugin's own records, form IDs in the load order). At the first shot the log names
  the entries that matched no weapon and the official calibre guns without an entry
  (`Per-gun velocity: ...`).
- **Editor IDs from the masters (2026-11-15).** The first table was written without the game's
  files, and Patrick's log (game-20261115-095501) found that only 22 of 61 entries matched. It is
  now made from every `WEAP` of the official masters (FalloutNV.esm, the DLC esms,
  GunRunnersArsenal.esm and the packs, read on the maintainer's side; the fixture is
  `crates/host/data/official_weapons.tsv`: plugin, editor ID, animation type, ammo name):
  - the 39 wrong IDs were remapped to the guns they meant, keeping each gun's multiplier;
  - the .32 revolver, the assault rifle and the railway rifle are not New Vegas weapons and were
    dropped, and so was the .32 varmint rifle (that receiver is a weapon mod);
  - the guns never covered were added (the Lever-Action Shotgun, K9000, FIDO, the Shoulder Mounted
    Machine Gun, the Sturdy Caravan Shotgun, the GRA custom variants), and so were the NPC,
    turret and test variants.

  Every official calibre gun now has an entry (94 records). The tests check that every entry is
  an official `WEAP` and that every official calibre gun has one, using the game's own calibre
  test (`ragdoll_core::rounds::caliber_of`, a gun's animation type 3 to 9). Not in the table: the
  Nail Gun (nails) and the Red Glare (rockets), which fire no calibre round, so a row would never
  be read.
- A gun without an entry (modded guns, or a wrong editor ID): its damage against its calibre's
  average loaded gun, `(damage / average)^0.25`, within 0.9 to 1.1.

### Detailed stats (`crates/host/src/item_details.rs`)

While a gun or an ammo of a known calibre is highlighted in the Pip-Boy's inventory, looting or
barter, the item card (with the equipment stats row on, `bEquipmentStats`) says
"K) Detailed stats", and K opens a panel (the game's message box): for a gun its calibre and
load, the muzzle velocity with every factor (calibre, this gun's multiplier and where it came
from, load, each mod), the bullet's mass and momentum and what it pushes into a body, the
projectiles a shot, drop and range, penetration, the card's DAM and DPS, fire rate (shots/s),
spread, AP cost, critical damage and chance, condition, clip, reload (by its animation: the
weapon's data has no time) and the attached mods; for an ammo its calibre, load, velocity, mass,
penetration, transfer and DT factors and the momentum it gives. The key is `[nv-rs Gunplay]
iDetailsKey` (37 = K), on Settings > Controls ("Detailed Stats Key": K, J, L, H, G, U, I, O, P, N,
B, Z, C, V, Insert, End), saved. DAM and DPS both stay visible (the equipment stats row never
cycles them).

### Weapon mods (`crates/host/src/weapon_mods.rs`)

Patrick, 2026-11-14 (Weapon Mods Expanded through Play with mods): everything above takes the
weapon's effective stats with its attached mods, read generically from the game's own data: the
item's attached slots (`ItemChange::GetModSlots`, `004bd820`, bits 1/2/4), each slot's action type
(`+0x180`), values (`+0x18C`, `+0x1AC`) and mod item (`+0x350`) on the weapon form. No plugin or mod
is named. The action types' meanings are those of the game's own readers:

| Action | Game's reader | Used for |
| --- | --- | --- |
| 1 damage + | the damage formula | hit force, knockdowns, kick |
| 3 spread - | `00524b80`, `00524be0` | steadiness: sway and kick down by up to 40 % (a cut of 1 degree or the weapon's minimum spread is all of it) |
| 4 weight - | `GetFormWeight` | melee swing force, sway, punch |
| 9 projectile speed x | `009bca60` (the player's shots only) | bullets: anyone's (the player's already scaled by the game) |
| 11 silence | the sounds | bullets: muzzle velocity x 0.96 |
| 12 split beam: + projectiles | `GetNumProjectiles`, `Fire` | pellets for hit force and penetration |
| 14 scope | `Fire`'s aiming | sway in sights 0.6, kick 0.6 (iron sights 0.7), punch 0.3 (0.5) |

A mod item whose name has `barrel` makes the round faster (x 1.06) or, with `short`, `snub`,
`sawed`, `sawn`, `compact` or `cut down`, slower (x 0.95). The weapon's own scope flag (`cFlags`
bit 4) counts as a scope. The Test Range's inventory already has every named IMOD of every loaded
plugin. The game's own item card (`00707e30`) shows the attached mods' stats itself; nv-rs adds no
compare of its own. The dismember roll (`00646e50`) gets no attacker, so it keeps the form's stats.

### Tuning

`[nv-rs Gunplay]` in Fallout.ini is read in a launcher's play run. The `NV_GUNPLAY_*` variables
win over it. A file value that can't be used falls back to the default and is named in the log
line; a bad variable stops the start.

| Key | Variable | Default | Range |
| --- | --- | --- | --- |
| `iRecoil` | `NV_GUNPLAY_RECOIL` | 100 | 0-300 |
| `iRecoilRecovery` | `NV_GUNPLAY_RECOVERY` | 70 | 0-100 |
| `iSway` | `NV_GUNPLAY_SWAY` | 100 | 0-300 |
| `bHitStop` | `NV_GUNPLAY_HIT_STOP` | 1 | 0/1 |
| `iHitStopMs` | `NV_GUNPLAY_HIT_STOP_MS` | 90 | 0-250 |
| `iMeleeReach` | `NV_GUNPLAY_MELEE_REACH` | 115 | 50-200 |
| `iImpulse` | `NV_GUNPLAY_IMPULSE` | 150 | 0-500 |
| `iHitForce` | `NV_GUNPLAY_HIT_FORCE` | 100 | 0-300 (0 = the game's own push; Settings > Gameplay "Hit Force") |
| `bKnockdowns` | `NV_GUNPLAY_KNOCKDOWNS` | 1 | 0/1 (Settings > Gameplay "Knockdowns") |
| `iKnockdownThreshold` | `NV_GUNPLAY_KNOCKDOWN_THRESHOLD` | 100 | 25-400 (Settings > Gameplay "Knockdown Threshold") |
| `bKnockdownCompanions` | `NV_GUNPLAY_KNOCKDOWN_COMPANIONS` | 0 | 0/1 |
| `iDismemberForce` | `NV_GUNPLAY_DISMEMBER_FORCE` | 45 | 0-1000 (0 = only the game's chance) |
| `iExplodeForce` | `NV_GUNPLAY_EXPLODE_FORCE` | 110 | 0-1000 (0 = only the game's chance) |
| `bBallistics` | `NV_GUNPLAY_BALLISTICS` | 1 | 0/1 (Settings > Gameplay "Ballistics": Classic / Projectile) |
| `bPenetration` | `NV_GUNPLAY_PENETRATION` | 1 | 0/1 (Settings > Gameplay "Penetration") |
| `iBulletSpeed` | `NV_GUNPLAY_BULLET_SPEED` | 100 | 25-200 (Settings > Gameplay "Bullet Speed", in 5s) |
| `iImpactStacking` | `NV_GUNPLAY_IMPACT_STACKING` | 300 | 100-1000 ms half-life (Settings > Gameplay "Impact Stacking", in 50s) |
| `iDetailsKey` | `NV_GUNPLAY_DETAILS_KEY` | 37 | a DirectInput key code (Settings > Controls "Detailed Stats Key") |
| `iHitReactions` | `NV_GUNPLAY_HIT_REACTIONS` | 100 | 0-200 (0 off; Settings > Gameplay "Hit Reactions", in 10s) |
| `iBalance` | `NV_GUNPLAY_BALANCE` | 100 | 0-200 (0 off; Settings > Gameplay "Balance", in 10s) |
| `bNewPhysics` | `NV_GUNPLAY_NEW_PHYSICS` | 0 | 0/1 (Settings > Gameplay "New Physics"; being built) |

### In-game Settings menu

The rows are on Settings > Display, after Field of View and the graphics rows. Each applies at
once and is saved in a launcher's play run:

| Row | Kind | Saves |
| --- | --- | --- |
| Weighty Combat | Off / On | `[nv-rs Fixes] bWeightyCombat` |
| Recoil | 0-200% in 10s | `iRecoil` |
| Weapon Sway | 0-200% in 10s | `iSway` |
| Impact Punch | Off / On | `bHitStop` |
| Dismember Force | Off, 5-150 in 5s | `iDismemberForce` |

How the Off / On row behaves:
- Turned Off, the hooks pass straight through to the original.
- Turned On again in the same run, they act again.
- Saved Off, the next start installs nothing, and the row is gone until `bWeightyCombat=1`.

The rows' change functions are at `7e060060` to `7e0600a0`. The Gameplay page's New Physics row
(Off / On, `bNewPhysics`) changes at `7e060220`. `CreateMenuElements` (`007cc6e0`) is
shared with the Field of View and graphics rows: `fixes::install` registers one function that makes
the rows of each that is on.

### Not in slice 1 (later)

- **Impulse on a non-lethal hit.** Living actors are keyframed. A stagger would need the game's
  knockdown and stagger path (`GetKnockbackSpeed` `00646580`, the hit's knockback), or a short
  ragdoll blend. This is the next physics step.
- **Severing a living limb.** The game severs only on a killing hit. Severing a living limb would
  need the crippled-limb path and new handling of a living actor's missing part. It is not
  planned without the game's support.
- **Melee swing feel beyond the impact punch:** a camera nudge on the swing and a reach shown in
  the crosshair.

### Needs an in-game check (Patrick)

Patrick played slice 1 (2026-11-13): recoil works. The sway showed nothing: it was written into the
first-person root's local translation in the look handling, and `Main::DrawWorld_MTSetup1stPerson`
(`00874c10`) zeroes that translation before it updates the root each frame. The sway now goes in
there ("Where the sway goes in" above). The hit-stop is replaced by the impact punch. Checked here:
unit tests and the build; the new sway and punch are not run in the game yet. To check in game:
- the kick's direction and size for a pistol, a rifle, an automatic and a shotgun;
- that the recovery does not fight the mouse;
- the sway: the weapon lags behind fast turns and settles back, and the iron sights stay usable;
  if the weapon moves the wrong way, the pose's signs need turning round;
- the impact punch on melee hits and heavy shots (a shotgun, a magnum), and that game time never
  slows;
- melee reach;
- kill pushes;
- severed limbs at the force thresholds;
- the five Settings rows (labels, saving, Defaults).

## Slice 2: the Test Range

Patrick's choices (2026-11-13):
- a main-menu button straight into the range, no save needed;
- a quiet, existing, well-lit space with room for targets at about 5 to 100 m;
- humanoid and creature targets at several distances, passive by default, with a toggle to make
  them hostile and a way to respawn them;
- every weapon of every loaded plugin and DLC, with ammo, and a quick way to fill ammo;
- the gunplay settings easy to reach;
- a clean exit to the main menu, and the player's real saves never touched.

Code: `crates/host/src/test_range.rs`, part of the weighty combat row (on with
`bWeightyCombat`).

### How to use it

1. On the main menu choose **Test Range** (just above Quit).
2. The game starts fresh in the range's cell (chosen from your FalloutNV.esm, "The place"
   below), with a default character: no character creation and no save.
3. About half a second into play the range is set up:
   - a container behind you with one of every weapon;
   - 500 rounds of every ammo type in your inventory;
   - you turned down the room's longest clear line, and a lane of targets along it at 5, 10, 20, 35, 50, 75 and 100 m as far as it is clear ("The place" below), humanoids and
     creatures alternating, a step to each side.

   A note says "press Home for the range menu".
4. **Home** opens the range menu (the game's own message box):

   | Button | What it does |
   | --- | --- |
   | Weapons | Opens the weapons container. |
   | Fill Ammo | Tops every ammo type back up to 500. |
   | Make Targets Hostile / Passive | Passive targets stand still and take hits (`SetRestrained 1`, aggression 0). Hostile ones fight (`SetRestrained 0`, aggression 2, `StartCombat player`). |
   | Respawn Targets | Removes the old targets, dead or alive, and lays a fresh lane down the line you face now. Face the longest open line of the room first. |
   | Rebuild Saved Range | Deletes this load order's prepared save (item 9); the next start sets the range up and saves it again. |
   | Exit to Main Menu | The game's own return to the title (`StartMenu::ChooseMainMenu`). |
   | Close | Closes the menu. |

5. **Tuning:** Esc > Settings > Display has Recoil, Weapon Sway, Impact Punch and Dismember
   Force. They change at once and are kept. The rest is in Fallout.ini's `[nv-rs Gunplay]`.
6. **No DLC or pack pop-ups** in the range. The DLC start quests' message boxes and the Caravan
   Pack, Gun Runners' Arsenal and other packs' "added" notifications do not show in a range game:
   a script's `ShowMessage` (`005b4630`) answers as shown without showing (the message form read
   first, as the command reads it), and items a script gives the player come quietly (the
   container's gift, `004821a0`, with its quiet flag). Only from the Test Range button to the end
   of that game (a load, a new game, the main menu); the game's log names each one kept off. A
   save or new game of your own shows them as always, and nothing is written anywhere.
7. **God mode, everything in the bag, no tutorials** (Patrick, 2026-11-14), in a range game only:
   - god mode: `PlayerCharacter::IsGodMode` (`009526b0`) answers yes, as `tgm` makes it answer,
     without the byte `tgm` sets, so nothing of it outlives the range: no damage, no
     over-encumbrance (infinite carry weight), action points and the rest of `tgm`'s; the limbs
     are put back to full every 2 seconds (`RestoreActorValue` of the seven conditions);
   - the inventory: every named weapon, armour, aid item (10 each) and weapon mod of the loaded
     plugins and DLC, and 1,000 of every named ammo (Fill Ammo tops them up to 1,000); misc items
     are left out, as some are quest items whose scripts start quests when added;
   - no tutorials or help: the tutorial manager (`00718630`) queues none and counts every one as
     seen (`00718840`), and no tutorial menu is made (`TutorialMenu::Create`, `007e8890`: the
     `ShowTutorialMenu` command's and the menus' own). The save's tutorial state is untouched
     (the routine that writes it reads the real bits).
8. **Saving is off** in the range. A manual save or quick save shows "Saving is off in the Test
   Range"; autosaves are dropped silently. No XP and no level-up either
   (`CharacterProgression::RewardExperience`, `008d5100`, and `BeginLevelUp`, `008d5210`, do
   nothing in a range game): the set-up and the targets' deaths gave slow level-ups.
9. **The prepared save** (Patrick, 2026-11-14: giving everything at each start froze the game for
   about a minute; `crates/host/src/range_save.rs`). The first start of a load order sets the
   range up as above, then, 90 frames later, saves it once (the range's own save, let through)
   and keeps it; every later start with the same load order loads it (the main menu's Continue
   way, `BGSSaveLoadManager::LoadGame`, `00850760`) and gives nothing again: god mode, the
   inventory, the targets and the container are in the save, and the range's hooks hold as
   before.
   - Kept in `userdata\testrange\<fingerprint>\` (`TestRange.fos`, `TestRange.nvse` when the
     script extender wrote one, `range.txt`: the place, container, targets, ammo), beside the
     launcher's `settings.ini`; `userdata\` is in `.gitignore`, and the play package copies a
     fixed list and refuses a `.fos`. Not in a save profile: Play and Play with mods each get their
     own entry (up to 4 load orders kept). Outside a play run (no `NV_SETTINGS`) nothing is cached.
   - In the game it is written and read in the virtual `<Documents>\My Games\FalloutNV\nv-rs Test
     Range\` (`sLocalSavePath`, `011c3f74`, points there only for that save and that load), which
     stays in the run's memory: the save copy-out (`save_out`) leaves that folder out, and the
     game's save list never looks in it. The player's save folders are neither read nor written.
   - The fingerprint: the loaded plugins (names, order, sizes, modified times), `sTestRangeCell`
     and the set-up's version (`range_save::GRANT_VERSION`).
   - **Made again** when any of those change, after the range menu's "Rebuild Saved Range", with
     `NV_TEST_RANGE_REBUILD=1` for a run, or when `userdata\testrange\` is deleted; and when the
     kept save fails to load (it is deleted and the range is set up afresh).
   - The log: `Test Range: no prepared save for this load order ...`, `saving the prepared range
     save`, `the prepared save is kept in ...`, `loading the prepared save ...`, `the prepared
     save is in play: nothing to give again`.

If the range cannot start, the main menu shows why in a message box and the game's log
(`userdata\logs\game-<date-time>.log`) has a line starting `Test Range:`:
- no cell of that name in the loaded game;
- the console could not be made;
- the game did not start in the cell.

The start runs at the main loop's next pass after the click (the seam xNVSE's tick runs at,
`0086b386`), not inside the main menu's click handler: from there the cell's load never finished
(Patrick, 312dda26).

A start that never reaches play does not sit forever:
- once the `coc` has returned, the range waits up to 90 s for play (no loading screen, not the
  main menu, the player in a cell, 30 frames in a row); then the main menu comes back with a
  message box saying so;
- inside the game's own load the range cannot take the game back, so a watchdog thread ends the
  run after 90 s without progress there (150 s once the `coc` returned), with a message box and
  the reason in the log.

The log has a line for every step of a start: the click, the main loop's pass, the cell tried, the
`coc`, the new game, every call the load makes at its own level and inside the cell's attach
(`load: calls ...`, `attach: calls ...`, and how long the slow ones took), the player's cell and
position, play begun, the container, the weapons, the ammo, the lane and each target placed.
While nothing moves on, the watchdog writes the last step and the chain of calls running now
every 10 s, so a log that stops names the function it stopped in.

The menu stays usable. Until 2026-11-14 the button did nothing on a start where the console had
not been opened: the game makes its console only on first use, and the range now asks for it the
way the console key does (`MenuConsole::Instance(1)`, `0071b160`).

### The place

**`auto`, the default:** the range reads the player's own FalloutNV.esm with the repo's plugin
reader (`esm`), in the background while the main menu is up, and picks its cell from what is
really there (`crates/host/src/range_cells.rs`). Only the base game's master is read, so the
choice is the same with or without DLC. No cell name is taken from memory: the first version's
`QASmoke` is a Fallout 3 cell that FalloutNV.esm does not have (Patrick, 2026-11-14).

**What is measured, for every interior cell:**
- its editor id;
- its placed references, and how many of them are walls, floors and other architecture (`STAT`,
  `SCOL`, `MSTT`);
- its actors (`ACHR`, `ACRE`);
- whether it has a navmesh;
- its light (the cell's ambient and directional colours, or a lighting template);
- its length: the longer side of the box the middle 90 % of its references stand in.

**What is fit for the range:** a cell that
- has a navmesh (a space made to be walked);
- has no actors (nobody to fight back, no quest NPC to break);
- has at least 20 statics;
- is lit;
- is not one of the editor's holding, storage or dummy cells, and not a passage, tunnel,
  corridor, sewer, vent or sleep cell by name (Patrick, 097ad477: `NorthernPassageINT` was a
  narrow passage and the targets stood in its walls).

**Which files:** FalloutNV.esm and the official DLC and pack masters in the load order (Dead
Money, Honest Hearts, Old World Blues, Lonesome Road, Gun Runners' Arsenal, the Caravan, Classic,
Mercenary and Tribal packs); the Mojave's open ground from FalloutNV.esm.

**The developers' test cells** (Patrick, 2026-11-14: a QASmoke-style test cell if there is a big
enough one): every interior whose editor id has `test`, `debug` or starts with `qa` is reported in
the game's log when the main menu shows, fit or not (`Test Range: developers' cell in <file>:
<cell>: navmesh floor N m2, largest clear rectangle W x L m, references span S m, statics, actors,
light: fit / not fit (why)`), and one that is fit is taken 1.5 times ahead of an ordinary place of
the same floor.

**Then its floor decides** (`crates/host/src/range_floor.rs`). The cell's navmesh is laid on a
1 m grid, square to the cell and again turned 45 degrees:
- a square is floor where a flat triangle (within about 8 degrees of level) covers it; where two
  floors overlap a metre or more apart (a walkway over a hall), or a steep triangle covers it, it
  is not;
- the range wants the best clear rectangle of floor a metre in from the navmesh's edge (obstacles
  are holes in the navmesh, so the rectangle has none): at least 12 m wide and 40 m long
  (hangar-sized: a small building never qualifies), width counting as much as length (up to 30 m
  wide and 110 m long), less below 16 m wide, less again for a floor that is not level;
- the light nudges the score; a fit developers' test cell goes well ahead (1.5 times).

**The Mojave too.** `WastelandNV`'s navmesh is laid on one grid of 2 m squares with every placed
actor's surroundings (200 m) left out, so the ground is away from towns, camps and creatures; its
best rectangle competes with the interiors (a little behind them). The game itself still spawns
what it spawns there; nothing is disabled.

The game's log lists the six best (`Test Range: candidate <place> (score ...): <n> m wide, <n> m
long clear flat floor ...`) and the one used, with why (`Test Range: started in <place> (...)`).

**The lane.** The player is set at one end of the chosen rectangle, 2 m in, facing down its
length (`player.setpos`, `player.setangle`). A floor 10 m wide takes two lanes side by side (a
humanoid lane and a creature lane, 5 m apart), 16 m three (a second humanoid lane at every other
distance). Each target's spot is checked before it is placed:
- it must be on the measured floor with 1.5 m of clearance from the navmesh's edge;
- the game's pick must find nothing solid from 2 m above it down to its knees, nor across its
  chest each way;
- otherwise it is nudged (up to 2 m to the side or 3 m along) or, when nothing near fits, skipped
  and said in the log. Its height is the floor's.

**Fallbacks, tried in order** until a new game begins:
1. `[nv-rs Gunplay] sTestRangeCell` (or `NV_TEST_RANGE_CELL`) when it names a cell
   (`QASmoke`, which the first builds wrote into Fallout.ini, counts as `auto`, and a play run
   writes `auto` back);
2. the ranked cells, best first (up to eight);
3. the last resort: the Mojave's worldspace at Goodsprings (`cow WastelandNV -18 0`). This is not
   quiet; it is only used when no interior starts.

A cell that is not in the loaded game, or where no game begins, is passed for the next. When
nothing starts, the main menu says why.

**Without a measured floor** (a configured cell, the last resort), the range measures the clear
line ahead with the game's own pick (`TES::Pick`, cast 64 units above the floor as `PlaceAtMe`'s
worker does), on the lane's middle line and both side lines, halving up to 105 m, turns the
player down the longest of eight directions, and still checks each target's spot with the pick.
The targets stand at the standard 5 to 100 m distances that fit, 2 m short of the wall; where
fewer than four fit, four are spread over the room there is. Respawn Targets keeps your facing.

**Checked here:** the scan and the floor on plugins made in memory (a wide hall against a narrow
gallery, open ground beside an actor), the rectangle search (turned halls, walkways, ramps), the
fallbacks, the lanes and the nudging. **Not checked here:** the real file, which is not in this repository. The ignored test
`range_cells::tests::the_range_has_a_place_in_the_real_falloutnv_esm` checks it on a machine with
the game:

```text
NV_DATA="C:\...\Fallout New Vegas\Data" cargo test --release -p host --lib range_cells -- --ignored --nocapture
```

It prints the ten best places with their floors and checks that the chosen one is wide enough,
that an interior is a cell of the file and that `WastelandNV` is there.

### How it is built: the game's own content and machinery only

- **Start.** The option is made as `CreateMenuElements` makes its own: `StartMenu::Option`,
  `007ce620`, title menu mode 1, moved above Quit in `pOptionsA`. Its choose function (platform
  address `7e0600c0`) runs `coc <cell>` exactly as the console runs a typed line (`run_line`):
  a temporary `Script` with the text, then `Script::CompileAndRun` (`005ac400`) with the
  console's compiler. From the main menu, the game's own `CenterOnCell` starts a new game there.
  Its new-game seam (`0093db69`) marks the range active.
- **Content.** Forms are enumerated from the game's form map (`011c54c0`), so every loaded plugin
  and DLC counts:
  - weapons: playable (not `cFlags` 0x80), not a creature's own (not 0x20), and named;
  - ammo: each weapon's `BGSAmmoForm`, or the first entry of its ammo list;
  - targets: the first base form named "Powder Ganger", "Fiend", "NCR Trooper", "Legionary",
    "Great Khan" or "Raider" (NPCs), and "Gecko", "Giant Rat", "Coyote", "Radscorpion", "Giant
    Mantis", "Feral Ghoul", "Mr. Handy" or "Protectron" (creatures). With no named humanoid, the
    player's own base (`0x7`) is used;
  - the container: a "Footlocker", "Ammo Box", "Toolbox" or "Metal Box", else any named one,
    emptied first.

  Targets and the container are placed by the game's `PlaceAtMe` worker (`005c4b30`), then moved
  by console lines (`MoveTo player x y 0`, `SetAngle`). Everything else is console lines too:
  `AddItem`, `RemoveItem`, `SetRestrained`, `SetAV Aggression`, `StartCombat`, `StopCombat`,
  `Disable`, `MarkForDelete`, `Activate player`.
- **Menu.** Home is read from the real window's keys (`d3d9real::held_keys`, Windows). The menu is
  the game's `ShowMessageBox` (`00703e80`, menu type 0x17, as xNVSE's `MessageBoxEx`). Its
  answer callback (platform address `7e0600b0`) reads `GetMessageMenuResult` (`00703fa0`), and
  the next frame of play acts on it.
- **Saves never touched.** While the range is active, saves are refused at three points, so
  nothing reaches the save folder and the copy-out (`NV_SAVE_OUT`) has nothing to copy or
  delete:
  - `BGSSaveLoadManager::SaveGame` (`008503b0`), used by the menus' saves and the `SaveGame`
    command;
  - the quick save (`008509a0`);
  - `UpdateQueuedSaves` (`00851d90`), whose autosave, forced and system queue bytes are cleared.

  The range ends on any of these seams, owned by the weighty combat row before xNVSE (engine-core
  `extend.rs`): a save loading (`00847fd9`), a new game from the menu (`007d33ce`), or the return
  to the main menu (`007d0b17`).

### Still to come

- A moving target (an NPC on a travel package).
- An on-screen readout of the last hit's force, outcome and kick.
- A god-mode toggle for hostile targets.
