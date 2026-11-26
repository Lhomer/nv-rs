//! Each NPC's own Delirium settings, from its FNV data (Patrick, 2026-11-26): a profile per actor
//! from the start, never one global body. Delirium has the settings (a body's weight, its size,
//! the agent's strength, its balance's stiffness, its extra reactions, near misses, avoiding)
//! but no idea of armor or personality; this layer maps what FNV knows of an actor (its race and
//! skeleton, its weight slider, NPC or creature, the body armor it wears, power armor included;
//! later its traits) onto them, through the table `data/profiles.tsv`.
//!
//! Every row of that table is neutral for now (Delirium's own body and tuning, at the actor's own
//! size): the balance numbers wait for Patrick. Changing the table is all a tuning takes.

/// The table ([`table`] reads it).
pub const TABLE: &str = include_str!("../data/profiles.tsv");

/// What FNV knows of an actor, as the profile's rows match it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub race: Race,
    pub actor: Actor,
    pub armor: Armor,
    /// The NPC's weight slider (FNV's 0 to 100; 50 when it has none).
    pub weight: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Race {
    /// FNV's human races (on its human skeleton).
    Human,
    /// Ghouls (on the human skeleton too).
    Ghoul,
    /// Anything else (super mutants, creatures): not Delirium's yet.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    Npc,
    Creature,
}

/// The body armor worn (its upper body slot's armor type; power armor by its flag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Armor {
    None,
    Light,
    Medium,
    Heavy,
    Power,
}

/// An actor's Delirium settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// The body's weight (kg; none: Delirium's own body's).
    pub kg: Option<f32>,
    /// The body's size over the actor's own skeleton (1: as fitted to it).
    pub scale: f32,
    /// The agent's strength (Delirium's `Agent::strength`, 1 its own).
    pub strength: f32,
    /// The balance's stiffness over Delirium's own (1).
    pub stiffness: f32,
    /// Delirium's extra reactions (its SDK's `NM_XR_*` bits; none: its own default set).
    pub reactions: Option<i32>,
    pub near_miss: bool,
    pub avoid: bool,
}

impl Default for Profile {
    /// Delirium's own body and tuning: neutral.
    fn default() -> Self {
        Profile {
            kg: None,
            scale: 1.0,
            strength: 1.0,
            stiffness: 1.0,
            reactions: None,
            near_miss: true,
            avoid: true,
        }
    }
}

/// One row of the table: what it matches and the columns it sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub matches: String,
    pub kg: Option<f32>,
    pub scale: Option<f32>,
    pub strength: Option<f32>,
    pub stiffness: Option<f32>,
    /// `Some(None)`: Delirium's default set; `Some(Some(bits))`: those.
    pub reactions: Option<Option<i32>>,
    pub near_miss: Option<bool>,
    pub avoid: Option<bool>,
    pub note: String,
}

/// The matches a row may have.
pub const MATCHES: [&str; 13] = [
    "default",
    "race:human",
    "race:ghoul",
    "actor:npc",
    "actor:creature",
    "armor:none",
    "armor:light",
    "armor:medium",
    "armor:heavy",
    "armor:power",
    "weight:thin",
    "weight:average",
    "weight:heavy",
];

/// The table's rows, or the first line that isn't one (its number and why).
pub fn read(text: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = |why: &str| format!("line {}: {why}: {line}", i + 1);
        let mut words = line.split_whitespace();
        let mut next = |what: &str| words.next().ok_or_else(|| bad(&format!("no {what}")));
        let matches = next("match")?.to_string();
        if !MATCHES.contains(&matches.as_str()) {
            return Err(bad("not a match the profiles know"));
        }
        let number = |w: &str, lo: f32, hi: f32| -> Result<Option<f32>, String> {
            if w == "-" {
                return Ok(None);
            }
            match w.parse::<f32>() {
                Ok(v) if (lo..=hi).contains(&v) => Ok(Some(v)),
                _ => Err(bad(&format!("{w} isn't a number from {lo} to {hi}"))),
            }
        };
        let flag = |w: &str| -> Result<Option<bool>, String> {
            match w {
                "-" => Ok(None),
                "on" => Ok(Some(true)),
                "off" => Ok(Some(false)),
                _ => Err(bad(&format!("{w} isn't on, off or -"))),
            }
        };
        let kg = number(next("kg")?, 20.0, 400.0)?;
        let scale = number(next("scale")?, 0.5, 2.0)?;
        // Delirium's own range for an agent's strength (its SDK's `nm_agent_set_strength`).
        let strength = number(next("strength")?, 0.1, 10.0)?;
        let stiffness = number(next("stiffness")?, 0.1, 10.0)?;
        let reactions = match next("reactions")? {
            "-" => None,
            "default" => Some(None),
            w => match w.parse::<i32>() {
                Ok(b) if (0..512).contains(&b) => Some(Some(b)),
                _ => {
                    return Err(bad(&format!(
                        "{w} isn't default, -, or the reactions' bits"
                    )))
                }
            },
        };
        let near_miss = flag(next("near_miss")?)?;
        let avoid = flag(next("avoid")?)?;
        let note = words.collect::<Vec<_>>().join(" ");
        if note.is_empty() {
            return Err(bad("no note"));
        }
        rows.push(Row {
            matches,
            kg,
            scale,
            strength,
            stiffness,
            reactions,
            near_miss,
            avoid,
            note,
        });
    }
    Ok(rows)
}

/// [`TABLE`]'s rows (the build's tests check it reads).
pub fn table() -> &'static [Row] {
    static ROWS: std::sync::OnceLock<Vec<Row>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| read(TABLE).unwrap_or_default())
}

/// Whether the row's match fits `f`.
fn fits(m: &str, f: &Facts) -> bool {
    match m {
        "default" => true,
        "race:human" => f.race == Race::Human,
        "race:ghoul" => f.race == Race::Ghoul,
        "actor:npc" => f.actor == Actor::Npc,
        "actor:creature" => f.actor == Actor::Creature,
        "armor:none" => f.armor == Armor::None,
        "armor:light" => f.armor == Armor::Light,
        "armor:medium" => f.armor == Armor::Medium,
        "armor:heavy" => f.armor == Armor::Heavy,
        "armor:power" => f.armor == Armor::Power,
        "weight:thin" => f.weight < 33,
        "weight:average" => (33..=66).contains(&f.weight),
        "weight:heavy" => f.weight > 66,
        _ => false,
    }
}

/// `f`'s profile by `rows`: each fitting row's set columns over the earlier ones'.
pub fn profile_by(rows: &[Row], f: &Facts) -> Profile {
    let mut p = Profile::default();
    for r in rows.iter().filter(|r| fits(&r.matches, f)) {
        if let Some(v) = r.kg {
            p.kg = Some(v);
        }
        p.scale = r.scale.unwrap_or(p.scale);
        p.strength = r.strength.unwrap_or(p.strength);
        p.stiffness = r.stiffness.unwrap_or(p.stiffness);
        if let Some(v) = r.reactions {
            p.reactions = v;
        }
        p.near_miss = r.near_miss.unwrap_or(p.near_miss);
        p.avoid = r.avoid.unwrap_or(p.avoid);
    }
    p
}

/// `f`'s profile by the table.
pub fn profile(f: &Facts) -> Profile {
    profile_by(table(), f)
}

/// Whether Delirium takes this actor at all (humans only for now: FNV's human skeleton).
pub fn takes(f: &Facts) -> bool {
    f.actor == Actor::Npc && matches!(f.race, Race::Human | Race::Ghoul)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(armor: Armor, weight: u8) -> Facts {
        Facts {
            race: Race::Human,
            actor: Actor::Npc,
            armor,
            weight,
        }
    }

    /// The table reads, has a row for every match the profiles know (in that order), and every
    /// row is neutral for now: each actor's profile is Delirium's own.
    #[test]
    fn the_table_reads_and_is_neutral() {
        let rows = read(TABLE).unwrap();
        let ms: Vec<&str> = rows.iter().map(|r| r.matches.as_str()).collect();
        assert_eq!(ms, MATCHES);
        assert_eq!(table().len(), rows.len());
        for armor in [
            Armor::None,
            Armor::Light,
            Armor::Medium,
            Armor::Heavy,
            Armor::Power,
        ] {
            for weight in [0, 50, 100] {
                for race in [Race::Human, Race::Ghoul] {
                    let f = Facts {
                        race,
                        ..facts(armor, weight)
                    };
                    assert_eq!(profile(&f), Profile::default(), "{f:?}");
                }
            }
        }
    }

    /// Rows apply in order, each one's set columns over the earlier ones', and only rows that fit.
    #[test]
    fn rows_apply_in_order_where_they_fit() {
        let text = "default - 1 1 1 default on on all\n\
                    armor:power 180 - 2 3 - off - power armor\n\
                    weight:heavy 110 1.1 - - 5 - off heavy\n\
                    armor:light - - 0.5 - - - - light\n";
        let rows = read(text).unwrap();
        let p = profile_by(&rows, &facts(Armor::Power, 90));
        assert_eq!(p.kg, Some(110.0));
        assert_eq!((p.scale, p.strength, p.stiffness), (1.1, 2.0, 3.0));
        assert_eq!((p.reactions, p.near_miss, p.avoid), (Some(5), false, false));
        assert_eq!(profile_by(&rows, &facts(Armor::Light, 50)).strength, 0.5);
        assert_eq!(
            profile_by(&rows, &facts(Armor::None, 50)),
            Profile::default()
        );
    }

    /// A line the profiles can't use says which and why.
    #[test]
    fn a_bad_line_is_refused() {
        for (line, why) in [
            ("armor:titanium - - - - - - - x", "not a match"),
            ("default 5 - - - - - - x", "isn't a number from 20"),
            ("default - - 50 - - - - x", "from 0.1 to 10"),
            ("default - - - - lots - - x", "reactions"),
            ("default - - - - - maybe - x", "isn't on, off"),
            ("default - - - - - - -", "no note"),
            ("default - -", "no strength"),
        ] {
            let e = read(line).unwrap_err();
            assert!(e.contains(why) && e.starts_with("line 1"), "{line}: {e}");
        }
    }

    /// Humans and ghouls (NPCs on FNV's human skeleton) go to Delirium; creatures and other
    /// races keep New Physics as it is.
    #[test]
    fn only_humans_go_to_delirium() {
        assert!(takes(&facts(Armor::Power, 50)));
        assert!(takes(&Facts {
            race: Race::Ghoul,
            ..facts(Armor::None, 50)
        }));
        assert!(!takes(&Facts {
            race: Race::Other,
            ..facts(Armor::None, 50)
        }));
        assert!(!takes(&Facts {
            actor: Actor::Creature,
            ..facts(Armor::None, 50)
        }));
    }
}
