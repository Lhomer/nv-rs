//! A brace's hands on what they brace against (Patrick, 2026-11-15, the b7aaa5a reel: the arms
//! bounced off the wall, the railing and the table instead of meeting them and stopping): a hand
//! reaches, decelerating into the surface; where it meets it, it plants (it stays at the contact
//! point: friction, no slide, no rebound) and the arm takes the load by bending at the elbow and
//! the shoulder, critically damped; it lets go only when the body has recovered, pushes off, or
//! has held long enough, and never reaches again in the same brace. And an armed actor braces
//! one-handed: its free arm braces while its weapon arm keeps its aim ([`bracing_arms`]).
//!
//! **What the host and the lab do with it** (`crate::active_ragdoll` in the host, `sim.rs` in
//! `tools/ragdoll-lab`): a [`Surface`] from the brace's obstacle at its start; each bracing arm a
//! [`Hand`]. Each frame:
//! - [`Hand::update`] with the shown hand's position (its wrist, the hand bone's joint) and the
//!   shoulder's: it plants on contact and lets go;
//! - the arm's motor targets reach with [`Hand::reach_weight`] (eased in, held, eased out);
//! - while planted, the physical body's hand is pinned at [`Hand::pin`] (a stiff, critically
//!   damped pin: the arm's joints carry the load to the body);
//! - the shown arm is put on [`Hand::goal`] by the arm's two-bone IK ([`plant_arm`]) with
//!   [`Hand::plant_weight`], so the hand the player sees stays exactly where it touched.
//!
//! Units: game units (70 a metre) and seconds.

use crate::flinch::{add, dot, length, scale, sub, unit, M3, V3};
use crate::UNITS_PER_METRE;

/// A two-handed weapon's support hand held on its grip while the body is handed back: fully
/// until the body's weight in the shown pose is down to this, then eased off with it (followed
/// all the way, the hand came off the grip mid-hand-back and back: a wobble).
pub const GRIP_LET_GO: f32 = 0.35;

/// The grip hold's weight for the body's weight in the shown pose `w` ([`GRIP_LET_GO`]).
pub fn grip_weight(w: f32) -> f32 {
    (w / GRIP_LET_GO).clamp(0.0, 1.0)
}

/// A hand's thickness (game units): the wrist rests this far off the surface.
pub const HAND: f32 = crate::flinch::BRACE_HAND;
/// How close to its rest on the surface a reaching hand counts as touching (game units), and how
/// far ahead of touching its approach starts slowing it ([`Hand::update`]).
pub const CONTACT_SKIN: f32 = 2.0;
pub const APPROACH: f32 = 6.0;
/// The reaching hand's path: a critically damped spring of this frequency (Hz) after the eased
/// reach target, never faster than this (m/s).
pub const REACH_HZ: f32 = 3.0;
/// The shown arm goes over to the hand's path this long after the brace starts (s, eased): a
/// moving arm is taken over, not stopped.
pub const REACH_BLEND: f32 = 0.2;
pub const REACH_SPEED: f32 = 1.5;
/// The reaching hand's path turns around the shoulder at most this fast (rad/s).
pub const REACH_SWEEP: f32 = 4.5;
/// The approach onto the plant (s): at least and at most; within them, as long as the hand's own
/// speed takes at an even deceleration.
pub const APPROACH_MIN: f32 = 0.12;
pub const APPROACH_MAX: f32 = 0.2;
/// The shoulder further from the plant than this share of the arm's reach: the body pushes off
/// (the arm cannot hold it), the hand lets go. At the full reach (1.0) the arm was held straight,
/// stretched out through a railing for 2 s (Patrick's 21-16-09 clip).
pub const PUSH_OFF: f32 = 0.97;
/// The reaching hand's path stays within this share of the arm's reach from the shoulder.
pub const REACH_SHORT: f32 = 0.97;
/// A plant no further from the shoulder than this share of the arm's reach (some room left for
/// the body to sway back before it pushes off).
pub const PLANT_REACH: f32 = 0.95;
/// A planted hand lets go once the shoulder has come back out this far (game units) from the
/// closest it came, at least [`PUSH_BACK_AFTER`] (s) after it came to rest on it: the body has taken
/// the load and pushes back off (the elbow never swings back and forth with the torso).
pub const PUSH_BACK: f32 = 0.5;
pub const PUSH_BACK_AFTER: f32 = 0.0;
/// The longest a hand stays planted (s), whatever the body does.
pub const MOST_PLANTED: f32 = 3.0;
/// The body counts as recovered (the planted hands let go) once its steps are done, the brace has
/// reached and held, and its centre of mass moves slower than this (m/s).
pub const RECOVERED_SPEED: f32 = 0.25;
/// The share of the push a wall brace holds ([`crate::tuning::Tuning::brace_holds`]) one arm holds
/// alone: past it an armed actor's weapon arm joins its free one.
pub const ONE_ARM: f32 = 0.6;
/// The weapon hand (0 left, 1 right): Fallout's actors hold every weapon in the right hand (a
/// two-handed one's support hand is the left).
pub const WEAPON_SIDE: u8 = 1;
/// A released hand's way back to its home ([`Phase::Released`]): a critically damped path of this
/// frequency (Hz), no faster than [`REACH_SPEED`]; locked on within [`REGRIP_LOCK`] (game units);
/// handed back once the free arm's wrist is within [`HANDOVER_NEAR`] of the home too (game units),
/// or after [`RETURN_MOST`] (s); the hand-over takes [`RETURN_FADE`] (s, eased). 3 s at weight 1
/// held a straight arm out for 2 s while the free arm never came home (Patrick's 21-16-09 clip):
/// now its way home (about 0.4 s) is done within 0.6 s and the hand-over starts by then (0.3 s
/// for the hand-over turned a hand at 8.5 rad/s).
pub const RETURN_HZ: f32 = 2.0;
/// The way back turns around the shoulder at most this fast (rad/s).
pub const RETURN_SWEEP: f32 = 3.5;
pub const REGRIP_LOCK: f32 = 0.5;
pub const HANDOVER_NEAR: f32 = 0.75;
pub const RETURN_MOST: f32 = 0.6;
pub const RETURN_FADE: f32 = 0.5;
/// The planted hand's pin (the physical body's): its frequency (Hz) on the arm's mass, critically
/// damped.
pub const PIN_HZ: f32 = 6.0;
pub const PIN_ZETA: f32 = 3.0;
/// The shown upper arm's twist about itself at most this fast (rad/s) while the brace puts it on
/// the hand's path.
pub const MOST_TWIST: f32 = 2.0;

/// How far behind the pelvis (game units, across) an obstacle is the hips' to lean on, never the
/// hands' ([`hand_may_brace`]).
pub const BEHIND_HIPS: f32 = 20.0;

/// Whether a hand may brace on `target` (where the brace sends it) for a body whose pelvis is at
/// `pelvis`, facing `forward` (flat), against an obstacle whose face is at `face`: never a target
/// behind the torso's coronal plane (a hand behind the back), nor an obstacle within
/// [`BEHIND_HIPS`] behind the pelvis: the hips lean on it instead (Patrick's 21-16-09 clip: the
/// hands went behind the back onto a railing at his hips, then one arm was held out straight
/// through it).
pub fn hand_may_brace(pelvis: V3, forward: V3, target: V3, face: Option<V3>) -> bool {
    let Some(fwd) = unit([forward[0], forward[1], 0.0]) else {
        return true;
    };
    let ahead = |p: V3| dot([p[0] - pelvis[0], p[1] - pelvis[1], 0.0], fwd);
    if ahead(target) < 0.0 {
        return false;
    }
    !face.is_some_and(|f| ahead(f) < 0.0 && -ahead(f) <= BEHIND_HIPS)
}

/// Whether a brace's obstacle (its face at `face`) is behind the torso of a body whose pelvis is
/// at `pelvis`, facing `forward`: no hand can brace on it, and the brace is not begun at all (it
/// was announced with no hands and let go 11 ms later: Patrick's 08-21-49 clip).
pub fn behind(pelvis: V3, forward: V3, face: V3) -> bool {
    let Some(fwd) = unit([forward[0], forward[1], 0.0]) else {
        return false;
    };
    dot([face[0] - pelvis[0], face[1] - pelvis[1], 0.0], fwd) < 0.0
}

/// A new brace's hand on `side` (`wanted`: the brace has it brace) from the brace before
/// (`old`): a hand still planted keeps its plant (a second push while it holds is held by it;
/// a new hand reached again from the plant, behind the back once), else a new reach.
pub fn carried_hand(old: Option<Hand>, wanted: bool, side: u8) -> Option<Hand> {
    if !wanted {
        return None;
    }
    match old {
        Some(h) if h.side == side && h.planted() => Some(h),
        _ => Some(Hand::new(side)),
    }
}

/// A released hand turned onto its home's rotation at most this fast (rad/s, [`turn_toward`]):
/// eased by its way home alone, a hand let go near its home turned at 10 rad/s in the lab.
pub const HAND_HOME_TURN: f32 = 6.0;

/// The rotation `from` turned toward `to` by at most `most` (rad).
pub fn turn_toward(from: &M3, to: &M3, most: f32) -> M3 {
    use crate::active::{m_from_q, qconj, qexp, qfrom_m, qlog, qmul, qnorm};
    let (a, b) = (qfrom_m(from), qfrom_m(to));
    let mut d = qnorm(qmul(b, qconj(a)));
    if d[0] < 0.0 {
        d = d.map(|x| -x);
    }
    let w = qlog(d);
    let l = length(w);
    if l <= most.max(0.0) || !l.is_finite() {
        return *to;
    }
    m_from_q(qnorm(qmul(qexp(scale(w, most.max(0.0) / l)), a)))
}

/// What a brace's hands meet: a wall's face, or a waist-high obstacle's top (with its face below
/// it), in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    /// A point on the face (game units) and the push's way (flat, unit): the face is across it.
    pub face: V3,
    pub dir: V3,
    /// A waist-high obstacle's top (world z): the hands go on it, past the face.
    pub top: Option<f32>,
}

impl Surface {
    /// The surface of a brace's obstacle: found `wall` units along `dir` from `origin` (the
    /// obstacle scan's origin: the pelvis), its top at `top` when it is waist-high. None without
    /// an obstacle (the hands reach into the air and come back).
    pub fn new(origin: V3, dir: V3, wall: Option<f32>, top: Option<f32>) -> Option<Surface> {
        let w = wall.filter(|w| w.is_finite() && *w > 0.0)?;
        let dir = unit([dir[0], dir[1], 0.0])?;
        Some(Surface {
            face: add(origin, scale(dir, w)),
            dir,
            top: top.filter(|z| z.is_finite()),
        })
    }

    /// How far the point `p` is off the surface (game units; negative inside it) and the surface's
    /// way out toward it (unit).
    pub fn gap(&self, p: V3) -> (f32, V3) {
        // Along the push from the face: negative in front of it (the actor's side).
        let along = dot(sub(p, self.face), self.dir);
        let out_face = scale(self.dir, -1.0);
        match self.top {
            None => (-along, out_face),
            Some(z) => {
                let above = p[2] - z;
                if along >= 0.0 {
                    (above, [0.0, 0.0, 1.0])
                } else if above <= 0.0 {
                    (-along, out_face)
                } else {
                    // Off the edge, above it and in front: the distance to the edge.
                    let d = (along * along + above * above).sqrt();
                    let n = add(scale(out_face, -along / d), [0.0, 0.0, above / d]);
                    (d, n)
                }
            }
        }
    }

    /// Where a wrist at `p` rests on the surface: moved along the surface's way out to a hand's
    /// thickness off it.
    pub fn rest(&self, p: V3) -> V3 {
        let (gap, n) = self.gap(p);
        add(p, scale(n, HAND - gap))
    }
}

/// Where a hand is in its brace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Reaching out, not touching yet: the wrist's path (where and how fast, game units), a
    /// critically damped spring after the reach eased from where the wrist set off (`from`, from
    /// the shoulder) to the brace's target around the shoulder (never through the body), held off
    /// the surface; none before the first frame.
    Reaching { path: Option<(V3, V3, V3)> },
    /// Touched: the wrist goes from `start` (moving at `v0`, game units a second) onto `at` over
    /// `approach` seconds, decelerating, and stays there; `t` seconds since it touched.
    Planted {
        start: V3,
        v0: V3,
        approach: f32,
        at: V3,
        t: f32,
        /// The surface's way out at the plant (the contact pushes along it, never pulls).
        normal: V3,
        /// The closest the shoulder has come to the plant since it touched (game units):
        /// the body pushing back off past [`PUSH_BACK`] lets it go ([`Hand::update`]).
        closest: f32,
        /// The shoulder's distance from the plant the frame before.
        last: f32,
    },
    /// Let go (or never touched), `t` seconds ago: the hand goes back to its home (a two-handed
    /// weapon's grip, carried by the weapon hand; else where the animation has it, carried by the
    /// body) along a critically damped path relative to the home, so the body's own motion carries
    /// it and it never goes past ([`RETURN_HZ`]): `off` its offset from the home and how fast that
    /// shrinks, `first` the offset's length when it set off, `home` the home at the last update.
    /// Within [`REGRIP_LOCK`] it locks on (`locked`). Once locked and the free arm's own wrist is
    /// on the home too ([`HANDOVER_NEAR`]) the arm is handed back to the free arm over
    /// [`RETURN_FADE`] (`back` seconds since). Never reaches again.
    Released {
        t: f32,
        off: V3,
        vel: V3,
        first: f32,
        home: V3,
        locked: bool,
        back: Option<f32>,
        why: &'static str,
    },
}

/// What a frame did to a hand ([`Hand::update`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// It touched the surface; it rests at this point.
    Planted(V3),
    /// It let go, and why.
    Released(&'static str),
}

/// One bracing hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hand {
    pub side: u8,
    pub phase: Phase,
    /// Seconds since it set off.
    pub reach_t: f32,
}

impl Hand {
    pub fn new(side: u8) -> Hand {
        Hand {
            side,
            phase: Phase::Reaching { path: None },
            reach_t: 0.0,
        }
    }

    /// A frame of `dt` seconds, `brace_t` seconds into the brace: `wrist` and `shoulder` where the
    /// shown arm has them now, `home` where the hand goes back to once released (a two-handed
    /// weapon's grip as the weapon hand carries it; else the animation's hand as the body carries
    /// it), `free` where the free arm's wrist is (the arm as the body and its targets have it,
    /// before the brace's IK: the hand-over waits for it to be home too), `reach` the
    /// arm's length (shoulder to wrist, straight), `target` where the brace sends the hand
    /// (`crate::flinch::brace_target`),
    /// `surface` what it braces against (if anything), `let_go` a reason to let go now (the body
    /// recovered, or no longer bracing).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        surface: Option<&Surface>,
        wrist: V3,
        home: V3,
        free: V3,
        shoulder: V3,
        reach: f32,
        target: V3,
        brace_t: f32,
        dt: f32,
        let_go: Option<&'static str>,
    ) -> Option<Event> {
        let k = crate::tuning::get();
        if matches!(self.phase, Phase::Reaching { path: Some(_) }) {
            self.reach_t += dt;
        }
        let shown = self.goal();
        match &mut self.phase {
            Phase::Reaching { path } => {
                let (mut pos, mut vel, from) =
                    path.unwrap_or((wrist, [0.0; 3], sub(wrist, shoulder)));
                // The reach eased from where it set off to the target, around the shoulder (its
                // way turned, its distance eased: a reach behind goes down past the hip, never
                // through the chest), never past the surface.
                let w = crate::active::ease(brace_t / k.brace_reach.max(1e-3));
                let to = sub(target, shoulder);
                let mut goal = add(shoulder, around(from, to, w));
                if let Some(s) = surface {
                    if s.gap(goal).0 < HAND {
                        goal = s.rest(goal);
                    }
                }
                // Never further from the shoulder than the arm reaches: what it touches it can
                // hold (an obstacle beyond the reach is never touched).
                let out = sub(goal, shoulder);
                let l = length(out);
                if l > reach * REACH_SHORT && l.is_finite() {
                    goal = add(shoulder, scale(out, reach * REACH_SHORT / l));
                }
                (pos, vel) = step_path(pos, vel, goal, shoulder, dt);
                *path = Some((pos, vel, from));
                // The body recovered before the hand got there: it comes back without touching.
                // A rest out of the arm's reach is not touched (it could not be held).
                let touching = surface.filter(|s| {
                    s.gap(pos).0 <= HAND + CONTACT_SKIN
                        && length(sub(s.rest(pos), shoulder)) <= reach * PLANT_REACH
                });
                if let (Some(s), None) = (touching, let_go) {
                    let (_, n) = s.gap(pos);
                    // Into the surface at its speed, decelerating evenly onto its rest; along it a
                    // little further, stopping (the palm meets it and holds, no slide back).
                    let closing = (-dot(vel, n)).max(0.0);
                    let gap = (s.gap(pos).0 - HAND).max(0.0);
                    let approach = if closing > 1e-3 {
                        (2.0 * gap / closing).clamp(APPROACH_MIN, APPROACH_MAX)
                    } else {
                        APPROACH_MIN
                    };
                    let along = sub(vel, scale(n, dot(vel, n)));
                    let at = s.rest(add(pos, scale(along, approach * 0.5)));
                    let d = length(sub(at, pos));
                    // A cubic from the start at its speed to the rest at none overshoots when the
                    // speed is more than three times the distance over the time: held to that.
                    let most = 3.0 * d / approach;
                    let speed = length(vel);
                    let v0 = if speed > most && speed > 0.0 {
                        scale(vel, most / speed)
                    } else {
                        vel
                    };
                    self.phase = Phase::Planted {
                        start: pos,
                        v0,
                        approach,
                        at,
                        t: 0.0,
                        normal: s.gap(at).1,
                        closest: f32::INFINITY,
                        last: f32::INFINITY,
                    };
                    return Some(Event::Planted(at));
                }
                // Never touched: after the reach and the hold (or once the body has recovered) it
                // comes back from where its path got to.
                if let Some(why) = let_go.or_else(|| {
                    (brace_t >= k.brace_reach + k.brace_hold).then_some("touched nothing")
                }) {
                    let blended = self.reach_t >= REACH_BLEND;
                    let off = sub(pos, home);
                    self.phase = Phase::Released {
                        t: 0.0,
                        off,
                        vel: [0.0; 3],
                        first: length(off),
                        home,
                        locked: false,
                        // Let go before the arm was fully on its path: handed straight back.
                        back: (!blended).then_some(RETURN_FADE),
                        why,
                    };
                    return Some(Event::Released(why));
                }
                None
            }
            Phase::Planted {
                at,
                t,
                approach,
                closest,
                last,
                ..
            } => {
                *t += dt;
                let at = *at;
                let here = shown.unwrap_or(at);
                let d = length(sub(at, shoulder));
                // The closest the shoulder came since it touched: the load taken; the body swaying
                // back out past it pushes off (held on, the elbow swung back and forth with the
                // torso).
                *closest = closest.min(d);
                // A frame ahead at its pace (the shoulder is the shown arm's of the frame before).
                let ahead = if last.is_finite() {
                    d + (d - *last).max(0.0)
                } else {
                    d
                };
                *last = d;
                let why = if d > reach * PUSH_OFF {
                    Some("pushed off")
                } else if *t >= *approach + PUSH_BACK_AFTER && ahead > *closest + PUSH_BACK {
                    Some("pushed back off it")
                } else if *t >= MOST_PLANTED {
                    Some("held long enough")
                } else {
                    let_go
                };
                if let Some(why) = why {
                    let off = sub(here, home);
                    self.phase = Phase::Released {
                        t: 0.0,
                        off,
                        vel: [0.0; 3],
                        first: length(off),
                        home,
                        locked: false,
                        back: None,
                        why,
                    };
                    return Some(Event::Released(why));
                }
                None
            }
            Phase::Released {
                t,
                off,
                vel,
                home: kept,
                locked,
                back,
                ..
            } => {
                *t += dt;
                *kept = home;
                // On along its way home (the last of it too: no snap), locked on within the lock.
                (*off, *vel) = step_home(*off, *vel, home, shoulder, dt);
                if length(*off) <= REGRIP_LOCK {
                    *locked = true;
                }
                match back {
                    Some(b) => *b += dt,
                    None => {
                        let home_too = *locked && length(sub(free, home)) <= HANDOVER_NEAR;
                        if home_too || *t >= RETURN_MOST {
                            *back = Some(0.0);
                        }
                    }
                }
                None
            }
        }
    }

    /// How much the arm's motor targets reach toward the brace (0 to 1): eased in over the brace's
    /// reach, full while planted, eased out from where it was over the release.
    pub fn reach_weight(&self, brace_t: f32) -> f32 {
        let k = crate::tuning::get();
        match self.phase {
            Phase::Reaching { .. } => crate::active::ease(brace_t / k.brace_reach.max(1e-3)),
            Phase::Planted { .. } => 1.0,
            Phase::Released { .. } => self.plant_weight(),
        }
    }

    /// Where the shown wrist goes this frame (game units): along its reaching path, then its
    /// approach onto the plant (decelerating to rest), then the plant; let go, along its way back
    /// to its home.
    pub fn goal(&self) -> Option<V3> {
        match self.phase {
            Phase::Reaching { path } => path.map(|p| p.0),
            Phase::Planted {
                start,
                v0,
                approach,
                at,
                t,
                ..
            } => {
                if t >= approach {
                    return Some(at);
                }
                // Hermite: from `start` at `v0` to `at` at rest.
                let s = (t / approach).clamp(0.0, 1.0);
                let (s2, s3) = (s * s, s * s * s);
                let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
                let h10 = s3 - 2.0 * s2 + s;
                let h01 = -2.0 * s3 + 3.0 * s2;
                Some(add(
                    add(scale(start, h00), scale(v0, h10 * approach)),
                    scale(at, h01),
                ))
            }
            Phase::Released { off, home, .. } => Some(add(home, off)),
        }
    }

    /// A released hand's offset from its home now (game units): where its way back has got to,
    /// to put on the home as it is this frame.
    pub fn home_offset(&self) -> Option<V3> {
        match self.phase {
            Phase::Released { off, .. } => Some(off),
            _ => None,
        }
    }

    /// Let go and locked on its home (a two-handed weapon's grip: the grip hold takes it from
    /// here, `Sim`'s and the host's).
    pub fn on_home(&self) -> bool {
        matches!(self.phase, Phase::Released { locked: true, .. })
    }

    /// How far back home a released hand is (0 as it let go, 1 locked on): the share of its way
    /// back done, for turning the hand onto its grip as it comes.
    pub fn home_share(&self) -> f32 {
        match self.phase {
            Phase::Released {
                off, first, locked, ..
            } => {
                if locked || first <= 1e-3 {
                    1.0
                } else {
                    (1.0 - length(off) / first).clamp(0.0, 1.0)
                }
            }
            _ => 0.0,
        }
    }

    /// How much the shown arm is put on [`Hand::goal`] (0 to 1): fully while it reaches and while
    /// planted, eased off over the release.
    pub fn plant_weight(&self) -> f32 {
        match self.phase {
            Phase::Reaching { path } => match path {
                Some(_) => crate::active::ease(self.reach_t / REACH_BLEND),
                None => 0.0,
            },
            Phase::Planted { .. } => 1.0,
            Phase::Released { back, .. } => {
                back.map_or(1.0, |b| 1.0 - crate::active::ease(b / RETURN_FADE))
            }
        }
    }

    /// Where the physical body's wrist is pinned (game units), the pin's share of its stiffness
    /// and the surface's way out, while planted: firmly, as a contact (it pushes the wrist out of
    /// the surface and holds it from sliding while it presses, never pulls it in:
    /// `crate::active::Targets::hand_normal`).
    pub fn pin(&self) -> Option<(V3, f32, V3)> {
        match self.phase {
            Phase::Planted { normal, .. } => self.goal().map(|g| (g, 1.0, normal)),
            _ => None,
        }
    }

    /// Where the arm's motors reach (game units): where the shown hand goes, all the way (its
    /// reach, its plant, its way back home), so the physical arm comes home with it; the weight
    /// is [`Hand::plant_weight`]'s (eased off once handed back).
    pub fn motor_goal(&self) -> Option<V3> {
        self.goal()
    }

    /// Let go (or never touched): coming back.
    pub fn released(&self) -> bool {
        matches!(self.phase, Phase::Released { .. })
    }

    /// Planted and at rest on its plant (its approach done).
    pub fn at_rest(&self) -> bool {
        matches!(self.phase, Phase::Planted { t, approach, .. } if t >= approach)
    }

    pub fn planted(&self) -> bool {
        matches!(self.phase, Phase::Planted { .. })
    }

    /// Done: let go and eased out.
    pub fn done(&self) -> bool {
        matches!(self.phase, Phase::Released { back: Some(b), .. } if b >= RETURN_FADE)
    }
}

/// A hand's path a frame on (where and how fast, game units) after `goal`: a critically damped
/// spring ([`REACH_HZ`]), no faster than [`REACH_SPEED`], and around the shoulder no faster than
/// [`REACH_SWEEP`] (the arm speed cap).
fn step_path(mut pos: V3, mut vel: V3, goal: V3, shoulder: V3, dt: f32) -> (V3, V3) {
    if dt <= 0.0 {
        return (pos, vel);
    }
    let before = pos;
    let om = std::f32::consts::TAU * REACH_HZ;
    let a = sub(scale(sub(goal, pos), om * om), scale(vel, 2.0 * om));
    vel = add(vel, scale(a, dt));
    let most = REACH_SPEED * UNITS_PER_METRE;
    let speed = length(vel);
    if speed > most {
        vel = scale(vel, most / speed);
    }
    pos = add(pos, scale(vel, dt));
    let (a, b) = (sub(before, shoulder), sub(pos, shoulder));
    let (la, lb) = (length(a), length(b));
    if la > 1.0 && lb > 1.0 {
        let angle = (dot(a, b) / (la * lb)).clamp(-1.0, 1.0).acos();
        let most = REACH_SWEEP * dt;
        if angle > most {
            let swept = add(shoulder, around(a, b, most / angle));
            vel = scale(sub(swept, before), 1.0 / dt);
            pos = swept;
        }
    }
    (pos, vel)
}

/// A released hand's offset from its home a frame on, and how fast it changes: a critically damped
/// spring to none ([`RETURN_HZ`]), no faster than [`REACH_SPEED`], and never faster toward home
/// than it can stop by the home (the speed toward it at most the spring's frequency times the
/// distance left): no overshoot, no swing.
fn step_home(mut off: V3, mut vel: V3, home: V3, shoulder: V3, dt: f32) -> (V3, V3) {
    if dt <= 0.0 {
        return (off, vel);
    }
    let om = std::f32::consts::TAU * RETURN_HZ;
    let a = sub(scale(off, -om * om), scale(vel, 2.0 * om));
    vel = add(vel, scale(a, dt));
    let most = REACH_SPEED * UNITS_PER_METRE;
    let speed = length(vel);
    if speed > most {
        vel = scale(vel, most / speed);
    }
    let d = length(off);
    if let Some(n) = unit(scale(off, -1.0)) {
        // Toward home at most om * d (and never past it in this step); across, damped away.
        let toward = dot(vel, n).clamp(0.0, (om * d).min(d / dt));
        let across = sub(vel, scale(n, dot(vel, n)));
        vel = add(scale(n, toward), scale(across, (-2.0 * om * dt).exp()));
    }
    // Around the shoulder no faster than [`RETURN_SWEEP`] (the arm speed cap): the step shortened
    // (shorter toward home never goes past it).
    let (a, b) = (
        sub(add(home, off), shoulder),
        sub(add(home, add(off, scale(vel, dt))), shoulder),
    );
    let (la, lb) = (length(a), length(b));
    if la > 1.0 && lb > 1.0 {
        let angle = (dot(a, b) / (la * lb)).clamp(-1.0, 1.0).acos();
        let most = RETURN_SWEEP * dt;
        if angle > most {
            vel = scale(vel, most / angle);
        }
    }
    off = add(off, scale(vel, dt));
    (off, vel)
}

/// From `a` toward `b` (both from the shoulder) by `w`: the way turned (spherically) and the
/// distance eased.
fn around(a: V3, b: V3, w: f32) -> V3 {
    let (la, lb) = (length(a), length(b));
    let (Some(ua), Some(ub)) = (unit(a), unit(b)) else {
        return add(a, scale(sub(b, a), w));
    };
    let c = dot(ua, ub).clamp(-1.0, 1.0);
    let angle = c.acos();
    let dir = if angle < 1e-3 {
        ua
    } else if angle > std::f32::consts::PI - 1e-2 {
        // Straight across: turned by way of straight down.
        let down = [0.0, 0.0, -1.0];
        let perp = unit(sub(down, scale(ua, dot(down, ua))))
            .or_else(|| unit(crate::flinch::cross(ua, [1.0, 0.0, 0.0])))
            .unwrap_or([0.0, 1.0, 0.0]);
        add(scale(ua, (w * angle).cos()), scale(perp, (w * angle).sin()))
    } else {
        let s = angle.sin();
        add(
            scale(ua, ((1.0 - w) * angle).sin() / s),
            scale(ub, (w * angle).sin() / s),
        )
    };
    scale(dir, la + (lb - la) * w)
}

/// An armed actor's support hand grips its weapon (two-handed) when the animation has it within
/// this of the weapon hand (game units, about 50 cm): its home after a brace is the grip.
pub const GRIP_NEAR: f32 = 35.0;

/// Whether an actor's support (left) hand grips its weapon: armed, and the animation has the
/// support hand `anim_support` within [`GRIP_NEAR`] of the weapon hand `anim_weapon`.
pub fn grips(armed: bool, anim_support: V3, anim_weapon: V3) -> bool {
    armed && length(sub(anim_support, anim_weapon)) <= GRIP_NEAR
}

/// Where a bone (`anim_hand`, rotation and position in the animation) is when it rides on
/// another (`anim_carrier` in the animation, `carrier` now): a released hand's home (its grip on
/// the weapon hand, or its place by the body).
pub fn carried(anim_carrier: (M3, V3), anim_hand: (M3, V3), carrier: (M3, V3)) -> (M3, V3) {
    use crate::flinch::{apply_m, mul, transpose};
    let back = transpose(&anim_carrier.0);
    let local_p = apply_m(&back, sub(anim_hand.1, anim_carrier.1));
    let local_r = mul(&back, &anim_hand.0);
    (
        mul(&carrier.0, &local_r),
        add(carrier.1, apply_m(&carrier.0, local_p)),
    )
}

/// The reason a brace's planted hands let go this frame, if any: the body recovered (its steps
/// done, the brace reached and held, its centre of mass slower than [`RECOVERED_SPEED`] m/s).
/// Why a brace's hands let go once its reaction's time is up ([`crate::active::REACTION_MOST`]); a
/// hand let go then goes home along its own way (a weapon's support hand onto its grip: handed
/// over at once it wobbled there), while the body is handed back (`crate::active::reaction_weight`).
pub const TIME_UP: &str = "the reaction's time is up";
/// How far (rad) the actor may turn from where its brace began before its hands let go: the game's
/// AI turns it (toward its attacker), and hands held where they touched twist the body round.
pub const TURNED_AWAY: f32 = 0.45;
/// How far (game units) the game may move the actor (its AI walking it: not the steps) from where
/// its brace began before its hands let go: a hand held where it touched dragged the body behind.
pub const WALKED_AWAY: f32 = 10.0;

/// Why the brace's hands let go now, if they do: the reaction's time is up
/// ([`crate::active::REACTION_MOST`] seconds after the last hit, `since_hit`: [`TIME_UP`]), the
/// actor turned away from where the brace began (`turned`, rad; [`TURNED_AWAY`]: the game's AI turns
/// it, and hands held where they touched twisted the body round: Patrick's c2a53c0 clip) or was
/// walked away from it (`walked`, game units; [`WALKED_AWAY`]), or the body recovered
/// ([`recovered`]).
pub fn let_go(
    stepping: bool,
    brace_t: f32,
    com_speed: f32,
    since_hit: f32,
    turned: f32,
    walked: f32,
) -> Option<&'static str> {
    if since_hit >= crate::active::REACTION_MOST {
        return Some(TIME_UP);
    }
    if turned >= TURNED_AWAY {
        return Some("turned away from the wall");
    }
    if walked >= WALKED_AWAY {
        return Some("walked away from the wall");
    }
    recovered(stepping, brace_t, com_speed)
}

/// The angle (rad) between two flat facings (unit or not); 0 for a missing one.
pub fn turned(from: V3, to: V3) -> f32 {
    let (Some(a), Some(b)) = (unit([from[0], from[1], 0.0]), unit([to[0], to[1], 0.0])) else {
        return 0.0;
    };
    dot(a, b).clamp(-1.0, 1.0).acos()
}

pub fn recovered(stepping: bool, brace_t: f32, com_speed: f32) -> Option<&'static str> {
    let k = crate::tuning::get();
    (!stepping && brace_t >= k.brace_reach + k.brace_hold && com_speed < RECOVERED_SPEED)
        .then_some("recovered")
}

/// Which arms brace (0 left, 1 right): `arms` the arms the body has, `armed` whether it holds a
/// weapon (in its right hand, [`WEAPON_SIDE`]), `push` the push (m/s) against what one arm holds
/// (`one_arm`, m/s), `reaches` whether each arm's hand can reach the surface. Unarmed, every arm.
/// Armed, the free arm (a two-handed weapon's support hand lets go of it); the weapon arm joins
/// (the weapon kept in hand) only on a push past what one arm holds or when the free arm is
/// missing or cannot reach.
pub fn bracing_arms(
    arms: [bool; 2],
    armed: bool,
    push: f32,
    one_arm: f32,
    reaches: [bool; 2],
) -> [bool; 2] {
    if !armed {
        return arms;
    }
    let weapon = WEAPON_SIDE as usize;
    let free = 1 - weapon;
    let free_ok = arms[free] && reaches[free];
    let mut out = [false; 2];
    out[free] = arms[free];
    // For reach, only where the weapon arm gets there and the free one does not (nor when the
    // body has no free arm: the weapon arm braces alone).
    out[weapon] = arms[weapon] && (push > one_arm || !arms[free] || (!free_ok && reaches[weapon]));
    out
}

/// A hand's goal held inside the shoulder's human range (`crate::flinch::shoulder_range`) for an
/// actor facing `forward` (its up the world's), the arm on `side`: what the brace's targets, its
/// plants and the shown arm go to.
pub fn held_goal(shoulder: V3, goal: V3, forward: V3, side: u8) -> V3 {
    crate::flinch::shoulder_range(shoulder, goal, forward, [0.0, 0.0, 1.0], side)
}

/// Whether the arm on `side` at `shoulder` takes a brace to `target`: within its `reach` (game
/// units) and not behind the back (`crate::flinch::behind_the_back`).
pub fn reaches(shoulder: V3, target: V3, reach: f32, forward: V3, side: u8) -> bool {
    can_reach(shoulder, target, reach)
        && !crate::flinch::behind_the_back(shoulder, target, forward, side)
}

/// Whether a hand on an arm `reach` long (game units) at `shoulder` can get to `target`.
pub fn can_reach(shoulder: V3, target: V3, reach: f32) -> bool {
    length(sub(target, shoulder)) <= reach * 0.98
}

/// The planted wrist's pin gains on an arm of `arm_mass` kg ([`PIN_HZ`], [`PIN_ZETA`]): stiffness
/// (N/m) and damping (N s/m).
pub fn pin_gains(arm_mass: f32) -> (f32, f32) {
    let w = std::f32::consts::TAU * PIN_HZ;
    (arm_mass * w * w, 2.0 * PIN_ZETA * arm_mass * w)
}

/// The shown arm put on `goal` (game units) with weight `w`: from the shown shoulder, elbow and
/// wrist, the upper arm's and the forearm's world turns (the forearm's carried down from the upper
/// arm's: an override of its chain, as [`crate::flinch::reach_arm_ik`] gives them). The elbow stays
/// in the plane it bends in now (the two-bone solve's pole is the elbow itself) and each bone turns
/// the least way, so a goal where the wrist already is changes nothing and the arm takes the load
/// by bending, without a jump or a twist; the upper arm's twist about itself is held to
/// `most_twist` (rad) a call. A goal past the arm's reach leaves it straight toward it.
pub fn plant_arm(
    shoulder: V3,
    elbow: V3,
    wrist: V3,
    goal: V3,
    w: f32,
    forward: V3,
    most_twist: f32,
) -> Option<(M3, M3)> {
    use crate::balance::{between, two_bone};
    use crate::flinch::{apply_m, mul, rotation, transpose};
    if w <= 0.0 {
        return None;
    }
    let to = add(wrist, scale(sub(goal, wrist), w.min(1.0)));
    let (u0, f0) = (sub(elbow, shoulder), sub(wrist, elbow));
    let (l1, l2) = (length(u0), length(f0));
    if l1 < 1.0 || l2 < 1.0 {
        return None;
    }
    // The pole a little past the elbow, away from the shoulder-wrist line: the bend's own side;
    // nearly straight, more and more the way a straight arm's elbow goes (back and down), so the
    // bend's side never flips.
    let line = sub(wrist, shoulder);
    let along = dot(u0, line) / dot(line, line).max(1e-6);
    let off = sub(u0, scale(line, along));
    let fwd = unit([forward[0], forward[1], 0.0]).unwrap_or([0.0; 3]);
    let straight = unit(sub([0.0, 0.0, -1.0], fwd)).unwrap_or([0.0, 0.0, -1.0]);
    let k = (1.0 - length(off) / 3.0).clamp(0.0, 1.0);
    let dir = unit(add(off, scale(straight, 3.0 * k))).unwrap_or(straight);
    let pole = add(elbow, scale(dir, 20.0));
    let (m, reached) = two_bone(shoulder, to, l1, l2, pole)?;
    let (u1, f1) = (sub(m, shoulder), sub(reached, m));
    // The upper arm turned so its bend's normal (the elbow's hinge as the arm is bent now) goes
    // onto the new bend's: the forearm flexes about its own hinge, never past it; a straight arm
    // the least way.
    // Between the two by how bent the arm is (no jump as it straightens or bends).
    let (bent0, bent1) = (crate::flinch::cross(u0, f0), crate::flinch::cross(u1, f1));
    let least = rotation(between(u0, u1));
    let bend = (length(bent0).min(length(bent1)) / (l1 * l2))
        .clamp(0.0, 1.0)
        .asin();
    let k = crate::active::ease((bend - 0.04) / 0.12);
    let r1 = match crate::flinch::align(u0, bent0, u1, bent1) {
        Some(hinged) if k > 0.0 => {
            let q = crate::active::qslerp(
                crate::active::qfrom_m(&least),
                crate::active::qfrom_m(&hinged),
                k,
            );
            crate::active::m_from_q(q)
        }
        _ => least,
    };
    // Its twist about itself at most `most_twist` this frame (a shoulder turns the arm about its
    // length only so fast): the swing kept, the twist held; the forearm is aimed after it, so the
    // wrist still reaches the goal.
    let r1 = {
        let twist_of = mul(&r1, &transpose(&least));
        let w = crate::flinch::rotation_vector(&twist_of);
        let axis = unit(u1).unwrap_or([0.0, 0.0, 1.0]);
        let t = dot(w, axis);
        if t.abs() > most_twist {
            mul(&rotation(scale(axis, t.signum() * most_twist)), &least)
        } else {
            r1
        }
    };
    let r2 = rotation(between(apply_m(&r1, f0), f1));
    Some((r1, mul(&mul(&transpose(&r1), &r2), &r1)))
}

/// Metres to game units.
pub fn units(m: f32) -> f32 {
    m * UNITS_PER_METRE
}

#[cfg(test)]
mod tests {
    /// No hand braces behind the torso, nor on an obstacle right behind the hips; one in front
    /// does. A planted hand is kept by a new brace, a reaching one starts again.
    #[test]
    fn hands_brace_in_front_only_and_keep_a_plant() {
        let (pelvis, fwd) = ([0.0, 0.0, 60.0], [0.0, 1.0, 0.0]);
        assert!(hand_may_brace(
            pelvis,
            fwd,
            [5.0, 30.0, 70.0],
            Some([0.0, 25.0, 70.0])
        ));
        assert!(!hand_may_brace(
            pelvis,
            fwd,
            [5.0, -10.0, 70.0],
            Some([0.0, -24.0, 70.0])
        ));
        assert!(!hand_may_brace(
            pelvis,
            fwd,
            [5.0, 2.0, 70.0],
            Some([0.0, -12.0, 70.0])
        ));
        assert!(hand_may_brace(pelvis, fwd, [5.0, 2.0, 70.0], None));
        assert!(behind(pelvis, fwd, [0.0, -24.0, 70.0]));
        assert!(!behind(pelvis, fwd, [0.0, 25.0, 70.0]));
        let mut planted = Hand::new(0);
        planted.phase = Phase::Planted {
            start: [0.0; 3],
            v0: [0.0; 3],
            approach: 0.1,
            at: [1.0, 2.0, 3.0],
            t: 0.5,
            normal: [0.0, 0.0, 1.0],
            closest: 20.0,
            last: 20.0,
        };
        assert_eq!(carried_hand(Some(planted), true, 0), Some(planted));
        assert_eq!(
            carried_hand(Some(Hand::new(0)), true, 0),
            Some(Hand::new(0))
        );
        assert_eq!(carried_hand(Some(planted), false, 0), None);
        assert_eq!(carried_hand(None, true, 1), Some(Hand::new(1)));
    }

    use super::*;

    /// A brace's hands let go once one hit's time is up, once the game turns the actor away from
    /// the wall or walks it off, or once the body recovered; not while it still reacts there.
    #[test]
    fn the_hands_let_go_when_the_reaction_is_over() {
        let held = let_go(false, 0.5, 1.0, 0.5, 0.0, 0.0);
        assert_eq!(held, None);
        assert_eq!(
            let_go(false, 0.5, 1.0, crate::active::REACTION_MOST, 0.0, 0.0),
            Some(TIME_UP)
        );
        assert_eq!(
            let_go(false, 0.5, 1.0, 0.5, TURNED_AWAY, 0.0),
            Some("turned away from the wall")
        );
        assert_eq!(
            let_go(false, 0.5, 1.0, 0.5, 0.0, WALKED_AWAY),
            Some("walked away from the wall")
        );
        assert_eq!(
            let_go(false, 0.5, 1.0, 0.5, 0.0, 0.0),
            recovered(false, 0.5, 1.0)
        );
        // The turn between flat facings, either way round, and none for a missing one.
        assert!(
            (turned([0.0, 1.0, 0.0], [1.0, 0.0, 0.3]) - std::f32::consts::FRAC_PI_2).abs() < 1e-5
        );
        assert!((turned([0.0, 1.0, 0.0], [0.0, -2.0, 0.0]) - std::f32::consts::PI).abs() < 1e-5);
        assert_eq!(turned([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), 0.0);
    }

    fn wall() -> Surface {
        Surface::new([0.0; 3], [0.0, 1.0, 0.0], Some(50.0), None).unwrap()
    }

    #[test]
    fn a_wall_and_a_top_are_met() {
        let s = wall();
        assert_eq!(s.gap([0.0, 40.0, 100.0]).0, 10.0);
        assert!(s.gap([0.0, 55.0, 100.0]).0 < 0.0);
        let r = s.rest([3.0, 47.0, 100.0]);
        assert!((r[1] - (50.0 - HAND)).abs() < 1e-4 && r[0] == 3.0);
        let top = Surface::new([0.0; 3], [0.0, 1.0, 0.0], Some(50.0), Some(70.0)).unwrap();
        assert_eq!(top.gap([0.0, 55.0, 80.0]).0, 10.0);
        assert_eq!(top.gap([0.0, 45.0, 60.0]).0, 5.0);
        let (d, _) = top.gap([0.0, 47.0, 74.0]);
        assert!((d - 5.0).abs() < 1e-4, "the edge: {d}");
    }

    /// A hand coming in plants once, decelerates onto its rest without passing it, stays there,
    /// and lets go only when told (or pushed off), never reaching again.
    #[test]
    fn a_hand_plants_and_stays() {
        let s = wall();
        let mut h = Hand::new(0);
        let dt = 1.0 / 60.0;
        let shoulder = [0.0, 0.0, 100.0];
        let mut p = [0.0, 30.0, 100.0];
        let mut planted = 0;
        let mut t = 0.0;
        let mut last: Option<V3> = None;
        let mut fastest = 0.0f32;
        for i in 0..90 {
            t += dt;
            let target = [0.0, 60.0, 100.0];
            if let Some(Event::Planted(_)) = h.update(
                Some(&s),
                p,
                [0.0, 30.0, 100.0],
                [0.0, 30.0, 100.0],
                shoulder,
                60.0,
                target,
                t,
                dt,
                None,
            ) {
                planted += 1;
            }
            // The free hand would go on into the wall at 60 units a second.
            p[1] += 60.0 * dt;
            let g = h.goal().unwrap();
            assert!(
                g[1] <= 50.0 - HAND + 1e-3,
                "it never passes its rest: {g:?}"
            );
            if let Some(l) = last {
                assert!(g[1] >= l[1] - 1e-3, "it never comes back: frame {i}");
                fastest = fastest.max(length(sub(g, l)) / dt);
            }
            last = Some(g);
        }
        assert_eq!(planted, 1);
        assert!(fastest <= REACH_SPEED * UNITS_PER_METRE + 1.0, "{fastest}");
        assert!(
            (last.unwrap()[1] - (50.0 - HAND)).abs() < 0.01,
            "at rest on the wall"
        );
        assert!(h.planted());
        // Let go: back home (a grip that moves) along a path that never goes past it nor swings,
        // locked on, handed back once the free arm's wrist is there too or `RETURN_MOST` is up.
        let home0: V3 = [0.0, 25.0, 90.0];
        assert_eq!(
            h.update(
                Some(&s),
                p,
                home0,
                p,
                shoulder,
                60.0,
                [0.0, 60.0, 100.0],
                t,
                dt,
                Some("recovered")
            ),
            Some(Event::Released("recovered"))
        );
        let start = sub(h.goal().unwrap(), home0);
        let toward = unit(scale(start, -1.0)).unwrap();
        let mut least = length(start);
        for i in 0..240 {
            t += dt;
            // The grip moves with the weapon hand; the free arm's wrist gets there after a while.
            let home = add(home0, [3.0 * (t * 2.0).sin(), 0.0, 0.0]);
            let free = if i < 90 { p } else { home };
            h.update(
                Some(&s),
                p,
                home,
                free,
                shoulder,
                60.0,
                [0.0, 60.0, 100.0],
                t,
                dt,
                None,
            );
            let rel = sub(h.goal().unwrap(), home);
            assert!(dot(rel, toward) <= 1e-3, "never past the grip: frame {i}");
            let d = length(rel);
            assert!(d <= least + 1e-3, "never further from it again: frame {i}");
            least = least.min(d);
            // Held until the free arm is there, or `RETURN_MOST` at most (home within 0.6 s).
            if (i as f32 + 1.0) * dt < RETURN_MOST {
                assert!(h.plant_weight() > 0.999, "held until the free arm is there");
            }
            if (i as f32 + 1.0) * dt >= RETURN_MOST + RETURN_FADE + dt {
                assert!(h.plant_weight() < 1e-3, "handed back by then: frame {i}");
            }
        }
        assert_eq!(h.home_share(), 1.0);
        assert!(h.done() && !h.planted(), "handed back, no reach again");
        // Pushed off: the shoulder past the arm's reach from the plant.
        let mut h = Hand::new(1);
        h.update(
            Some(&s),
            [0.0, 44.5, 100.0],
            [0.0, 30.0, 100.0],
            [0.0, 30.0, 100.0],
            shoulder,
            60.0,
            [0.0, 60.0, 100.0],
            0.1,
            dt,
            None,
        );
        assert!(h.planted());
        let e = h.update(
            Some(&s),
            [0.0, 44.5, 100.0],
            [0.0, 30.0, 100.0],
            [0.0, 30.0, 100.0],
            [0.0, -30.0, 100.0],
            60.0,
            [0.0, 60.0, 100.0],
            0.2,
            dt,
            None,
        );
        assert_eq!(e, Some(Event::Released("pushed off")));
    }

    /// The plant's arm: a goal at the wrist changes nothing; a nearer goal bends the elbow in its
    /// own plane, the wrist on the goal.
    #[test]
    fn the_planted_arm_bends_in_place() {
        use crate::flinch::{apply_m, length, mul};
        let (s, e, w) = ([0.0, 0.0, 100.0], [0.0, 10.0, 75.0], [0.0, 35.0, 70.0]);
        let (r1, r2) = plant_arm(s, e, w, w, 1.0, [0.0, 1.0, 0.0], 1.0).unwrap();
        let moved = add(
            add(s, apply_m(&r1, sub(e, s))),
            apply_m(&mul(&r1, &r2), sub(w, e)),
        );
        assert!(length(sub(moved, w)) < 0.05, "{moved:?}");
        assert!(length(sub(apply_m(&r1, sub(e, s)), sub(e, s))) < 0.05);
        let goal = [0.0, 28.0, 72.0];
        let (r1, r2) = plant_arm(s, e, w, goal, 1.0, [0.0, 1.0, 0.0], 1.0).unwrap();
        let elbow = add(s, apply_m(&r1, sub(e, s)));
        let wrist = add(elbow, apply_m(&mul(&r1, &r2), sub(w, e)));
        assert!(length(sub(wrist, goal)) < 0.1, "{wrist:?}");
        assert!(
            elbow[0].abs() < 0.01,
            "the elbow stays in its plane: {elbow:?}"
        );
    }

    #[test]
    fn armed_bodies_brace_one_handed() {
        let both = [true, true];
        assert_eq!(bracing_arms(both, false, 1.0, 1.5, both), [true, true]);
        assert_eq!(bracing_arms(both, true, 1.0, 1.5, both), [true, false]);
        assert_eq!(
            bracing_arms(both, true, 2.0, 1.5, both),
            [true, true],
            "a hard push"
        );
        assert_eq!(
            bracing_arms(both, true, 1.0, 1.5, [false, true]),
            [true, true],
            "out of reach"
        );
        assert_eq!(
            bracing_arms(both, true, 1.0, 1.5, [false, false]),
            [true, false],
            "neither reaches"
        );
        assert_eq!(
            bracing_arms([false, true], true, 1.0, 1.5, both),
            [false, true],
            "no left arm"
        );
    }
}
