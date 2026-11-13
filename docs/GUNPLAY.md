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
| `iDismemberForce` | `NV_GUNPLAY_DISMEMBER_FORCE` | 45 | 0-1000 (0 = only the game's chance) |
| `iExplodeForce` | `NV_GUNPLAY_EXPLODE_FORCE` | 110 | 0-1000 (0 = only the game's chance) |

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

The rows' change functions are at `7e060060` to `7e0600a0`. `CreateMenuElements` (`007cc6e0`) is
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

## Slice 2 (plan only): the Test Range

**Goal.** A main-menu button "Test Range" that loads a small space. The space has:
- targets and NPCs: standing, moving, armoured and unarmoured;
- a weapon rack or menu to pick any weapon with ammo;
- a way back to the main menu.

**Plan:**
1. **Button.** Add a list entry to the main menu (`MainMenu`) the way the Settings rows are added:
   wrap the menu's element creation and add an entry with its own callback in the platform area.
   The callback starts a new game as "New" does, but with a flag that is checked on the first
   frame of play.
2. **The space.** Use an existing interior cell of the base game as the range (no new plugin, no
   shipped asset). Candidates are an empty test cell the game ships, or the Lucky 38 cocktail lounge
   or a Vault 21 room. On the first frame of play with the flag set, move the player there with the
   game's own `MoveTo` (`TESObjectREFR::MoveTo`). Then place targets with `PlaceAtMe` (`TESObjectREFR::PlaceAtMe`):
   - the game's own NPC and creature base forms, as non-hostile (or hostile on a toggle), at
     marked distances: 10, 25 and 50 m;
   - a "dummy" that is a non-combatant NPC with high health and no AI;
   - a moving target: an NPC given a travel package between two markers.
3. **Weapons.** Add one of each base-game weapon with ammo to the player's inventory with the game's
   own `AddItem`. Optionally offer a small menu of weapon categories built on the game's own
   message box menu. A "reset range" entry respawns the targets.
4. **Safety.** The range is a new game in a separate save slot, so it never touches the player's
   saves. Saving on the range is turned off, or saves go to a separate name.
5. **Tuning live.** The Settings rows of slice 1 already work there. A small on-screen readout of
   the last hit's force, outcome and kick (the frame-rate counter's overlay) helps tune the
   thresholds.

**Open questions for Patrick.** Which cell? Hostile targets or not by default? Base-game weapons
only, or every loaded plugin's?
