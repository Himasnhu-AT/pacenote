//! Exhaust pops and bangs: the main-thread [`PopScheduler`] decides each pop (lift-off crackle,
//! upshift bang, downshift blip, limiter, misfire) and sends [`ExhaustPop`]; the synth plays it
//! ([`CarEvent::Pop`] → [`PopVoices`]) and `art::exhaust` draws its flame. One source, so the
//! bang and the flame always match.
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/AUDIO_PASS.md` §1-§2 (trigger rules,
//! burst timing, recipe, routing) and §6; `docs/festival/ART_BIBLE.md` §3.1 (its `Backfire` is
//! this message); `docs/festival/DECISIONS.md` D1-D2.
//!
//! - Main thread: [`PopScheduler`] (pure, per car, game time) → [`ExhaustPop`] messages and
//!   [`PopFeed`] (the synth events `audio::gather` sends with the frame).
//! - Audio thread: [`PopVoices`], a pool of 6 synthesised pops (crack, recorded pop, body, sub
//!   thump, pipe ring), each scaled to the engine's own loudness, mixed after the compressor.
//!
//! Look- and sound-only: nothing here feeds the sim.

use bevy::prelude::*;

use super::car::CarEvent;
use super::dsp::{Biquad, Delay, Noise, RATE, smoothstep};
use super::samples::{Bank, Id};
use crate::career::garage::PopProfile;

/// What kind of pop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopKind {
    Crackle,
    Bang,
    /// Damage backfire: black smoke, no flame.
    Misfire,
}

/// One pop, on the frame it is due. Audio and VFX read the same message.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ExhaustPop {
    /// The car entity (player, guide's remote car, rival).
    pub car: Entity,
    /// 0..1: size of the flame, level of the bang.
    pub intensity: f32,
    pub kind: PopKind,
    /// Per-pop variation (flame shape, synth layers).
    pub seed: u32,
}

/// Synth events for this frame's pops (drained by `audio::gather` into the frame it publishes).
#[derive(Resource, Default, Debug)]
pub struct PopFeed {
    pub events: Vec<CarEvent>,
}

/// Live pop counters for the inspector and tests (`ecs.res.get {"name": "PopStats"}`).
/// `scheduled` counts `ExhaustPop`s sent; `played` the pops the synth started (AUDIO_PASS §8.6:
/// they are equal while audio runs).
#[derive(Resource, Reflect, Clone, Debug, Default)]
#[reflect(Resource)]
pub struct PopStats {
    pub scheduled: u32,
    pub played: u32,
    pub bangs: u32,
    pub misfires: u32,
}

/// Runs the scheduler (Update). `art::exhaust` reads `ExhaustPop` after it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PopSet;

pub struct PopsPlugin;

impl Plugin for PopsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ExhaustPop>()
            .init_resource::<PopFeed>()
            .init_resource::<PopStats>()
            .register_type::<PopStats>()
            .add_systems(Update, schedule_pops.in_set(PopSet));
    }
}

// ---------------------------------------------------------------- the scheduler (main thread)

/// What the scheduler reads from a car each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PopInput {
    pub active: bool,
    /// The driver's throttle 0..1 (the pedal, not the ignition cut).
    pub throttle: f32,
    pub rpm: f32,
    pub redline: f32,
    pub gear: i8,
    pub engine_damage: f32,
}

impl Default for PopInput {
    fn default() -> Self {
        PopInput { active: true, throttle: 0.0, rpm: 950.0, redline: 7600.0, gear: 1, engine_damage: 0.0 }
    }
}

/// One pop the scheduler emits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pop {
    /// 0..1 (AUDIO_PASS §2.2's per-pop amplitude `a`).
    pub intensity: f32,
    pub kind: PopKind,
    pub seed: u32,
}

#[derive(Clone, Copy, Debug)]
struct Due {
    at: f64,
    pop: Pop,
    /// A lift-off burst's pop: putting the throttle back on ends the burst.
    lift: bool,
}

/// Lowest rpm a lift crackles at.
pub const LIFT_RPM: f32 = 3200.0;
/// Throttle history window for a lift (any pedal speed: a gamepad trigger let go over 150 ms
/// still counts).
const LIFT_WINDOW: f64 = 0.2;
/// Most pops per second per car.
const MAX_RATE: usize = 10;
const MAX_DUE: usize = 16;

/// Decides when a car's exhaust pops (AUDIO_PASS §2.1-§2.2), in game time. Pure and seeded.
#[derive(Clone, Debug)]
pub struct PopScheduler {
    rng: Noise,
    pub profile: PopProfile,
    t: f64,
    /// Throttle samples (time, value), newest last; only the last [`LIFT_WINDOW`] matter.
    hist: [(f64, f32); 32],
    hist_n: usize,
    /// The throttle has been up (≥ 0.6) since the last lift.
    armed: bool,
    last_gear: i8,
    last_rpm: f32,
    lim_phase: f32,
    due: [Option<Due>; MAX_DUE],
    /// Times of the last pops played (rate cap).
    recent: [f64; MAX_RATE],
    recent_w: usize,
    seen: bool,
}

impl PopScheduler {
    pub fn new(seed: u32, profile: PopProfile) -> Self {
        PopScheduler {
            rng: Noise::new(seed ^ 0x5eed_0b0b),
            profile,
            t: 0.0,
            hist: [(f64::MIN, 0.0); 32],
            hist_n: 0,
            armed: false,
            last_gear: 0,
            last_rpm: 0.0,
            lim_phase: 0.0,
            due: [None; MAX_DUE],
            recent: [f64::MIN; MAX_RATE],
            recent_w: 0,
            seen: false,
        }
    }

    /// Pops waiting to play.
    pub fn pending(&self) -> usize {
        self.due.iter().flatten().count()
    }

    /// The highest throttle in the last [`LIFT_WINDOW`] seconds.
    fn peak(&self) -> f32 {
        self.hist[..self.hist_n].iter().filter(|(t, _)| self.t - t <= LIFT_WINDOW + 1e-6).fold(0.0f32, |m, (_, v)| m.max(*v))
    }

    fn add(&mut self, at: f64, intensity: f32, kind: PopKind, lift: bool) {
        let seed = self.rng.next_u32();
        let d = Due { at, pop: Pop { intensity: intensity.clamp(0.0, 1.0), kind, seed }, lift };
        if let Some(slot) = self.due.iter_mut().find(|d| d.is_none()) {
            *slot = Some(d);
        }
    }

    /// A burst after a lift (or a downshift): `n` pops over `window` s from now, the first a
    /// Bang if `bang_first`. Amplitude `a = i × rand(0.6..1) × (1 − 0.5 t/window)`.
    fn burst(&mut self, n: usize, window: f32, i: f32, bang_first: bool, lift: bool) {
        let mut at = self.rng.range(0.03, 0.09) as f64;
        for k in 0..n {
            if k > 0 {
                at += if self.rng.chance(0.3) { self.rng.range(0.025, 0.045) } else { self.rng.range(0.04, 0.22) } as f64;
            }
            if at > window as f64 {
                break;
            }
            let fade = 1.0 - 0.5 * (at as f32 / window).min(1.0);
            let a = i * self.rng.range(0.6, 1.0) * fade;
            let kind = if k == 0 && bang_first { PopKind::Bang } else { PopKind::Crackle };
            self.add(self.t + at, a, kind, lift);
        }
    }

    /// A damaged engine may backfire on a lift or a shift (chance `dmg²` above 0.3).
    fn misfire(&mut self, dmg: f32) {
        if dmg > 0.3 && self.rng.chance(dmg * dmg) {
            let at = self.t + self.rng.range(0.05, 0.3) as f64;
            self.add(at, 0.6 + 0.4 * dmg, PopKind::Misfire, false);
        }
    }

    /// Advances `dt` seconds of game time with this frame's state and appends the pops due
    /// now to `out`.
    pub fn update(&mut self, dt: f32, p: &PopInput, out: &mut Vec<Pop>) {
        self.t += dt.max(0.0) as f64;
        if self.hist_n == self.hist.len() {
            self.hist.copy_within(1.., 0);
            self.hist_n -= 1;
        }
        self.hist[self.hist_n] = (self.t, p.throttle);
        self.hist_n += 1;
        if !self.seen {
            self.seen = true;
            self.last_gear = p.gear;
            self.last_rpm = p.rpm;
        }
        if !p.active || p.rpm < 300.0 {
            self.due = [None; MAX_DUE];
            self.armed = false;
            self.last_gear = p.gear;
            self.last_rpm = p.rpm;
            return;
        }
        let redline = p.redline.max(2000.0);
        let rally = self.profile == PopProfile::Rally;
        // Lift-off crackle: from a high throttle in the last 200 ms to a closed one.
        if p.throttle >= 0.6 {
            self.armed = true;
        }
        if self.armed && p.throttle <= 0.2 {
            self.armed = false;
            if self.peak() >= 0.6 && p.rpm >= LIFT_RPM {
                let i = smoothstep(LIFT_RPM, redline, p.rpm);
                let mut n = 2 + (4.0 * i).round() as usize;
                if rally {
                    n = (n + 1).min(6);
                }
                let level = 0.25 + 0.75 * i;
                let bang = i > if rally { 0.55 } else { 0.7 };
                self.burst(n, 0.4 + 0.8 * i, level, bang, true);
                self.misfire(p.engine_damage);
            }
        }
        // Gear changes.
        if p.gear != self.last_gear && p.gear > 0 && self.last_gear > 0 {
            let rpm = p.rpm.max(self.last_rpm);
            if p.gear > self.last_gear {
                if p.throttle >= 0.7 && rpm >= 0.7 * redline && self.rng.chance(0.9) {
                    let rpm_n = ((rpm - 0.7 * redline) / (0.3 * redline)).clamp(0.0, 1.0);
                    let t = self.t + 0.01;
                    self.add(t, 0.7 + 0.3 * rpm_n, PopKind::Bang, false);
                    let extra = (self.rng.next_u32() % 3) as usize;
                    for _ in 0..extra {
                        let at = self.t + self.rng.range(0.04, 0.15) as f64;
                        let a = self.rng.range(0.35, 0.6);
                        self.add(at, a, PopKind::Crackle, false);
                    }
                }
            } else if rpm >= 2800.0 {
                let i = 0.7 * smoothstep(2800.0, redline, rpm).max(0.35);
                let n = 1 + (self.rng.next_u32() % 3) as usize;
                self.burst(n, 0.4, i, false, false);
            }
            self.misfire(p.engine_damage);
        }
        // Rev limiter: each ignition cut (~14 a second) may crackle.
        if p.rpm >= redline - 80.0 && p.throttle > 0.5 {
            self.lim_phase += 14.0 * dt;
            while self.lim_phase >= 1.0 {
                self.lim_phase -= 1.0;
                if self.rng.chance(0.35) {
                    let t = self.t;
                    self.add(t, 0.6, PopKind::Crackle, false);
                }
            }
        } else {
            self.lim_phase = 0.0;
        }
        // Back on the throttle during a lift burst: at most one more pop.
        if p.throttle > 0.35 {
            let next = self.due.iter().flatten().filter(|d| d.lift).map(|d| d.at).fold(f64::MAX, f64::min);
            for d in &mut self.due {
                if d.is_some_and(|d| d.lift && d.at > next) {
                    *d = None;
                }
            }
        }
        // Due now, oldest first; at most 10 a second.
        loop {
            let now = self.t + 1e-9;
            let mut best: Option<usize> = None;
            for k in 0..MAX_DUE {
                if let Some(d) = self.due[k]
                    && d.at <= now
                    && best.is_none_or(|b| d.at < self.due[b].map_or(f64::MAX, |x| x.at))
                {
                    best = Some(k);
                }
            }
            let Some(k) = best else { break };
            let Some(d) = self.due[k].take() else { break };
            if self.t - self.recent[self.recent_w] < 1.0 {
                continue;
            }
            self.recent[self.recent_w] = self.t;
            self.recent_w = (self.recent_w + 1) % MAX_RATE;
            out.push(d.pop);
        }
        self.last_gear = p.gear;
        self.last_rpm = p.rpm;
    }
}

/// The car's pop profile: the active garage car's (a fitted Turbo also makes it Rally).
pub fn profile_of(garage: Option<&crate::career::garage::Garage>) -> PopProfile {
    let Some(g) = garage else { return PopProfile::Rally };
    let car = g.active_car();
    if car.fitted.turbo > 0 {
        return PopProfile::Rally;
    }
    car.def().map_or(PopProfile::Rally, |d| d.pop_profile)
}

/// Pan of a pop heard from the camera: `0.6 × sin(relative bearing)` outside, a little right
/// of centre inside (the tail pipe is behind the right rear wheel).
pub fn pop_pan(cam: Option<&GlobalTransform>, tip: Vec3, interior: f32) -> f32 {
    let outside = cam.map_or(0.0, |c| {
        let d = tip - c.translation();
        let flat = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
        0.6 * flat.dot(c.right().as_vec3()).clamp(-1.0, 1.0)
    });
    outside * (1.0 - interior) + 0.15 * interior
}

#[derive(Default)]
struct SchedState {
    car: Option<Entity>,
    sched: Option<PopScheduler>,
    out: Vec<Pop>,
}

/// The player's car (or the guide's replicated one) → pops: `ExhaustPop` + synth events.
#[allow(clippy::too_many_arguments)]
fn schedule_pops(
    time: Res<Time>,
    spec: Option<Res<crate::drive::CarSpec>>,
    run: Option<Res<crate::drive::StageRun>>,
    view: Option<Res<crate::drive::CameraView>>,
    garage: Option<Res<crate::career::garage::Garage>>,
    cars: Query<(Entity, &crate::drive::DrivenCar, Option<&crate::drive::DriverInput>, &GlobalTransform)>,
    remote: Query<(Entity, &crate::guide::sync::NetCar, &GlobalTransform), With<engine::net::Remote>>,
    cams: Query<&GlobalTransform, With<engine::gfx::MainCamera>>,
    mut st: Local<SchedState>,
    mut feed: ResMut<PopFeed>,
    mut stats: ResMut<PopStats>,
    mut writer: MessageWriter<ExhaustPop>,
) {
    let spec = spec.map(|s| s.0.clone()).unwrap_or_default();
    let held = run.as_deref().is_some_and(|r| r.countdown > 0.0 || r.finished.is_some() || r.retired || r.down.is_some());
    let interior = if view.as_deref().is_some_and(|v| *v != crate::drive::CameraView::Chase) { 1.0 } else { 0.0 };
    let (entity, input, tf) = if let Some((e, d, i, tf)) = cars.iter().next() {
        let car = &d.car;
        let throttle = if held { 0.0 } else { i.map_or(0.0, |i| i.0.throttle) };
        (e, PopInput { active: true, throttle, rpm: car.rpm, redline: spec.redline, gear: car.gear, engine_damage: car.damage.engine }, tf)
    } else if let Some((e, n, tf)) = remote.iter().next() {
        (e, PopInput { active: true, throttle: n.throttle, rpm: n.rpm, redline: spec.redline, gear: n.gear, engine_damage: 0.0 }, tf)
    } else {
        st.car = None;
        st.sched = None;
        return;
    };
    let profile = profile_of(garage.as_deref());
    if st.car != Some(entity) || st.sched.is_none() {
        st.car = Some(entity);
        st.sched = Some(PopScheduler::new(entity.to_bits() as u32, profile));
    }
    let st = &mut *st;
    let Some(sched) = st.sched.as_mut() else { return };
    sched.profile = profile;
    st.out.clear();
    sched.update(time.delta_secs(), &input, &mut st.out);
    if st.out.is_empty() {
        return;
    }
    let tip = tf.transform_point(crate::art::exhaust::tip(&spec));
    let pan = pop_pan(cams.iter().next(), tip, interior);
    for p in &st.out {
        writer.write(ExhaustPop { car: entity, intensity: p.intensity, kind: p.kind, seed: p.seed });
        feed.events.push(CarEvent::Pop { intensity: p.intensity, kind: p.kind, seed: p.seed, pan });
        stats.scheduled += 1;
        match p.kind {
            PopKind::Bang => stats.bangs += 1,
            PopKind::Misfire => stats.misfires += 1,
            PopKind::Crackle => {}
        }
    }
    // Nobody drained it (no audio gather this frame): don't let it grow.
    if feed.events.len() > 64 {
        feed.events.drain(..32);
    }
}

// ---------------------------------------------------------------- the synth (audio thread)

/// Crackle and Bang peaks over the engine's 50 ms RMS (AUDIO_PASS §2.4).
pub const CRACKLE_DB: f32 = 8.0;
pub const BANG_DB: f32 = 13.0;
/// Hard cap on a pop's peak (−3 dBFS).
pub const POP_CAP: f32 = 0.708;
/// A pop is measured over its first 25 ms (where every layer peaks) to set its gain.
const MEASURE: usize = 1200;
const VOICES: usize = 6;

/// One pop: crack + recorded + body + sub thump + pipe ring (AUDIO_PASS §2.3), every layer
/// drawn from its seed.
#[derive(Clone, Debug)]
struct PopVoice {
    on: bool,
    age: u32,
    n: [Noise; 2],
    /// Crack: noise burst (`burst` samples flat, then decaying by `crack_k` per sample) into a
    /// resonant low-pass, one per channel (decorrelated).
    crack_lp: [Biquad; 2],
    burst: u32,
    crack_env: f32,
    crack_k: f32,
    /// Recorded pop.
    rec: Id,
    rec_pos: f64,
    rec_rate: f32,
    rec_g: f32,
    /// Body: noise into a band-pass.
    body_bp: Biquad,
    body_env: f32,
    body_k: f32,
    body_g: f32,
    /// Sub thump: a sine falling ×0.7 over 30 ms.
    sub_ph: f32,
    sub_hz: f32,
    sub_fall: f32,
    sub_env: f32,
    sub_k: f32,
    sub_g: f32,
    /// Pipe ring (Bangs): a comb on the body.
    ring: Delay<512>,
    ring_d: f32,
    ring_env: f32,
    ring_k: f32,
    ring_g: f32,
    /// Output gains (level × pan).
    gl: f32,
    gr: f32,
}

fn decay_k(tau_secs: f32) -> f32 {
    (-1.0 / (tau_secs * RATE)).exp()
}

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

impl PopVoice {
    fn off() -> Self {
        PopVoice {
            on: false,
            age: 0,
            n: [Noise::new(1), Noise::new(2)],
            crack_lp: [Biquad::default(); 2],
            burst: 0,
            crack_env: 0.0,
            crack_k: 0.0,
            rec: Id::Pop1,
            rec_pos: 0.0,
            rec_rate: 1.0,
            rec_g: 0.0,
            body_bp: Biquad::default(),
            body_env: 0.0,
            body_k: 0.0,
            body_g: 0.0,
            sub_ph: 0.0,
            sub_hz: 60.0,
            sub_fall: 1.0,
            sub_env: 0.0,
            sub_k: 0.0,
            sub_g: 0.0,
            ring: Delay::default(),
            ring_d: 300.0,
            ring_env: 0.0,
            ring_k: 0.0,
            ring_g: 0.0,
            gl: 0.0,
            gr: 0.0,
        }
    }

    /// A new pop from its seed (unit level: gains set by [`PopVoices::trigger`]).
    fn start(&mut self, kind: PopKind, seed: u32) {
        let mut r = Noise::new(seed ^ 0xb0b5_1eed);
        let bang = kind != PopKind::Crackle;
        let lp = Biquad::lowpass(r.range(1800.0, 4500.0), r.range(2.0, 4.0));
        *self = PopVoice {
            on: true,
            age: 0,
            n: [Noise::new(seed ^ 0x1234_5677), Noise::new(seed ^ 0x7654_3211)],
            crack_lp: [lp; 2],
            burst: (r.range(0.001, 0.003) * RATE) as u32,
            crack_env: 1.0,
            crack_k: decay_k(if bang { r.range(0.015, 0.025) } else { r.range(0.008, 0.015) }),
            rec: [Id::Pop1, Id::Pop2, Id::Pop3][(r.next_u32() % 3) as usize],
            rec_pos: (r.range(0.0, 0.010) * RATE) as f64,
            rec_rate: r.range(0.85, 1.15),
            rec_g: db(if bang { -4.0 } else { -9.0 }),
            body_bp: Biquad::bandpass(r.range(180.0, 450.0), 1.5),
            body_env: 1.0,
            body_k: decay_k(if bang { r.range(0.06, 0.09) } else { 0.03 }),
            // The band-pass passes a sliver of white noise: make it up.
            body_g: db(if bang { -3.0 } else { -6.0 }) * 4.0,
            sub_ph: 0.0,
            sub_hz: r.range(55.0, 80.0),
            sub_fall: 0.7f32.powf(1.0 / (0.03 * RATE)),
            sub_env: 1.0,
            sub_k: decay_k(0.05),
            sub_g: db(if bang { -2.0 } else { -15.0 }),
            ring: Delay::default(),
            ring_d: r.range(0.006, 0.009) * RATE,
            ring_env: 1.0,
            ring_k: decay_k(0.08),
            ring_g: if bang { db(-10.0) } else { 0.0 },
            gl: 0.0,
            gr: 0.0,
        };
    }

    /// One sample: (left, right) of the main layers and the sub alone (mono), before the
    /// voice's gain.
    #[inline]
    fn raw(&mut self, bank: Option<&Bank>) -> (f32, f32, f32) {
        let mut c = [0.0; 2];
        for (ch, out) in c.iter_mut().enumerate() {
            let x = self.n[ch].white() * self.crack_env;
            *out = self.crack_lp[ch].run(x);
        }
        if self.age >= self.burst {
            self.crack_env *= self.crack_k;
        }
        let (rec, rec_done) = match bank {
            Some(b) => {
                let x = b.get(self.rec);
                let v = super::samples::read_clamped(x, self.rec_pos);
                self.rec_pos += self.rec_rate as f64;
                (v * self.rec_g, self.rec_pos >= x.len() as f64)
            }
            None => (0.0, true),
        };
        let body = self.body_bp.run(self.n[0].white()) * self.body_env * self.body_g;
        self.body_env *= self.body_k;
        let ring = if self.ring_g > 0.0 {
            let y = body + 0.45 * self.ring.read(self.ring_d);
            self.ring.write(y);
            let r = (y - body) * self.ring_env * self.ring_g * 3.0;
            self.ring_env *= self.ring_k;
            r
        } else {
            0.0
        };
        self.sub_ph = (self.sub_ph + self.sub_hz / RATE).fract();
        self.sub_hz = (self.sub_hz * self.sub_fall).max(30.0);
        let sub = (self.sub_ph * std::f32::consts::TAU).sin() * self.sub_env * self.sub_g;
        self.sub_env *= self.sub_k;
        self.age += 1;
        let mid = rec + body + ring;
        if self.age > self.burst && self.crack_env < 1e-4 && self.body_env < 1e-4 && self.sub_env < 1e-4 && (self.ring_g == 0.0 || self.ring_env < 1e-4) && rec_done {
            self.on = false;
        }
        (c[0] + mid, c[1] + mid, sub)
    }
}

/// The pop pool of the player's car: 6 voices, the oldest stolen. Renders the chase mix (with
/// a ground-slap reflection) and the cockpit mix (low-passed, sub up), crossfaded by the
/// camera's `interior`.
#[derive(Clone, Debug)]
pub struct PopVoices {
    v: [PopVoice; VOICES],
    /// Start order (for stealing the oldest).
    started: [u32; VOICES],
    /// Ground slap (chase): 9 ms later, −10 dB, low-passed at 3 kHz.
    slap: [Delay<512>; 2],
    slap_lp: [Biquad; 2],
    /// Cockpit: everything low-passed at 1.4 kHz.
    cabin_lp: [Biquad; 2],
    /// Samples since the last voice ended (the tails of the shared filters ring out first).
    idle: u32,
    /// Pops started since the synth was made.
    pub played: u32,
}

impl Default for PopVoices {
    fn default() -> Self {
        PopVoices {
            v: std::array::from_fn(|_| PopVoice::off()),
            started: [0; VOICES],
            slap: [Delay::default(), Delay::default()],
            slap_lp: [Biquad::lowpass(3000.0, 0.7); 2],
            cabin_lp: [Biquad::lowpass(1400.0, 0.7); 2],
            idle: u32::MAX,
            played: 0,
        }
    }
}

impl PopVoices {
    /// Starts a pop. `eng_rms` is the engine's own 50 ms RMS: a Crackle peaks [`CRACKLE_DB`]
    /// over it and a Bang [`BANG_DB`] (± 2 dB with the pop's intensity), never over
    /// [`POP_CAP`]. A Misfire is a Bang without a flame.
    pub fn trigger(&mut self, intensity: f32, kind: PopKind, seed: u32, pan: f32, eng_rms: f32, bank: Option<&Bank>) {
        // A free voice, else the oldest.
        let i = (0..VOICES).find(|&k| !self.v[k].on).unwrap_or_else(|| (0..VOICES).min_by_key(|&k| self.started[k]).unwrap_or(0));
        self.played = self.played.wrapping_add(1);
        self.started[i] = self.played;
        self.idle = 0;
        let v = &mut self.v[i];
        v.start(kind, seed);
        // Measure the pop's own peak over its first 25 ms (on a copy), so every random draw
        // lands at the same level.
        let mut probe = v.clone();
        let mut peak = 1e-6f32;
        for _ in 0..MEASURE {
            let (l, r, s) = probe.raw(bank);
            peak = peak.max((l + s).abs()).max((r + s).abs());
        }
        let base = if kind == PopKind::Crackle { CRACKLE_DB } else { BANG_DB };
        let level_db = base + (4.0 * (intensity.clamp(0.0, 1.0) - 0.5)).clamp(-2.0, 2.0);
        let want = (eng_rms.max(0.003) * db(level_db)).min(POP_CAP);
        let g = want / peak;
        // Equal power, unity at the centre.
        let (l, r) = super::dsp::pan(pan);
        v.gl = g * l;
        v.gr = g * r;
    }

    pub fn active(&self) -> bool {
        self.v.iter().any(|v| v.on)
    }

    /// One stereo sample of every pop playing, for a camera `interior` (0 chase .. 1 cockpit).
    #[inline]
    pub fn sample(&mut self, interior: f32, bank: Option<&Bank>) -> (f32, f32) {
        // Idle: nothing playing and the shared tails long gone.
        if self.idle > 4800 {
            return (0.0, 0.0);
        }
        let (mut l, mut r, mut sub) = (0.0, 0.0, 0.0);
        let mut any = false;
        for v in self.v.iter_mut().filter(|v| v.on) {
            any = true;
            let (a, b, s) = v.raw(bank);
            l += a * v.gl;
            r += b * v.gr;
            sub += s * (v.gl + v.gr) * 0.5;
        }
        if any {
            self.idle = 0;
        } else {
            self.idle += 1;
            if self.idle > 4800 {
                for ch in 0..2 {
                    self.slap[ch] = Delay::default();
                    self.slap_lp[ch].reset();
                    self.cabin_lp[ch].reset();
                }
                return (0.0, 0.0);
            }
        }
        let out = 1.0 - interior;
        let mut lr = [l, r];
        for (ch, x) in lr.iter_mut().enumerate() {
            let dry = *x;
            self.slap[ch].write(dry);
            let slap = self.slap_lp[ch].run(self.slap[ch].read(0.009 * RATE)) * db(-10.0);
            let chase = dry + slap + sub;
            // Inside: low-passed and 5 dB down, the sub 3 dB up (felt through the shell).
            let cabin = self.cabin_lp[ch].run(dry) * db(-5.0) + sub * db(3.0);
            *x = chase * out + cabin * interior;
        }
        (lr[0], lr[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn default() -> PopInput {
        PopInput::default()
    }

    /// Holds `rpm` at full throttle for 1 s, then lifts over `ramp` seconds (0 = a key) and
    /// coasts at the same rpm; the pops within 1.2 s of the lift.
    fn lift(seed: u32, rpm: f32, ramp: f32, profile: PopProfile) -> Vec<Pop> {
        let mut s = PopScheduler::new(seed, profile);
        let mut out = Vec::new();
        let p = PopInput { throttle: 1.0, rpm, gear: 3, ..default() };
        for _ in 0..60 {
            s.update(DT, &p, &mut out);
        }
        assert!(out.is_empty(), "no pops under power");
        let mut t = 0.0;
        while t < 1.2 {
            t += DT;
            let th = if ramp > 0.0 { (1.0 - t / ramp).max(0.0) } else { 0.0 };
            s.update(DT, &PopInput { throttle: th, ..p }, &mut out);
        }
        out
    }

    #[test]
    fn lift_at_6000_pops_two_to_six_with_keys_and_a_slow_trigger() {
        for profile in [PopProfile::Road, PopProfile::Rally] {
            for ramp in [0.0, 0.15] {
                let ok = (0..50).filter(|&seed| (2..=6).contains(&lift(seed, 6000.0, ramp, profile).len())).count();
                assert!(ok >= 45, "{profile:?} ramp {ramp}: {ok}/50 runs with 2-6 pops");
            }
        }
    }

    #[test]
    fn lift_at_3500_gives_two_or_three_and_none_below_3200() {
        for seed in 0..50 {
            for profile in [PopProfile::Road, PopProfile::Rally] {
                let n = lift(seed, 3500.0, 0.0, profile).len();
                assert!((2..=3).contains(&n), "seed {seed} {profile:?}: {n} pops at 3500");
                assert_eq!(lift(seed, 3100.0, 0.0, profile).len(), 0);
                assert_eq!(lift(seed, 3100.0, 0.15, profile).len(), 0);
            }
        }
        // A very slow lift (0.6 s) isn't a lift-off.
        assert_eq!(lift(3, 6000.0, 0.6, PopProfile::Rally).len(), 0);
    }

    #[test]
    fn a_big_lift_starts_with_a_bang_and_fades() {
        let mut bangs = 0;
        for seed in 0..50 {
            let pops = lift(seed, 7300.0, 0.0, PopProfile::Road);
            bangs += (pops[0].kind == PopKind::Bang) as u32;
            assert!(pops[1..].iter().all(|p| p.kind == PopKind::Crackle));
            let seeds: std::collections::HashSet<u32> = pops.iter().map(|p| p.seed).collect();
            assert_eq!(seeds.len(), pops.len(), "every pop differs");
        }
        assert_eq!(bangs, 50);
        // Higher rpm, louder pops.
        let mean = |rpm: f32| (0..30).flat_map(|s| lift(s, rpm, 0.0, PopProfile::Road)).map(|p| p.intensity).sum::<f32>() / 30.0;
        assert!(mean(7000.0) > mean(4000.0) * 1.5);
    }

    #[test]
    fn upshift_under_power_bangs_and_downshift_crackles() {
        let mut bangs = 0;
        for seed in 0..100 {
            let mut s = PopScheduler::new(seed, PopProfile::Road);
            let mut out = Vec::new();
            let p = PopInput { throttle: 1.0, rpm: 6800.0, gear: 2, ..default() };
            for _ in 0..30 {
                s.update(DT, &p, &mut out);
            }
            for _ in 0..13 {
                s.update(DT, &PopInput { gear: 3, rpm: 5000.0, ..p }, &mut out);
            }
            bangs += out.iter().any(|p| p.kind == PopKind::Bang) as u32;
        }
        assert!(bangs >= 85, "upshift bangs {bangs}/100");
        // Downshift from 4000: at least one crackle.
        for seed in 0..30 {
            let mut s = PopScheduler::new(seed, PopProfile::Road);
            let mut out = Vec::new();
            let p = PopInput { throttle: 0.0, rpm: 3000.0, gear: 3, ..default() };
            for _ in 0..30 {
                s.update(DT, &p, &mut out);
            }
            for _ in 0..30 {
                s.update(DT, &PopInput { gear: 2, rpm: 4000.0, ..p }, &mut out);
            }
            assert!(!out.is_empty(), "seed {seed}");
        }
        // Shifting up at low rpm: nothing.
        let mut s = PopScheduler::new(1, PopProfile::Rally);
        let mut out = Vec::new();
        let p = PopInput { throttle: 1.0, rpm: 3000.0, gear: 2, ..default() };
        for g in [2, 3, 3, 3] {
            s.update(DT, &PopInput { gear: g, ..p }, &mut out);
        }
        assert!(out.is_empty());
    }

    #[test]
    fn throttle_back_on_ends_the_burst_and_the_rate_is_capped() {
        for seed in 0..20 {
            let mut s = PopScheduler::new(seed, PopProfile::Rally);
            let mut out = Vec::new();
            let p = PopInput { throttle: 1.0, rpm: 7000.0, gear: 3, ..default() };
            for _ in 0..30 {
                s.update(DT, &p, &mut out);
            }
            s.update(DT, &PopInput { throttle: 0.0, ..p }, &mut out);
            s.update(DT, &PopInput { throttle: 0.0, ..p }, &mut out);
            let before = out.len();
            for _ in 0..90 {
                s.update(DT, &PopInput { rpm: 6000.0, ..p }, &mut out);
            }
            assert!(out.len() <= before + 1, "seed {seed}: {} pops after the throttle came back", out.len() - before);
        }
        // Limiter plus lifts every few frames: never over 10 in any second.
        let mut s = PopScheduler::new(9, PopProfile::Rally);
        let mut times = Vec::new();
        let mut out = Vec::new();
        for f in 0..600 {
            let th = if f % 12 < 6 { 1.0 } else { 0.0 };
            out.clear();
            s.update(DT, &PopInput { throttle: th, rpm: 7590.0, gear: 3, ..default() }, &mut out);
            times.extend(out.iter().map(|_| f as f32 * DT));
        }
        assert!(times.len() > 20, "{}", times.len());
        for (k, t) in times.iter().enumerate() {
            let n = times[k..].iter().take_while(|u| **u < t + 1.0 - 1e-3).count();
            assert!(n <= 10, "{n} pops in a second from {t}");
        }
    }

    #[test]
    fn limiter_crackles_now_and_then() {
        let mut s = PopScheduler::new(4, PopProfile::Road);
        let mut out = Vec::new();
        for _ in 0..120 {
            s.update(DT, &PopInput { throttle: 1.0, rpm: 7580.0, gear: 3, ..default() }, &mut out);
        }
        // ~14 cuts a second × 35 %: about 10 in 2 s.
        assert!((4..=16).contains(&out.len()), "{}", out.len());
        assert!(out.iter().all(|p| p.kind == PopKind::Crackle));
    }

    #[test]
    fn damaged_engine_misfires_on_lifts() {
        let mut misfires = 0;
        for seed in 0..40 {
            let mut s = PopScheduler::new(seed, PopProfile::Road);
            let mut out = Vec::new();
            let p = PopInput { throttle: 1.0, rpm: 5000.0, gear: 3, engine_damage: 0.9, ..default() };
            for _ in 0..30 {
                s.update(DT, &p, &mut out);
            }
            for _ in 0..60 {
                s.update(DT, &PopInput { throttle: 0.0, ..p }, &mut out);
            }
            misfires += out.iter().filter(|p| p.kind == PopKind::Misfire).count();
        }
        assert!((22..=40).contains(&misfires), "{misfires}");
    }

    fn render(v: &mut PopVoices, secs: f32, interior: f32) -> Vec<(f32, f32)> {
        (0..(secs * RATE) as usize).map(|_| v.sample(interior, None)).collect()
    }

    fn peak(x: &[(f32, f32)]) -> f32 {
        x.iter().fold(0.0f32, |m, (l, r)| m.max(l.abs()).max(r.abs()))
    }

    #[test]
    fn pops_sit_over_the_engine_and_never_clip() {
        for (kind, lo, hi) in [(PopKind::Crackle, 6.0, 10.0), (PopKind::Bang, 11.0, 15.0)] {
            for seed in 0..20 {
                let mut v = PopVoices::default();
                let eng = 0.05;
                v.trigger(0.6, kind, seed, 0.0, eng, None);
                let x = render(&mut v, 1.0, 0.0);
                let over = 20.0 * (peak(&x) / eng).log10();
                assert!(over > lo && over < hi, "{kind:?} seed {seed}: {over:.1} dB over the engine");
                assert!(!v.active(), "{kind:?} ends within 1 s");
            }
        }
        // Capped at −3 dBFS however loud the engine.
        let mut v = PopVoices::default();
        v.trigger(1.0, PopKind::Bang, 3, 0.0, 0.8, None);
        let p = peak(&render(&mut v, 0.3, 0.0));
        assert!(p <= POP_CAP * 1.15, "{p}");
    }

    /// Spectral centroid (Goertzel probes every 50 Hz) and length (samples above −40 dB of the
    /// peak).
    fn centroid_and_length(x: &[(f32, f32)]) -> (f32, usize) {
        let m: Vec<f32> = x.iter().map(|(l, r)| (l + r) * 0.5).collect();
        let (mut num, mut den) = (0.0, 0.0);
        let mut f = 25.0;
        while f < 12_000.0 {
            let e = super::super::dsp::goertzel(&m[..m.len().min(12_000)], f);
            num += e * f;
            den += e;
            f += 50.0;
        }
        let pk = m.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        let len = m.iter().rposition(|v| v.abs() > pk * 0.01).unwrap_or(0);
        (num / den.max(1e-20), len)
    }

    #[test]
    fn no_two_pops_in_a_burst_alike_and_the_cockpit_is_darker() {
        let pops: Vec<_> = (0..6u32)
            .map(|k| {
                let mut v = PopVoices::default();
                v.trigger(0.7, if k == 0 { PopKind::Bang } else { PopKind::Crackle }, 1000 + k * 7919, 0.0, 0.05, None);
                centroid_and_length(&render(&mut v, 0.5, 0.0))
            })
            .collect();
        for a in 0..pops.len() {
            for b in a + 1..pops.len() {
                let (ca, la) = pops[a];
                let (cb, lb) = pops[b];
                let dc = (ca - cb).abs() / ca.max(cb);
                let dl = (la as f32 - lb as f32).abs() / la.max(lb) as f32;
                assert!(dc > 0.05 || dl > 0.10, "pops {a} and {b} alike: centroid {ca:.0}/{cb:.0} length {la}/{lb}");
            }
        }
        for kind in [PopKind::Crackle, PopKind::Bang] {
            let both = |interior: f32| {
                let mut v = PopVoices::default();
                v.trigger(0.7, kind, 77, 0.0, 0.05, None);
                render(&mut v, 0.5, interior)
            };
            let (out, inside) = (both(0.0), both(1.0));
            let (co, _) = centroid_and_length(&out);
            let (ci, _) = centroid_and_length(&inside);
            assert!(ci <= co * 0.6, "{kind:?}: cockpit centroid {ci:.0} vs chase {co:.0}");
            let sub = |x: &[(f32, f32)]| {
                let m: Vec<f32> = x.iter().map(|(l, r)| (l + r) * 0.5).collect();
                [45.0, 55.0, 65.0, 75.0, 85.0, 95.0].iter().map(|f| super::super::dsp::goertzel(&m, *f)).sum::<f32>()
            };
            assert!(sub(&inside) >= sub(&out) * 0.99, "{kind:?}: sub inside {} vs chase {}", sub(&inside), sub(&out));
        }
    }

    #[test]
    fn pan_follows_the_exhaust() {
        let mut v = PopVoices::default();
        v.trigger(0.7, PopKind::Crackle, 5, 0.6, 0.05, None);
        let x = render(&mut v, 0.2, 0.0);
        let (l, r) = x.iter().fold((0.0, 0.0), |(a, b), (l, r)| (a + l * l, b + r * r));
        assert!(r > l * 2.0, "right {r} left {l}");
        // From the camera: an exhaust to the camera's right pans right.
        let cam = GlobalTransform::from(Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y));
        assert!(pop_pan(Some(&cam), Vec3::new(2.0, 0.0, 0.0), 0.0) > 0.15);
        assert!(pop_pan(Some(&cam), Vec3::new(-2.0, 0.0, 0.0), 0.0) < -0.15);
    }
}
