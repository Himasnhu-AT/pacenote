//! Leaderboards: per-stage records, ranking, merging and the exchange file. Pure Rust (no
//! Bevy systems), tested.
//!
//! - A [`Record`] is one team's run on one stage: driver + guide names, clock time,
//!   penalties, splits, date, game version and the hash of the stage it was driven on.
//! - The [`Board`] keeps **each team's best per stage and stage hash**: a run only replaces
//!   the team's record when it is faster. Records driven on an older road (the generator
//!   changed, so the hash differs) stay, flagged stale, and rank below current ones.
//! - [`Board::merge`] folds another board in (a friend's export) with the same rule, so
//!   merging is commutative and idempotent.
//! - Each record has a content `id` (hash of stage, team, time, date, stage hash): a future
//!   online board can upsert records by id and keep the same "best per team" rule.

use serde::{Deserialize, Serialize};

use super::ghost::Ghost;

/// File format version of [`Board`] and [`BoardFile`].
pub const BOARD_FORMAT: u32 = 1;

/// One team's run on a stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Record {
    /// Content hash (hex): stable across machines, for dedupe and online sync.
    pub id: String,
    pub stage: u32,
    pub driver: String,
    /// Empty for solo practice.
    pub guide: String,
    /// Stage clock (s), without penalties.
    pub time: f32,
    /// Penalty seconds (recoveries).
    pub penalty: f32,
    /// Cumulative times (with penalties) at each quarter of the stage.
    pub splits: Vec<f32>,
    /// Unix seconds (UTC).
    pub date: u64,
    /// Game version that drove it.
    pub version: String,
    /// [`super::stage_hash`] of the road it was driven on (hex).
    pub stage_hash: String,
    /// [`crate::vehicle::REVISION`] of the car that drove it (0: before handling was
    /// versioned). Only runs with the current handling rank; older ones list as stale.
    pub handling: u32,
}

/// The handling revision new runs are driven with.
pub fn handling() -> u32 {
    crate::vehicle::REVISION as u32
}

impl Record {
    /// Builds a record and fills in its id.
    #[allow(clippy::too_many_arguments)]
    pub fn new(stage: u32, driver: &str, guide: &str, time: f32, penalty: f32, splits: Vec<f32>, date: u64, version: &str, stage_hash: &str) -> Record {
        let mut r = Record {
            id: String::new(),
            stage,
            driver: clean_name(driver),
            guide: clean_name(guide),
            time,
            penalty,
            splits,
            date,
            version: version.into(),
            stage_hash: stage_hash.into(),
            handling: handling(),
        };
        r.id = r.content_id();
        r
    }
    /// Time + penalties: what ranks.
    pub fn total(&self) -> f32 {
        self.time + self.penalty
    }
    pub fn solo(&self) -> bool {
        self.guide.is_empty()
    }
    /// Who drove it: case- and space-insensitive "driver|guide".
    pub fn team(&self) -> String {
        team_key(&self.driver, &self.guide)
    }
    /// "Driver & Guide" (or "Driver (solo)").
    pub fn team_label(&self) -> String {
        team_label(&self.driver, &self.guide)
    }
    pub fn content_id(&self) -> String {
        let mut h = Hasher::default();
        h.u64(self.stage as u64);
        h.str(&self.team());
        h.u64(self.time.to_bits() as u64);
        h.u64(self.penalty.to_bits() as u64);
        h.u64(self.date);
        h.str(&self.stage_hash);
        format!("{:016x}", h.0)
    }
    /// Ranking order: faster first, then the earlier run, then by team (total order).
    pub fn rank_cmp(&self, o: &Record) -> std::cmp::Ordering {
        self.total().total_cmp(&o.total()).then(self.date.cmp(&o.date)).then_with(|| self.team().cmp(&o.team()))
    }
    fn slot(&self) -> (u32, String, String, u32) {
        (self.stage, self.team(), self.stage_hash.clone(), self.handling)
    }
}

/// Trims, collapses inner whitespace and caps names at 24 characters.
pub fn clean_name(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().filter(|c| !c.is_control() && *c != '|').take(24).collect()
}

/// A board hash as (road hash, conditions): `"abc@rain-dusk"` → `("abc", "rain-dusk")`,
/// `"abc"` → `("abc", "")` (the stage's default weather and time).
pub fn split_hash(h: &str) -> (&str, &str) {
    h.split_once('@').unwrap_or((h, ""))
}

pub fn team_key(driver: &str, guide: &str) -> String {
    format!("{}|{}", clean_name(driver).to_lowercase(), clean_name(guide).to_lowercase())
}

pub fn team_label(driver: &str, guide: &str) -> String {
    let (d, g) = (clean_name(driver), clean_name(guide));
    if g.is_empty() { format!("{d} (solo)") } else { format!("{d} & {g}") }
}

/// What [`Board::insert`] did.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Inserted {
    /// The run is the team's new best on this stage (or its first).
    pub team_best: bool,
    /// The run is the fastest current (non-stale) team run on the stage.
    pub stage_record: bool,
    /// 1-based rank among current records of its kind (team or solo) after inserting.
    pub rank: usize,
    /// The team's previous best on this road, if any.
    pub previous: Option<Record>,
}

/// All records, best per team per stage per road hash.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub format: u32,
    pub records: Vec<Record>,
}

impl Default for Board {
    fn default() -> Self {
        Board { format: BOARD_FORMAT, records: Vec::new() }
    }
}

impl Board {
    /// Adds a run; keeps it only if it beats the team's record in its slot.
    pub fn insert(&mut self, r: Record) -> Inserted {
        let slot = r.slot();
        let previous = self.records.iter().find(|o| o.slot() == slot).cloned();
        let team_best = previous.as_ref().is_none_or(|p| r.rank_cmp(p).is_lt());
        if team_best {
            self.records.retain(|o| o.slot() != slot);
            self.records.push(r.clone());
            self.sort();
        }
        let ranked = self.ranking(r.stage, &r.stage_hash, r.solo());
        let mine = if team_best { &r } else { previous.as_ref().unwrap_or(&r) };
        let rank = ranked.iter().position(|o| o.id == mine.id).map_or(ranked.len() + 1, |i| i + 1);
        let stage_record = team_best && !r.solo() && rank == 1;
        Inserted { team_best, stage_record, rank, previous }
    }

    /// Folds `other` in: per slot the better record survives. Returns how many records
    /// were added or improved.
    pub fn merge(&mut self, other: &Board) -> usize {
        let mut changed = 0;
        for r in &other.records {
            let mut r = r.clone();
            r.driver = clean_name(&r.driver);
            r.guide = clean_name(&r.guide);
            if r.id.is_empty() {
                r.id = r.content_id();
            }
            if !(r.time.is_finite() && r.penalty.is_finite() && r.time > 0.0) {
                continue;
            }
            let slot = r.slot();
            match self.records.iter().position(|o| o.slot() == slot) {
                Some(i) if r.rank_cmp(&self.records[i]).is_lt() => {
                    self.records[i] = r;
                    changed += 1;
                }
                Some(_) => {}
                None => {
                    self.records.push(r);
                    changed += 1;
                }
            }
        }
        self.sort();
        changed
    }

    pub fn sort(&mut self) {
        self.records.sort_by(|a, b| a.stage.cmp(&b.stage).then_with(|| a.rank_cmp(b)));
    }

    /// Current records for a stage (driven on the road with `hash`), fastest first.
    pub fn ranking(&self, stage: u32, hash: &str, solo: bool) -> Vec<&Record> {
        let mut v: Vec<&Record> = self.records.iter().filter(|r| r.stage == stage && r.stage_hash == hash && r.solo() == solo && r.handling == handling()).collect();
        v.sort_by(|a, b| a.rank_cmp(b));
        v
    }

    /// Everything on a stage for display: current team runs, then current solo runs, then
    /// stale ones (another road or an older car's handling, same weather and time), each
    /// fastest first. `bool` = stale.
    /// Runs in other weather or at another time of day are another board: not listed.
    pub fn listing(&self, stage: u32, hash: &str) -> Vec<(&Record, bool)> {
        let mut out: Vec<(&Record, bool)> = Vec::new();
        out.extend(self.ranking(stage, hash, false).into_iter().map(|r| (r, false)));
        out.extend(self.ranking(stage, hash, true).into_iter().map(|r| (r, false)));
        let (road, cond) = split_hash(hash);
        let mut stale: Vec<&Record> = self
            .records
            .iter()
            .filter(|r| {
                let (rr, rc) = split_hash(&r.stage_hash);
                r.stage == stage && (rr != road || r.handling != handling()) && rc == cond
            })
            .collect();
        stale.sort_by(|a, b| a.rank_cmp(b));
        out.extend(stale.into_iter().map(|r| (r, true)));
        out
    }

    /// The fastest current team run on a stage.
    pub fn leader(&self, stage: u32, hash: &str) -> Option<&Record> {
        self.ranking(stage, hash, false).into_iter().next()
    }

    /// A team's current record on a stage.
    pub fn best_of(&self, stage: u32, hash: &str, team: &str) -> Option<&Record> {
        self.records.iter().find(|r| r.stage == stage && r.stage_hash == hash && r.team() == team && r.handling == handling())
    }
}

/// The exchange file: a board plus the ghosts that go with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardFile {
    pub format: u32,
    /// Always "pacenote" (so a wrong file is rejected cleanly).
    pub game: String,
    /// Unix seconds.
    pub exported: u64,
    pub records: Vec<Record>,
    pub ghosts: Vec<Ghost>,
}

impl Default for BoardFile {
    fn default() -> Self {
        BoardFile { format: BOARD_FORMAT, game: "pacenote".into(), exported: 0, records: Vec::new(), ghosts: Vec::new() }
    }
}

impl BoardFile {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn from_json(s: &str) -> Result<BoardFile, String> {
        let f: BoardFile = serde_json::from_str(s).map_err(|e| format!("not a board file: {e}"))?;
        if f.game != "pacenote" {
            return Err(format!("board file is for '{}', not pacenote", f.game));
        }
        if f.format > BOARD_FORMAT {
            return Err(format!("board file format {} is newer than this game ({BOARD_FORMAT})", f.format));
        }
        Ok(f)
    }
    pub fn board(&self) -> Board {
        Board { format: BOARD_FORMAT, records: self.records.clone() }
    }
}

/// Small 64-bit hasher on [`pcg_core::mix64`] (stable across platforms and builds, unlike
/// `std`'s `DefaultHasher`).
#[derive(Clone, Copy, Debug)]
pub struct Hasher(pub u64);

impl Default for Hasher {
    fn default() -> Self {
        Hasher(0x9E37_79B9_7F4A_7C15)
    }
}

impl Hasher {
    pub fn u64(&mut self, v: u64) {
        self.0 = pcg_core::mix64(self.0 ^ v.wrapping_mul(0xA24B_AED4_963E_E407));
    }
    pub fn f32(&mut self, v: f32) {
        self.u64(v.to_bits() as u64);
    }
    pub fn str(&mut self, s: &str) {
        for c in s.as_bytes().chunks(8) {
            let mut b = [0u8; 8];
            b[..c.len()].copy_from_slice(c);
            self.u64(u64::from_le_bytes(b));
        }
        self.u64(s.len() as u64);
    }
}

/// Unix seconds now.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "2026-10-28" for unix seconds (UTC).
pub fn date_text(unix: u64) -> String {
    let (y, m, d) = civil_from_days((unix / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// "1:23.456" (or "--:--.---" for none).
pub fn clock(t: f32) -> String {
    if !t.is_finite() || t < 0.0 {
        return "--:--.---".into();
    }
    let ms = (t as f64 * 1000.0).round() as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, (ms / 1000) % 60, ms % 1000)
}

/// "+1.234" / "-0.500" (seconds difference).
pub fn delta(d: f32) -> String {
    let ms = (d.abs() as f64 * 1000.0).round() as u64;
    let sign = if d < 0.0 && ms > 0 { '-' } else { '+' };
    if ms >= 60_000 { format!("{sign}{}:{:02}.{:03}", ms / 60_000, (ms / 1000) % 60, ms % 1000) } else { format!("{sign}{}.{:03}", ms / 1000, ms % 1000) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_weather_is_another_board() {
        let mut b = Board::default();
        let r = |hash: &str, t: f32| Record::new(3, "Ann", "Bo", t, 0.0, vec![], 1, "0.1", hash);
        b.insert(r("aa", 100.0));
        b.insert(r("aa@rain-dusk", 120.0));
        b.insert(r("old", 90.0));
        b.insert(r("old@rain-dusk", 80.0));
        let dry: Vec<(f32, bool)> = b.listing(3, "aa").iter().map(|(r, s)| (r.time, *s)).collect();
        assert_eq!(dry, vec![(100.0, false), (90.0, true)], "the wet time stays off the dry board");
        let wet: Vec<(f32, bool)> = b.listing(3, "aa@rain-dusk").iter().map(|(r, s)| (r.time, *s)).collect();
        assert_eq!(wet, vec![(120.0, false), (80.0, true)]);
        assert_eq!(b.leader(3, "aa").unwrap().time, 100.0);
        assert_eq!(split_hash("aa@fog-noon"), ("aa", "fog-noon"));
    }

    fn rec(stage: u32, d: &str, g: &str, t: f32, date: u64) -> Record {
        Record::new(stage, d, g, t, 0.0, vec![t * 0.25, t * 0.5, t * 0.75], date, "0.1.0", "aa")
    }

    #[test]
    fn old_handling_runs_list_as_stale() {
        let mut b = Board::default();
        // A fast run from before the handling changed (files from then load with 0).
        let mut old = rec(1, "Ann", "Bo", 90.0, 1);
        old.handling = 0;
        old.id = old.content_id();
        b.records.push(old.clone());
        assert!(b.ranking(1, "aa", false).is_empty() && b.leader(1, "aa").is_none());
        // A slower run with today's car still counts as the team's best and the stage record.
        let ins = b.insert(rec(1, "Ann", "Bo", 100.0, 2));
        assert!(ins.team_best && ins.stage_record && ins.rank == 1, "{ins:?}");
        let l = b.listing(1, "aa");
        assert_eq!(l.len(), 2);
        assert!(!l[0].1 && l[0].0.time == 100.0);
        assert!(l[1].1 && l[1].0.time == 90.0, "the old run lists as stale");
    }

    #[test]
    fn insert_keeps_each_teams_best() {
        let mut b = Board::default();
        let first = b.insert(rec(1, "Ann", "Bo", 200.0, 10));
        assert!(first.team_best && first.stage_record && first.rank == 1 && first.previous.is_none());
        let slower = b.insert(rec(1, "ann ", " BO", 210.0, 11));
        assert!(!slower.team_best, "same team (case/space-insensitive), slower: kept the old one");
        assert_eq!(slower.previous.as_ref().unwrap().time, 200.0);
        assert_eq!(b.records.len(), 1);
        let faster = b.insert(rec(1, "Ann", "Bo", 190.0, 12));
        assert!(faster.team_best && faster.stage_record);
        assert_eq!(b.records.len(), 1);
        assert_eq!(b.records[0].time, 190.0);
        let other = b.insert(rec(1, "Cy", "Di", 195.0, 13));
        assert!(other.team_best && !other.stage_record && other.rank == 2);
        // Solo practice ranks on its own and never takes the stage record.
        let solo = b.insert(rec(1, "Cy", "", 150.0, 14));
        assert!(solo.team_best && !solo.stage_record && solo.rank == 1);
        assert_eq!(b.leader(1, "aa").unwrap().team(), "ann|bo");
    }

    #[test]
    fn ranking_sorts_by_total_then_date() {
        let mut b = Board::default();
        let mut slow_pen = rec(2, "A", "B", 100.0, 5);
        slow_pen.penalty = 10.0;
        slow_pen.id = slow_pen.content_id();
        b.insert(slow_pen);
        b.insert(rec(2, "C", "D", 105.0, 7));
        b.insert(rec(2, "E", "F", 105.0, 6));
        let order: Vec<String> = b.ranking(2, "aa", false).iter().map(|r| r.driver.clone()).collect();
        assert_eq!(order, ["E", "C", "A"], "105 (earlier) < 105 (later) < 100 + 10 penalty");
    }

    #[test]
    fn stale_records_are_flagged_and_kept_apart() {
        let mut b = Board::default();
        let mut old = rec(3, "A", "B", 90.0, 1);
        old.stage_hash = "old".into();
        old.id = old.content_id();
        b.insert(old);
        let r = b.insert(rec(3, "A", "B", 120.0, 2));
        assert!(r.team_best && r.stage_record, "a run on the new road counts even if slower than the stale one");
        let l = b.listing(3, "aa");
        assert_eq!(l.len(), 2);
        assert!(!l[0].1 && l[0].0.time == 120.0);
        assert!(l[1].1 && l[1].0.time == 90.0, "stale last");
        assert!(b.leader(3, "aa").unwrap().time == 120.0);
    }

    #[test]
    fn merge_keeps_best_and_is_idempotent_and_commutative() {
        let mut a = Board::default();
        a.insert(rec(1, "A", "B", 100.0, 1));
        a.insert(rec(1, "C", "D", 120.0, 1));
        a.insert(rec(4, "A", "B", 300.0, 1));
        let mut b = Board::default();
        b.insert(rec(1, "A", "B", 95.0, 2));
        b.insert(rec(1, "C", "D", 130.0, 2));
        b.insert(rec(1, "E", "F", 99.0, 2));
        let mut ab = a.clone();
        assert_eq!(ab.merge(&b), 2, "A&B improved, E&F added; C&D's slower run ignored");
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab, ba);
        let before = ab.clone();
        assert_eq!(ab.merge(&before), 0);
        assert_eq!(ab, before);
        assert_eq!(ab.ranking(1, "aa", false).iter().map(|r| r.total()).collect::<Vec<_>>(), [95.0, 99.0, 120.0]);
        // Junk is skipped.
        let mut junk = Board::default();
        junk.records.push(Record { stage: 1, driver: "X".into(), guide: "Y".into(), time: f32::NAN, stage_hash: "aa".into(), ..default_rec() });
        assert_eq!(ab.merge(&junk), 0);
    }

    fn default_rec() -> Record {
        Record::default()
    }

    #[test]
    fn file_round_trips_and_rejects_others() {
        let mut b = Board::default();
        b.insert(rec(1, "Ann", "Bo", 201.25, 1_700_000_000));
        let f = BoardFile { exported: 1, records: b.records.clone(), ..default() };
        let json = f.to_json();
        let back = BoardFile::from_json(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.board().records[0].id, b.records[0].id);
        assert!(BoardFile::from_json(r#"{"game":"other"}"#).is_err());
        assert!(BoardFile::from_json(r#"{"game":"pacenote","format":99}"#).is_err());
        assert!(BoardFile::from_json("not json").is_err());
        // Missing fields default (older/newer minor versions still load).
        let partial = BoardFile::from_json(r#"{"game":"pacenote","records":[{"stage":2,"driver":"A","time":5.0}]}"#).unwrap();
        assert_eq!(partial.records[0].stage, 2);
    }

    fn default() -> BoardFile {
        BoardFile::default()
    }

    #[test]
    fn names_and_formatting() {
        assert_eq!(clean_name("  Mia   \t Kova  "), "Mia Kova");
        assert_eq!(clean_name("a|b"), "ab");
        assert_eq!(clean_name(&"x".repeat(40)).len(), 24);
        assert_eq!(team_label("Mia", ""), "Mia (solo)");
        assert_eq!(team_label("Mia", "Leo"), "Mia & Leo");
        assert_eq!(clock(83.4567), "1:23.457");
        assert_eq!(clock(5.0), "0:05.000");
        assert_eq!(delta(-0.5), "-0.500");
        assert_eq!(delta(1.25), "+1.250");
        assert_eq!(delta(0.0), "+0.000");
        assert_eq!(date_text(0), "1970-01-01");
        assert_eq!(date_text(1_793_145_600), "2026-10-28");
        assert_eq!(date_text(951_782_400), "2000-02-29");
    }

    #[test]
    fn ids_are_stable() {
        let a = rec(1, "Ann", "Bo", 200.0, 10);
        let b = rec(1, " ann", "bo ", 200.0, 10);
        assert_eq!(a.id, b.id);
        assert_ne!(a.id, rec(1, "Ann", "Bo", 200.001, 10).id);
        assert_eq!(a.id.len(), 16);
    }
}
