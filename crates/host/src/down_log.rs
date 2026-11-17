//! The pose log's drawn get-up (Patrick's bdf710a review: the log had no drawn data in knock
//! states 1 to 6, so a get-up hoisted upside down could only be seen in the video). Every frame
//! an actor is down or getting up (`crate::knockdown`'s get-up, its watch included), after the
//! world pass has drawn it (`crate::active_ragdoll::after_draw`): its drawn pelvis, head and feet
//! over the floor it was set on, the torso's up-axis (pelvis to head, its height: 1 upright, -1
//! upside down) and the get-up animation's pelvis once the game's animation has the bones. The
//! clips sample it as `"down":{...}` each frame (`crate::room_record`); when the get-up is over,
//! a `[target get-up frames]` note sums it up against the review's targets.

use crate::room_events;
use engine::prelude::*;
use std::sync::Mutex;
use std::time::Instant;

type V3 = [f32; 3];

const WORLD_TRANSLATION: u32 = 0x8c;

/// The nodes measured: the pelvis, the head, the feet.
#[derive(Clone, Copy, Debug)]
pub struct Nodes {
    pub pelvis: u32,
    pub head: u32,
    pub feet: [u32; 2],
}

/// One frame as drawn (game units over the floor; the up-axis's height).
#[derive(Clone, Copy, Debug)]
struct Sample {
    state: i8,
    pelvis: f32,
    head: f32,
    feet: [f32; 2],
    up: f32,
    anim_pelvis: Option<f32>,
}

#[derive(Debug)]
struct Down {
    actor: u32,
    label: String,
    state: i8,
    floor: f32,
    nodes: Option<Nodes>,
    tracked: bool,
    since: Instant,
    sample: Option<Sample>,
    /// The sums: frames, the drawn pelvis's most over the animation's, the lowest node under the
    /// floor, the upside-down frames (the head more than 20 under the pelvis), the pelvis's
    /// largest move in a frame beyond the animation's, the torso's lowest up-axis.
    frames: u32,
    over_anim: f32,
    lowest: f32,
    inverted: u32,
    pop: f32,
    up_least: f32,
    last: Option<(V3, Option<V3>)>,
    /// The animation's pelvis's last step (the blend's end measured against it).
    anim_step: Option<V3>,
}

static DOWN: Mutex<Vec<Down>> = Mutex::new(Vec::new());

/// `actor` (for the log, `label`) is down or getting up this frame, in knock state `state`, set on
/// the floor at `floor`; `find` answers its nodes (asked once).
pub fn track(actor: u32, label: &str, state: i8, floor: f32, find: impl FnOnce() -> Option<Nodes>) {
    let mut downs = DOWN.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(d) = downs.iter_mut().find(|d| d.actor == actor) {
        d.state = state;
        d.floor = floor;
        d.tracked = true;
        return;
    }
    downs.push(Down {
        actor,
        label: label.to_string(),
        state,
        floor,
        nodes: find(),
        tracked: true,
        since: Instant::now(),
        sample: None,
        frames: 0,
        over_anim: f32::NEG_INFINITY,
        lowest: f32::INFINITY,
        inverted: 0,
        pop: 0.0,
        up_least: 1.0,
        last: None,
        anim_step: None,
    });
}

/// After the world pass (`crate::active_ragdoll::after_draw`): each tracked actor's drawn nodes
/// measured; `anim_pelvis` answers the get-up animation's pelvis (world) once it has the bones,
/// and whether the get-up's blend draws this frame (its own eased move is not a pop).
/// An actor no longer tracked is summed up and dropped.
pub fn measure(e: &Engine, anim_pelvis: impl Fn(u32) -> (Option<V3>, bool)) {
    let mut downs = DOWN.lock().unwrap_or_else(|p| p.into_inner());
    if downs.is_empty() {
        return;
    }
    let mut over = Vec::new();
    downs.retain_mut(|d| {
        if !d.tracked {
            over.push(summary(d));
            return false;
        }
        d.tracked = false;
        let Some(n) = d.nodes else {
            return true;
        };
        let at = |node: u32| read3(e, node + WORLD_TRANSLATION);
        let (pelvis, head) = (at(n.pelvis), at(n.head));
        let feet = [at(n.feet[0]), at(n.feet[1])];
        let (anim, blending) = anim_pelvis(d.actor);
        let torso = sub(head, pelvis);
        let tl = length(torso);
        let up = if tl > 1e-3 { torso[2] / tl } else { 1.0 };
        let s = Sample {
            state: d.state,
            pelvis: pelvis[2] - d.floor,
            head: head[2] - d.floor,
            feet: [feet[0][2] - d.floor, feet[1][2] - d.floor],
            up,
            anim_pelvis: anim.map(|a| a[2] - d.floor),
        };
        d.frames += 1;
        if let Some(a) = s.anim_pelvis {
            d.over_anim = d.over_anim.max(s.pelvis - a);
        }
        d.lowest = d
            .lowest
            .min(s.pelvis.min(s.head).min(s.feet[0]).min(s.feet[1]));
        if head[2] < pelvis[2] - 20.0 {
            d.inverted += 1;
        }
        d.up_least = d.up_least.min(up);
        // A pop: the pelvis's move beyond the animation's own, where that is known (its last
        // step carried a frame past the blend's end, the hand-over measured); not while the blend
        // eases it, nor while the ragdoll falls or lies (no animation).
        if let Some((was, was_anim)) = d.last {
            let step = match (was_anim, anim) {
                (Some(a), Some(b)) => Some(sub(b, a)),
                _ => None,
            };
            let own = step.or(if was_anim.is_some() {
                d.anim_step
            } else {
                None
            });
            if let Some(own) = own.filter(|_| !blending) {
                d.pop = d.pop.max(length(sub(sub(pelvis, was), own)));
            }
            d.anim_step = step;
        }
        d.last = Some((pelvis, anim));
        d.sample = Some(s);
        true
    });
    drop(downs);
    for line in over {
        room_events::record("note", &format!("\"text\":{}", room_events::text(&line)));
        eprintln!("{line}");
    }
}

/// The `[target get-up frames]` note of a get-up over.
fn summary(d: &Down) -> String {
    format!(
        "{}: [target get-up frames] {} frames drawn over {:.1} s down and getting up: the drawn \
         pelvis at most {} over the get-up animation's (target 10), the lowest of pelvis, head and \
         feet {:.1} on the floor (target -3 or over), {} frames upside down (target 0), the pelvis's \
         largest move in a frame beyond the animation's outside the blend {:.1} (target 2), the torso's up-axis at \
         least {:.2}",
        d.label,
        d.frames,
        d.since.elapsed().as_secs_f32(),
        if d.over_anim.is_finite() {
            format!("{:.1}", d.over_anim)
        } else {
            "- (the animation never had the bones)".to_string()
        },
        if d.lowest.is_finite() { d.lowest } else { 0.0 },
        d.inverted,
        d.pop,
        d.up_least
    )
}

/// The clips' sample of `actor`'s drawn get-up: `"down":{...}`, `"down":null` when it is not
/// down.
pub fn field(actor: u32) -> String {
    let downs = DOWN.lock().unwrap_or_else(|p| p.into_inner());
    let Some(s) = downs
        .iter()
        .find(|d| d.actor == actor)
        .and_then(|d| d.sample)
    else {
        return "\"down\":null".to_string();
    };
    format!(
        "\"down\":{{\"knock_state\":{},\"pelvis_over_floor\":{},\"head_over_floor\":{},\"feet_over_floor\":[{},{}],\"torso_up\":{},\"anim_pelvis_over_floor\":{}}}",
        s.state,
        room_events::num(s.pelvis),
        room_events::num(s.head),
        room_events::num(s.feet[0]),
        room_events::num(s.feet[1]),
        room_events::num(s.up),
        s.anim_pelvis.map_or("null".to_string(), room_events::num)
    )
}

/// The actors down or getting up now (the clips sample each every frame).
pub fn actors() -> Vec<u32> {
    DOWN.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|d| d.actor)
        .collect()
}

/// Whether `actor` is down or getting up now.
pub fn tracked(actor: u32) -> bool {
    DOWN.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|d| d.actor == actor)
}

/// Everything forgotten (a load, the tests).
pub fn clear() {
    DOWN.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

fn read3(e: &Engine, at: u32) -> V3 {
    [0, 4, 8].map(|o| e.mem.f32(at + o))
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn length(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
