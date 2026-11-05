//! Style scoring: skills detected from the sim, chained into a Rhythm that banks Style and
//! Notes.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §2 (rules and
//! numbers) and SK-1..SK-8; rulings `docs/festival/DECISIONS.md` S1-S5 (GAME_SYSTEMS's
//! `SkillEvent` is folded into [`StyleEvent`]). Readers: the Style HUD (`hud_style`, F), the
//! festival sounds (`audio::festival`, F), Slide Sections (`open::challenges`, F).
//!
//! Detection is a read-only observer of `drive::DrivenCar`, in `FixedUpdate` after
//! `SimulateSet`; it never feeds the sim (TECH_PASS §5). The core is pure: [`Detector::step`]
//! takes a [`StyleFrame`] (a snapshot of the car and what is around it) and appends
//! [`StyleEvent`]s; the systems in [`StylePlugin`] only build frames and send the events.
//!
//! How skills feed the Rhythm: a skill adds its points to the pending total P when it
//! *completes* (a slide when it ends, a jump at a clean landing). Continuous skills (Slide,
//! Flat Out, Slipstream) keep the Rhythm alive while they run, add +0.2 to the multiplier every
//! 2 s, and show their running points in [`Rhythm::live`]; a break loses those too.

use bevy::prelude::*;

use super::racers::OtherCar;

/// Seconds with no skill live before a Rhythm banks.
pub const RHYTHM_TIMEOUT: f32 = 3.5;
/// The Rhythm multiplier's cap.
pub const MULT_CAP: f32 = 6.0;
/// Style per Note when a Rhythm banks.
pub const STYLE_PER_NOTE: u32 = 25;
/// Most Notes one Rhythm pays.
pub const NOTES_CAP: u32 = 2_000;
/// Multiplier added by each completed skill and by each [`LIVE_MULT_SECS`] of a continuous one.
pub const MULT_STEP: f32 = 0.2;
/// Extra multiplier for a skill different from the last completed one.
pub const VARIETY: f32 = 0.1;
/// Seconds of a continuous skill per multiplier step.
pub const LIVE_MULT_SECS: f32 = 2.0;

// Slide (drift).
const SLIDE_START_BETA: f32 = 15.0;
const SLIDE_START_KMH: f32 = 35.0;
const SLIDE_HOLD: f32 = 0.25;
const SLIDE_KEEP_BETA: f32 = 10.0;
const SLIDE_KEEP_KMH: f32 = 25.0;
const SLIDE_GRACE: f32 = 0.3;
const SLIDE_SPIN: f32 = 100.0;
const SWITCH_WINDOW: f32 = 1.0;
const SWITCH_POINTS: u32 = 150;
// Flat Out.
const FLAT_TARMAC: f32 = 150.0;
const FLAT_LOOSE: f32 = 115.0;
const FLAT_END: f32 = 0.5;
// Air.
const AIR_MIN: f32 = 0.30;
const LANDING_WINDOW: f32 = 0.6;
const LANDING_UP: f32 = 0.85;
/// Seconds of body-on-ground a landing may have (the suspension bottoming out touches the
/// body on most landings in this sim); longer is a belly flop.
pub const LANDING_SCRAPE: f32 = 0.3;
/// "Big Air" from this many metres.
pub const BIG_AIR_M: f32 = 25.0;
// Close Shave.
const SHAVE_BODY: f32 = 1.8;
const SHAVE_GAP: f32 = 1.2;
const SCRAPER_GAP: f32 = 0.6;
/// Body gaps below this (m) are overlapping bodies: no shave.
const SHAVE_OVERLAP: f32 = -0.1;
const SHAVE_CLOSING_KMH: f32 = 25.0;
const SHAVE_CONFIRM: f32 = 1.0;
const SHAVE_REPEAT: f32 = 5.0;
const SHAVE_RANGE: f32 = 40.0;
// Slipstream.
const DRAFT_NEAR: f32 = 4.0;
const DRAFT_FAR: f32 = 20.0;
const DRAFT_LATERAL: f32 = 1.2;
const DRAFT_KMH: f32 = 70.0;
const DRAFT_HOLD: f32 = 1.0;
const DRAFT_END: f32 = 0.5;
const DRAFT_RATE: f32 = 25.0;
// Splinter.
const SPLINTER_MAX: u8 = 12;
// Switchback.
const HAIRPIN_CURV: f32 = 0.083;
const HAIRPIN_EXIT_CURV: f32 = 0.075;
const HAIRPIN_KMH: f32 = 30.0;
// Clean Line.
const CLEAN_LINE_M: f32 = 1_000.0;
const CLEAN_ON_ROAD: f32 = 0.8;
// Clean Pass.
const PASS_HOLD: f32 = 3.0;
// Breaks.
const CONTACT_KMH: f32 = 8.0;
const CONTACT_RANGE: f32 = 3.6;
const ROLLED_UP: f32 = 0.3;
const SCUFF_GAP: f32 = 0.5;
/// Float slack for time thresholds reached by summing 1/60 s steps.
const EPS: f32 = 1e-3;

/// A skill (GAME_SYSTEMS §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleKind {
    Slide,
    Switch,
    FlatOut,
    Air,
    CloseShave,
    Slipstream,
    Splinter,
    Switchback,
    CleanLine,
    CleanPass,
    Challenge,
}

impl StyleKind {
    /// Every kind, in declaration order.
    pub const ALL: [StyleKind; 11] = [
        StyleKind::Slide,
        StyleKind::Switch,
        StyleKind::FlatOut,
        StyleKind::Air,
        StyleKind::CloseShave,
        StyleKind::Slipstream,
        StyleKind::Splinter,
        StyleKind::Switchback,
        StyleKind::CleanLine,
        StyleKind::CleanPass,
        StyleKind::Challenge,
    ];

    /// The HUD's word for it (upper case on screen).
    pub fn label(self) -> &'static str {
        match self {
            StyleKind::Slide => "Slide",
            StyleKind::Switch => "Switch",
            StyleKind::FlatOut => "Flat Out",
            StyleKind::Air => "Air",
            StyleKind::CloseShave => "Close Shave",
            StyleKind::Slipstream => "Slipstream",
            StyleKind::Splinter => "Splinter",
            StyleKind::Switchback => "Switchback",
            StyleKind::CleanLine => "Clean Line",
            StyleKind::CleanPass => "Clean Pass",
            StyleKind::Challenge => "Challenge",
        }
    }
}

/// A special name for a scored skill (the HUD shows it instead of the kind's label).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StyleTag {
    #[default]
    None,
    /// An Air of [`BIG_AIR_M`] or more.
    BigAir,
    /// A Close Shave with a body gap of 0.6 m or less.
    PaintScraper,
}

impl StyleTag {
    pub fn label(self) -> Option<&'static str> {
        match self {
            StyleTag::None => None,
            StyleTag::BigAir => Some("Big Air"),
            StyleTag::PaintScraper => Some("Paint Scraper"),
        }
    }
}

/// Why a Rhythm broke (the HUD's reason line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LostReason {
    /// A damaging hit (tree, wall, rock...).
    Hit,
    /// Contact with a traffic or rival car at ≥ 8 km/h closing speed.
    Contact,
    Recover,
    /// Upside down or on its side.
    Rolled,
    CarSwap,
    EventStart,
    Quit,
}

impl LostReason {
    /// The HUD's reason line.
    pub fn label(self) -> &'static str {
        match self {
            LostReason::Hit => "Hit something",
            LostReason::Contact => "Car contact",
            LostReason::Recover => "Recovered",
            LostReason::Rolled => "Rolled",
            LostReason::CarSwap => "Car swap",
            LostReason::EventStart => "Event start",
            LostReason::Quit => "Quit",
        }
    }
}

/// What the scorer reports. HUD, audio and challenges listen; live values (pending points,
/// multiplier, timer) are in [`Rhythm`].
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum StyleEvent {
    /// A skill scored. `chain`: skills in the Rhythm so far (the audio climbs a semitone per
    /// step). `tag` names a special version of it (Big Air, Paint Scraper).
    Tick { kind: StyleKind, points: u32, chain: u8, tag: StyleTag },
    /// The Rhythm banked: `points` = round(P × M) Style, paying `notes`.
    Bank { points: u32, mult: f32, notes: u32, chain: u8 },
    /// The Rhythm broke: `points` pending were lost.
    Lost { points: u32, why: LostReason },
}

/// Index of each continuous skill in [`Rhythm::live`].
pub const LIVE_SLIDE: usize = 0;
pub const LIVE_FLAT: usize = 1;
pub const LIVE_DRAFT: usize = 2;
/// The continuous skills, in [`Rhythm::live`] order.
pub const LIVE_KINDS: [StyleKind; 3] = [StyleKind::Slide, StyleKind::FlatOut, StyleKind::Slipstream];

/// The Rhythm in progress (all zero / ×1 when none is open).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Rhythm {
    /// Pending points P (before the multiplier), completed skills only.
    pub points: u32,
    /// Multiplier M, 1.0..=MULT_CAP.
    pub mult: f32,
    /// Skills completed in this Rhythm.
    pub chain: u8,
    /// Seconds left before it banks.
    pub timer: f32,
    /// The last skill completed (the variety bonus).
    pub last: Option<StyleKind>,
    /// A Rhythm is open (a skill completed or one is running).
    pub open: bool,
    /// Running points of the continuous skills (Slide, Flat Out, Slipstream; 0 = not running),
    /// not yet in `points`.
    pub live: [u32; 3],
    /// Scuffs (wall brushes) in this Rhythm: each halved M.
    pub scuffs: u8,
    /// Raw Slide + Switch points ever scored, counting the slide in progress: Slide Sections
    /// measure the difference between their gates. Only grows, except when a running slide is
    /// voided (spin) or broken.
    pub slide_meter: f32,
    /// Seconds in the air (0 on the ground).
    pub air: f32,
}

impl Default for Rhythm {
    fn default() -> Self {
        Rhythm { points: 0, mult: 1.0, chain: 0, timer: 0.0, last: None, open: false, live: [0; 3], scuffs: 0, slide_meter: 0.0, air: 0.0 }
    }
}

impl Rhythm {
    /// Pending points with the running skills: what a break would lose.
    pub fn at_stake(&self) -> u32 {
        self.points + self.live.iter().sum::<u32>()
    }

    /// What banking now would give (Style), running skills included.
    pub fn value(&self) -> u32 {
        (self.at_stake() as f32 * self.mult).round() as u32
    }
}

/// Notes paid for banking `style` Style.
pub fn notes_for(style: u32) -> u32 {
    (style / STYLE_PER_NOTE).min(NOTES_CAP)
}

/// The road under the car (nearest road station).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadInfo {
    /// Which road (the open world's road index; 0 on a classic stage).
    pub road: u32,
    /// Station (m along the road).
    pub s: f32,
    /// Signed curvature (1/m).
    pub curv: f32,
    pub half_width: f32,
    /// Signed lateral offset from the centreline (m).
    pub lateral: f32,
}

impl RoadInfo {
    /// On the road (Switchback and Clean Line tolerance: half width + 0.5 m).
    pub fn on_road(&self) -> bool {
        self.lateral.abs() <= self.half_width + 0.5
    }
}

/// One fixed step as the scorer sees it. Build with [`StyleFrame::of`] or by hand in tests.
#[derive(Clone, Copy, Debug)]
pub struct StyleFrame<'a> {
    pub dt: f32,
    /// Scoring is allowed this step: not in a countdown, a pause, a finished run or while the
    /// car is down. When false nothing scores and the timers hold.
    pub live: bool,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Unit forward and up of the body.
    pub fwd: Vec3,
    pub up: Vec3,
    /// Wheels on the ground.
    pub contacts: u8,
    /// Most of the wheels on the ground are on a loose surface (Flat Out's lower threshold).
    pub loose: bool,
    /// `car.damage.hits`, `car.impacts`, `car.broken.len()`, `car.body_contact`.
    pub hits: u32,
    pub impacts: f32,
    pub broken: usize,
    pub body_contact: bool,
    pub road: Option<RoadInfo>,
    /// Traffic and rivals.
    pub others: &'a [OtherCar],
    /// In a race: the player's route progress (m) and each rival's (id, progress).
    pub progress: Option<f32>,
    pub rivals: &'a [(u32, f32)],
    /// In an event: a quarter split (or the finish) was passed this step (Clean Line).
    pub split: bool,
    pub in_event: bool,
}

impl<'a> StyleFrame<'a> {
    /// A frame from the car, everything else empty (set the other fields after).
    pub fn of(car: &crate::vehicle::Car, dt: f32) -> StyleFrame<'a> {
        use crate::stage::Surface;
        let (mut on, mut loose) = (0u8, 0u8);
        for w in &car.wheels {
            if w.contact {
                on += 1;
                if matches!(w.surface, Some(Surface::Gravel | Surface::Loose | Surface::Mud | Surface::Snow | Surface::Dusty)) {
                    loose += 1;
                }
            }
        }
        StyleFrame {
            dt,
            live: true,
            pos: car.pos,
            vel: car.vel,
            fwd: car.forward(),
            up: car.up(),
            contacts: on,
            loose: on > 0 && loose * 2 >= on,
            hits: car.damage.hits,
            impacts: car.impacts,
            broken: car.broken.len(),
            body_contact: car.body_contact,
            road: None,
            others: &[],
            progress: None,
            rivals: &[],
            split: false,
            in_event: false,
        }
    }

    fn left(&self) -> Vec3 {
        self.up.cross(self.fwd)
    }

    /// Forward speed (km/h).
    pub fn kmh(&self) -> f32 {
        self.vel.dot(self.fwd) * 3.6
    }

    /// Body slip angle β (degrees, −180..180, + = sliding towards the left).
    pub fn beta(&self) -> f32 {
        libm::atan2f(self.vel.dot(self.left()), self.vel.dot(self.fwd)).to_degrees()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Slide {
    #[default]
    Idle,
    /// The start condition has held `t` s so far (its points count once it starts).
    Hold { t: f32, pts: f32 },
    On { pts: f32, grace: f32, mult_t: f32, spin: bool },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Flat {
    #[default]
    Idle,
    On { pts: f32, below: f32, mult_t: f32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Air {
    #[default]
    Ground,
    Up { t: f32, from: Vec3 },
    Landing { t: f32, dist: f32, window: f32, hits: u32, scrape: f32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Draft {
    #[default]
    Idle,
    Hold { t: f32 },
    On { pts: f32, out: f32, mult_t: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Hairpin {
    road: u32,
    entry_s: f32,
    last_s: f32,
    t: f32,
    speed: f32,
    ok: bool,
}

/// Another car the scorer is tracking (Close Shave, contact).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    id: u32,
    /// Its distance ahead (+) or behind along the player's forward, last step.
    ahead: f32,
    /// Seconds since it last scored a Close Shave / touched the player.
    since_shave: f32,
    since_contact: f32,
    /// In range this step.
    alive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Shave {
    id: u32,
    wait: f32,
    points: u32,
    tag: StyleTag,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pass {
    id: u32,
    /// Seen behind this rival since the last pass.
    behind: bool,
    /// Seconds ahead since passing it.
    ahead: Option<f32>,
    touched: bool,
}

/// The scorer. Feed it one [`StyleFrame`] per fixed step; read [`Detector::rhythm`].
#[derive(Clone, Debug, Default)]
pub struct Detector {
    pub rhythm: Rhythm,
    prev: Option<StyleFrame<'static>>,
    slide: Slide,
    /// Sign of the last strong slip (|β| ≥ 10°) and the seconds since it.
    switch_sign: f32,
    switch_t: f32,
    /// Seconds since a slide was last running.
    since_slide: f32,
    flat: Flat,
    air: Air,
    draft: Draft,
    seen: Vec<Seen>,
    shaves: Vec<Shave>,
    splinters: u8,
    hairpin: Option<Hairpin>,
    line_m: f32,
    line_on: f32,
    passes: Vec<Pass>,
    since_scuff: f32,
}

/// A copy of the frame without the borrowed slices (what the next step compares against).
fn keep(f: &StyleFrame) -> StyleFrame<'static> {
    StyleFrame { others: &[], rivals: &[], ..*f }
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

impl Detector {
    /// One fixed step. Appends what happened to `out`.
    pub fn step(&mut self, f: &StyleFrame, out: &mut Vec<StyleEvent>) {
        let Some(prev) = self.prev.replace(keep(f)) else { return };
        let dt = f.dt;
        // A jump in position (recover, respawn, restart): the car didn't drive there.
        let moved = flat_dist(f.pos, prev.pos);
        if moved > 4.0 + prev.vel.length().max(f.vel.length()) * dt * 2.0 {
            self.lose(LostReason::Recover, out);
            self.reset_skills();
            return;
        }
        if !f.live {
            return;
        }
        let hit = f.hits > prev.hits;
        let knock = f.impacts > prev.impacts + 1e-4;
        let splinters = f.broken.saturating_sub(prev.broken);
        let kmh = f.kmh();
        self.since_scuff += dt;

        // Other cars: contact first (it breaks the Rhythm), then shaves.
        let contact = self.watch_others(f, knock || hit, out);
        if hit {
            self.lose(LostReason::Hit, out);
            self.reset_skills();
        } else if contact {
            self.lose(LostReason::Contact, out);
            self.reset_skills();
        } else if f.up.y < ROLLED_UP {
            self.lose(LostReason::Rolled, out);
            self.reset_skills();
        } else if knock && splinters == 0 && self.rhythm.open && self.since_scuff >= SCUFF_GAP {
            // A brush below damage: halve M, keep P.
            self.since_scuff = 0.0;
            self.rhythm.mult = (self.rhythm.mult * 0.5).max(1.0);
            self.rhythm.scuffs = self.rhythm.scuffs.saturating_add(1);
        }

        for _ in 0..splinters {
            if self.splinters < SPLINTER_MAX {
                // (The first one opens the Rhythm, which zeroes the count: count after.)
                self.complete(StyleKind::Splinter, if kmh >= 80.0 { 80 } else { 40 }, StyleTag::None, out);
                self.splinters += 1;
            }
        }
        if f.up.y >= ROLLED_UP {
            self.slide(f, kmh, out);
            self.flat_out(f, kmh, out);
            self.air(f, hit, out);
            self.drafting(f, kmh, out);
            self.switchback(f, kmh, hit, out);
        }
        self.clean_line(f, moved, hit || contact, out);
        self.clean_pass(f, out);

        // The timeout: running skills keep the Rhythm alive.
        let active = matches!(self.slide, Slide::On { .. }) || matches!(self.flat, Flat::On { .. }) || matches!(self.draft, Draft::On { .. }) || self.air_active();
        if active {
            if !self.rhythm.open {
                self.open();
            }
            self.rhythm.timer = RHYTHM_TIMEOUT;
        } else if self.rhythm.open {
            self.rhythm.timer -= dt;
            if self.rhythm.timer <= 0.0 {
                self.bank(out);
            }
        }
        self.rhythm.air = match self.air {
            Air::Up { t, .. } => t,
            _ => 0.0,
        };
    }

    fn air_active(&self) -> bool {
        match self.air {
            Air::Up { t, .. } => t >= AIR_MIN - EPS,
            Air::Landing { .. } => true,
            Air::Ground => false,
        }
    }

    fn open(&mut self) {
        let (meter, live) = (self.rhythm.slide_meter, self.rhythm.live);
        self.rhythm = Rhythm { open: true, timer: RHYTHM_TIMEOUT, slide_meter: meter, live, ..default() };
        self.splinters = 0;
        self.since_scuff = SCUFF_GAP;
    }

    /// A skill completed: P grows, M steps up.
    fn complete(&mut self, kind: StyleKind, points: u32, tag: StyleTag, out: &mut Vec<StyleEvent>) {
        if points == 0 {
            return;
        }
        if !self.rhythm.open {
            self.open();
        }
        let r = &mut self.rhythm;
        r.points += points;
        r.mult += MULT_STEP;
        if r.last.is_some_and(|l| l != kind) {
            r.mult += VARIETY;
        }
        r.mult = r.mult.min(MULT_CAP);
        r.chain = r.chain.saturating_add(1);
        r.last = Some(kind);
        r.timer = RHYTHM_TIMEOUT;
        out.push(StyleEvent::Tick { kind, points, chain: r.chain, tag });
    }

    /// +0.2 for every 2 s of a continuous skill.
    fn live_mult(&mut self, mult_t: &mut f32, dt: f32) {
        *mult_t += dt;
        if *mult_t >= LIVE_MULT_SECS - EPS {
            *mult_t -= LIVE_MULT_SECS;
            self.rhythm.mult = (self.rhythm.mult + MULT_STEP).min(MULT_CAP);
        }
    }

    /// Banks now (timeout, an event marker, a challenge gate, the pause menu or map): running
    /// skills are completed first.
    pub fn bank(&mut self, out: &mut Vec<StyleEvent>) {
        if !self.rhythm.open {
            return;
        }
        if let Slide::On { pts, spin: false, .. } = self.slide {
            let before = self.rhythm.live[LIVE_SLIDE] as f32;
            self.rhythm.slide_meter += pts.round() - before;
            self.rhythm.live[LIVE_SLIDE] = 0;
            self.complete(StyleKind::Slide, pts.round() as u32, StyleTag::None, out);
        }
        if let Flat::On { pts, .. } = self.flat {
            self.complete(StyleKind::FlatOut, pts.round() as u32, StyleTag::None, out);
        }
        if let Draft::On { pts, .. } = self.draft {
            self.complete(StyleKind::Slipstream, pts.round() as u32, StyleTag::None, out);
        }
        self.slide = Slide::Idle;
        self.flat = Flat::Idle;
        self.draft = Draft::Idle;
        let r = &self.rhythm;
        let style = (r.points as f32 * r.mult).round() as u32;
        if style > 0 {
            out.push(StyleEvent::Bank { points: style, mult: r.mult, notes: notes_for(style), chain: r.chain });
        }
        self.close();
    }

    /// Breaks the Rhythm (if one is open): the pending points are lost.
    pub fn lose(&mut self, why: LostReason, out: &mut Vec<StyleEvent>) {
        if !self.rhythm.open {
            return;
        }
        let lost = self.rhythm.at_stake();
        // A running slide's points leave the meter too.
        self.rhythm.slide_meter -= self.rhythm.live[LIVE_SLIDE] as f32;
        out.push(StyleEvent::Lost { points: lost, why });
        self.slide = Slide::Idle;
        self.flat = Flat::Idle;
        self.draft = Draft::Idle;
        self.close();
    }

    fn close(&mut self) {
        let meter = self.rhythm.slide_meter;
        self.rhythm = Rhythm { slide_meter: meter, ..default() };
        self.shaves.clear();
    }

    /// Forgets every skill in progress (after a recover or a break): they start over.
    fn reset_skills(&mut self) {
        self.slide = Slide::Idle;
        self.flat = Flat::Idle;
        self.air = Air::Ground;
        self.draft = Draft::Idle;
        self.hairpin = None;
        self.shaves.clear();
        for p in &mut self.passes {
            p.ahead = None;
            p.behind = false;
        }
        self.line_m = 0.0;
        self.line_on = 0.0;
        self.rhythm.live = [0; 3];
    }

    /// A roadside challenge finished with `stars` (the Challenge skill: 500 per star).
    pub fn challenge(&mut self, stars: u8, out: &mut Vec<StyleEvent>) {
        self.complete(StyleKind::Challenge, 500 * stars as u32, StyleTag::None, out);
    }

    /// Scores `points` of `kind` as if it had just completed (dev `style.inject`).
    pub fn inject(&mut self, kind: StyleKind, points: u32, out: &mut Vec<StyleEvent>) {
        self.complete(kind, points, StyleTag::None, out);
    }

    /// Forgets everything, the last frame included (a new run, leaving the game). Nothing is
    /// sent: a Rhythm in progress is dropped.
    pub fn clear(&mut self) {
        let meter = self.rhythm.slide_meter;
        *self = Detector::default();
        self.rhythm.slide_meter = meter;
    }

    fn slide(&mut self, f: &StyleFrame, kmh: f32, out: &mut Vec<StyleEvent>) {
        let dt = f.dt;
        let beta = f.beta();
        let b = beta.abs();
        let start = b >= SLIDE_START_BETA && kmh >= SLIDE_START_KMH && f.contacts >= 2;
        let keep = b >= SLIDE_KEEP_BETA && kmh >= SLIDE_KEEP_KMH;
        let rate = 4.0 * (b.min(55.0) - 10.0).max(0.0) * kmh.clamp(0.0, 140.0) / 50.0;
        let before = self.rhythm.live[LIVE_SLIDE] as f32;
        self.since_slide += dt;
        self.switch_t += dt;
        match self.slide {
            Slide::Idle => {
                if start {
                    self.slide = Slide::Hold { t: dt, pts: rate * dt };
                }
            }
            Slide::Hold { t, pts } => {
                self.slide = if !start {
                    Slide::Idle
                } else if t + dt >= SLIDE_HOLD - EPS {
                    Slide::On { pts: pts + rate * dt, grace: 0.0, mult_t: t + dt, spin: false }
                } else {
                    Slide::Hold { t: t + dt, pts: pts + rate * dt }
                };
            }
            Slide::On { mut pts, mut grace, mut mult_t, mut spin } => {
                spin |= b > SLIDE_SPIN;
                if keep {
                    grace = 0.0;
                    pts += rate * dt;
                } else {
                    grace += dt;
                }
                if grace >= SLIDE_GRACE - EPS {
                    // Over: the live part leaves the meter, the final total (unless voided by
                    // a spin) goes in.
                    self.slide = Slide::Idle;
                    self.rhythm.live[LIVE_SLIDE] = 0;
                    self.rhythm.slide_meter -= before;
                    if !spin {
                        self.rhythm.slide_meter += pts.round();
                        self.complete(StyleKind::Slide, pts.round() as u32, StyleTag::None, out);
                    }
                } else {
                    self.live_mult(&mut mult_t, dt);
                    self.slide = Slide::On { pts, grace, mult_t, spin };
                }
            }
        }
        if let Slide::On { pts, spin, .. } = self.slide {
            self.since_slide = 0.0;
            let live = if spin { 0 } else { pts.round() as u32 };
            self.rhythm.slide_meter += live as f32 - before;
            self.rhythm.live[LIVE_SLIDE] = live;
        }
        // Switch: the slip flicks from one side to the other inside a slide.
        if keep && b < SLIDE_SPIN {
            let sign = beta.signum();
            if self.switch_sign != 0.0 && sign != self.switch_sign && self.switch_t <= SWITCH_WINDOW + EPS && self.since_slide <= SWITCH_WINDOW + EPS {
                self.rhythm.slide_meter += SWITCH_POINTS as f32;
                self.complete(StyleKind::Switch, SWITCH_POINTS, StyleTag::None, out);
            }
            self.switch_sign = sign;
            self.switch_t = 0.0;
        } else if self.switch_t > SWITCH_WINDOW + EPS {
            self.switch_sign = 0.0;
        }
    }

    fn flat_out(&mut self, f: &StyleFrame, kmh: f32, out: &mut Vec<StyleEvent>) {
        let t = if f.loose { FLAT_LOOSE } else { FLAT_TARMAC };
        match self.flat {
            Flat::Idle => {
                if kmh >= t {
                    self.flat = Flat::On { pts: 3.0 * (kmh - t + 10.0) * f.dt, below: 0.0, mult_t: f.dt };
                }
            }
            Flat::On { mut pts, mut below, mut mult_t } => {
                if kmh >= t - 5.0 {
                    pts += 3.0 * (kmh - t + 10.0) * f.dt;
                    below = 0.0;
                } else {
                    below += f.dt;
                }
                if below >= FLAT_END - EPS {
                    self.flat = Flat::Idle;
                    self.rhythm.live[LIVE_FLAT] = 0;
                    self.complete(StyleKind::FlatOut, pts.round() as u32, StyleTag::None, out);
                    return;
                }
                self.live_mult(&mut mult_t, f.dt);
                self.flat = Flat::On { pts, below, mult_t };
            }
        }
        if let Flat::On { pts, .. } = self.flat {
            self.rhythm.live[LIVE_FLAT] = pts.round() as u32;
        }
    }

    fn air(&mut self, f: &StyleFrame, hit: bool, out: &mut Vec<StyleEvent>) {
        self.air = match self.air {
            // Take-off: where the last wheel left (the previous step's position).
            Air::Ground if f.contacts == 0 => Air::Up { t: f.dt, from: self.prev_pos(f) },
            Air::Ground => Air::Ground,
            Air::Up { t, from } if f.contacts == 0 => Air::Up { t: t + f.dt, from },
            Air::Up { t, from } => {
                if t >= AIR_MIN - EPS {
                    Air::Landing { t, dist: flat_dist(f.pos, from), window: 0.0, hits: f.hits, scrape: if f.body_contact { f.dt } else { 0.0 } }
                } else {
                    Air::Ground
                }
            }
            Air::Landing { t, dist, window, hits, scrape } => {
                let scrape = scrape + if f.body_contact { f.dt } else { 0.0 };
                if hit || f.hits > hits || scrape > LANDING_SCRAPE + EPS || f.up.y < LANDING_UP {
                    // A bad landing voids it (a hit also breaks the Rhythm, above).
                    Air::Ground
                } else if window + f.dt >= LANDING_WINDOW - EPS {
                    let points = (250.0 * t + 6.0 * dist).round() as u32;
                    self.complete(StyleKind::Air, points, if dist >= BIG_AIR_M { StyleTag::BigAir } else { StyleTag::None }, out);
                    Air::Ground
                } else {
                    Air::Landing { t, dist, window: window + f.dt, hits, scrape }
                }
            }
        };
    }

    /// The position one step back (the take-off point when this step is the first one up).
    fn prev_pos(&self, f: &StyleFrame) -> Vec3 {
        // `prev` was replaced with this frame at the top of `step`: step back by the velocity.
        f.pos - f.vel * f.dt
    }

    /// Tracks the other cars: returns whether the player touched one (a knock with a car
    /// beside it at a closing speed of 8 km/h or more); scores Close Shaves.
    fn watch_others(&mut self, f: &StyleFrame, knock: bool, out: &mut Vec<StyleEvent>) -> bool {
        let dt = f.dt;
        let left = f.left();
        for s in &mut self.seen {
            s.alive = false;
            s.since_shave += dt;
            s.since_contact += dt;
        }
        let mut contact = false;
        for o in f.others {
            let d = o.pos - f.pos;
            if d.length_squared() > SHAVE_RANGE * SHAVE_RANGE {
                continue;
            }
            let ahead = d.dot(f.fwd);
            let lateral = d.dot(left);
            let rel = f.vel - o.fwd * o.speed;
            let k = match self.seen.iter().position(|s| s.id == o.id) {
                Some(k) => k,
                None => {
                    self.seen.push(Seen { id: o.id, ahead, since_shave: SHAVE_REPEAT, since_contact: 10.0, alive: true });
                    self.seen.len() - 1
                }
            };
            let s = &mut self.seen[k];
            s.alive = true;
            if knock && d.length() < CONTACT_RANGE {
                s.since_contact = 0.0;
                if rel.length() * 3.6 >= CONTACT_KMH {
                    contact = true;
                }
            }
            let crossed = (s.ahead > 0.0) != (ahead > 0.0);
            s.ahead = ahead;
            if crossed && s.since_shave >= SHAVE_REPEAT - EPS && s.since_contact >= SHAVE_CONFIRM {
                let gap = lateral.abs() - SHAVE_BODY;
                let closing = rel.dot(f.fwd).abs() * 3.6;
                // (A negative gap is the bodies overlapping: contact, not a shave.)
                if (SHAVE_OVERLAP..=SHAVE_GAP).contains(&gap) && closing >= SHAVE_CLOSING_KMH {
                    let scraper = gap <= SCRAPER_GAP;
                    let mut points = if scraper { 250.0 } else { 120.0 };
                    if f.fwd.dot(o.fwd) < 0.0 {
                        points *= 1.5;
                    }
                    s.since_shave = 0.0;
                    self.shaves.push(Shave { id: o.id, wait: 0.0, points: points as u32, tag: if scraper { StyleTag::PaintScraper } else { StyleTag::None } });
                }
            }
        }
        self.seen.retain(|s| s.alive || s.since_shave < SHAVE_REPEAT);
        // A shave scores 1 s after the pass if neither car touched the other around it.
        let mut k = 0;
        while k < self.shaves.len() {
            let sh = &mut self.shaves[k];
            sh.wait += dt;
            let touched = self.seen.iter().any(|s| s.id == sh.id && s.since_contact < sh.wait + SHAVE_CONFIRM);
            if touched || contact {
                self.shaves.swap_remove(k);
            } else if sh.wait >= SHAVE_CONFIRM - EPS {
                let sh = self.shaves.swap_remove(k);
                self.complete(StyleKind::CloseShave, sh.points, sh.tag, out);
            } else {
                k += 1;
            }
        }
        contact
    }

    fn drafting(&mut self, f: &StyleFrame, kmh: f32, out: &mut Vec<StyleEvent>) {
        let left = f.left();
        let zone = kmh >= DRAFT_KMH
            && f.others.iter().any(|o| {
                let d = o.pos - f.pos;
                let ahead = d.dot(f.fwd);
                (DRAFT_NEAR..=DRAFT_FAR).contains(&ahead) && d.dot(left).abs() <= DRAFT_LATERAL && o.speed * 3.6 >= DRAFT_KMH
            });
        let dt = f.dt;
        self.draft = match self.draft {
            Draft::Idle if zone => Draft::Hold { t: dt },
            Draft::Idle => Draft::Idle,
            Draft::Hold { t } if zone => {
                if t + dt >= DRAFT_HOLD - EPS {
                    Draft::On { pts: 0.0, out: 0.0, mult_t: 0.0 }
                } else {
                    Draft::Hold { t: t + dt }
                }
            }
            Draft::Hold { .. } => Draft::Idle,
            Draft::On { mut pts, out: mut gone, mut mult_t } => {
                if zone {
                    pts += DRAFT_RATE * dt;
                    gone = 0.0;
                } else {
                    gone += dt;
                }
                if gone >= DRAFT_END - EPS {
                    self.rhythm.live[LIVE_DRAFT] = 0;
                    self.draft = Draft::Idle;
                    self.complete(StyleKind::Slipstream, pts.round() as u32, StyleTag::None, out);
                    return;
                }
                self.live_mult(&mut mult_t, dt);
                Draft::On { pts, out: gone, mult_t }
            }
        };
        if let Draft::On { pts, .. } = self.draft {
            self.rhythm.live[LIVE_DRAFT] = pts.round() as u32;
        }
    }

    fn switchback(&mut self, f: &StyleFrame, kmh: f32, hit: bool, out: &mut Vec<StyleEvent>) {
        let Some(r) = f.road else {
            self.hairpin = None;
            return;
        };
        match &mut self.hairpin {
            None => {
                if r.curv.abs() >= HAIRPIN_CURV {
                    self.hairpin = Some(Hairpin { road: r.road, entry_s: r.s, last_s: r.s, t: f.dt, speed: kmh * f.dt, ok: r.on_road() });
                }
            }
            Some(h) => {
                if h.road != r.road || (r.s - h.last_s).abs() > 20.0 {
                    // Onto another road (or a jump along it): not this hairpin.
                    self.hairpin = None;
                    return;
                }
                h.last_s = r.s;
                h.t += f.dt;
                h.speed += kmh * f.dt;
                h.ok &= r.on_road() && !hit;
                if r.curv.abs() < HAIRPIN_EXIT_CURV {
                    let h = *h;
                    self.hairpin = None;
                    let mean = h.speed / h.t.max(1e-3);
                    if h.ok && mean >= HAIRPIN_KMH && (r.s - h.entry_s).abs() >= 8.0 {
                        self.complete(StyleKind::Switchback, (200.0 + 10.0 * (mean - HAIRPIN_KMH)).round() as u32, StyleTag::None, out);
                    }
                }
            }
        }
    }

    fn clean_line(&mut self, f: &StyleFrame, moved: f32, dirty: bool, out: &mut Vec<StyleEvent>) {
        if dirty {
            self.line_m = 0.0;
            self.line_on = 0.0;
            return;
        }
        self.line_m += moved;
        if f.road.is_some_and(|r| r.on_road()) {
            self.line_on += moved;
        }
        let due = if f.in_event { f.split } else { self.line_m >= CLEAN_LINE_M };
        if due {
            let clean = self.line_m > 1.0 && self.line_on >= self.line_m * CLEAN_ON_ROAD;
            self.line_m = 0.0;
            self.line_on = 0.0;
            if clean {
                self.complete(StyleKind::CleanLine, 300, StyleTag::None, out);
            }
        }
    }

    fn clean_pass(&mut self, f: &StyleFrame, out: &mut Vec<StyleEvent>) {
        let Some(me) = f.progress else {
            self.passes.clear();
            return;
        };
        for &(id, theirs) in f.rivals {
            let touching = self.seen.iter().any(|s| s.id == id && s.since_contact <= f.dt * 0.5);
            let k = match self.passes.iter().position(|p| p.id == id) {
                Some(k) => k,
                None => {
                    self.passes.push(Pass { id, behind: false, ahead: None, touched: false });
                    self.passes.len() - 1
                }
            };
            let p = &mut self.passes[k];
            if me <= theirs {
                p.behind = true;
                p.ahead = None;
                p.touched = touching;
                continue;
            }
            p.touched |= touching;
            if p.behind {
                // Was behind, now ahead: the pass starts.
                p.behind = false;
                p.ahead = Some(0.0);
                continue;
            }
            if let Some(t) = p.ahead {
                let t = t + f.dt;
                if t >= PASS_HOLD - EPS {
                    p.ahead = None;
                    if !p.touched {
                        self.complete(StyleKind::CleanPass, 300, StyleTag::None, out);
                    }
                } else {
                    p.ahead = Some(t);
                }
            }
        }
    }
}

// --- In the game -------------------------------------------------------------------------

/// The scorer the game runs (one, for the player's car).
#[derive(Resource, Default)]
pub struct StyleState(pub Detector);

/// Asks the scorer to act now (sent by `open::challenges`; the scorer applies it in `Update`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum StyleCommand {
    /// Bank the Rhythm (a challenge gate opened).
    Bank,
}

/// The scoring step in `FixedUpdate`, after the car moved (`drive::SimulateSet`). Challenges
/// that read [`Rhythm`] order themselves after it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyleSet;

/// Registers the scorer. Called from `hud_style::StyleHudPlugin` (the wave-0 plugin F owns),
/// so `career/mod.rs` keeps its list as is.
pub fn register(app: &mut App) {
    use crate::drive::SimulateSet;
    app.init_resource::<StyleState>()
        .init_resource::<Triggers>()
        .add_message::<StyleCommand>()
        .configure_sets(FixedUpdate, StyleSet.after(SimulateSet))
        .add_systems(FixedUpdate, score.in_set(StyleSet).run_if(resource_exists::<crate::world::StageWorld>))
        .add_systems(Update, triggers.run_if(resource_exists::<crate::world::StageWorld>))
        .add_systems(OnExit(engine::flow::AppState::InGame), quit);
    #[cfg(feature = "dev")]
    dev::register(app);
}

/// The road nearest the car (any road of the open world, else the stage's).
fn road_info(stage: &crate::world::StageWorld, p: Vec2) -> Option<RoadInfo> {
    let (k, road, at) = match &stage.open {
        Some(o) => {
            let n = o.net.nearest(p, 30.0)?;
            (n.road as u32, &o.net.roads[n.road].road, n.at)
        }
        None => {
            let at = stage.road.nearest(p, 30.0)?;
            (0, &*stage.road, at)
        }
    };
    Some(RoadInfo { road: k, s: at.s, curv: road.curv[at.i.min(road.len() - 1)], half_width: road.half_width(&at), lateral: at.lateral })
}

/// What stops scoring: the pause, the loading screen, being the guide.
pub(crate) type Gates<'w> = (Option<Res<'w, engine::pause::Paused>>, Option<Res<'w, crate::loading::Loading>>, Option<Res<'w, crate::meta::Role>>);
type ScoreLocals<'s> = (Local<'s, Vec<StyleEvent>>, Local<'s, Vec<(u32, f32)>>, Local<'s, usize>);

#[allow(clippy::too_many_arguments)]
fn score(
    time: Res<Time<Fixed>>,
    stage: Res<crate::world::StageWorld>,
    run: Res<crate::drive::StageRun>,
    cars: Query<&crate::drive::DrivenCar>,
    others: Res<super::racers::OtherCars>,
    (paused, loading, role): Gates,
    mut state: ResMut<StyleState>,
    mut rhythm: ResMut<Rhythm>,
    (mut events, mut pay): (MessageWriter<StyleEvent>, MessageWriter<super::Payout>),
    (mut buf, mut rivals, mut splits): ScoreLocals,
) {
    if role.is_some_and(|r| *r == crate::meta::Role::Guide) {
        return;
    }
    let Ok(c) = cars.single() else { return };
    let car = &c.car;
    let mut f = StyleFrame::of(car, time.delta_secs());
    f.live = loading.is_none() && !paused.is_some_and(|p| p.0) && run.countdown <= 0.0 && run.finished.is_none() && !run.retired && run.down.is_none();
    f.road = road_info(&stage, Vec2::new(car.pos.x, car.pos.z));
    f.others = &others.0;
    f.in_event = !stage.free_roam();
    let n = run.splits.len() + run.finished.is_some() as usize;
    f.split = n > *splits;
    *splits = n;
    // In a race: each rival's progress along the route (its id as in `OtherCars`).
    rivals.clear();
    for o in others.0.iter().filter(|o| matches!(o.kind, super::racers::OtherKind::Rival(_))) {
        if let Some(at) = stage.road.nearest(Vec2::new(o.pos.x, o.pos.z), 40.0) {
            rivals.push((o.id, at.s));
        }
    }
    if !rivals.is_empty() {
        f.progress = Some(run.progress);
        f.rivals = &rivals;
    }
    buf.clear();
    state.0.step(&f, &mut buf);
    publish(&mut state, &mut rhythm, &mut buf, &mut events, &mut pay);
}

/// Copies the live state out and sends what happened (a bank pays its Notes).
fn publish(state: &mut StyleState, rhythm: &mut Rhythm, buf: &mut Vec<StyleEvent>, events: &mut MessageWriter<StyleEvent>, pay: &mut MessageWriter<super::Payout>) {
    if *rhythm != state.0.rhythm {
        *rhythm = state.0.rhythm.clone();
    }
    for e in buf.drain(..) {
        if let StyleEvent::Bank { notes, .. } = e
            && notes > 0
        {
            pay.write(super::Payout { amount: notes, reason: super::PayReason::Rhythm });
        }
        events.write(e);
    }
}

/// What the trigger system saw last frame.
#[derive(Resource, Default)]
struct Triggers {
    paused: bool,
    marker: Option<u32>,
    route: Option<u32>,
    countdown: bool,
    car: Option<usize>,
}

/// Banks and breaks that come from outside the car: the pause menu or map opening, driving into
/// an event marker, a challenge gate (bank); an event start or restart, a car swap (break); a
/// finished challenge (the Challenge skill).
#[allow(clippy::too_many_arguments)]
fn triggers(
    stage: Res<crate::world::StageWorld>,
    run: Res<crate::drive::StageRun>,
    paused: Option<Res<engine::pause::Paused>>,
    approach: Option<Res<crate::open::events::Approach>>,
    garage: Option<Res<super::garage::Garage>>,
    mut seen: ResMut<Triggers>,
    mut commands: MessageReader<StyleCommand>,
    mut results: MessageReader<crate::open::challenges::ChallengeResult>,
    mut state: ResMut<StyleState>,
    mut rhythm: ResMut<Rhythm>,
    mut events: MessageWriter<StyleEvent>,
    mut pay: MessageWriter<super::Payout>,
    mut buf: Local<Vec<StyleEvent>>,
) {
    buf.clear();
    let d = &mut state.0;
    let paused = paused.is_some_and(|p| p.0);
    if paused && !seen.paused {
        d.bank(&mut buf);
    }
    seen.paused = paused;
    let marker = approach.and_then(|a| a.inside);
    if marker.is_some() && marker != seen.marker {
        d.bank(&mut buf);
    }
    seen.marker = marker;
    for c in commands.read() {
        match c {
            StyleCommand::Bank => d.bank(&mut buf),
        }
    }
    for r in results.read() {
        d.challenge(r.stars, &mut buf);
    }
    // A new route (an event started or left) or a restart (its countdown again): a fresh run.
    let route = Some(stage.route_id());
    let countdown = run.countdown > 0.0;
    if (seen.route.is_some() && route != seen.route) || (countdown && !seen.countdown) {
        d.lose(LostReason::EventStart, &mut buf);
        d.clear();
    }
    seen.route = route;
    seen.countdown = countdown;
    let car = garage.map(|g| g.active);
    if seen.car.is_some() && car != seen.car {
        d.lose(LostReason::CarSwap, &mut buf);
        d.clear();
    }
    seen.car = car;
    publish(&mut state, &mut rhythm, &mut buf, &mut events, &mut pay);
}

/// Leaving the game drops the Rhythm (nothing banks).
fn quit(mut state: ResMut<StyleState>, mut rhythm: ResMut<Rhythm>, mut seen: ResMut<Triggers>) {
    state.0.clear();
    *rhythm = Rhythm { slide_meter: rhythm.slide_meter, ..default() };
    *seen = Triggers::default();
}

#[cfg(feature = "dev")]
mod dev {
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use super::*;

    pub fn register(app: &mut App) {
        app.add_control_command("style.state", "Style: the Rhythm (P, M, chain, timer, running skills)", params(&[]), |w, _| {
            let r = w.get_resource::<Rhythm>().ok_or("no style scorer")?;
            Ok(json!({
                "open": r.open, "points": r.points, "mult": r.mult, "chain": r.chain, "timer": r.timer,
                "last": r.last.map(|k| k.label()), "scuffs": r.scuffs, "slide_meter": r.slide_meter, "air": r.air,
                "live": LIVE_KINDS.iter().zip(r.live).filter(|(_, p)| *p > 0).map(|(k, p)| json!({"skill": k.label(), "points": p})).collect::<Vec<_>>(),
                "value": r.value(),
            }))
        })
        .add_control_command(
            "style.inject",
            "Style: score a skill now (as if completed), or bank / break the Rhythm",
            params(&[("skill", "string", "skill label (Slide, Air, Close Shave...) or bank / lose"), ("points", "integer", "points (default 500)")]),
            |w, p: Value| {
                let what = p.get("skill").and_then(Value::as_str).ok_or("`skill` is required")?.to_lowercase();
                let points = p.get("points").and_then(Value::as_u64).unwrap_or(500) as u32;
                let mut out = Vec::new();
                {
                    let mut st = w.get_resource_mut::<StyleState>().ok_or("no style scorer")?;
                    match what.as_str() {
                        "bank" => st.0.bank(&mut out),
                        "lose" => st.0.lose(LostReason::Hit, &mut out),
                        _ => {
                            let kind = StyleKind::ALL.into_iter().find(|k| k.label().to_lowercase() == what).ok_or("unknown skill")?;
                            st.0.inject(kind, points, &mut out);
                        }
                    }
                }
                let r = w.resource::<StyleState>().0.rhythm.clone();
                *w.resource_mut::<Rhythm>() = r;
                for e in &out {
                    if let StyleEvent::Bank { notes, .. } = e
                        && *notes > 0
                    {
                        w.write_message(crate::career::Payout { amount: *notes, reason: crate::career::PayReason::Rhythm });
                    }
                    w.write_message(*e);
                }
                Ok(json!({"events": out.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>()}))
            },
        );
    }
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod tests;
