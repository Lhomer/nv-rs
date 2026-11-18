use ragdoll_lab::np_lab::*;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let speed: f32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(3.0);
    let wall: f32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let rise: f32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(4.0);
    let effort: f32 = args.get(4).and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let (_, anim, _) = bind_pose();
    let scene = Scene {
        walls: if wall > 0.0 {
            vec![([0.0, -wall, 0.0], [0.0, 1.0, 0.0])]
        } else {
            vec![]
        },
        rise_at: if rise > 0.0 { Some(rise) } else { None },
        effort,
        keep_down: std::env::var_os("NO_KEEP").is_none(),
        retry: std::env::var_os("RETRY").is_some(),
    };
    let deg: f32 = std::env::var("DIR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let a = deg.to_radians();
    let dir = [a.sin(), -a.cos(), 0.0];
    let hits = vec![NpHit {
        at: 0.2,
        body: "Bip01 Spine2",
        offset: [-dir[0] * 8.0, -dir[1] * 8.0, 4.0],
        speed,
        dir,
    }];
    let run = run_scene("probe", &hits, 12.0, &|_| anim.clone(), &scene);
    let root = 0;
    for (i, f) in run.frames.iter().enumerate() {
        let from: f32 = std::env::var("FROM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2.5);
        let to: f32 = std::env::var("TO")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(99.0);
        let every: usize = std::env::var("EVERY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);
        if i % every == 0 && f.t >= from && f.t <= to {
            println!(
                "t {:5.2} label {:9} phase {} pelvis z {:6.1} y {:6.1} com ({:6.1},{:6.1},{:5.1}) floorN {:6.0} wallN {:5.0} lift {} inwall {:4.1} low {:5.1} gap {:4.2} clamp {} past {:5.1} rise {} over {:5.2} tilt {:3.0}",
                f.t, f.label.unwrap_or("-"), f.phase, f.nodes[root].1[2] - run.floor, f.nodes[root].1[1],
                f.com[0], f.com[1], f.com[2] - run.floor, f.floor_load, f.wall_load, f.lifting as u8, f.in_wall, f.lowest, f.gap, f.clamped, f.past_limit, f.rising as u8, f.over_feet, f.tilt
            );
        }
    }
}
