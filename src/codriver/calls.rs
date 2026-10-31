//! What the co-driver says and when: pace notes read ahead of the car by time, chained with
//! "into" like a real co-driver, and the finish. Pure (no Bevy systems), tested.

use crate::guide::{Note, upcoming_in};
use crate::stage::{Corner, Turn};

/// Seconds of driving ahead a note is called (at speed; slow, it's called by distance).
pub const LEAD_S: f32 = 4.5;
/// Nearest and furthest a note is called ahead (m).
pub const LEAD_MIN: f32 = 70.0;
pub const LEAD_MAX: f32 = 280.0;
/// Notes this close behind each other (m) are read in one breath ("…into right 3").
pub const CHAIN_GAP: f32 = 30.0;

/// A line to speak.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    /// Higher goes first; [`Priority::Urgent`] cuts off whatever is being said.
    pub priority: Priority,
    /// Road position (m) past which the line is stale and dropped unsaid.
    pub stale_after: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Chat,
    Note,
    Warn,
    Urgent,
}

impl Line {
    pub fn new(text: impl Into<String>, priority: Priority) -> Line {
        Line { text: text.into(), priority, stale_after: None }
    }
}

/// Reads one stage's notes in order as the car goes.
#[derive(Clone, Debug)]
pub struct Planner {
    notes: Vec<Note>,
    corners: Vec<Corner>,
    length: f32,
    /// Next note to call.
    next: usize,
    finish_called: bool,
    /// Call timing (Settings > Co-driver): read-ahead × this (1 normal, more = earlier).
    pub lead_scale: f32,
    /// "Three left" instead of "left three" (Settings > Co-driver > Call order).
    pub grade_first: bool,
}

impl Planner {
    pub fn new(corners: &[Corner], crests: &[f32], length: f32) -> Planner {
        let mut notes = Vec::new();
        upcoming_in(corners, crests, 0.0, usize::MAX, &mut notes);
        Planner { notes, corners: corners.to_vec(), length, next: 0, finish_called: false, lead_scale: 1.0, grade_first: false }
    }

    /// How far ahead notes are called at `speed` (m/s), at normal timing.
    pub fn lead(speed: f32) -> f32 {
        (speed.abs() * LEAD_S).clamp(LEAD_MIN, LEAD_MAX)
    }

    /// [`Self::lead`] with this planner's timing.
    pub fn lead_scaled(&self, speed: f32) -> f32 {
        let k = self.lead_scale.clamp(0.5, 2.0);
        (speed.abs() * LEAD_S * k).clamp(LEAD_MIN * k, LEAD_MAX * k)
    }

    /// Back to note `s` (after a restart or recover): notes behind `s` won't be called.
    pub fn seek(&mut self, s: f32) {
        self.next = self.notes.partition_point(|n| n.end < s);
        self.finish_called = self.length - s < 50.0;
    }

    /// The lines due with the car at `s` metres doing `speed` m/s.
    pub fn update(&mut self, s: f32, speed: f32, out: &mut Vec<Line>) {
        let lead = self.lead_scaled(speed);
        // Notes the car already passed (a recover forward, a fast stretch) go unsaid.
        while self.notes.get(self.next).is_some_and(|n| n.end < s) {
            self.next += 1;
        }
        while let Some(n) = self.notes.get(self.next).copied() {
            if n.at - s > lead {
                break;
            }
            let mut text = String::new();
            let d = n.at - s;
            if d > 40.0 {
                text.push_str(&distance(d));
                text.push(' ');
            }
            self.phrase(&mut text, &n);
            self.next += 1;
            let mut last = n;
            // Chain notes that follow close behind.
            while let Some(m) = self.notes.get(self.next).copied() {
                if m.at - last.end > CHAIN_GAP {
                    break;
                }
                text.push_str(", into ");
                self.phrase(&mut text, &m);
                self.next += 1;
                last = m;
            }
            out.push(Line { text, priority: Priority::Note, stale_after: Some(n.at) });
        }
        if !self.finish_called && self.length - s < lead.max(150.0) && self.next >= self.notes.len() {
            self.finish_called = true;
            let d = self.length - s;
            let text = if d > 40.0 { format!("{} to flying finish, flat out", distance(d)) } else { "flying finish".into() };
            out.push(Line { text, priority: Priority::Note, stale_after: Some(self.length) });
        }
    }

    fn phrase(&self, out: &mut String, n: &Note) {
        match n.corner {
            Some(k) => {
                let c = &self.corners[k];
                let side = if c.turn == Turn::Left { "left" } else { "right" };
                if self.grade_first {
                    out.push_str(grade_word(c.grade));
                    out.push(' ');
                    out.push_str(side);
                } else {
                    out.push_str(side);
                    out.push(' ');
                    out.push_str(grade_word(c.grade));
                }
                if c.end - c.start > 70.0 {
                    out.push_str(" long");
                }
                if c.crest {
                    out.push_str(" over crest");
                }
                if c.narrows {
                    out.push_str(", narrows");
                }
                if c.grade <= 2 {
                    out.push_str(", careful");
                }
            }
            None => out.push_str("over crest"),
        }
    }
}

/// Grade 1 (hairpin) … 6 (flat-out kink), as spoken.
pub fn grade_word(g: u8) -> &'static str {
    match g {
        0 | 1 => "hairpin",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        _ => "six",
    }
}

/// Rounded like a co-driver: 10 m steps under 200, 50 above.
pub fn distance(d: f32) -> String {
    let step = if d < 200.0 { 10.0 } else { 50.0 };
    format!("{}", ((d / step).round() * step) as u32)
}

/// What to say about the car's damage after a hit of severity `sev` (see
/// [`crate::vehicle::Damage::hit`]), given the damage before and after. `None`: not worth a word.
pub fn damage_line(before: &crate::vehicle::Damage, after: &crate::vehicle::Damage) -> Option<Line> {
    if after.wrecked() && !before.wrecked() {
        let part = worst_part(after);
        return Some(Line::new(format!("That's it, the {part} is gone. We're out of the stage."), Priority::Urgent));
    }
    let sev = after.last;
    if sev < 0.04 {
        return None;
    }
    let (de, ds, du) = (after.engine - before.engine, after.steering - before.steering, after.suspension - before.suspension);
    let what = if de >= ds && de >= du {
        "engine"
    } else if ds >= du {
        "steering"
    } else {
        "suspension"
    };
    let state = |d: f32| {
        if d > 0.7 {
            "is badly damaged, nurse it home"
        } else if d > 0.35 {
            "is damaged"
        } else {
            "took a knock"
        }
    };
    let level = match what {
        "engine" => after.engine,
        "steering" => after.steering,
        _ => after.suspension,
    };
    let mut text = if sev > 0.3 { "Big hit! ".to_string() } else { "Ouch. ".to_string() };
    text.push_str(&format!("The {what} {}", state(level)));
    if what == "steering" && after.steering > 0.2 && after.pull.abs() > 0.2 {
        text.push_str(if after.pull > 0.0 { ", it's pulling left" } else { ", it's pulling right" });
    }
    text.push('.');
    Some(Line::new(text, if sev > 0.3 || level > 0.7 { Priority::Urgent } else { Priority::Warn }))
}

fn worst_part(d: &crate::vehicle::Damage) -> &'static str {
    if d.engine >= d.steering && d.engine >= d.suspension {
        "engine"
    } else if d.steering >= d.suspension {
        "steering"
    } else {
        "suspension"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_order_and_timing() {
        let cs = [corner(300.0, 340.0, Turn::Left, 3)];
        let mut p = Planner::new(&cs, &[], 1000.0);
        p.grade_first = true;
        let mut out = Vec::new();
        p.update(260.0, 20.0, &mut out);
        assert!(out[0].text.contains("three left"), "{}", out[0].text);
        // Earlier calls read further ahead.
        let mut early = Planner::new(&cs, &[], 1000.0);
        early.lead_scale = 1.3;
        assert!(early.lead_scaled(30.0) > Planner::lead(30.0));
        let mut late = Planner::new(&cs, &[], 1000.0);
        late.lead_scale = 0.75;
        assert!(late.lead_scaled(30.0) < Planner::lead(30.0));
    }
    use crate::vehicle::Damage;

    fn corner(start: f32, end: f32, turn: Turn, grade: u8) -> Corner {
        Corner { start, end, turn, grade, angle: 60.0, crest: false, narrows: false }
    }

    #[test]
    fn notes_are_called_ahead_once_and_chained() {
        let corners = vec![corner(300.0, 340.0, Turn::Left, 4), corner(360.0, 400.0, Turn::Right, 2), corner(900.0, 1000.0, Turn::Left, 1)];
        let mut p = Planner::new(&corners, &[600.0], 1200.0);
        let mut out = Vec::new();
        p.update(0.0, 0.0, &mut out);
        assert!(out.is_empty(), "{out:?}");
        // At 20 m/s the lead is 90 m.
        p.update(215.0, 20.0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "90 left four, into right two, careful");
        out.clear();
        p.update(220.0, 20.0, &mut out);
        assert!(out.is_empty(), "called twice: {out:?}");
        p.update(520.0, 20.0, &mut out);
        assert_eq!(out[0].text, "80 over crest");
        out.clear();
        p.update(1150.0, 20.0, &mut out);
        // The hairpin went by unsaid (the car skipped past it); the finish is called.
        assert_eq!(out.len(), 1);
        assert!(out[0].text.contains("flying finish"), "{out:?}");
    }

    #[test]
    fn seek_skips_what_is_behind() {
        let corners = vec![corner(100.0, 140.0, Turn::Left, 3), corner(500.0, 540.0, Turn::Right, 5)];
        let mut p = Planner::new(&corners, &[], 2000.0);
        p.seek(300.0);
        let mut out = Vec::new();
        p.update(300.0, 50.0, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].text.ends_with("right five"), "{out:?}");
    }

    #[test]
    fn real_stage_notes_cover_every_corner() {
        let g = crate::guide::GuideStage::generate(3).unwrap();
        let mut p = Planner::new(&g.corners, &g.crests, g.road.length());
        let mut out = Vec::new();
        let mut s = 0.0;
        while s < g.road.length() {
            p.update(s, 25.0, &mut out);
            s += 5.0;
        }
        let said: usize = out.iter().map(|l| l.text.matches("left").count() + l.text.matches("right").count()).sum();
        assert_eq!(said, g.corners.len());
        assert!(out.last().unwrap().text.contains("finish"));
    }

    #[test]
    fn damage_talk() {
        let a = Damage::default();
        let mut b = a;
        b.hit(0.02, bevy::math::Vec3::new(0.0, 0.0, -2.0), crate::vehicle::HitZone::Front);
        assert!(damage_line(&a, &b).is_none());
        let mut c = a;
        c.hit(0.5, bevy::math::Vec3::new(-0.8, 0.0, -2.0), crate::vehicle::HitZone::Front);
        let l = damage_line(&a, &c).unwrap();
        assert!(l.text.starts_with("Big hit"), "{l:?}");
        assert_eq!(l.priority, Priority::Urgent);
        let mut d = c;
        d.hit(1.2, bevy::math::Vec3::new(0.0, 0.0, -2.0), crate::vehicle::HitZone::Front);
        assert!(damage_line(&c, &d).unwrap().text.contains("out of the stage"));
    }
}
