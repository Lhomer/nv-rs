//! The spine top's peak for a chest hit at several frame times (the solver's dt-consistency).
use ragdoll_core::np::Np;
use ragdoll_lab::np_lab::*;
fn main() {
    let speed: f32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0.32);
    let (model, anim, floor) = bind_pose();
    let spine = model.body("Bip01 Spine2").unwrap();
    for dt in [0.004f32, 0.008, 0.016, 1.0 / 60.0, 0.033] {
        let mut np = Np::new(model.clone(), &anim, 1.0);
        let mut peak = 0.0f32;
        let n = (1.5 / dt) as usize;
        for i in 0..n {
            if i == (0.1 / dt) as usize {
                let at = ragdoll_core::flinch::add(anim[spine].1, [0.0, 8.0, 4.0]);
                np.hit(spine, at, [0.0, -speed * model.mass, 0.0]);
            }
            let d = np.frame(&anim, floor, dt);
            peak = peak.max(
                ragdoll_core::np_skeleton::angle_between(&d[spine].0, &anim[spine].0).to_degrees(),
            );
        }
        println!(
            "dt {dt:.4}: spine peak {peak:.2} deg, steps {}",
            np.balance.steps
        );
    }
}
