//! How often a downed body stands when asked (lab sweep over landing poses): hits of several
//! speeds and directions, the AI asking to stand 2.5 s after; and asking again 1 s after a rise
//! gives up when `RETRY` is set.
use ragdoll_lab::np_lab::*;
fn main() {
    let retry = std::env::var_os("RETRY").is_some();
    let (_, anim, _) = bind_pose();
    let (mut ok, mut n) = (0, 0);
    for speed in [3.8f32, 4.35, 5.0] {
        for deg in [-30.0f32, -10.0, 0.0, 15.0, 35.0, 180.0] {
            let a = deg.to_radians();
            let dir = [a.sin(), -a.cos(), 0.0];
            let hits = vec![NpHit {
                at: 0.2,
                body: "Bip01 Spine2",
                offset: [-dir[0] * 8.0, -dir[1] * 8.0, 4.0],
                speed,
                dir,
            }];
            let scene = Scene {
                walls: vec![],
                rise_at: Some(2.7),
                effort: 1.0,
                keep_down: true,
                retry,
            };
            let r = run_scene("sweep", &hits, 12.0, &|_| anim.clone(), &scene);
            let fell = r.frames.iter().any(|f| f.label.is_some());
            let stood = r
                .frames
                .iter()
                .find(|f| f.t > 2.7 && f.label.is_none())
                .map(|f| f.t - 2.7);
            let end = r.frames.last().unwrap();
            n += 1;
            if !fell || stood.is_some() {
                ok += 1;
            }
            println!(
                "speed {speed} dir {deg:4}: fell {fell} stood {stood:?} end {:?} pelvis {:.0}",
                end.label,
                end.nodes[0].1[2] - r.floor
            );
        }
    }
    println!("stood {ok} of {n}");
}
