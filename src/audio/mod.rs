//! Car and stage audio, all synthesised in code (no sound files): see docs/AUDIO.md.
//!
//! - [`car`]: engine (rpm/load pulse train), turbo, pops, limiter, gear whine, tyres per
//!   surface and slip, stones, wind, suspension thumps, impacts.
//! - [`ambience`]: wind, leaves, birds, cicadas, moor calls, crickets, rain, per biome/weather.
//! - [`stream`]: the synth behind one rodio source; bus gains mirrored from the engine mixer.
//!
//! Buses (engine mixer): `car` (under `sfx`, so the default voice duck takes it down 4 dB),
//! `ambience` (built-in, ducked 6 dB), `voice` (built-in, the partner's voice), `ui`.
//! Silent (nothing spawned) when the engine sound backend isn't a device: `--mute`, headless,
//! test-driven runs. `--car-audio-wav <path>` (with `--car-audio-secs N`, default 20) renders the
//! same synth on the main thread and writes a WAV, so muted/offscreen runs can be checked.

pub mod ambience;
pub mod car;
pub mod dsp;
pub mod stream;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use bevy::audio::AddAudioSource;
use bevy::prelude::*;
use engine::actions::ActionState;
use engine::sound::{BusId, Mixer, SoundBackend, SoundConfig, SoundSet};

use crate::drive::{CameraView, CarSpec, DrivenCar, StageRun};
use crate::guide::GuideStage;
use crate::guide::sync::NetCar;
use crate::stage::Surface;
use crate::vehicle::{Car, Spec};
use crate::world::StageWorld;
use ambience::AmbParams;
use car::{CarEvent, CarParams, Contact};
use stream::{Frame, Mix, RallyStream, RallySynth, Shared};

/// Name of the car bus (child of `sfx`).
pub const CAR_BUS: &str = "car";

pub struct RallyAudioPlugin;

impl Plugin for RallyAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RallyAudioStats>()
            .init_resource::<RemoteCarSound>()
            .register_type::<RallyAudioStats>()
            .insert_resource(Link { shared: Arc::new(Shared::default()), started: false, frame: Frame::default() })
            .add_systems(Startup, add_buses)
            .add_systems(PostUpdate, (start_output, net_car_sound, gather, capture).chain().after(SoundSet::Mix));
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            app.add_audio_source::<RallyStream>();
        }
    }
}

/// The car as heard on a machine that doesn't simulate it (the guide). Without a
/// `DrivenCar`, the synth plays this instead: the engine, turbo, pops and wind, but no tyres
/// (the replicated state has no wheel surface or slip). Filled every frame from
/// `guide::sync::NetCar` on the `Remote` entity; `active: false` means silence.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct RemoteCarSound {
    pub active: bool,
    pub rpm: f32,
    pub gear: i8,
    /// The driver's throttle 0..1.
    pub throttle: f32,
    /// |velocity| m/s.
    pub speed: f32,
}

/// On the guide: the replicated car (`NetCar` on the `Remote` entity) → [`RemoteCarSound`].
fn net_car_sound(cars: Query<&NetCar, With<engine::net::Remote>>, mut r: ResMut<RemoteCarSound>) {
    let next = cars
        .iter()
        .next()
        .map(|c| RemoteCarSound { active: true, rpm: c.rpm, gear: c.gear, throttle: c.throttle, speed: c.vel.length() })
        .unwrap_or_default();
    if *r != next {
        *r = next;
    }
}

/// The car sound's parameters for a replicated car (engine and wind only).
pub fn remote_params(r: &RemoteCarSound, spec: &Spec) -> CarParams {
    CarParams {
        active: r.active,
        rpm: r.rpm,
        idle_rpm: spec.idle_rpm,
        redline: spec.redline,
        throttle: r.throttle,
        shifting: false,
        gear: r.gear,
        speed: r.speed,
        surfaces: [Contact::default(); car::SURFACES],
        interior: 0.0,
    }
}

/// Live numbers for the inspector: `ecs.res.get {"name": "RallyAudioStats"}`.
#[derive(Resource, Reflect, Clone, Debug, Default)]
#[reflect(Resource)]
pub struct RallyAudioStats {
    /// "device", "capture" or "off".
    pub output: String,
    /// Share of real time the audio thread spends in the rally synth (0..1), ~0.5 s average.
    pub dsp_load: f32,
    pub rpm: f32,
    pub throttle: f32,
    pub gear: i8,
    pub speed: f32,
    /// Car / ambience bus gains as sent (linear).
    pub car_gain: f32,
    pub amb_gain: f32,
    pub impacts: u32,
    pub thumps: u32,
    pub shifts: u32,
    /// Rolling and sliding per surface class: gravel, tarmac, mud, snow, off-road.
    pub roll: [f32; 5],
    pub slide: [f32; 5],
}

#[derive(Resource)]
struct Link {
    shared: Arc<Shared>,
    started: bool,
    frame: Frame,
}

/// Main-thread render for `--car-audio-wav`.
#[derive(Resource)]
struct Capture {
    synth: Box<RallySynth>,
    path: String,
    buf: Vec<f32>,
    cap: usize,
    frac: f64,
    done: bool,
}

fn add_buses(mut mixer: ResMut<Mixer>) {
    mixer.add_bus(CAR_BUS, BusId::SFX);
}

fn start_output(
    mut commands: Commands,
    mut link: ResMut<Link>,
    backend: Res<SoundBackend>,
    cfg: Res<SoundConfig>,
    streams: Option<ResMut<Assets<RallyStream>>>,
    mut stats: ResMut<RallyAudioStats>,
) {
    if link.started {
        return;
    }
    link.started = true;
    stats.output = "off".into();
    if let Some(path) = engine::cli::dev_value("car-audio-wav") {
        let secs: f32 = engine::cli::dev_value("car-audio-secs").and_then(|v| v.parse().ok()).unwrap_or(20.0);
        let cap = (secs.clamp(0.5, 600.0) * dsp::RATE) as usize * 2;
        commands.insert_resource(Capture { synth: Box::new(RallySynth::new(7)), path, buf: Vec::with_capacity(cap + stream::BLOCK * 2), cap, frac: 0.0, done: false });
        stats.output = "capture".into();
        return;
    }
    if backend.kind() != engine::sound::BackendKind::Device {
        return;
    }
    let Some(mut streams) = streams else { return };
    let gain = if engine::cli::dev_flag("mute-output") { 0.0 } else { cfg.device_volume };
    let h = streams.add(RallyStream::new(RallySynth::new(7), link.shared.clone(), 1.0));
    commands.spawn((Name::new("Rally audio"), AudioPlayer::<RallyStream>(h), PlaybackSettings::ONCE.with_volume(bevy::audio::Volume::Linear(gain))));
    stats.output = "device".into();
}

fn surface_class(s: Surface) -> usize {
    match s {
        Surface::Gravel => car::GRAVEL,
        Surface::Tarmac => car::TARMAC,
        Surface::Mud => car::MUD,
        Surface::Snow => car::SNOW,
        Surface::Verge | Surface::Rough => car::OFFROAD,
    }
}

/// Lowest low-pass on the chain from `id` up to master.
fn chain_lowpass(mixer: &Mixer, id: BusId) -> f32 {
    let mut hz = 20_000.0f32;
    let mut cur = Some(id);
    while let Some(b) = cur.and_then(|i| mixer.get(i)) {
        hz = hz.min(b.lowpass_hz);
        cur = b.parent;
    }
    hz
}

/// The car sound's parameters from the simulation state (pure: also used by tests).
pub fn car_params(car: &Car, spec: &Spec, throttle: f32, interior: f32) -> CarParams {
    let speed = car.vel.length();
    let nominal = spec.mass * 9.81 / 4.0;
    let mut surfaces = [Contact::default(); car::SURFACES];
    for w in &car.wheels {
        let (true, Some(s)) = (w.contact, w.surface) else { continue };
        let k = surface_class(s);
        let share = 0.25 * (w.load / nominal).clamp(0.0, 2.5).sqrt();
        let wheel_v = (w.spin * spec.wheel_radius).abs();
        let roll_v = speed.max(wheel_v);
        // Slip speed (m/s): longitudinal from the slip ratio, lateral from the slip angle.
        let slip_v = w.slip_ratio.abs() * roll_v.max(1.0) + w.slip_angle.sin().abs() * speed.max(1.5);
        surfaces[k].roll += share * (roll_v / 25.0).min(1.6);
        surfaces[k].slide += share * 2.0 * dsp::smoothstep(0.3, 7.0, slip_v);
    }
    CarParams {
        active: true,
        rpm: car.rpm,
        idle_rpm: spec.idle_rpm,
        redline: spec.redline,
        throttle,
        shifting: car.shifting > 0.0,
        gear: car.gear,
        speed,
        surfaces,
        interior,
    }
}

/// Finds one-shots in state changes between sim steps: impacts, suspension slams and
/// landings, gear changes.
#[derive(Default, Clone, Debug)]
pub struct Tracker {
    seen: bool,
    fixed_t: f32,
    impacts: f32,
    gear: i8,
    comp: [f32; 4],
    rate: [f32; 4],
    contact: [bool; 4],
    cooldown: [f32; 4],
    stats_t: f32,
    last_ns: u64,
    last_frames: u64,
}

impl Tracker {
    /// Compares `car` with the last state (`dt` seconds of sim later; 0 = no step, nothing
    /// happens) and queues events into `f`. Returns counts (impacts, thumps, shifts).
    pub fn detect(&mut self, car: &Car, dt: f32, f: &mut Frame) -> [u32; 3] {
        let mut n = [0; 3];
        if !self.seen {
            self.seen = true;
            self.impacts = car.impacts;
            self.gear = car.gear;
            for i in 0..4 {
                self.comp[i] = car.wheels[i].compression;
                self.contact[i] = car.wheels[i].contact;
            }
            return n;
        }
        if dt <= 0.0 {
            return n;
        }
        let speed = car.vel.length();
        let hit = car.impacts - self.impacts;
        if hit > 0.4 {
            f.push(CarEvent::Impact(hit));
            n[0] += 1;
        }
        if car.gear != self.gear && car.gear > 0 && self.gear > 0 {
            f.push(CarEvent::Shift);
            n[2] += 1;
        }
        for i in 0..4 {
            let w = &car.wheels[i];
            let rate = (w.compression - self.comp[i]) / dt;
            self.cooldown[i] -= dt;
            let landing = w.contact && !self.contact[i] && rate > 0.5;
            let slam = rate > 1.0 && rate > self.rate[i] * 1.8 + 0.3;
            if (landing || slam) && self.cooldown[i] <= 0.0 && speed > 1.0 {
                let s = ((rate - 0.5) / 2.5).clamp(0.15, 1.0);
                f.push(CarEvent::Thump(s, if i % 2 == 0 { -0.6 } else { 0.6 }));
                self.cooldown[i] = 0.25;
                n[1] += 1;
            }
            self.rate[i] = rate;
            self.comp[i] = w.compression;
            self.contact[i] = w.contact;
        }
        self.impacts = car.impacts;
        self.gear = car.gear;
        n
    }
}

#[allow(clippy::too_many_arguments)]
fn gather(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    actions: Option<Res<ActionState>>,
    run: Option<Res<StageRun>>,
    stage: Option<Res<StageWorld>>,
    guide_stage: Option<Res<GuideStage>>,
    cam: Option<Res<CameraView>>,
    spec: Option<Res<CarSpec>>,
    cars: Query<&DrivenCar>,
    remote: Res<RemoteCarSound>,
    mixer: Res<Mixer>,
    mut link: ResMut<Link>,
    mut tr: Local<Tracker>,
    mut stats: ResMut<RallyAudioStats>,
) {
    let link = &mut *link;
    let f = &mut link.frame;
    f.n_events = 0;
    let interior = if cam.is_some_and(|c| *c != CameraView::Chase) { 1.0 } else { 0.0 };
    // Ambience: the stage's biome / weather / time.
    // (The guide has no `StageWorld`, only the `GuideStage` it generated from the stage id.)
    let def = stage.as_deref().map(|s| &s.def).or(guide_stage.as_deref().map(|g| &g.def));
    f.amb = match def {
        Some(d) => AmbParams { active: true, biome: d.biome, weather: d.weather, time: d.time, interior },
        None => AmbParams { active: false, ..f.amb },
    };
    let spec = spec.map(|s| s.0.clone()).unwrap_or_default();
    let car_state = cars.iter().next();
    match car_state {
        Some(c) => {
            let car = &c.car;
            let held = run.as_deref().is_some_and(|r| r.countdown > 0.0 || r.finished.is_some());
            let throttle = if held { 0.0 } else { actions.as_deref().map_or(0.0, |a| a.value("throttle")) };
            let ft = fixed.elapsed_secs();
            let dt = ft - tr.fixed_t;
            tr.fixed_t = ft;
            f.car = car_params(car, &spec, throttle, interior);
            let counts = tr.detect(car, dt, f);
            stats.impacts += counts[0];
            stats.thumps += counts[1];
            stats.shifts += counts[2];
            let speed = f.car.speed;
            stats.rpm = car.rpm;
            stats.throttle = throttle;
            stats.gear = car.gear;
            stats.speed = speed;
        }
        None => {
            tr.seen = false;
            f.car = remote_params(&remote, &spec);
            if remote.active {
                stats.rpm = remote.rpm;
                stats.throttle = remote.throttle;
                stats.gear = remote.gear;
                stats.speed = remote.speed;
            }
        }
    }
    let car_bus = mixer.bus(CAR_BUS).unwrap_or(BusId::SFX);
    f.mix = Mix {
        car: mixer.effective_gain(car_bus),
        amb: mixer.effective_gain(BusId::AMBIENCE),
        car_lp: chain_lowpass(&mixer, car_bus),
        amb_lp: chain_lowpass(&mixer, BusId::AMBIENCE),
    };
    stats.car_gain = f.mix.car;
    stats.amb_gain = f.mix.amb;
    for i in 0..car::SURFACES {
        stats.roll[i] = f.car.surfaces[i].roll;
        stats.slide[i] = f.car.surfaces[i].slide;
    }
    link.shared.publish(&link.frame);
    // CPU share of the audio thread, every ~0.5 s.
    tr.stats_t += time.delta_secs();
    if tr.stats_t >= 0.5 {
        tr.stats_t = 0.0;
        let ns = link.shared.render_ns.load(Ordering::Relaxed);
        let frames = link.shared.rendered.load(Ordering::Relaxed);
        let df = frames.saturating_sub(tr.last_frames);
        if df > 0 {
            stats.dsp_load = (ns.saturating_sub(tr.last_ns) as f64 / (df as f64 / dsp::RATE as f64 * 1e9)) as f32;
        }
        tr.last_ns = ns;
        tr.last_frames = frames;
    }
}

/// `--car-audio-wav`: render the synth on the main thread in step with game time, then write.
fn capture(time: Res<Time>, link: Res<Link>, cap: Option<ResMut<Capture>>, mut stats: ResMut<RallyAudioStats>) {
    let Some(mut cap) = cap else { return };
    if cap.done {
        return;
    }
    let t0 = std::time::Instant::now();
    let c = &mut *cap;
    c.synth.apply(&link.frame);
    c.frac += time.delta_secs_f64() * dsp::RATE as f64;
    let blocks = (c.frac / stream::BLOCK as f64).floor() as usize;
    c.frac -= (blocks * stream::BLOCK) as f64;
    for _ in 0..blocks {
        if c.buf.len() >= c.cap {
            break;
        }
        let b = c.synth.render_block();
        c.buf.extend_from_slice(b);
    }
    let audio = (blocks * stream::BLOCK) as f64 / dsp::RATE as f64;
    if audio > 0.0 {
        stats.dsp_load = (t0.elapsed().as_secs_f64() / audio) as f32;
    }
    if c.buf.len() >= c.cap {
        c.done = true;
        c.buf.truncate(c.cap);
        let clip = engine::sound::SoundClip::from_interleaved(std::mem::take(&mut c.buf), 2, dsp::RATE as u32);
        match std::fs::write(&c.path, clip.to_wav()) {
            Ok(()) => info!("car audio: wrote {} ({:.1} s)", c.path, clip.duration()),
            Err(e) => warn!("car audio: can't write {}: {e}", c.path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::{self, Flat, Input};

    /// Drives the real car sim and the synth together: accelerate through the gears, lift,
    /// slide on gravel; checks events, levels, and that the pitch follows the rpm.
    #[test]
    fn sim_drives_the_synth() {
        let spec = Spec::default();
        let ground = Flat(0.0, Surface::Gravel);
        let mut car = Car::new(Vec3::new(0.0, 0.9, 0.0), 0.0);
        let mut synth = RallySynth::new(3);
        let mut tr = Tracker::default();
        let mut frame = Frame::default();
        let mut audio: Vec<f32> = Vec::with_capacity(48_000 * 2 * 20);
        let mut rpms: Vec<(usize, f32, f32)> = Vec::new();
        let mut counts = [0u32; 3];
        let dt = 1.0 / 60.0;
        let mut frac = 0.0f64;
        for i in 0..(16 * 60) {
            let t = i as f32 * dt;
            let input = match t {
                t if t < 0.5 => Input::default(),
                t if t < 10.0 => Input { throttle: 1.0, ..default() },
                t if t < 12.0 => Input::default(),
                _ => Input { throttle: 1.0, steer: 1.0, handbrake: t < 13.0, ..default() },
            };
            vehicle::step(&mut car, &spec, input, &ground, dt);
            frame.n_events = 0;
            frame.car = car_params(&car, &spec, input.throttle, 0.0);
            let c = tr.detect(&car, dt, &mut frame);
            for k in 0..3 {
                counts[k] += c[k];
            }
            synth.apply(&frame);
            frac += dt as f64 * 48_000.0;
            let blocks = (frac / stream::BLOCK as f64) as usize;
            frac -= (blocks * stream::BLOCK) as f64;
            rpms.push((audio.len() / 2, car.rpm, input.throttle));
            synth.render_into(blocks * stream::BLOCK, &mut audio);
        }
        assert!(audio.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        assert!(counts[2] >= 2, "upshifts heard: {counts:?}");
        let mono: Vec<f32> = audio.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect();
        if let Ok(path) = std::env::var("PACENOTE_AUDIO_WAV") {
            let clip = engine::sound::SoundClip::from_interleaved(audio.clone(), 2, 48_000);
            std::fs::write(&path, clip.to_wav()).unwrap();
            let rpm_csv: String = rpms.iter().map(|(f, r, th)| format!("{f},{r},{th}\n")).collect();
            std::fs::write(format!("{path}.csv"), rpm_csv).unwrap();
        }
        // Pitch tracking under load (1..10 s): harmonic sum minus half-orders, best candidate.
        let (mut ok, mut total) = (0, 0);
        let w = 4800;
        for (i, &(start, _, th)) in rpms.iter().enumerate().step_by(15) {
            // rpm in the middle of the window (it climbs fast in low gears).
            let rpm = rpms.get(i + 3).map_or(0.0, |r| r.1);
            if th < 1.0 || start < 48_000 || start + w > mono.len() || start > 48_000 * 10 - w {
                continue;
            }
            let x = &mono[start..start + w];
            let want = rpm / 30.0;
            // Search ±45 % around the expected firing frequency (no octave jumps), so a pitch
            // that ignored rpm would land on the search edges.
            let score = |f: f32| (1..=4).map(|k| dsp::goertzel(x, f * k as f32) - dsp::goertzel(x, f * (k as f32 - 0.5))).sum::<f32>();
            let mut best = (0.0f32, f32::MIN);
            let mut f = want / 1.45;
            while f < want * 1.45 {
                let s = score(f);
                if s > best.1 {
                    best = (f, s);
                }
                f *= 1.015;
            }
            total += 1;
            if (best.0 / want - 1.0).abs() < 0.08 {
                ok += 1;
            } else {
                println!("miss at {:.2}s: rpm {rpm:.0} want {want:.1} got {:.1}", start as f32 / 48_000.0, best.0);
            }
        }
        assert!(total >= 8 && ok * 10 >= total * 8, "pitch follows rpm in {ok}/{total} windows");
    }

    /// A replicated car (no `DrivenCar`) still sounds: engine and wind, no tyres.
    #[test]
    fn remote_car_sounds_without_tyres() {
        let spec = Spec::default();
        let off = remote_params(&RemoteCarSound::default(), &spec);
        assert!(!off.active);
        let on = remote_params(&RemoteCarSound { active: true, rpm: 5200.0, gear: 3, throttle: 0.8, speed: 30.0 }, &spec);
        assert!(on.active && on.rpm == 5200.0 && on.throttle == 0.8 && on.gear == 3);
        assert!(on.surfaces.iter().all(|c| c.roll == 0.0 && c.slide == 0.0));
        let mut synth = RallySynth::new(1);
        synth.apply(&Frame { car: on, ..Frame::default() });
        let mut out = Vec::new();
        synth.render_into(24_000, &mut out);
        let rms = (out[24_000..].iter().map(|v| v * v).sum::<f32>() / (out.len() - 24_000) as f32).sqrt();
        assert!(rms > 0.02 && out.iter().all(|v| v.is_finite()), "{rms}");
    }
}
