//! Writes New Physics' lab scenarios' frames as JSON for `scripts/np_video.py`:
//! `cargo run --release --example np_frames -- OUT_DIR [m1|m2|all]`.
use ragdoll_lab::np_lab::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let which = args.get(2).cloned().unwrap_or_else(|| "all".into());
    std::fs::create_dir_all(&out).expect("the output folder");
    let mut scenarios: Vec<(String, NpRun, f32)> = Vec::new();
    if which == "m1" || which == "all" {
        for v in [0.3f32, 0.8, 2.0] {
            let r = run(
                &format!("New Physics M1: chest hit {v} m/s (recoil)"),
                &chest_hit(v),
                1.6,
            );
            scenarios.push((
                format!("np_m1_recoil_{}", format!("{v}").replace('.', "_")),
                r,
                0.2,
            ));
        }
    }
    for (file, run, hit_at) in scenarios {
        let rc = recoil(&run, hit_at);
        let summary = format!(
            "peak {:.1} deg (spine top), head {:.1}, back under 2 deg after {:.2} s",
            rc.world_peak, rc.head_peak, rc.back
        );
        let json = frames_json(&run, |f| {
            format!(
                "t {:.2} s   spine top {:5.1} deg   head {:5.1} deg   |   {}",
                f.t, f.spine_world, f.head, summary
            )
        });
        let path = format!("{out}/{file}.json");
        std::fs::write(&path, json).expect("write");
        println!("{path}");
    }
}
