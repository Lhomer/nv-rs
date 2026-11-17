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
    if which == "m2" || which == "all" {
        for v in [0.8f32, 2.0] {
            let r = run(
                &format!("New Physics M2: chest hit {v} m/s, a catching step"),
                &chest_hit(v),
                3.5,
            );
            scenarios.push((
                format!("np_m2_push_{}", format!("{v}").replace('.', "_")),
                r,
                0.2,
            ));
        }
        let side = vec![NpHit {
            at: 0.2,
            body: "Bip01 Spine2",
            offset: [-8.0, 0.0, 4.0],
            speed: 0.8,
            dir: [1.0, 0.0, 0.0],
        }];
        scenarios.push((
            "np_m2_side_0_8".into(),
            run("New Physics M2: side push 0.8 m/s", &side, 3.0),
            0.2,
        ));
        let w = walk(101.0, 0.35);
        for v in [0.8f32, 2.0] {
            let hits = vec![NpHit {
                at: 1.0,
                body: "Bip01 Spine2",
                offset: [0.0, 8.0, 4.0],
                speed: v,
                dir: [0.0, -1.0, 0.0],
            }];
            let r = run_on(
                &format!("New Physics M2: hit {v} m/s while walking"),
                &hits,
                3.0,
                &w,
            );
            scenarios.push((
                format!("np_m2_walk_{}", format!("{v}").replace('.', "_")),
                r,
                1.0,
            ));
        }
    }
    for (file, run, hit_at) in scenarios {
        let rc = recoil(&run, hit_at);
        let summary = format!(
            "peak {:.1} deg (spine top), head {:.1}",
            rc.world_peak, rc.head_peak
        );
        let phases = ["standing", "stepping", "recovering", "falling"];
        let json = frames_json(&run, |f| {
            format!(
                "t {:.2} s  spine {:4.1} deg  {}  steps {}  moved {:.0} u  |  {}",
                f.t,
                f.spine_world,
                phases[f.phase as usize],
                f.steps,
                ragdoll_core::flinch::length(f.moved),
                summary
            )
        });
        let path = format!("{out}/{file}.json");
        std::fs::write(&path, json).expect("write");
        println!("{path}");
    }
}
