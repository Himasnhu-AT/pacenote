//! The career save: `<DataDir>/career.json` (GAME_SYSTEMS §4.4, DECISIONS E9).
//!
//! **Owner: stream F (GAMEPLAY).** Versioned, written atomically (temp file + rename, through
//! `engine::paths::write_atomic`). A missing file starts a fresh career; a file that doesn't
//! parse (or comes from a newer version) is kept as `career.bad.json` and the game starts fresh
//! with a notice. Offline only, so there is no anti-tamper.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::garage::Garage;
use super::grade::Grade;
use super::wallet::Wallet;

/// The save format's version. Bump it (and migrate in [`load`]) when a field changes meaning.
pub const VERSION: u32 = 1;
/// Where a save that couldn't be read is kept.
pub const BAD_FILE: &str = "career.bad.json";
/// Ledger lines kept (the Logbook's "last 50").
pub const LEDGER_LEN: usize = 50;

/// Where the car was in free roam (Continue puts it back there). E reads it (DECISIONS E8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldPose {
    pub pos: [f32; 3],
    /// Yaw (rad), as `vehicle::Car::new` takes it.
    pub heading: f32,
}

/// A challenge's best and stars (written by `open::challenges`, F1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChallengeProgress {
    pub best: f32,
    pub stars: u8,
}

/// Best finishing position per difficulty (0 = never finished), for one event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RaceProgress {
    pub rookie: u8,
    pub pro: u8,
    pub ace: u8,
    pub legend: u8,
}

impl RaceProgress {
    pub fn best(&self, d: super::racers::Difficulty) -> u8 {
        use super::racers::Difficulty::*;
        match d {
            Rookie => self.rookie,
            Pro => self.pro,
            Ace => self.ace,
            Legend => self.legend,
        }
    }
    pub fn best_mut(&mut self, d: super::racers::Difficulty) -> &mut u8 {
        use super::racers::Difficulty::*;
        match d {
            Rookie => &mut self.rookie,
            Pro => &mut self.pro,
            Ace => &mut self.ace,
            Legend => &mut self.legend,
        }
    }
    /// Has this difficulty been won (its first-win bonus paid)?
    pub fn won(&self, d: super::racers::Difficulty) -> bool {
        self.best(d) == 1
    }
}

/// Lifetime numbers for the Logbook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub style_banked: u64,
    pub best_rhythm: u32,
    pub km: f32,
    pub top_kmh: f32,
    pub longest_air_m: f32,
    pub close_shaves: u32,
    pub wins: u32,
    pub races: u32,
}

/// One ledger line: what moved, how much (+ in, − out).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LedgerLine {
    pub what: String,
    pub amount: i64,
    /// Seconds since the Unix epoch.
    pub at: u64,
}

/// Everything the career keeps besides the wallet, garage and grade.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CareerProgress {
    /// Free-roam pose (Continue).
    pub world: Option<WorldPose>,
    /// By `ChallengeDef::id`.
    pub challenges: BTreeMap<u32, ChallengeProgress>,
    /// By event id.
    pub races: BTreeMap<u32, RaceProgress>,
    pub stats: Stats,
    /// Newest last, at most [`LEDGER_LEN`].
    pub ledger: Vec<LedgerLine>,
    /// The mode the last event was started in (the marker's default): "race" or "trial".
    pub last_mode: String,
    /// The last race difficulty picked (0 Rookie .. 3 Legend).
    pub last_difficulty: u8,
}

impl CareerProgress {
    /// Adds a ledger line, dropping the oldest past [`LEDGER_LEN`].
    pub fn log(&mut self, what: impl Into<String>, amount: i64) {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.ledger.push(LedgerLine { what: what.into(), amount, at });
        if self.ledger.len() > LEDGER_LEN {
            let extra = self.ledger.len() - LEDGER_LEN;
            self.ledger.drain(..extra);
        }
    }
}

/// The file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CareerFile {
    pub version: u32,
    pub wallet: Wallet,
    pub garage: Garage,
    pub grade: Grade,
    /// Pose, challenges, races, stats, ledger (one object: serde can't flatten maps with
    /// number keys).
    pub progress: CareerProgress,
}

impl Default for CareerFile {
    fn default() -> Self {
        CareerFile { version: VERSION, wallet: Wallet::default(), garage: Garage::default(), grade: Grade::default(), progress: CareerProgress::default() }
    }
}

impl CareerFile {
    /// Fixes what a hand-edited or older file could get wrong: an empty garage, an active
    /// index out of range, cars no longer in the roster, levels past a part's top.
    pub fn sanitise(mut self) -> CareerFile {
        self.garage.owned.retain(|c| c.def().is_some());
        if self.garage.owned.is_empty() {
            self.garage = Garage::default();
        }
        if self.garage.active >= self.garage.owned.len() {
            self.garage.active = 0;
        }
        for c in &mut self.garage.owned {
            for k in super::garage::UpgradeKind::ALL {
                let b = c.bought.get(k);
                c.bought.set(k, b);
                let f = c.fitted.get(k).min(c.bought.get(k));
                c.fitted.set(k, f);
            }
            c.number = c.number.clamp(1, 99);
            c.damage = c.damage.clamp(0.0, 1.0);
        }
        self.version = VERSION;
        self
    }
}

/// How loading went.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded {
    /// Read and parsed.
    Ok(CareerFile),
    /// No file: a fresh career.
    Fresh,
    /// Unreadable or not ours: kept aside as [`BAD_FILE`], a fresh career (the reason).
    Bad(String),
}

/// Reads `path`. A corrupt file is renamed to [`BAD_FILE`] next to it.
pub fn load(path: &Path) -> Loaded {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Fresh,
        Err(e) => return set_aside(path, format!("can't read it: {e}")),
    };
    match serde_json::from_str::<CareerFile>(&text) {
        Ok(f) if f.version > VERSION => set_aside(path, format!("it is from a newer version ({})", f.version)),
        Ok(f) => Loaded::Ok(f.sanitise()),
        Err(e) => set_aside(path, format!("it doesn't parse: {e}")),
    }
}

fn set_aside(path: &Path, why: String) -> Loaded {
    let bad = path.with_file_name(BAD_FILE);
    if let Err(e) = std::fs::rename(path, &bad) {
        warn!("career: couldn't move the bad save aside: {e}");
    }
    warn!("career: {} ({why}); kept as {} and starting fresh", path.display(), bad.display());
    Loaded::Bad(why)
}

/// Writes `f` to `path` atomically.
pub fn save(path: &Path, f: &CareerFile) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(f).map_err(std::io::Error::other)?;
    engine::paths::write_atomic(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career::garage::{OwnedCar, def};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pacenote-career-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join(super::super::CAREER_FILE)
    }

    #[test]
    fn round_trip() {
        let p = tmp("rt");
        assert_eq!(load(&p), Loaded::Fresh);
        let mut f = CareerFile::default();
        f.wallet = Wallet { balance: 4_321, earned: 10_000, spent: 17_679 };
        f.garage.owned.push(OwnedCar::stock(def("vale").unwrap()));
        f.garage.owned[1].bought.engine = 2;
        f.garage.owned[1].fitted.engine = 1;
        f.garage.active = 1;
        f.grade.xp = 12_345;
        f.progress.world = Some(WorldPose { pos: [1.0, 2.0, 3.0], heading: 0.5 });
        f.progress.races.insert(101, RaceProgress { pro: 2, ..default() });
        f.progress.challenges.insert(7, ChallengeProgress { best: 131.5, stars: 2 });
        f.progress.log("Purse P2", 1_155);
        save(&p, &f).unwrap();
        assert!(!p.with_extension("json.tmp").exists(), "no temp file left behind");
        assert_eq!(load(&p), Loaded::Ok(f));
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_and_the_career_starts_fresh() {
        let p = tmp("bad");
        std::fs::write(&p, "{ \"version\": 1, \"wallet\": { \"balance\": ").unwrap();
        assert!(matches!(load(&p), Loaded::Bad(_)));
        assert!(!p.exists() && p.with_file_name(BAD_FILE).exists());
        assert_eq!(load(&p), Loaded::Fresh);
        // From a newer game: not ours to overwrite.
        std::fs::write(&p, "{ \"version\": 99 }").unwrap();
        assert!(matches!(load(&p), Loaded::Bad(_)));
    }

    #[test]
    fn a_hand_edited_file_is_made_sane() {
        let p = tmp("sane");
        std::fs::write(&p, r#"{ "version": 1, "garage": { "owned": [ { "model": "nope" } ], "active": 5 } }"#).unwrap();
        let Loaded::Ok(f) = load(&p) else { panic!() };
        assert_eq!(f.garage, Garage::default());
        assert_eq!(f.wallet, Wallet::default(), "missing fields take their defaults");
        let mut g = CareerFile::default();
        g.garage.owned[0].fitted.engine = 3;
        let g = g.sanitise();
        assert_eq!(g.garage.owned[0].fitted.engine, 0, "can't have a level fitted that wasn't bought");
    }

    #[test]
    fn the_ledger_keeps_the_last_50() {
        let mut p = CareerProgress::default();
        for i in 0..60 {
            p.log(format!("line {i}"), i);
        }
        assert_eq!(p.ledger.len(), LEDGER_LEN);
        assert_eq!(p.ledger[0].what, "line 10");
    }
}
