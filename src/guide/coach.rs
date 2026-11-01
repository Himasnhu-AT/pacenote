//! Call timing: when the guide reads each note, and how well that suited the driver.
//!
//! A real co-driver reads a note a few seconds before the corner: early enough to brake, not
//! so early it's forgotten. The guide sees the ideal moment on the tablet (a bar that fills as
//! the next note comes into reach, green while it's time to call), and gets a word back after
//! each call: EARLY, ON TIME, LATE or MISSED. The run's call-timing score shows at the finish.
//!
//! A "call" is the guide starting to talk (voice chat: the push-to-talk press or open mic
//! opening) or pressing the call key (players without a mic, or to tick a note off). It counts
//! for the first note ahead that hasn't been called, if that note is within [`ACCEPT_S`];
//! talk further out is chatter, not a call. Notes chained to the one before (< 30 m, "into")
//! are covered by that call. A note the car reaches uncalled is MISSED.
//!
//! Timing is in seconds of driving to the note, with the AI co-driver's read-ahead
//! ([`crate::codriver::calls`]): slow, a note is due by distance instead. [`Coach`] is pure and
//! tested; [`track`] feeds it on the guide's machine from the replicated run.

use bevy::prelude::*;

use crate::codriver::calls::{LEAD_MIN, LEAD_S};

use super::{GuideFeed, GuideStage, Note};

/// Ideal call: this many seconds of driving before the note (the AI co-driver's read-ahead).
pub const IDEAL_S: f32 = LEAD_S;
/// On time between these (s to go).
pub const EARLY_S: f32 = 7.0;
pub const LATE_S: f32 = 2.5;
/// Calls further out than this aren't calls (s to go).
pub const ACCEPT_S: f32 = 12.0;
/// Below this speed time-to-go is reckoned at this speed (m/s): the AI's slowest read-ahead.
pub const SLOW_MPS: f32 = LEAD_MIN / LEAD_S;
/// Two calls closer than this (s) are one (open mic flicker, a double press).
pub const DEBOUNCE_S: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Early,
    OnTime,
    Late,
    Missed,
}

impl Grade {
    pub fn points(self) -> u32 {
        match self {
            Grade::OnTime => 100,
            Grade::Early => 50,
            Grade::Late => 40,
            Grade::Missed => 0,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Grade::Early => "EARLY",
            Grade::OnTime => "ON TIME",
            Grade::Late => "LATE",
            Grade::Missed => "MISSED",
        }
    }

    /// From seconds to go when called.
    pub fn of(to_go: f32) -> Grade {
        if to_go > EARLY_S {
            Grade::Early
        } else if to_go >= LATE_S {
            Grade::OnTime
        } else {
            Grade::Late
        }
    }
}

/// Seconds of driving from `s` to `at` at `speed` (m/s).
pub fn time_to(at: f32, s: f32, speed: f32) -> f32 {
    (at - s) / speed.abs().max(SLOW_MPS)
}

/// What the tablet shows for the next note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    /// Index into the coach's notes.
    pub note: usize,
    /// Seconds to go.
    pub to_go: f32,
    /// 0 = the note is [`ACCEPT_S`] away … 1 = at the note; the on-time window is
    /// [`window`].
    pub fill: f32,
    /// It's time to call it.
    pub now: bool,
}

/// The on-time window on the cue bar (fill values).
pub fn window() -> (f32, f32) {
    (1.0 - EARLY_S / ACCEPT_S, 1.0 - LATE_S / ACCEPT_S)
}

/// The run's call-timing tally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub on_time: u32,
    pub early: u32,
    pub late: u32,
    pub missed: u32,
}

impl Tally {
    pub fn add(&mut self, g: Grade) {
        match g {
            Grade::OnTime => self.on_time += 1,
            Grade::Early => self.early += 1,
            Grade::Late => self.late += 1,
            Grade::Missed => self.missed += 1,
        }
    }
    pub fn count(&self) -> u32 {
        self.on_time + self.early + self.late + self.missed
    }
    /// 0..=100, None before any note was graded.
    pub fn score(&self) -> Option<u32> {
        let n = self.count();
        (n > 0).then(|| (self.on_time * 100 + self.early * 50 + self.late * 40 + n / 2) / n)
    }
}

/// Grades one stage's notes as the car goes.
#[derive(Clone, Debug, Default)]
pub struct Coach {
    notes: Vec<Note>,
    grades: Vec<Option<Grade>>,
    pub tally: Tally,
    /// The latest grade and the run time it was given at (for the flash on the tablet).
    pub last: Option<(Grade, f32)>,
    last_call: Option<f32>,
}

impl Coach {
    pub fn new(notes: Vec<Note>) -> Coach {
        let n = notes.len();
        Coach { notes, grades: vec![None; n], ..Coach::default() }
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn grade(&self, i: usize) -> Option<Grade> {
        self.grades.get(i).copied().flatten()
    }

    /// Start again with the car at `s` (restart, recover backwards): notes ahead are ungraded,
    /// notes behind don't count.
    pub fn reset(&mut self, s: f32) {
        self.grades.iter_mut().for_each(|g| *g = None);
        self.tally = Tally::default();
        self.last = None;
        self.last_call = None;
        // Notes already behind (or at) the start are nobody's to call.
        for (i, n) in self.notes.iter().enumerate() {
            if n.at <= s + 1.0 {
                self.grades[i] = Some(Grade::OnTime);
            }
        }
    }

    /// The first note ahead of `s` not graded yet.
    fn next(&self, s: f32) -> Option<usize> {
        (0..self.notes.len()).find(|&i| self.grades[i].is_none() && self.notes[i].at > s)
    }

    fn set(&mut self, i: usize, g: Grade, now: f32) {
        self.grades[i] = Some(g);
        self.tally.add(g);
        self.last = Some((g, now));
        // The notes chained behind it ride on the same call.
        let mut j = i + 1;
        while j < self.notes.len() && self.notes[j].into && self.grades[j].is_none() {
            self.grades[j] = Some(g);
            j += 1;
        }
    }

    /// The guide called, with the car at `s` doing `speed`, at run time `now`. Returns the
    /// grade if it counted for a note.
    pub fn call(&mut self, s: f32, speed: f32, now: f32) -> Option<Grade> {
        if self.last_call.is_some_and(|t| now - t < DEBOUNCE_S && now >= t) {
            return None;
        }
        self.last_call = Some(now);
        let i = self.next(s)?;
        let to_go = time_to(self.notes[i].at, s, speed);
        if to_go > ACCEPT_S {
            return None;
        }
        let g = Grade::of(to_go);
        self.set(i, g, now);
        Some(g)
    }

    /// The car moved on to `s`: notes it reached uncalled are missed.
    pub fn advance(&mut self, s: f32, now: f32) {
        for i in 0..self.notes.len() {
            if self.notes[i].at > s {
                break;
            }
            if self.grades[i].is_none() {
                self.set(i, Grade::Missed, now);
            }
        }
    }

    /// The bar for the next note.
    pub fn cue(&self, s: f32, speed: f32) -> Option<Cue> {
        let i = self.next(s)?;
        let to_go = time_to(self.notes[i].at, s, speed);
        let fill = (1.0 - to_go / ACCEPT_S).clamp(0.0, 1.0);
        Some(Cue { note: i, to_go, fill, now: (LATE_S..=EARLY_S).contains(&to_go) && to_go <= IDEAL_S + 1.0 })
    }
}

/// The guide's call timing this run.
#[derive(Resource, Default)]
pub struct GuideCalls {
    pub coach: Coach,
    /// (stage id, road hash) the coach was built for.
    stage: Option<(u32, u64)>,
    last_s: f32,
    last_time: f32,
    was_talking: bool,
}

/// Feeds the coach: a new stage or a restart starts it again; calls come from the guide's
/// voice (talk starting) and the call key; notes the car reaches uncalled are missed.
pub fn track(
    stage: Option<Res<GuideStage>>,
    feed: Res<GuideFeed>,
    actions: Option<Res<engine::actions::ActionState>>,
    voice: Option<Res<crate::voice::VoiceState>>,
    mut calls: ResMut<GuideCalls>,
) {
    let Some(g) = stage else { return };
    let calls = &mut *calls;
    let run = feed.run;
    let key = (g.def.id, g.road_hash);
    let restarted = run.countdown > 0.0 || run.time + 1.0 < calls.last_time || feed.s + 50.0 < calls.last_s;
    if calls.stage != Some(key) {
        let mut notes = Vec::new();
        super::upcoming(&g, 0.0, usize::MAX, &mut notes);
        calls.coach = Coach::new(notes);
        calls.coach.reset(feed.s);
        calls.stage = Some(key);
    } else if restarted {
        calls.coach.reset(feed.s);
    }
    calls.last_s = feed.s;
    calls.last_time = run.time;
    let talking = voice.is_some_and(|v| v.transmitting);
    let started_talking = talking && !calls.was_talking;
    calls.was_talking = talking;
    if !run.running() {
        return;
    }
    let pressed = actions.is_some_and(|a| a.just_pressed("guide_call"));
    if started_talking || pressed {
        let speed = feed.car.map_or(0.0, |c| c.speed());
        calls.coach.call(feed.s, speed, feed.clock);
    }
    calls.coach.advance(feed.s, feed.clock);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(at: f32, into: bool) -> Note {
        Note { at, end: at + 40.0, corner: Some(0), into }
    }

    fn coach() -> Coach {
        let mut c = Coach::new(vec![note(300.0, false), note(600.0, false), note(660.0, true), note(1200.0, false)]);
        c.reset(0.0);
        c
    }

    #[test]
    fn grades_follow_seconds_to_go() {
        assert_eq!(Grade::of(9.0), Grade::Early);
        assert_eq!(Grade::of(IDEAL_S), Grade::OnTime);
        assert_eq!(Grade::of(1.0), Grade::Late);
        // Slow, it's distance: 70 m to go at walking pace is the ideal moment.
        assert!((time_to(70.0, 0.0, 2.0) - IDEAL_S).abs() < 1e-4);
    }

    #[test]
    fn a_call_counts_for_the_next_note_and_its_chain() {
        let mut c = coach();
        // 30 m/s, 300 m out: 10 s, early.
        assert_eq!(c.call(0.0, 30.0, 1.0), Some(Grade::Early));
        // 120 m before the second note at 30 m/s: 4 s, on time; the "into" note rides along.
        assert_eq!(c.call(480.0, 30.0, 20.0), Some(Grade::OnTime));
        assert_eq!(c.grade(2), Some(Grade::OnTime));
        // Far from anything: chatter.
        assert_eq!(c.call(700.0, 30.0, 25.0), None);
        // Reaching the last note without a call: missed.
        c.advance(1200.0, 45.0);
        assert_eq!(c.grade(3), Some(Grade::Missed));
        assert_eq!(c.tally, Tally { on_time: 1, early: 1, late: 0, missed: 1 });
        assert_eq!(c.tally.score(), Some(50));
    }

    #[test]
    fn calls_are_debounced_and_late_calls_are_late() {
        let mut c = coach();
        assert_eq!(c.call(260.0, 25.0, 10.0), Some(Grade::Late));
        // The open mic flickers: not a second call.
        assert_eq!(c.call(262.0, 25.0, 10.2), None);
        assert_eq!(c.tally.count(), 1);
    }

    #[test]
    fn the_cue_lights_up_in_the_window() {
        let c = coach();
        let far = c.cue(0.0, 20.0).unwrap();
        assert!(!far.now && far.fill == 0.0);
        let due = c.cue(300.0 - 30.0 * IDEAL_S, 30.0).unwrap();
        assert!(due.now && (due.to_go - IDEAL_S).abs() < 1e-3);
        let (lo, hi) = window();
        assert!(due.fill > lo && due.fill < hi);
    }

    #[test]
    fn reset_ignores_notes_behind_the_car() {
        let mut c = coach();
        c.reset(650.0);
        assert_eq!(c.tally.count(), 0);
        assert_eq!(c.cue(650.0, 30.0).unwrap().note, 2);
    }
}
