# Plan: the next stage-1 typed families, the step after the frame is shown

Status: a proposal for the maintainer (2026-11-11). Planning only: no engine code changed with it. Nothing
here was measured in the game; the figures marked *estimate* are worked out from the translations' code and
need step 0 below to confirm them.

Read with [ADR-0012](../adr/0012-modern-64-bit-engine.md), [ADR-0014](../adr/0014-typed-migration-in-two-stages.md),
[ADR-0015](../adr/0015-hand-typing-to-the-end-with-real-threads.md), [ADR-0016](../adr/0016-script-extenders-built-in.md)
and [ENGINE_CRATE.md](../ENGINE_CRATE.md), "Typed code".

## In short

- **The step.** `Main::PostSwapProcess` (`008705d0`), which main runs after `Main::Swap` (stage 7 of
  [FRAME_SKELETON.md](../FRAME_SKELETON.md)). It has four parts: the sound (`0086f640`), the process lists
  (`0086f890`), the queued node updates (`00552570`, **already typed** in `scene`), and the scripts
  (`0086f670`, which ends in `00455490`).
- **No call counts for this step are in the repository.** The dispatch family's list
  (`measure/typed/hand/batchD1.tsv`) and the profiler's tables are kept outside it. So step 0 is a profile
  of this step, and the order below is the expected order, to be confirmed or changed by that profile.
- **Proposed families, in expected order of speed gain:**
  1. `script_pass`: the frame's walk over quests, the loaded cells' references and the player that decides
     which scripts run (`00455490` down to each reference's `RunScript`), but not the interpreter.
  2. `proc_lists`: the per-frame passes over process levels 1, 2 and 3 (`0096d810` with `0096b810`,
     `0096b470`, `0096b050` and their helpers), but not the actors' own updates.
  3. `script_eval`: the expression evaluator and the interpreter's small helpers that **no built-in wraps**
     (`CExpression::Eval` `00593db0` and its operand stack, `ScriptCompiler::GetFunctionDef`,
     `ScriptLocals::GetVariable`, `Script::GetReferencedObject` and the number tests).
  4. `audio_step`: the sound's per-frame update. Recommended **not now**: little to gain, and the sound's
     bit-exact decision applies to all of it.
- **Recommended first family: `script_pass`**, if step 0 shows the reference walk is as large as its code
  suggests (below); otherwise `proc_lists` first, which is the smallest and safest of the four.
- **The script interpreter's spine stays untyped for now.** `Script::Run`, `ScriptRunner::Run`, the
  `InitActionList` step, `ParseLine`, `EvaluateLine`, the operand reader and `ParseParameters` are all in
  `overridable.tsv`, and six of them are wrapped by the built-in xNVSE in every play run. A wrap clears the
  direct flag of the family that holds the address **for good** (`Engine::wrap` → `put` →
  `typed.replaced`), so any of them inside a family would turn that whole family's direct calls off in play.
- **Blocking:** nothing in the code. The speed gates and the recorded runs need Windows and an owned game
  install, which this planning session did not have.

## 1. The step and its functions

### Where it is

From `research/engine-map/frame.tsv` (call tree of `Main::OnIdle`, depth 3) and the translations:

| Exe | Name (Xbox PDB, engine map) | What it does | Typed today |
| --- | --- | --- | --- |
| `008705d0` | `Main::PostSwapProcess` | Calls `0086f640`, `0086f890`, `00552570` (threads > 1), `0086f670` | no |
| `0086f640` | `Main::OnIdle_UpdateAudio` | `BSAudio::QInstance` `00453a70`, `BSAudio::Update` `00ad7740`, `FalloutRadio::Update` `00832ad0`, `FalloutAudioMedia::Update` `0082fb70`, `FalloutAudio::UpdateRegionSounds` `0082d7c0` | no |
| `0086f890` | `Main::OnIdle_UpdateProcessLists` | One processor only: `ProcessLists::ChangeProcessLevelTempList` `0096eb40`, `AITaskManager::UpdateTasks` `008c94e0`, `CombatManager::UpdateCombatants` `00991dc0`. Always, while the world steps: `ProcessLists::UpdateProcessLists` `0096d810`. Else, in a menu or with time frozen: `PlayerCharacter::UpdateAutoAimActor` `00964260` | no |
| `00552570` | (the queued node updates) | The scene graphs the AI threads changed | **yes**, `scene` (id 19) |
| `0086f670` | (unnamed) | Clears three bytes, the condition functions' caches (`005a38e0`), then the frame's script pass `00455490` | no |

In play the game's processor count is above 1, so the three one-processor calls of `0086f890` run on the
AI threads instead and are not part of this step. The per-actor work of the high level (level 0) is not
here either: `ProcessLists::RunActorUpdates` (`0096c7c0`) and the movement pass (`0096db30`) run on the AI
threads ([FRAME_SKELETON.md](../FRAME_SKELETON.md), "The functions").

### How hot (what the repository has, and an estimate)

What is in the repository: the call tree and the function sizes (engine map), the translations, and the
per-call saving measured for the first family: the `leaves` gate removed 209,900 calls by address a frame
for 1.45 ms, **about 6.9 ns a call** (docs/HANDOFF.md, "Typed leaves, step T2"). No profile row of any
function below is committed; `NV_PROFILE`'s tables and the dispatch batch list live outside the repository.

The *estimates*, from what each loop calls by address for each item it visits (a typed caller turns the
calls into Rust calls where the callee is typed and not overridable):

| Part | Per item | Items a frame | Calls by address a frame (*estimate*) | At 6.9 ns |
| --- | --- | --- | --- | --- |
| Script pass: references walked by `TESObjectCELL::RunScripts` | 2 (`TESObjectREFR::Get3D` `0043fcd0`, `RunScript` `00565870`) plus about 6 to 10 inside `RunScript` (`00418800`, `00418830`, `0042ede0`, `0055d310`, `004bf220`, the slots `+0xfc`, `+0x48`) | Each loaded cell's references with 3D, until a script answers that it ran: from a handful up to every reference of the 5x5 grid when none does | from about 100 up to about 50,000-100,000 | up to about 0.35-0.7 ms |
| Script pass: quests | about 3 (`00455620`, `0060d4d0`, then `Script::Run` by address) | every quest of the data handler's list (hundreds with the add-ons) | about 1,000-3,000 | about 0.01-0.02 ms |
| Process levels 1 and 2 | about 6 (`00968670`, `00931850`, `007df1f0`, `00867da0`, slot `0x100`, the leaves) | every actor at levels 1 and 2 | low thousands | about 0.01-0.03 ms |
| Process level 3 | about 5, and a clock read every fifth actor | up to the time budget (`011e0e64`) | depends on the budget, not on speed: a faster loop does more actors in the same time | none (see risks) |
| Sound | a few hundred calls in all | once a frame | hundreds | under 0.01 ms |

The script pass's range depends on one thing the code says and nobody has counted: `TESObjectCELL::RunScripts`
(`0054c740`) stops at the first reference whose `RunScript` answers true, and `00455490` stops at the first
grid cell whose walk did. If scripted references usually answer true, the walk is short; if they usually
answer false (no `GameMode` block, a quest delay, a script of 4 bytes or fewer), it visits every reference
with 3D in the loaded grid every frame. Step 0 answers it in one profile.

So in this step only the script pass could plausibly be worth one or two per cent of a frame; the process
lists and the sound are well under that. That fits the update half so far: `scene`, `particles`,
`collision` and `multibound` were each "not a speed change by itself". The process lists' and the
evaluator's value is more in ADR-0015's other aim (the code the game's other threads run, typed and
checkable) than in fps.

### The functions, by family

Sizes are the engine map's, in bytes. "Ovr" is a row in `crates/engine-core/data/overridable.tsv`;
"wrapped" is a row of `engine_core::extend::WRAPS`, installed in every play run (ADR-0016).

**`script_pass`** (all `engine-fallout-cell` and `engine-fallout-script` translations today):

| Exe | Name | Size | Note |
| --- | --- | --- | --- |
| `0086f670` | the step's script part | 48 | clears `012682f8`, `011df678`, `011dea2a` |
| `005a38e0` | the condition functions' cache clear | 147 | fifteen words |
| `00455490` | the frame's script pass | 367 | **Ovr** ("script or frame function"), not wrapped: typed, reached by address only |
| `00455600`, `00455620`, `00450ff0`, `00450fd0` | the quest list head, the quest test, the cell test and its getter | 19, 31, 29, 18 | |
| `005ac740` | `Script::GetProcessScripts` | 10 | |
| `0060d4d0` | the quest's run | 57 | calls `Script::Run` by address |
| `0054c740` | `TESObjectCELL::RunScripts` | 370 | the reference walk; its list helpers (`first_node`, `node_item`, `node_next`) come with it |
| `0056a380` | `TESObjectREFR::UpdateChildActivates` | 240 | its `TESObjectREFR::Activate` (`00573170`, 1,602, with an xNVSE seam at `0057318e`) stays by address |
| `00565870` | `TESObjectREFR::RunScript` | 377 | also called by the process lists, the AI task manager and the actors: **both threads** |
| `00418800`, `00418830`, `0042ede0` | `ExtraDataList::GetScript`, `GetScriptLocals`, `GetSayToExtra` | 42, 42, 29 | |
| `0055d310` | `TESObjectREFR::HasContainer` | 157 | |
| `004bf220` | `InventoryChanges::GetInventoryChanges` | 262 | |
| `004d2480` | `InventoryChanges::RunScripts` | 581 | a container's items' scripts; batch 2 |

Already typed and called with `typed_call!`: `0043fcd0` (`dispatch`), `0040fbf0`/`0040fba0` (the spin lock,
`dispatch`), `00440da0`, `006815c0`, `00726070`, `008d8520`, `007af430` (`leaves`), `00483710` (`dispatch`).
Stays by address: `Script::Run` `005ac1e0` (**Ovr**), and through it the whole interpreter.

**`proc_lists`** (`engine-fallout-ai` and `engine-fallout-misc` translations):

| Exe | Name | Size | Note |
| --- | --- | --- | --- |
| `0086f890` | `Main::OnIdle_UpdateProcessLists` | | the one-processor branch's three calls stay by address |
| `0096d810` | `ProcessLists::UpdateProcessLists` | 789 | with its body (the timers, the calendar words, the frame-rate skip, the crime timer) |
| `0096b810` | level 1's update | 935 | |
| `0096b470` | level 2's update | 915 | |
| `0096b050` | level 3's update | 1,056 | the time budget: `QueryPerformanceCounter` (import `00fdf0a0`), `__allmul` `00ecc300`, `__alldiv` `00ecc250` |
| `005de0f0`, `005be5c0` | a level's first and end index | 23, 23 | |
| `00931850` | `MobileObject::GetCurrentProcessType` | 36 | |
| `007df1f0` | the process's last-update hour | 17 | |
| `00867da0`, `00867c60`, `00867d20`, `00867d60` | `Calendar::GetHour`, the year, month and day | 50 and small | |
| `0096a2d0`, `005ae3d0`, `00470470`, `004702f0`, `0096aaf0` | the collected actors' list: make, add, clear, free, leave the level | small | |
| `0092f190` | the "never updated" path | 39 | |
| `00907650` | `LowProcess::ComputeLastTimeProcessed` | 66 | |
| `00404eb0`, `00404ee0` | the profiling scope (`ProcessLists.cpp` line 0x521) | 41, 19 | |

Already typed: `00968670` (`dispatch`), `00576d30`, `00440da0`, `00440d80`, `008d8520`, `0045cd60`, `00437bb0`
(`leaves`), `0043d4d0` (`multibound`). Stays by address: every actor's own update through its vtable (slot
`0x25c`, the process's slot `0x28`, `0x1ec`), `00977cb0`, the follower barks `008d5bc0`, `008d1560`, the
destructible objects `004772f0`, the crime update `009723f0`, and `00565870` until `script_pass` is in.

**`script_eval`** (`engine-fallout-forms` and `engine-fallout-script`):

| Exe | Name | Size | Note |
| --- | --- | --- | --- |
| `00593db0` | `CExpression::Eval` | 3,398 | an xNVSE seam at `005949d4` (`e.site`), kept in place |
| `00593490`, `00594b60`, `00594c60`, `00594cd0`, `00594d20`, `00594d40`, `005934d0` | the expression's constructor, its operator and operand stacks (make, pop, empty, clear), the error | small | |
| `004b1230`, `004b12d0` | `IsAnInteger`, `IsAFloat` | 150, 231 | `strtol` `00eca6bd` and `atof` `00eca573` stay by address (C runtime) unless `crt` takes them |
| `005b1120` | `ScriptCompiler::GetFunctionDef` | 96 | reads `ScriptTables` (ADR-0016 option A): the typed read keeps it |
| `005a9140` | `ScriptLocals::GetVariable` | 332 | |
| `005ac4f0` | `Script::GetReferencedObject` | 430 | |

Not in any family (all **Ovr**, the wrapped ones marked): `Script::Run` `005ac1e0`, `ScriptRunner::Run`
`005e0d20` (wrapped), `InitActionList` `005e1160` (wrapped), `ParseLine` `005e13a0` (wrapped),
`EvaluateLine` `005e1550` (wrapped), the operand reader `005ac7a0` (seam `005acb9d`), `ParseParameters`
`005accb0` (wrapped), `00593870`, `ScriptEventList::SetActionFlag` `005ac750` (wrapped), the event list's
destructor `0041af70` (wrapped). A later step can type their bodies in a family of their own that is
expected to run by address in play (as `animation`'s `UpdateAllButTarget` is), once the gain is shown.

**`audio_step`** (`engine-bs` and `engine-fallout-misc`): `0086f640`, `BSAudio::Update` `00ad7740` and
`BSAudioManager::Update` `00ada1d0`, `FalloutRadio::Update` `00832ad0` with `ClearInactiveReceivers`
`008329a0` and `UpdateStations` `00833d00`, `FalloutAudioMedia::Update` `0082fb70` and
`FalloutAudio::UpdateRegionSounds` `0082d7c0` (4,783). About 130 functions to depth 3, one in
`overridable.tsv` (`00595560`).

## 2. Each family: what it touches, and the risks

### `script_pass` (proposed id 28)

- **Guest objects:** `TES` (its interior cell, `pGridCellA`), the grid cells' `TESObjectCELL` and their
  loaded data (`+0x4c` the references, `+0x54` the activate list), `TESObjectREFR` with its `ExtraDataList`
  (script, locals, say-to, container changes, activate children and timer), `TESQuest`, `Script`'s header,
  the reference lock and the script pass's lock (`dispatch`'s spin lock).
- **Other families:** calls `dispatch` (`0043fcd0`, the spin lock, `00483710`) and `leaves` directly; the
  geometry and scene families not at all. The cell walk's layouts may share `multibound`'s `TES` and
  `GridCellArray` layouts (`crates/engine-typed/src/multibound`): reuse them, don't declare them twice.
- **Overridable:** `00455490` is the family's own root and is overridable: it is registered at its address
  and called only by address (as now: `0086f670` and the cell code call it by address), and the family's
  members never `typed_call!` it. `Script::Run` and `TESObjectREFR::Activate` stay `e.call`. The
  translations' frozen direct-call counts don't move.
- **Floats:** none in the walk itself (the quest delay maths is in `Script::Run`, by address). The family
  can stay `Threads::Both` without cost.
- **Threads:** the pass runs on main; `RunScript` (`00565870`) is also reached from the AI threads (the
  task manager, the actors' and the process levels' updates), so the row is `Both`.
- **Risks:**
  - The walk's early stop is the whole speed question (section 1). Typing it doesn't change it: the exe
    stops there, and the translation does too.
  - The list walks hold the reference lock while calling `RunScript`, which runs scripts that may enable,
    disable or delete references (the delayed actions are queued, but `Activate` runs inline). A typed
    walk must read `next` exactly where the translation does (before the call), or a removal during the
    script reads a freed node.
  - A data race risk under real threads (ADR-0015's amendment): none new, the walk waits on no guest word.
  - xNVSE's events (`OnActivate` seam inside `Activate`) are reached by address, unchanged.

### `proc_lists` (proposed id 11)

- **Guest objects:** `ProcessLists` (`011e0e80`: the four levels' arrays, the comment and crime timers,
  `iNumberHighActors`, `fSecondsPassedNoProcess`, the system time clock, the process flags), the actors
  and their `BaseProcess` (`+0x68`), the calendar (`011de7b8`), the globals `011e0e30` (level 3's resume
  index), `011e0e34` (updates off), `011e0e20` (the counter frequency), `011e0e64` (the budget setting),
  the process lock `011f11a0`.
- **Other families:** `leaves`, `dispatch` (`00968670`), `multibound` (`0043d4d0`), and, once it is in,
  `script_pass` (`00565870` from `run_actor_update`).
- **Overridable:** none in the family or below it to depth 3.
- **Floats:** the hour tests compare `float`s stored from the calendar with `double` constants
  (`01070c80` 0.15 h, `010290d0` 0.3 h, `01012070` 1.0) and subtract frame times. If the profile with
  `NV_PROFILE_THREAD_CALLS` shows the members on main only, the row is `Threads::Main` and the rule is plain
  `f32` (ADR-0014 point 7); but a `double` constant loaded by `FLD qword` is not rounded by the 24-bit
  control word, so plain `f32` maths that rounds the constant first would compare differently. Keep the
  exact x87 helpers for these compares, as `scene`, `particles` and `collision` did for byte identity, and
  say why at each. Frame rate to integer (`1 / dt`) goes through `x87::fistp_chop` and its family, never
  `as`. The `__allmul`/`__alldiv` budget maths stays integer.
- **Threads:** `0096d810` is called from `0086f890` (main) and from `TES::TestCell` (`00459d20`, the
  test-cell command's load, main); the AI threads run level 0 and the movement pass, not these. Claim `Main`
  only with the thread-calls table.
- **Risks:**
  - **Level 3's time budget.** It reads `QueryPerformanceCounter` every fifth actor and stops at the
    deadline. With `NV_REPEATABLE` every clock reading advances the virtual clock by 1 us, so the typed loop
    must make exactly the same reads in the same order (by address, through the import) or the recorded run
    differs. In play a faster loop gets further through level 3 in the same budget, so it does more work,
    not less time: level 3 is for completeness, not speed.
  - The level loops call the actors' updates through their vtables (slots `0x100`, `0x25c`, `0x2ec`,
    `0x10`, the process's `0x28`, `0x1ec`): these stay `e.vcall`. If a later family types `Actor`'s or a
    process's update, the slot test (`engine_typed::slot_target`) makes the call direct then.
  - The collected-actor list is a guest block from `operator new` (`00401000`); its allocation order must
    stay the exe's (the heap reproduces the exe's pools).
  - The translation leaves out the exception frame that frees the list when its constructor throws
    (`Translation incomplete` on all three). Typed code keeps that gap and its marker note.

### `script_eval` (proposed id 12)

- **Guest objects:** `CExpression` (the 0x48-byte token buffer, the operator and operand stacks of 16-byte
  operands, the error code), `ScriptLocals` and their variable list, `Script`'s referenced-object list, the
  script command table (`0x01190910`, or wherever `ScriptTables` puts it).
- **Other families:** `crt` for the number parsers if `strtol`/`atof` become typed; `leaves`.
- **Overridable:** the evaluator calls the operand reader `005ac7a0` (**Ovr**) and, inside, the xNVSE seam
  at `005949d4`: both stay as they are (`e.call`, `e.site`). Every caller of `Eval` that matters is wrapped or
  overridable (`00593870`, `EvaluateLine`), so `Eval` itself is reached by address in play: the gain is only
  inside it (the operand stack's pushes and pops, the number tests). Measure before committing to it.
- **Floats:** the evaluator works in `double`. The translation computes in Rust `f64` (53-bit). The exe on
  main runs with the 24-bit control word (the update families' note), where each `double` operation rounds
  its mantissa to 24 bits; on the AI threads (package scripts) at 53 bits. So the translation already
  differs from the exe on main for any non-integer script maths. The family is `Threads::Both`. The
  differential test compares typed against the translation, which it would match with `f64`; matching the
  exe needs per-thread precision (the exact helper under the main thread's word). This is an open question
  for the maintainer (below), and the reason the family isn't first.
- **Risks:** the largest functions of the four (`Eval` 3,398 bytes, with the token buffer's
  overrun-into-"skip push" behaviour the translation documents); the ADR-0016 descriptor reads must stay
  reads of `ScriptTables`, never the exe's immediates.

### `audio_step` (not proposed now)

- Touches `BSAudioManager`, the radio's stations and receivers, the music media, the acoustic spaces and
  region sounds. It decides which sounds start, stop and fade, and their volumes, which reach the mixer: the
  standing bit-exact sound decision (ADR-0012 consequences, ADR-0014 point 7) keeps its x87 modelling, and
  its gate is the `NV_SOUND_RECORD` files of two `NV_REPEATABLE` runs compared with `cmp`.
- Gain: hundreds of calls a frame, under 0.01 ms (*estimate*). Defer it to a sound family with a reason
  (threads: the mixer's own), not this speed work.

## 3. Test plan (ADR-0014 point 6)

### Differential tests (no game data; run anywhere, CI included)

One file per family under `crates/engine/tests/`, as the update half's families have:

| File | What it builds | Covers |
| --- | --- | --- |
| `typed_script_pass.rs` | `TES` with an interior cell or a 5x5 grid of cells (`GridCellArray`, as `typed_multibound.rs` builds), each cell's loaded data with reference lists (`+0x4c`, `+0x54`); references with and without 3D, flag `0x800`, deleted, without a base form, base types `0x2a`/`0x2b` with and without a process and 3D, a container, a script with and without locals, a say-to extra; a data handler's quest list; vtables whose slots log and answer | every member, both answers of every `RunScript`, the early stop in a cell and across cells, menu mode on and off for the activate list, the re-entry byte, `GetProcessScripts` false |
| `typed_proc_lists.rs` | `ProcessLists` with four levels of actors (null, `00576d30` set, the flags `0x800`/`0x20`, process types 1/2/3 and wrong ones, characters and not, updates disabled), a calendar, timers below and above zero, frame times giving rates above 50, between 10 and 50 and below 10 | every member; level 3's resume index at 0, inside and past the end; `all` set and clear; the budget reached at an index divisible by five; the crime timer's NaN branch (`JP` after `TEST AH, 0x41`) |
| `typed_script_eval.rs` | compiled expressions (integer and float operands, every operator, a division by zero with and without `parse`, too few operands, more than one left, a token of 72 bytes or more, an expression over 0x200 bytes), locals with each variable kind, referenced-object lists | every member, both control words |

Each test, as the harness requires:

- `.family(..)` for the family and every family its calls reach (`leaves`, `dispatch`, `multibound`, the
  family itself); `.goes_by_address(..)` for a member another test runs direct.
- Float inputs (`Input::F32`, `F64`) with the NaNs, zeros, denormals and huge values, and `float_sweep(64)` for
  `proc_lists`' timers and `script_eval`'s operands; integer inputs one per branch.
- `.expect_stops(..)` only where the exe stops too; a stop at an untranslated address means the setup lacks
  a double.
- Planted mutants: one per function (a branch inverted, a store moved across a call, a word count off by
  one, a rounding changed), each shown caught before the family's gate, as every update-half family did.
- `.translation_calls_plainly(..)` where a translation calls a member as a Rust function; where a member is
  called plainly by some sites and by address by others, the family's step moves the plain sites to `e.call`
  first, so the words compare.
- Hygiene (`crates/nvinspect/tests/repo_hygiene.rs`) passes: markers, registration, no `as` from float to
  integer, no overridable address in a `typed_call!`, a `Diff::new` per marked function.

### What runs without game data (Linux here, Windows CI)

`cargo fmt --all -- --check`, `cargo test --locked --workspace` (the differential tests and hygiene among
them), `cargo clippy --locked --workspace --all-targets -- -D warnings`, and the ledger's tests
(`cargo test --manifest-path scripts/ledger/Cargo.toml`). Regenerate `docs/LEDGER.md` by hand before the
merge (the typed column), since CI doesn't run `ledger --check`.

### What needs the game (Windows, an owned install, the maintainer's machine)

1. **The recorded street run, byte-identical:** the Goodsprings street with `NV_REPEATABLE=<frames>` and
   `NV_D3D9_RECORD`, once with the family on and once with `NV_TYPED=<family>:off`, compared with `cmp`, in
   the fixes preset and `NV_PRESET=vanilla`. For `proc_lists` the route must be long enough for some low
   level actor's hour test to fire (0.15 game hours is about 18 s at the default time scale); for
   `script_pass` it must pass scripted references and a container with scripted items. Add
   `NV_SOUND=out NV_SOUND_RECORD` to both runs: scripts start sounds, so the sound files must also compare.
2. **An interior run** (Doc Mitchell's house, Save 138) the same way: `00455490`'s interior branch.
3. **The Goodsprings speed gate:** the same exe, `NV_PRESET=vanilla NV_UNCAP=1`, `NV_TYPED` unset against
   `NV_TYPED=<family>:off`, BAAB, Goodsprings first (spread 0.75%), Doc's house reported. Expect `proc_lists`
   inside the noise (like `scene`): its gate is byte identity, with no speed claim. `script_pass` gets a
   speed claim only if step 0's count is large.
4. **Step 0, the profile** (before any code): `NV_PROFILE=1 NV_PROFILE_UNDER=008705d0` in Goodsprings and
   the crowded street, then `NV_PROFILE_UNDER=00455490,0096d810,0086f640` for the split, with
   `NV_PROFILE_THREAD_CALLS=<file>` for each family's `Threads` row. Record the calls a frame and self time
   in the PR and in this file.

## 4. Implementation order, in reviewable PRs

| PR | Content | Gate |
| --- | --- | --- |
| 0 | Step 0's profile, written into this file (no code) | the maintainer agrees the order |
| 1 | `script_pass` batch 1: the walk (`0086f670`, `005a38e0`, `00455490` by address, the quest helpers, `0054c740`, `0056a380`, `00565870` and its extra-data getters, `0055d310`, `004bf220`), off (`past_gate: false`), with `typed_script_pass.rs` and its mutants; plain sites of members in the translations moved to `e.call` first | CI green |
| 2 | `script_pass` batch 2: `InventoryChanges::RunScripts` `004d2480` and what it reaches | CI green |
| 3 | `script_pass` on by default after the recorded runs (street and interior, both presets) and, if claimed, the speed gate; MILESTONES, HANDOFF and ENGINE_CRATE entries | the maintainer's go |
| 4 | `proc_lists`: `0096d810` with its body, the three level updates and helpers, off, with `typed_proc_lists.rs`; `run_actor_update`'s `00565870` as a `typed_call!` into `script_pass` | CI green |
| 5 | `proc_lists` on by default after its recorded runs | the maintainer's go |
| 6 | `script_eval`, only after the precision question below is settled and step 0 shows calls worth saving | as above |

Each PR stays within one family's folder (`crates/engine-typed/src/<family>/`), its test file, one row in
`engine-typed/src/lib.rs`'s `provide` list, and the translations' call sites it moves to `e.call`. The
typed crate depends only on `engine-core`, so the translations keep calling members by address, which
reaches the typed code when the family is on.

### Conflicts with in-flight work

- **slaterain/nv-rs PR #473** ("Drawing thread, step 1": the world draw's lists detached into a view,
  `NV_DRAW_DETACH=1`, off by default; commit `816ed25` on `ec8ffe9`, the same base as this plan). It changes
  `crates/engine-typed/src/draw/*` and `shader/batch.rs` only. **No file overlap** with the families here.
  Both add a row near the end of `engine-typed/src/lib.rs`'s `provide` list only if a later step of it adds a
  family; a one-line merge at worst.
- **A conceptual one, for later:** once the world draw runs on its own thread, this post-swap step runs on
  main while the draw thread draws. The script pass enables, disables and activates references, which
  attach and detach scene graph nodes, and the queued node updates (`00552570`) already sit in this step.
  Neither stage 1 here nor #473's step 1 runs anything concurrently, so nothing conflicts now; the drawing
  thread's later steps must treat this step's writes to the scene graph as the frame's update, and the
  typed walk makes that list easier to read.
- **Lhomer/nv-rs:** no open pull requests (checked 2026-11-11). Upstream main and this fork's main were both
  at `ec8ffe9`.
- **Family ids:** the families in play use 0-10, 17-25 and 27; the core's own tests use 26, 27, 29 and 30 for
  made-up families (`typed/mod.rs`, `typed/moved.rs`). Proposed: `script_pass` 28, `proc_lists` 11,
  `script_eval` 12 (11-16 are unused in the crates as of `ec8ffe9`); check `typed::FAMILIES` at the time.

## 5. Open questions for the maintainer

1. **Profile first?** Is step 0 (one profiled Goodsprings and street run of this step) acceptable before
   any code, given that the order depends on how far the reference walk goes in a frame?
2. **The interpreter's spine.** Should the wrapped and overridable functions (`ScriptRunner::Run`,
   `EvaluateLine`, `ParseLine`, `InitActionList`, `ParseParameters`, the operand reader, `Script::Run`) be
   typed at all for now? In play they are wrapped by the built-in xNVSE, so they would run by address, and a
   family holding them loses its direct calls. The proposal leaves them out until there is a reason (stage 2
   of the script objects, or threads).
3. **The evaluator's precision.** On main the exe evaluates script `double` maths at 24 bits; the
   translation at 53. The audit left rounding differences to the migration. Should `script_eval` match the
   exe per thread (the exact helper under the main thread's word), keep the translation's `f64`, or wait for a
   recording that shows the difference matters?
4. **Level 3's budget under real threads.** A faster level-3 loop does more low-level actors per frame in
   the same budget. That is the exe's own behaviour on a faster PC; is it acceptable that typing changes how
   many low-level actors update a frame in play (not in `NV_REPEATABLE` runs, where the clock is virtual)?
5. **Sound.** Agree that `audio_step` waits for a sound family with its own reason (threads or stage 2),
   rather than being typed now for speed?
6. **Ids and names:** `script_pass`, `proc_lists`, `script_eval`, `audio_step` as `NV_TYPED` names, and the
   ids above?

## 6. Checks run for this plan (Linux, this session)

On `ec8ffe9` (upstream main), Linux x86-64, the pinned toolchain 1.99.1, without game data:

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

The plan itself was made from the repository only: `research/engine-map/frame.tsv` and `engine_map.tsv`,
the translations' markers and bodies (call targets read by a throwaway script over the `Translated from`
markers, not committed), `crates/engine-core/data/overridable.tsv`, `engine_core::extend::WRAPS` and
`SITES`, and `Engine::wrap`'s effect on the direct flags (`crates/engine-core/src/extend.rs`,
`engine.rs` `put`, `typed/mod.rs` `replaced`).
