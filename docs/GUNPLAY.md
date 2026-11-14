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
7. **Saving is off** in the range. A manual save or quick save shows "Saving is off in the Test
   Range"; autosaves are dropped silently.

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

**Then its floor decides** (`crates/host/src/range_floor.rs`). The cell's navmesh is laid on a
1 m grid, square to the cell and again turned 45 degrees:
- a square is floor where a flat triangle (within about 8 degrees of level) covers it; where two
  floors overlap a metre or more apart (a walkway over a hall), or a steep triangle covers it, it
  is not;
- the range wants the best clear rectangle of floor a metre in from the navmesh's edge (obstacles
  are holes in the navmesh, so the rectangle has none): at least 8 m wide and 25 m long, width
  counting as much as length (up to 30 m wide and 110 m long), less below 12 m wide, less again
  for a floor that is not level;
- the light nudges the score; a developers' test or QA cell goes a little ahead.

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
