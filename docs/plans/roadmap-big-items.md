# Plan: the roadmap's big items, assessed for the Lhomer/nv-rs fork

Status: a proposal for Patrick (the fork's owner), 2026-11-11. Planning only: no engine code changed with it.
Written against `ec8ffe9` (upstream `slaterain/nv-rs` main and this fork's main were the same commit), the upstream
issues #61, #268 and #273 and the open upstream PR #473, read on 2026-11-11.

The roadmap is the "Direction" table of [MILESTONES.md](../MILESTONES.md) and point 6 of
[ADR-0012](../adr/0012-modern-64-bit-engine.md). The rules it lives under: [ADR-0014](../adr/0014-typed-migration-in-two-stages.md)
(the typed migration in two stages), [ADR-0015](../adr/0015-hand-typing-to-the-end-with-real-threads.md) (hand typing,
real threads), [ADR-0016](../adr/0016-script-extenders-built-in.md) (the extenders built in) and [AGENTS.md](../../AGENTS.md)
(look and play exactly like the original; nothing invented).

## In short

| # | Item | State today | Blocked on | Size (PRs) | Linux/CI vs Windows | Clash with upstream | Player sees |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 6 | **Platforms: Linux** (then macOS) | Builds on Linux, 6 of about 56,400 tests fail there (section 10); no window, input, sound or shader compiler off Windows; wgpu backend exists | nothing hard; four recorded tables from Windows | **8-12** for a playable Linux build | mostly Linux; tables and comparison runs on Windows | **low** (parts the maintainer invited, #268) | the game on Linux and the Steam Deck |
| 5 | Modding: JIP LN, the last xNVSE commands, >255 plugins | xNVSE 533 of 654 built in; JIP not started; 255 plugins | JIP: xNVSE's core (done); >255: forms and saves | 15 (xNVSE rest) + 40-80 (JIP) + 15-30 (>255) | unit tests on Linux; mods on Windows | **high** (the maintainer merges xNVSE batches daily); licences do not pass to forks | mod lists that need JIP run |
| 4 | Graphics: AA, upscaling, HDR, GPU-driven, draw distance | wgpu the default renderer (Vulkan), D3D9 fallback; drawing thread step 1 open (#473) | GPU-driven: typed draw + drawing thread; distance: memory and speed | 4-8 (AA, upscaling), 20+ (GPU-driven) | replays of a recorded stream on Linux; looks on Windows | **high** (d3d9-wgpu changes every day) | sharper image, more frame rate headroom |
| 1 | Memory and loading: no 4 GB limit, mapped archives, background loading | 3.2 GB guest heap; stage 2 machinery and the lock-free maps' opt-in move; bulk platform buffers already outside | the typed migration of each family that holds bytes | tens to hundreds; a fork-sized slice is 3-6 | heap census and long walks on Windows | medium | bigger mod lists without crashes; more loaded world |
| 2 | CPU: a job system on every core | real threads on in play; parallel culling helpers; render thread | the typed code of each system; races proven absent | 20-40 per system moved to jobs | race tests on Linux; distributions on Windows | medium-high (ADR-0015 is the maintainer's line) | frame rate in towns |
| 3 | Smoothness: 200+ fps, fixed-rate simulation, precompiled shaders | `NV_UNCAP` (timer without its sleep), shipped pipeline list, pipeline cache | fixed rate: typed scene, animation, Havok and the drawing thread | 15-30 | interpolation tests on Linux; feel on Windows | high (touches the frame's order) | smooth high-refresh play |
| 7 | Co-op | a downstream project (cranebeaker/fnv-coop) on the removed layer | a stable engine; the maintainer keeps it downstream for now (#273) | 50+ | two-process runs | low if kept downstream | playing together |

**Recommended order for the fork:** 6 (Linux) → 4 (the wgpu post-processing part only) → 5 (JIP, in step with the
maintainer) → 1 (mapped archives and background loading as fork-sized slices; the 4 GB limit rides on the migration) →
2 → 3 → 7 (help cranebeaker downstream rather than fork it again).

**The #1 pick: a native Linux build that plays,** starting with the four parts the maintainer asked contributors to take
in #268. It is the only big item that is not on the maintainer's critical path, can be built and tested on Linux (this
session built it and ran its tests there), can go upstream piece by piece, and gives players something they can see. Section 9 has its first milestone.

**One question decides it:** does Patrick have a Linux machine (or a Steam Deck) with a Vulkan GPU and his own copy of
the game on it? If not, every Linux play test needs someone else's machine, and the fallback #1 is the wgpu
post-processing (item 4), which he can test fully on Windows.

**Two cautions for the whole fork:**

- **Permissions do not carry to forks.** The maintainer, on #273 (2026-11-03): the permissions from the mod authors
  (xNVSE, JIP and others) and from fnv_decomp's authors were granted to nv-rs and "don't carry over to forks
  automatically". Work in this fork that translates xNVSE, JIP or fnv_decomp material should either go upstream to
  nv-rs or have its own permission first. This weighs most on item 5.
- **Upstream moves fast.** The last 50 commits on main span two days (2026-11-10 and 11), many in `crates/d3d9-wgpu`,
  `crates/nvse`, `crates/engine-typed` and the Direct3D 9 platform files. A fork that edits those files will rebase
  often. The maintainer asked contributors to leave `d3d9device.rs`, `d3d9resource.rs`, `d3d9query.rs`, `d3d9.rs`,
  `window.rs`, `player_move.rs` and `d3d9real.rs` as they are while they are tuned for frame rate (#268, 2026-11-02).

## 1. Memory and loading

**What exists.**
- The guest heap reaches into the upper 2 GB: two ranges, `0x20000000..0x7E000000` and `0x80000000..0xF0000000`,
  about 3.2 GB (HANDOFF 2026-11-03, `mem.rs`, `mem/heap.rs`), with the scrap heaps' reservations in the gap below
  it (#448, #450, on by default).
- Flat guest memory: one 4 GB host reservation per `Mem`, a byte-per-page table, plain loads and stores
  (`crates/engine-core/src/mem.rs`; ADR-0015's amendment).
- The platform's bulk buffers are already outside the 32-bit space: textures, vertex and index buffers live on the
  Direct3D side, and the sound buffers' samples moved out with #430 ("the last platform-side bulk buffer"). So 4K and
  8K textures do not fill the guest heap; forms, models and the game's own heaps do.
- Stage 2 machinery (ENGINE_CRATE.md, "Moved families"): the moved area `0x10000000..0x20000000`, the slab, buffers,
  stubs, K1 stubs, the dual run, and the first moved family, the lock-free maps, opt-in (`NV_TYPED=lockfree:moved` or
  `:freed`). The `geometry` (23-40 MB on the census route) and `anim_keys` (about 12 MB) families were typed so that
  stage 2 can move their arrays.
- The heap census (`NV_CENSUS`), the touch census (`NV_TOUCH`) and the per-thread call table: the tools that say
  what a move needs.
- Loading: the game's own loader threads already run as real threads in play; texture files are read before the
  renderer's lock (#412). No memory-mapped archives (`crates/engine-platform/src/files.rs` reads the player's files
  into the run's copy or in place through `ReadFile`), no cell cache beyond the game's own.

**Upstream in flight.** The typed families, one or two a day, by the maintainer and their agents. Nothing open on
the heap or the loader.

**Prerequisites.** "No 4 GB limit" is the end state of the whole migration: ADR-0015 found only about half of the
live bytes in families touched by 300 functions or fewer, and the form records last. The guest heap cannot pass
4 GB while untyped code holds 32-bit pointers to it. What ADR-0014 leaves open ("what no 4 GB limit means for the
release, and when") is the maintainer's to decide.

**Size.** The limit: as many PRs as the migration has families (tens to hundreds). Fork-sized slices: memory-mapped
archive reads for the files the game opens read-only (3-4 PRs, behind a switch, the bytes the game reads unchanged),
a cell cache that keeps a cell's file reads warm (3-6 PRs, after a measurement shows load time in the reads).

**Linux/CI vs Windows.** Unit tests of a mapped reader anywhere. Whether it helps: load-time runs on Windows with
the game; the 4 GB question: long outdoor walks with `NV_CENSUS` on Windows.

**Clash.** Medium: `files.rs` is not on the maintainer's do-not-touch list, but stage 2 is their design.

**Player sees.** Fewer "heap full" stops with big mod lists (the walks that stopped at 1.5 GB no longer stop at
3.2 GB); faster cell loads only if reads are the cost, which nobody has measured yet.

## 2. CPU: a job system on every core

**What exists.** Real threads by default in play (`crates/engine-core/src/real_threads.rs`, `NV_THREADS=stepped`
the reference mode); the render thread (#347/#351: town +15.6%); the culling walk shared out to helper threads
(`NV_PAR_RENDER`, #429: three helpers from 8 logical processors); the device lock apart from the kernel lock.
There is no general job system.

**Upstream in flight.** The drawing thread (#473 is step 1 of several); the typed families of the update half
(`scene`, `particles`, `collision`, `multibound`, `animation`), which are the groundwork for running them as jobs.

**Prerequisites.** ADR-0015: the code the game's other threads run is typed first; results must be "proven
identical" (MILESTONES), which for jobs that change the order of the original's work means stepped-thread runs
byte-identical and real-thread runs compared as distributions over several recordings. A job needs typed code
(translations reach guest memory with plain loads and stores; ADR-0015's amendment warns about loops that wait on
guest memory) and, for objects written from two jobs, stage 2's atomic cells.

**Size.** 20-40 PRs per system moved onto jobs (AI per actor, animation, physics islands, scene update), each with
its race tests.

**Linux/CI vs Windows.** Race campaigns (shuffled schedules, soaks) run headless; the speed gates and recordings
need Windows and the game.

**Clash.** Medium-high: it is the maintainer's main line (ADR-0015 points 3-5).

**Player sees.** Frame rate in crowded places (the street runs at about 57-67 fps today).

## 3. Smoothness: 200+ fps, fixed-rate simulation, precompiled shaders

**What exists.** `NV_UNCAP` (`crates/host/src/uncap.rs`: `BSTimer`'s step without its 10 ms sleep, timed on the
microsecond clock); the game's own timers otherwise cap it at about 133 fps (PERFORMANCE.md). For shaders: the wgpu
backend converts the game's bytecode at load time (`crates/d3d9-wgpu/src/sm`), keeps a pipeline cache and ships a
list of the pipelines a run needs, made at the start behind the loading screen (#433, #437); compiling the game's HLSL
goes through the player's `d3dx9_38.dll` on Windows (`crates/engine-platform/src/d3dxreal.rs`). No fixed-rate
simulation and no interpolation.

**Upstream in flight.** Frame-rate tuning of the Direct3D 9 platform files and the wgpu backend.

**Prerequisites.** A fixed-rate simulation with interpolated drawing needs: the simulation step separated from the
draw (the drawing thread's work); two states of every drawn transform (typed scene graph and animation, stage 2
layout); the Havok step's rate decoupled. ADR-0012: gameplay-changing modern systems are switchable, and the original's
high-frame-rate bugs are themselves behaviour that must stay available under `vanilla`.

**Size.** 15-30 PRs after the drawing thread lands; precompiled shaders are mostly done (2-4 PRs to ship converted
WGSL and pipelines per GPU family, if the start-up time shows it matters).

**Linux/CI vs Windows.** Interpolation maths and the fixed-step scheduler are unit-testable; the feel only on
Windows with the game (and against recordings of the original for gameplay timing).

**Clash.** High: it changes the frame's order, which every typed family and the drawing thread depend on.

**Player sees.** Smooth motion on 144-240 Hz screens; physics that behaves the same at every frame rate (fixes preset).

## 4. Graphics

**What exists.** `crates/d3d9-wgpu`: Direct3D 9 on wgpu (Vulkan, Direct3D 12, Metal), the game's shader bytecode
converted to WGSL, the default renderer since #463 (Fallout.ini's `sRenderer`, Settings.cmd's Renderer; Vulkan first,
Direct3D 12 after a failed Vulkan, the real Direct3D 9 the fallback when wgpu cannot draw, #466/#474). Frames match the
real Direct3D 9's within 1% of pixels at the reference places (MILESTONES, milestone B). The recorded call stream
(`crates/d3d9-stream`) can be replayed into any backend (`d3d9-replay`, `shot-diff` in `crates/d3d9-wgpu/src/bin`).
The game's own HDR and its image-space effects are translated and drawn by the backend. Nothing exists for modern AA,
upscaling, HDR output, GPU-driven drawing or a longer draw distance.

**Upstream in flight.** #473, "Drawing thread, step 1" (`crates/engine-typed/src/draw/*`, `shader/batch.rs`), open,
no reviews yet; the maintainer marks `draw/detach.rs` and the cut as the drawing thread's and the list accessors as
owned by another agent. The wgpu backend itself changes almost daily (#452 to #474).

**Prerequisites.** AA and upscaling as post-processing at `Present`: none beyond the backend (the game draws into its
own targets; the backend can resolve and scale the final one). GPU-driven drawing: the typed draw (done, `draw`
family) and the drawing thread. Draw distance: the grid setting (`uGridsToLoad`) raises memory and CPU load: items 1
and 2.

**Size.** FXAA or SMAA, and a spatial upscaler (FSR 1-style) at Present: 4-8 PRs. HDR output (scRGB/PQ swap chain):
3-5. GPU-driven drawing: 20+.

**Linux/CI vs Windows.** A post-process pass can be developed against replays of a recorded stream on Linux (the
stream is recorded on Windows from Patrick's own game and stays local: LEGAL.md); the look is judged on Windows.

**Clash.** High in `crates/d3d9-wgpu` (`device.rs`, `window.rs`, `prepare.rs` change often); a post-process in a
module of its own, called once at Present, keeps it small.

**Player sees.** A sharper picture without the original's MSAA cost, higher resolutions at the same frame rate.

## 5. Modding

**What exists.** Built-in xNVSE (`crates/nvse`, ADR-0016): the command table, script context, argument reader,
string and array variables, expressions, loops and user functions, events, the co-save, input, inventory references:
**533 of 654** commands (`table.rs`'s count test). JIP LN NVSE: no crate yet (ADR-0016's I-13); the run-time compiler
and the console's extended commands are I-14. Plugins: at most 255 in the load order (MODS.md, "the load order");
form ids carry the plugin index in their top byte throughout the exe.

**Upstream in flight.** xNVSE command batches (#409, #415, #420, #421, #425, #426 on 2026-11-10 alone), by the
maintainer's agents.

**Prerequisites.** JIP needs xNVSE's core (done) and its plugin interface (not yet). More than 255 plugins "with
existing mods unchanged" means a new form-id scheme behind the exe's 8-bit index: the forms' load, the save format
(`crates/fos`, FOS_SAVES.md), every comparison of a form id's top byte; realistically after the forms are typed.

**Size.** The rest of xNVSE: about 10-15 PRs (121 commands, I-14). JIP: 40-80 (1,118 commands at `0x2200` and
`0x2600`, its always-on game patches as fix rows). >255 plugins: 15-30, invasive.

**Linux/CI vs Windows.** Every command has unit tests that run anywhere; whether a mod works needs Windows, the game
and the mod.

**Clash.** High for xNVSE (the maintainer is mid-way through it), medium for JIP (not started; ask first). And the
licence caution above: JIP is GPL and its permission was given to nv-rs.

**Player sees.** The single biggest gap for real mod lists: most current mods need JIP LN.

## 6. Platforms: Linux and macOS

**What exists.**
- The whole workspace builds on Linux x86-64 with the pinned 1.99.1, and 56,388 of its tests pass there; six fail
  (section 10): one race between tests, four file-system answers that only Windows has (file times, case-insensitive
  names) and one maths command whose last bits come from the platform's `tan`/`tanh`. CI runs on `windows-latest`
  only (`.github/workflows/ci.yml`).
- `crates/d3d9-wgpu` builds for Vulkan and Metal and is the default renderer on Windows. Its window code has
  `cfg(not(windows))` stubs (`client_size` answers `None`, `minimised` false).
- The platform layer answers `CreateWindowExA` with a Win32 window on Windows and a placeholder elsewhere
  (`crates/engine-platform/src/window.rs`); DirectInput is fed from that window's messages.
- Sound plays through WASAPI on Windows only (`sound_out.rs`: "elsewhere `start` answers that there is no device");
  the resampler's tables are read from the player's `RESAMPLEDMO.DLL` and the distortion's edge table measured from
  `dsdmo.dll` at run time (`resampler.rs`, `dsfx.rs`).
- Shader compiling (`D3DXCompileShader`, `D3DXAssembleShader`, the constant tables) goes through the player's
  `d3dx9_38.dll` (`d3dxreal.rs`), Windows only. Texture loading is translated (`crates/d3dx-tex`), so textures need no
  DLL or cache.
- The machine tables (the adapter table, `scripts/d3d9-capture`; the DirectSound table, `scripts/dsound-capture`)
  default to `%LOCALAPPDATA%\nv-rs`.
- No `winit` or `cpal` in `Cargo.lock`; the Bevy viewer and its Linux and macOS builds were removed on 2026-11-03
  (ADR-0011, PLAYTESTING.md).

**Upstream issues.**
- #61 (Linux port, Tokra110, open since 2026-10-08): written for the removed viewer.
- #268 (the play build on Linux and macOS, the platform side; open): the maintainer's answer of 2026-11-02 says yes
  to sound through `cpal` (a 48 kHz, two-channel `f32` stream on the output's own thread, no output rather than
  conversion if the device won't open), yes to recorded tables made on the player's Windows install and kept local
  (a non-Windows default location, XDG on Linux, is "a fine small first PR"), yes to a recorded shader-compiler cache
  read by a module with no outside crates (keyed by every input, `ID3DXInclude` called in the same order, the DLL that
  made it recorded, a miss stops with a fill hint), and yes to the shader converter (which has since landed in
  `d3d9-wgpu`). Not now: DXVK Native. The maintainer keeps the device seam and the distortion's edge table
  ("Please don't add a fallback for it") and asked contributors to leave the Direct3D 9 call sites alone. One PR per
  part, linked to #268, with a Linux CI job. Two later comments (HellboundGlory, 2026-11-07 and 09) ran the Windows
  build under Wine 11.21: main menu, a new game and Doc's house with no sound; Wine's `RtlGenerate8dot3Name` stub and
  the edge table stop it.
- What changed since that answer: the winit guidance there assumed the Bevy viewer owned the event loop, which is gone,
  and the wgpu backend now exists. Both are questions for the maintainer before the window PR (section 9).

**Prerequisites.** None in the migration. The tables and the shader cache are recorded on a Windows install of the
game (Patrick's PC), then read on Linux.

**Size.** Linux, playable with sound: 8-12 PRs (section 9 has the first five). macOS on top: 4-6, and a Mac is needed
("please build on a Mac yourself", #268).

**Linux/CI vs Windows.** Build, unit tests, key-mapping and cache tests, and the backend on a software Vulkan
(lavapipe) in CI; play on a Linux machine with a GPU and the game; the tables and caches recorded on Windows; the
cross-platform check (the same recorded Direct3D 9 stream on both) needs both.

**Clash.** Low if the new code stays in new files and leaves the listed Direct3D 9 files to the maintainer; this is the
work the maintainer asked contributors for.

**Player sees.** The game on Linux and the Steam Deck without Proton, and the base for macOS.

## 7. Co-op (#273)

**What exists.** Nothing in the repository. cranebeaker's GPL fork (`cranebeaker/fnv-coop`) has host-authoritative
story co-op in its own crates with hooks in `world` and `viewer`, the hand-written layer that was removed on 2026-11-03.

**Upstream.** The maintainer (2026-11-03): co-op is on their roadmap too; "keeping it downstream in your crates makes
sense" for now; the hooks will have to move to the engine path; the permissions do not carry to forks.

**Prerequisites.** A stable engine API (the host and the typed families), deterministic simulation where both sides
must agree, save and join snapshots from the game's own save code.

**Size.** 50+ PRs; most of it already exists downstream on the old layer.

**Clash.** Low if left downstream; a third co-op line in this fork would duplicate two others.

**Player sees.** Co-op, eventually. Recommendation: contribute to cranebeaker's port of their hooks to the engine
path, not here.

## 8. Recommended order, and why

1. **Linux (item 6).** Invited by the maintainer, off their critical path, testable on Linux and in CI, upstreamable
   part by part, and a visible result. Section 9.
2. **wgpu post-processing (part of item 4):** AA and a spatial upscaler at `Present`, in a module of its own, after
   Linux has made Patrick at home in `d3d9-wgpu`, and once its daily churn slows. The fallback #1 if there is no Linux
   machine.
3. **JIP LN (part of item 5),** in step with the maintainer: ask on upstream which of I-13 they want from a
   contributor before writing any, because of the licence and the pace of `crates/nvse`.
4. **Mapped archives and background loading (part of item 1),** each after a measurement shows reads cost load time.
   The 4 GB limit itself rides on the migration: the typed families plan (`docs/plans/next-typed-family.md` on branch
   `claude/plan-next-typed-family`) is the fork's way to help there.
5. **Jobs (item 2)** and **fixed-rate simulation (item 3)** after the drawing thread lands and the update half is typed.
6. **Co-op (item 7):** downstream with cranebeaker.
7. **More than 255 plugins:** last, after the forms are typed.

## 9. The #1 pick in detail: a native Linux build, first milestone

**Milestone L1:** on a Linux machine with a Vulkan GPU, the play build starts from Patrick's own game data, draws the
intro and the main menu with wgpu on Vulkan in its own window, takes keyboard and mouse, starts a new game and reaches
Doc Mitchell's house, silent; Windows unchanged and green; Linux in CI. Sound (cpal) is milestone L2, once the
maintainer's edge table is settled.

### Before the first PR

- Ask on #268 (a comment, Patrick's own words): the window now that Bevy is gone (where `winit` may live now that the
  root workspace already has `wgpu`; who owns the event loop; whether the game's `CreateWindowExA` should map to a
  winit window off Windows); whether the device seam is still to be cut by the maintainer; whether a Linux CI job is
  welcome at the size it needs (below).
- Check that #62 and #108 (named in #61 and #268) are closed or superseded, so nothing is built twice.

### PR L1-1: a Linux CI job

- `.github/workflows/ci.yml`: a `check (linux)` job on `ubuntu-latest` next to the unchanged Windows job: fmt, the
  workspace tests, clippy with `-D warnings`, the ledger's tests.
- Measured here: with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` (as Windows CI sets), four parallel rustc jobs
  ran out of a 15 GB machine's memory on `engine-havok`'s test build (SIGKILL), and the first build with debug info and
  incremental state used up the session's disk (about 30 GB free at the start); `-j 2` without them builds and the
  tests take about 64 minutes. GitHub's Linux runners have 16 GB and 4 cores: the job needs `-j 2` (or a larger runner) and the same cache set-up as Windows.
- Tests that need the real DLLs or a display adapter already skip themselves off Windows. Six tests fail on Linux today
  (section 10), and the job needs each handled:
  - three `files.rs` searches stop at `FindFirstFileA`'s "file times are only provided on Windows", and one opens
    `CARGO.TOML` expecting Windows' case-insensitive lookup: L1-4 makes them pass; until then the job skips those four
    by name, with a note pointing at L1-4 (never a blanket `cfg(windows)`);
  - `nvse`'s `the_results_match_the_real_game_bit_for_bit`: `tan`/`tanh` take their last bits from the platform's maths
    library. The fix is a portable routine that gives the real game's bits everywhere (the exe's own C runtime routine,
    as the `crt` family types others): a PR of its own after L1-1, skipped by name until then;
  - `engine-core`'s `cxx_exception::tests::a_stop_does_reach_the_panic_hook` is a race between tests over the global
    panic hook (it fails about two runs in three of the whole suite here): fix it before the job, or the job is red at
    random.
- Clippy with `-D warnings` stops on Linux in `engine-platform` (section 10): items and variables only the Windows paths
  use (`d3d9resource.rs`, `d3d9device.rs`, `files.rs`, `d3dx.rs`). Two of those files are on the maintainer's
  do-not-touch list, so ask before the job is added: they may prefer to take those lines themselves, or a job that runs
  clippy without `-D warnings` until then.
- Patrick on Windows: nothing; the Windows job must stay green.

### PR L1-2: the machine tables' default place off Windows

- `%LOCALAPPDATA%\nv-rs` stays on Windows; on Linux `$XDG_DATA_HOME/nv-rs` (else `~/.local/share/nv-rs`), on macOS
  `~/Library/Application Support/nv-rs`, for the adapter table, the DirectSound table and the D3DX cache
  (`engine-platform`'s `d3d9.rs`, `dsound.rs`, `nvapi.rs`, `d3dx.rs`, `host/src/lib.rs`). The env variables still win.
- Tests: the path chosen for each platform's environment (unit, no files).
- Patrick on Windows: run the play build once to see the same files are found.

### PR L1-3: the shader-compiler cache

- A new module with no outside crates (in `engine-platform`, beside `d3dxreal.rs` but not editing its call sites
  beyond one switch): `NV_D3DX_SHADERS=record` on Windows writes each `D3DXCompileShader`, `D3DXAssembleShader` and
  `D3DXGetShaderConstantTable` answer of the player's DLL to a cache file in the tables' folder, keyed by every input
  (source bytes, defines, the include callbacks' answers in order, entry point, profile, flags; the constant table by
  bytecode), with the DLL's version and hash in its header; elsewhere (or with `=cache`) answers from it, calling
  `ID3DXInclude` in the same order, and stops on a miss naming the key and how to fill it (the maintainer's terms).
- Tests: round trip on generated sources and a fake DLL answer; a miss stops; the include order is replayed; nothing
  from the game committed.
- Patrick on Windows: record the cache over a run that visits the intro, the main menu, Doc's house and Goodsprings
  (`NV_D3DX_SHADERS=record`), and check that a second run with `=cache` is byte-identical (`NV_REPEATABLE`,
  `NV_D3D9_RECORD`, `cmp`).

### PR L1-4: case-insensitive paths, file times and the save folders

- Windows paths are case-insensitive; the game asks for `Data\Textures\...` while a Linux copy of the game data may be
  `data/textures/...`. A lookup in `files.rs` (and `FindFirstFile`'s) that resolves a Windows path against the real
  directory case-insensitively, cached per directory, off Windows only; `SHGetFolderPathA`'s My Documents and Local
  Application Data mapped to XDG places (or a Proton prefix, as #61 proposed). Windows' code path unchanged.
- `FindFirstFileA`/`FindNextFileA`'s file times off Windows: the times from `std::fs::Metadata` converted to
  `FILETIME` (100 ns since 1601), and the attributes from the metadata, where the Windows path answers from the OS.
- Tests: generated directory trees with mixed case; the Windows answers pinned; the three search tests and the
  upper-case open test in `files.rs` that fail on Linux today pass.
- Patrick on Linux: the game data copied from his own install, the run reaching the main menu's file reads.

### PR L1-5: a window, input and wgpu on Linux

- After the maintainer's answer on the window. The likely shape: a host-side module (in the crate the maintainer
  names) makes a winit window when the game calls `CreateWindowExA` off Windows, on the platform layer's window thread
  (winit's `with_any_thread` on X11 and Wayland; macOS later needs the process's main thread), hands its
  `raw-window-handle` to `d3d9-wgpu` (whose surface already takes one), answers `client_size` and `minimised` from it,
  and maps winit's physical key codes to DirectInput `DIK_*` codes and its raw mouse motion and buttons to the
  DirectInput mouse the game reads. No change to `window.rs`'s Windows path or the Direct3D 9 call sites.
- Tests: the full key table against `dinput.h`'s values (the small PR the maintainer also offered on #268); the
  window's size and minimise answers; `d3d9-wgpu`'s own tests on lavapipe in the Linux CI job, if the job can install
  `mesa-vulkan-drivers`.
- Patrick on Linux: the intro, the main menu, New, Doc's house; `NV_SHOT` captures compared with the same captures
  from Windows-wgpu (`shot-diff`).
- **The cross-platform check:** with the same machine tables and shader cache, an `NV_REPEATABLE` run records the same
  Direct3D 9 stream on Linux as on Windows (the game's own commands, before any backend). `cmp` of the two records is
  the strongest test the port has that nothing platform-specific leaked into the game. Patrick records on both and
  compares.

### What Patrick runs on Windows for L1

1. `scripts/d3d9-capture` and `scripts/dsound-capture` once (the tables), copied to the Linux machine's
   `~/.local/share/nv-rs`. Never committed or shared (LEGAL.md).
2. The shader cache recording run (L1-3), the same way.
3. One `NV_REPEATABLE=<frames> NV_D3D9_RECORD=<file>` run over the intro and the main menu, to compare with Linux's.
4. The Windows play build after each PR: unchanged behaviour (the same recorded run before and after, `cmp`).

### Milestone L2 (sound) and after

- `cpal` output at 48 kHz stereo `f32` on the output's thread (the maintainer's terms), the resampler's tables recorded
  on Windows like the other tables, and the distortion's edge table once the maintainer has settled it (they asked
  that nobody add a fallback). Gate: `NV_SOUND_RECORD` files identical between Windows and Linux on the same tables.
- Then a Linux play note in PLAYTESTING.md, the Steam Deck (gamepad through the same DirectInput mapping), and macOS
  with a tester who has a Mac.

## 10. Checks run for this plan

This plan was made from the repository and the four upstream pages named at the top. The build and tests that run
without game data were run on this branch's base, `ec8ffe9`, for the companion plan (`docs/plans/next-typed-family.md`,
section 6, branch `claude/plan-next-typed-family`); the results are recorded there and summarised here:

| Check (what CI runs) | Result here |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| `cargo build --locked --workspace` (dev profile, with debug info) | passed, 3 min 35 s |
| `cargo test --locked --manifest-path scripts/ledger/Cargo.toml` | passed, 22 tests |
| `cargo test --locked --workspace --no-fail-fast`, run as CI does (`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`), `-j 2` | 110 test binaries: **56,388 passed, 6 failed**, 47 ignored, in 64 min |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` (same settings, `-j 2`) | fails on Linux, in `engine-platform` only: 7 warnings in the library and 1 more in its tests, items and variables only the Windows paths use (`files.rs` 5, `d3dx.rs`, `d3d9resource.rs`, `d3d9device.rs`). Every other crate is clean (the warnings-allowed run finished in 6.5 min). |

The six test failures, none in code these plans touch, are Linux-specific or a race (CI runs only on Windows):

- `engine-core`, `cxx_exception::tests::a_stop_does_reach_the_panic_hook`: **flaky**. It passed 3 of 3 times alone
  and failed 2 of 3 runs of the whole `engine-core` lib suite (`left: 2, right: 1`). Its counting panic hook can end up
  chained on another counting hook when `static_init`'s test swaps the global hook and puts an older one back, so one
  panic counts twice. A test race, not an engine bug.
- `engine-platform`, three `files.rs` searches (`a_search_whose_folder_changed_since_it_began_stops`,
  `a_wildcard_search_lists_then_runs_out`, `find_close_after_a_search_that_found_nothing_answers_false`): `FindFirstFileA`
  stops with "file times are only provided on Windows" (`files.rs:730`, a `cfg(not(windows))` panic).
- `engine-platform`, `a_read_only_open_reads_the_players_file_in_place`: opens `CARGO.TOML` expecting Windows'
  case-insensitive lookup; Linux's file system is case-sensitive.
- `nvse`, `commands::math::tests::the_results_match_the_real_game_bit_for_bit`: `tan` and `tanh` give other last bits
  on Linux (`dTan90`: -2.2877334e7 against the real game's -2.2877332e7; `dTanh30` likewise): the commands call Rust's
  `f32::tan`/`tanh`, which use the platform's maths library (the MSVC runtime on Windows, glibc here).

Two notes for anyone repeating this on a 4-core, 15 GB Linux machine: four parallel rustc jobs ran out of memory on
`engine-havok`'s test build (SIGKILL), so use `-j 2`; and the first build with debug info and incremental state used
up the session's disk (about 30 GB free at the start), so set `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` as
Windows CI does.

## 11. Open questions for Patrick and the maintainer

1. **Hardware:** a Linux machine or Steam Deck with Vulkan and the game, for L1's play tests? (Decides #1.)
2. **Upstream or fork-only?** Is the fork's work meant to go upstream? If yes, each PR follows CONTRIBUTING and #268's
   one-part-per-PR rule; if no, the licence caution above applies to anything from xNVSE, JIP or fnv_decomp.
3. **The window off Windows** (for the maintainer, on #268): where `winit` lives now that the Bevy viewer is gone and
   `wgpu` is in the root workspace, and who owns the event loop.
4. **The Linux CI job's size:** acceptable on the standard runner with `-j 2`, or does the maintainer prefer a larger
   runner or a job that runs only the core crates?
5. **JIP:** would the maintainer take JIP's I-13 from a contributor, and in what order?
6. **Post-processing** (AA, upscaling): welcome in `d3d9-wgpu` as a switch, off by default, outside the files being
   tuned?
