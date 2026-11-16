# The Android test build

A barebones Android build of the real engine, for trying the active ragdoll's physics on a
phone (Patrick, 2026-11-15: Samsung Galaxy Z TriFold, arm64 Snapdragon, Vulkan). It boots
straight into the phone's Test Range (`crates/host/src/phone_range.rs`): one invincible target,
a railing, a half-wall, a table and a low obstacle for the world interaction, five weapons with
ammo kept topped up, and touch controls. It is a test build: the game draws, plays and reacts as
on Windows, but there is no sound, no text on the buttons, and no way to reach the rest of the
game from it.

**Not tried on a phone yet.** It is built and packaged by CI, but no device or game files exist
where it was made. Its first runs will tell what still stops it: send `nvrs-log.txt` (below).

## Get it

The APK, rebuilt from the branch by CI (`.github/workflows/android-test.yml`):

https://github.com/Lhomer/nv-rs/releases/download/android-test/nv-rs-android-test.apk

Install it on the phone: open the link in the phone's browser, open the downloaded file, and
allow the browser to install unknown apps when Android asks (Settings, Apps, the browser,
Install unknown apps). Or from a PC with USB debugging on: `adb install -r nv-rs-android-test.apk`.
Each new APK is signed with the same test key (kept in the release, `nvrs-test.keystore`,
password `nvrs-test`), so it installs over the last one and keeps the files in the app's folder.

## Copy your game files

The app reads your own Fallout: New Vegas 1.4.0.525 files (the Windows install's, the same
`FalloutNV.exe` the Windows play build uses). None are in the APK or the repository, and the
app never writes to them, nor to any save: the game's own writes stay in memory, and the range
refuses every save.

Make a folder named `FalloutNV` holding, from your Windows install (`...\steamapps\common\Fallout New Vegas`):

```
FalloutNV/
  FalloutNV.exe
  Fallout_default.ini
  Data/
    FalloutNV.esm
    Update.bsa
    Fallout - Meshes.bsa
    Fallout - Textures.bsa
    Fallout - Textures2.bsa      (the fonts are in it)
    Fallout - Misc.bsa           (the menus are in it)
    Shaders/                     (the whole folder)
```

Optional, not used yet (no sound): `Data/Fallout - Sound.bsa`, `Data/Fallout - Voices1.bsa`,
`Data/Music`. Leave out the DLC and mods: the range needs only the base game. About 3.5 GB.

Put the folder in one of these places (the first found is used):

1. **`/sdcard/NVRS/FalloutNV`** (`Internal storage/NVRS/FalloutNV`, the easy one to reach from a
   PC or a file manager). The app needs **All files access** for it. This is a special access,
   not a permission: the app's Permissions page says "No permissions required", which is right.
   The app asks by itself: on its first start without the game it opens the All files access
   page for NV-RS Test. Switch it on and go back. To find it by hand: Settings, Apps, Special
   access (Samsung: the menu at the top right of Apps), All files access, NV-RS Test.
2. **The app's own folder**, no access needed:
   `Internal storage/Android/data/io.github.lhomer.nvrs.test/files/FalloutNV`. Many file managers
   hide `Android/data` (a PC over USB still shows it). Install and open the app once first, so the
   folder exists. Uninstalling the app deletes it (an update does not).

## Play

Open NV-RS Test (landscape). Without the game files it shows what it looked for instead of
starting: each place it checked and what is missing there, whether All files access is on (tap
the screen to open its page again), what to copy, and where the log is. Once it finds the files
it starts by itself. If it says it found them but does not start, close and open it again. The
game: the main menu (the start-up movies and
logos are skipped), then the Test Range, which takes a minute or two to load on first start.
The target stands 6 m ahead; the obstacles are behind it, so shots push it into them.

The controls (see-through shapes over the game; no text):

| Where | Shape | Does |
| --- | --- | --- |
| Left half | ring and knob | Move (W, A, S, D): put a thumb anywhere on the left half and lean |
| Right half | anywhere not on a button | Look: swipe |
| Right, large red | red disc | Fire (hold for automatic weapons) |
| Right, blue | blue disc | Aim (hold) |
| Right, yellow | yellow disc | Reload |
| Bottom middle, white | white disc | Jump |
| Right, green | green disc | Next weapon: 9mm pistol, hunting shotgun, hunting rifle, anti-materiel rifle, sledgehammer |
| Top right, purple | purple disc | Reset the target: a fresh one where the first stood, by the obstacles |
| Top right, grey | grey disc | The range menu (Home): weapons, ammo, hostile or passive targets, back to the main menu |

The target is essential and has 100,000 health, healed every five seconds: it can be knocked
down, staggered and pushed into the obstacles, never killed. Ammo is topped up every five
seconds.

## When it does not work

The app writes its log to `nvrs-log.txt` in **Download** (`Internal storage/Download`, which any
file manager shows; Android lets an app make its own files there without a permission). If it
cannot, the log goes in the app's own folder, and the screen names where it is. Send it: it names
what stopped the run. With no game files the screen itself says where the app looked and what is
missing.

## How it is built

- `android/`: the app (`nvrs_android`, a NativeActivity through `android-activity`), its own
  workspace so the main workspace and the Windows play build are untouched. It runs the room's
  game on a thread of its own (`host::phone_range::run`, shared with the Linux room,
  docs/LINUX.md); the activity's thread hands over the native window and the touches.
- `crates/engine-platform`: `cfg(wgpu_window)` (build.rs: Windows, Android and Linux) forwards the
  game's Direct3D 9 calls to wgpu (`d3d9real.rs`) on Android too, with the activity's native
  window (`set_native_window`) and the touches (`inject_key`, `inject_button`, `inject_motion`) in
  place of the Win32 window and its keyboard and mouse. The real Direct3D 9 parts stay Windows only.
- `crates/d3d9-wgpu`: the surface on the native window (`surface_for`, `client_size`) and the
  touch controls drawn over the frame (`touch_overlay`).
- `crates/host/src/phone_range.rs` and `test_range.rs`: the phone's range (on with
  `phone_range::set_on`, which the app calls, or `NV_TEST_RANGE_PHONE=1`).
- CI packages it with `cargo-apk` 0.10 (NDK from the runner), signs it with the release's test
  key and publishes the APK at the link above on each successful build of a `claude/**` branch.
