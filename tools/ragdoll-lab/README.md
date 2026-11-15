# nv-rs Ragdoll Lab

A browser page for trying the game's active ragdoll quickly: hit a capsule mannequin with a gun,
a melee weapon or an explosion and watch it flinch, take catch steps, brace against a wall, step
off a ledge or fall into its ragdoll and get up. It runs in any browser (WebAssembly and a 2D
canvas, no WebGL), from GitHub Pages or from a local folder.

## What is the game's

The controller is the game's own code: `crates/ragdoll-core`, which the host
(`crates/host`: `active_ragdoll`, `balance`, `knockdown`, `hit_force`, `ballistics`) re-exports
and runs in the game. That covers the bone roles and springs, the kick's spread, the capture point
and the catch-step plan, the ground classes, the two-bone IK, the stepping, the brace's timing,
the momentum a hit transfers (calibres, loads, swings, blasts, armour), the impact stack, the
topple speed, the reaction and the launch. Every tuning value those use is a slider, and the
lab's values are what that code reads (`ragdoll_core::tuning`; the game never sets them, so it
runs on the constants).

`src/sim.rs` is the game's glue around that code (`active_ragdoll::hit`, `apply`,
`balance_frame`, `push`, the brace and the fall; `knockdown::after_hit`, `knock_down`, the get-up),
written after the host's functions with the game's bones replaced by the mannequin's and the
game's ray casts (the floor under the feet, a wall in a step's way at knee height, the wall ahead
of the shoulders a brace stops at) by Rapier's. As in the game, the pose is written once over
the animated pose, the same for unarmed, armed and attacking bodies; the test
`one_write_path_for_every_actor` checks that write for all three.

The world around the body is the game's logic too (`ragdoll_core::obstacle`): the lab answers its
floor and ray questions from Rapier, the game from its ray casts and navmesh. The grounds
`railing`, `gaprailing`, `halfwall`, `table` and `curb` put obstacles behind the mannequin (a
push from its front meets them), for braces on a waist-high top, folding over, and tripping. What
hits leave (fatigue, pain, hurt limbs; `ragdoll_core::condition`) is kept across a run's hits.

The standing body is the game's active ragdoll body (`ragdoll_core::active`): capsules with
PD motors on every joint tracking the animation, soft right after a hit and ramping back, the
hit's push travelling through the chain, balance from the motors, the pelvis's support and the
capture-point steps, an eased blend back to the animation. Its sliders are under "Active
ragdoll".

## What is not

- **The ragdoll** is Rapier's, not Havok's. Its bone layout is the game's (the 20 Bip01 bones of
  the rig, the same parents). Its total mass is the knockdowns' figure for the body (80 kg for a
  human, 250 kg for a super mutant). Its bone lengths, per-bone mass shares (standard segment
  fractions) and joint limits (a biped ragdoll's usual cones and hinges) stand in for the game's
  `skeleton.nif`, which is game data and is not in this repository.
- **The animation** under the pose layer is the mannequin's rest pose (the game's idle moves a
  little).
- **The get-up** blends back to the rest pose over 1.6 s (the game plays its get-up animation).
- **A hit on a body already down** pushes the struck bone with the hit's momentum (in the game
  the knockdowns leave a downed body to the game's own hit impulse).
- **Explosions** here only go through the knockdowns' physics; in the game a blast can also knock
  an actor down through the game's own explosion knock-down.

## Using it

- Click a body part to hit it. Drag to turn the view, scroll to zoom.
- Pick the weapon (gun and ammo, melee weapon with Strength, skill and power attack, or an
  explosive), the body, its scale and armour, the ground (flat, slope, uneven, stairs, wall,
  ledge), the limbs it has, and whether it holds a weapon (it aims it, the weapon drawn in its
  hand) or is attacking. An armed body keeps its aim through a flinch below the stagger; a
  stagger, a catch step or a knockdown interrupts an attack.
- Pause, step a frame, slow motion (0.5x, 0.25x, 0.1x), reset. Keys: Space, `.`, R.
- **Tuning:** every value has a slider (double-click its name for the game's value).
  **Export ini** writes them as text: the `[nv-rs Gunplay]` lines are the game's ini settings
  (paste them into `nv-rs.ini`); the `[nv-rs Ragdoll Lab]` lines are constants compiled into the
  game (change them in `crates/ragdoll-core` to carry them over). **Import ini** reads either back.

## Scripted runs (for an automated browser)

URL parameters run scenarios at load and report the results:

- `?scenario=all&headless=1`: every built-in scenario (see the page's Scenarios list).
- `?scenario=ledge&headless=1`: one of them.
- `?ground=wall&hits=44@chest*2~0.15,sledge:power@chest%2B1&headless=1`: a custom one.
- `&tune=kick:20,iBalance:150`: tuning values (field names or ini keys) for the run.

`hits` is a comma list of `weapon[:load|:power]@part[*count[~interval]][+delay][/from]`:
`weapon` a gun (`9mm`, `10mm`, `44`, `357`, `556`, `308`, `4570`, `50mg`, `12ga`, ...), a melee
weapon (`fists`, `knife`, `bat`, `sledge`, ...) or an explosive (`frag`, `dynamite`, `missile`,
`nuke`, whose part is its distance in metres); `part` a bone (`head`, `chest`, `belly`, `pelvis`,
`lthigh`, `rcalf`, ...); `*count~interval` a burst; `+delay` seconds after the hit before (write
`%2B` for `+` in a URL); `/from` the attacker's direction in degrees (0 the body's front, which
pushes toward the wall, the ledge and the stairs; 90 its right). Other keys: `ground`, `body`
(`human`, `mutant`), `scale`, `armour`, `range` (m), `limbs` (four digits: left leg, right leg,
left arm, right arm; `0111` has lost the left leg), `armed=1` or `armed=rifle` (holding a
two-handed weapon, aiming), `armed=pistol` (a one-handed one, the left arm hanging),
`attacking=1` (attacking; armed too), `expect` (`upright`, `fell`, `braced`, `steps>=1`,
`steps=0`, `fall:<kind>`, `flinched`, `aim<=D` degrees, `armspin<=W` and `ragdollarmspin<=W`
rad/s, `interrupted`, `attacking`, `vjump<=V` m/s, `jerk<=J` m/s3, `onehand` / `bothhands`
(which hands braced), `handbounce<=N`, `handjitter<=V` m/s, `handslip<=C` cm, `elbowosc<=N`).

Each run goes until the body has recovered (or 12 s after the last hit). The report (JSON in
`<pre id="results">`, in `window.ragdollLab.results`, and on the console;
`window.ragdollLab.done` and `body[data-done="1"]` when it is ready) has: `upright`, `fell`,
`fall` (`knockdown`, `stepped off a ledge`, `more than its steps could catch`, `one leg`,
`crumpled against a wall`), `steps`, `braced`, `distance_m` (how far the body ended from where it
stood), `recovery_s` (from the first hit until it stood at rest again), `peak_speed_mps` against
`topple_mps`, the decisions (`None`, `Stagger`, `Knockdown`), `arm_spin` and `ragdoll_arm_spin`
(the fastest an arm bone turned standing, and in the ragdoll, rad/s), `flinch_deg`,
`aim_drift_deg` (an armed body's weapon hand off its aim), `interrupted`, `velocity_jump_mps`
(the biggest change of a bone's velocity in one frame while it stands) and `jerk` (m/s3),
`brace_hands` (which arms braced), `hand_losses` (a bracing hand coming off the surface while it
holds), `hand_jitter_mps` and `hand_slip_cm` (a settled hand's speed and slide),
`elbow_reversals` (a planted elbow swinging back more than 3 degrees), whether the expectations
held, and the log's last lines. Every built-in scene is held to no hand bounce, and
`braces_hold_across_pushes_and_weapons` runs 108 braces across grounds, weapons and sides
(`ragdoll_core::brace`, docs/GUNPLAY.md).

## Building

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install --locked wasm-bindgen-cli --version 0.2.100
```

```bash
cd tools/ragdoll-lab && cargo test --release -p ragdoll-core -p ragdoll-lab
```

```bash
cd tools/ragdoll-lab && cargo build --release --target wasm32-unknown-unknown && wasm-bindgen --target web --no-typescript --out-dir www/pkg target/wasm32-unknown-unknown/release/ragdoll_lab.wasm
```

```bash
python3 -m http.server -d tools/ragdoll-lab/www 8000
```

Then open http://localhost:8000/. `.github/workflows/ragdoll-lab-pages.yml` does the same and
publishes `www/` on GitHub Pages. The lab is its own Cargo workspace (its own `Cargo.lock`), not a
member of the root one; nothing of it goes into the game or its package.

The page bundles Rapier and its dependencies (MIT, Apache-2.0, Zlib, Unlicense and Unicode-3.0
licences; `cargo metadata` in this folder lists each crate's).
