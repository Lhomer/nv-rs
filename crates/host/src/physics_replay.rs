//! The hit reactions' replays: what the game's self-test (`crate::physics_selftest`) did to an
//! actor, frame by frame, as plain text the CI can run again over the stand-in body
//! (`crate::delirium_stand_in`, through New Physics' own path on the tests' mannequin). Only the
//! game-side inputs: each frame's seconds, the hits (the body struck, the impulse, its direction),
//! the death's frame, the ground's slope under the actor and what the scenario expects. Nothing of
//! GTA IV's (no Delirium numbers, nothing read from its files) and no pose or file of the game's:
//! a replay is safe to paste anywhere.
//!
//! ```text
//! nv-rs physics replay 1
//! scenario light-standing
//! expect stays 67.0
//! slope 0.000 0.000
//! bumps 12.0 100.0
//! dt 0.0083 0.0081 ...
//! frames 600 0.0083
//! hit 120 Bip01_Spine2 25.9 0.000 -1.000 0.000
//! kill 900
//! end
//! ```

/// A replay's ground grid's spacing (game units: 0.35 m).
pub const GRID_SPACING: f32 = 0.35 * ragdoll_core::np_body::UNITS;

/// A scenario's expectation (`crate::physics_checks::Expect`) and its inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    pub name: String,
    /// `stays`, `rises` or `dies`, and the standing pelvis height (game units).
    pub expect: String,
    pub standing: f32,
    /// The ground's rise along x and y (units a unit) under the actor.
    pub slope: [f32; 2],
    /// Its bumps (a trench's dirt): a bump's height and length (game units); 0: smooth.
    pub bumps: [f32; 2],
    /// The ground as the game's self-test cast it there (5 by 5 heights, row by row along y,
    /// [`GRID_SPACING`] apart, against the actor's spot; game units); empty: none (the bumps
    /// stand for it).
    pub grid: Vec<f32>,
    /// Each frame's seconds.
    pub dt: Vec<f32>,
    /// The hits: the frame, the body's node, the impulse (N s), its direction.
    pub hits: Vec<(usize, String, f32, [f32; 3])>,
    /// The frame the actor died, if it did.
    pub kill: Option<usize>,
    /// The phone's range's obstacles stood behind the actor, as the game's self-test placed them
    /// (`crate::phone_range::Piece::spot`): a railing, a half wall, a table and a low wall.
    pub obstacles: bool,
}

impl Scenario {
    pub fn expect(&self) -> crate::physics_checks::Expect {
        crate::physics_checks::Expect {
            stays_standing: self.expect == "stays",
            rises: self.expect == "rises",
            dies: self.expect == "dies",
            standing: self.standing,
        }
    }
}

/// The text of `scenarios` (one file, ASCII).
pub fn write(scenarios: &[Scenario]) -> String {
    let mut out = String::from("nv-rs physics replay 1\n");
    for s in scenarios {
        out += &format!("scenario {}\n", s.name);
        out += &format!("expect {} {:.1}\n", s.expect, s.standing);
        out += &format!("slope {:.4} {:.4}\n", s.slope[0], s.slope[1]);
        if s.bumps[0] > 0.0 {
            out += &format!("bumps {:.1} {:.1}\n", s.bumps[0], s.bumps[1]);
        }
        if s.grid.len() == 25 {
            let g: Vec<String> = s.grid.iter().map(|z| format!("{z:.1}")).collect();
            out += &format!("grid {}\n", g.join(" "));
        }
        for chunk in s.dt.chunks(32) {
            let line: Vec<String> = chunk.iter().map(|d| format!("{d:.5}")).collect();
            out += &format!("dt {}\n", line.join(" "));
        }
        for (frame, body, p, d) in &s.hits {
            out += &format!(
                "hit {frame} {} {p:.2} {:.4} {:.4} {:.4}\n",
                body.replace(' ', "_"),
                d[0],
                d[1],
                d[2]
            );
        }
        if let Some(k) = s.kill {
            out += &format!("kill {k}\n");
        }
        if s.obstacles {
            out += "obstacles\n";
        }
        out += "end\n";
    }
    out
}

/// The scenarios of a replay's text; an error names the line.
pub fn parse(text: &str) -> Result<Vec<Scenario>, String> {
    let mut out = Vec::new();
    let mut cur: Option<Scenario> = None;
    for (n, line) in text.lines().enumerate() {
        let bad = |what: &str| format!("line {}: {what}: {line}", n + 1);
        let w: Vec<&str> = line.split_whitespace().collect();
        let f = |i: usize| -> Result<f32, String> {
            w.get(i)
                .and_then(|x| x.parse::<f32>().ok())
                .filter(|x| x.is_finite())
                .ok_or_else(|| bad("a number"))
        };
        match w.first().copied() {
            None => {}
            Some("nv-rs") => {}
            Some(c) if c.starts_with('#') => {}
            Some("scenario") => {
                cur = Some(Scenario {
                    name: w.get(1).ok_or_else(|| bad("a name"))?.to_string(),
                    expect: String::new(),
                    standing: 0.0,
                    slope: [0.0; 2],
                    bumps: [0.0; 2],
                    grid: Vec::new(),
                    dt: Vec::new(),
                    hits: Vec::new(),
                    kill: None,
                    obstacles: false,
                })
            }
            Some(k) => {
                let s = cur.as_mut().ok_or_else(|| bad("before any scenario"))?;
                match k {
                    "expect" => {
                        s.expect = w.get(1).ok_or_else(|| bad("an expectation"))?.to_string();
                        s.standing = f(2)?;
                    }
                    "slope" => s.slope = [f(1)?, f(2)?],
                    "bumps" => s.bumps = [f(1)?.clamp(0.0, 60.0), f(2)?.clamp(20.0, 400.0)],
                    "grid" => {
                        s.grid = (1..=25)
                            .map(|i| f(i).map(|z| z.clamp(-100.0, 100.0)))
                            .collect::<Result<_, _>>()?;
                    }
                    "dt" => {
                        for i in 1..w.len() {
                            s.dt.push(f(i)?.clamp(0.0, 0.1));
                        }
                    }
                    // So many frames of the same seconds (a replay written by hand).
                    "frames" => {
                        let n = (f(1)? as usize).min(100_000);
                        let dt = f(2)?.clamp(0.0, 0.1);
                        s.dt.extend(std::iter::repeat(dt).take(n));
                    }
                    "hit" => s.hits.push((
                        f(1)? as usize,
                        w.get(2).ok_or_else(|| bad("a body"))?.replace('_', " "),
                        f(3)?,
                        [f(4)?, f(5)?, f(6)?],
                    )),
                    "kill" => s.kill = Some(f(1)? as usize),
                    "obstacles" => s.obstacles = true,
                    "end" => out.extend(cur.take()),
                    _ => return Err(bad("unknown line")),
                }
            }
        }
    }
    out.extend(cur);
    Ok(out)
}

/// The CI's own replay when none is given: the self-test's scenarios at 120 frames a second
/// (Patrick's PC), on flat ground and on a slope, with his 9mm's impulse (25.9 N s) and a hit past
/// the knockdown threshold (160 N s).
pub fn synthetic() -> Vec<Scenario> {
    let dt = 1.0 / 120.0;
    let frames = |s: f32| vec![dt; (s / dt) as usize];
    let s = |name: &str,
             expect: &str,
             seconds: f32,
             slope: [f32; 2],
             hits: Vec<(usize, &str, f32, [f32; 3])>,
             kill: Option<usize>| Scenario {
        name: name.into(),
        expect: expect.into(),
        standing: 0.0,
        slope,
        bumps: [0.0; 2],
        grid: Vec::new(),
        dt: frames(seconds),
        hits: hits
            .into_iter()
            .map(|(f, b, p, d)| (f, b.to_string(), p, d))
            .collect(),
        kill,
        obstacles: false,
    };
    let back = [0.0, -1.0, 0.0];
    vec![
        s(
            "light-standing",
            "stays",
            4.0,
            [0.0; 2],
            vec![(60, "Bip01 Spine2", 25.9, back)],
            None,
        ),
        s(
            "light-head",
            "stays",
            4.0,
            [0.0; 2],
            vec![(60, "Bip01 Head", 25.9, back)],
            None,
        ),
        s(
            "light-staggering",
            "stays",
            4.0,
            [0.0; 2],
            vec![
                (60, "Bip01 Spine2", 25.9, back),
                (90, "Bip01 Spine2", 25.9, back),
            ],
            None,
        ),
        s(
            "heavy-rises",
            "rises",
            14.0,
            [0.0; 2],
            vec![(60, "Bip01 Spine2", 160.0, back)],
            None,
        ),
        s(
            "heavy-rises-slope",
            "rises",
            14.0,
            [0.12, 0.0],
            vec![(60, "Bip01 Spine2", 160.0, back)],
            None,
        ),
        s(
            "downed-hit",
            "rises",
            16.0,
            [0.0; 2],
            vec![
                (60, "Bip01 Spine2", 160.0, back),
                (420, "Bip01 R Forearm", 25.9, back),
            ],
            None,
        ),
        // Patrick's 23:16 clip: a light 9mm on a Powder Ganger standing in a trench's uneven dirt
        // (it must stay up), then a heavy hit and three more 9mm hits once it is down, a quarter
        // second apart, before its rise (New Physics settles under a second on flat ground; it must rise,
        // its hands planted, nothing launched, stretched, sunk in the dirt or sliding).
        Scenario {
            bumps: [6.0, 140.0],
            ..s(
                "trench",
                "rises",
                22.0,
                [0.0; 2],
                vec![
                    (60, "Bip01 Spine2", 25.9, back),
                    (540, "Bip01 Spine2", 160.0, back),
                    (650, "Bip01 R Forearm", 25.9, back),
                    (680, "Bip01 Spine2", 25.9, back),
                    (710, "Bip01 L Calf", 25.9, back),
                ],
                None,
            )
        },
        s(
            "dies-downed",
            "dies",
            6.0,
            [0.0; 2],
            vec![(60, "Bip01 Spine2", 160.0, back)],
            Some(360),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A replay's text reads back as written.
    #[test]
    fn a_replay_reads_back_as_written() {
        let scenarios = synthetic();
        let text = write(&scenarios);
        assert!(text.is_ascii());
        let back = parse(&text).unwrap();
        assert_eq!(back.len(), scenarios.len());
        for (a, b) in scenarios.iter().zip(&back) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.expect, b.expect);
            assert_eq!(a.dt.len(), b.dt.len());
            assert_eq!(a.hits.len(), b.hits.len());
            assert_eq!(a.hits[0].1, b.hits[0].1);
            assert_eq!(a.kill, b.kill);
            assert_eq!(a.bumps, b.bumps);
            assert_eq!(a.grid, b.grid);
        }
        assert!(parse("hit 1 x 2 0 0 0").is_err());
    }
}
