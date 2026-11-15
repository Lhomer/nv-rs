//! `cfg(wgpu_window)`: the targets where the game's Direct3D 9 calls are forwarded to a backend
//! that draws into a window of the system's (`d3d9real.rs`, `d3d9_wgpu`): Windows (the real
//! Direct3D 9 or wgpu) and Android (wgpu on Vulkan, in the activity's native window; the Android
//! test build, docs/ANDROID.md). Elsewhere the layer only models.

fn main() {
    println!("cargo:rustc-check-cfg=cfg(wgpu_window)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "windows" || os == "android" {
        println!("cargo:rustc-cfg=wgpu_window");
    }
}
