# Gunplay: weighty combat

Patrick, 2026-11-13: guns and melee that feel weighty, as in the newer first-person games of the
series: recoil, kick, inertia, hit-stop and reach; hits that push the body by calibre or weapon
weight; limbs severed when the force is enough; then a Test Range reached from the main menu.

This is **not 1:1 with FalloutNV.exe**. It is an nv-rs addition, a row of the `fixes` preset:
`[nv-rs Fixes] bWeightyCombat`, `NV_WEIGHTY_COMBAT`, on by default. The `vanilla` preset and
`bWeightyCombat=0` keep the original's combat exactly. Code: `crates/host/src/gunplay.rs`. The
FIXES.md row is "Weighty combat".

## Assets and licensing

Nothing is added to the game's data. Every effect is **procedural**: the code turns the view,
offsets the weapon, scales the time multiplier, scales the strength of the game's own impulse, and
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
| Sway and inertia | The weapon lags the player's turn, bobs with walking speed and is pushed back by a shot, through a spring of about 90 ms. It is an offset of the first-person 3D's local translation (`PlayerCharacter +0x694`), in the view's axes. It is written only over what the game left there: if the game rewrites the translation, the sway starts again from the game's value, so it never drifts. Iron sights: 20%. Off in third person, the Pip-Boy and V.A.T.S. | `009445b0` |
| Hit-stop | A player melee hit sets the time multiplier to 0.08 for `iHitStopMs` of real time, ×0.8 to ×1.8 by weapon weight. A player kill heavy enough to sever does the same for 3/4 as long. Never during V.A.T.S. or another slow motion (multiplier or target not 1). The look handling puts it back to 1, and the game's ramp does too if that frame never comes. | `Actor::CombatHit` `00899cb0`, kill impulse `008ae000` |
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
| `iHitStopMs` | `NV_GUNPLAY_HIT_STOP_MS` | 70 | 0-250 |
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
| Hit-Stop | Off / On | `bHitStop` |
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
- **Melee swing feel beyond hit-stop:** a camera nudge on the swing and a reach shown in the
  crosshair.
- **Procedural weapon roll/pitch sway** (rotation, not only translation), once the translation sway
  is checked in game.

### Needs an in-game check (Patrick)

Checked so far: unit tests of the formulas and the ini plumbing, and the build. The game has not
been run with this. To check in game:
- the kick's direction and size for a pistol, a rifle, an automatic and a shotgun;
- that the recovery does not fight the mouse;
- the sway: whether the first-person 3D's translation is where the weapon's position comes from.
  If nothing moves, the game rewrites it after `009445b0`, and the sway must move to a later point.
  If the arms move the wrong way, the root's axes differ.
- the hit-stop's length and that nothing stays slowed;
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
