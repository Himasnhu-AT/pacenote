//! Offline renders of scripted driving scenes: the real car sim (`vehicle::step`) drives the
//! synth exactly as the game does (parameters once per 60 Hz frame, audio in 256-frame
//! blocks), so the sound can be checked by numbers and by ear without a device.
//!
//! `PACENOTE_AUDIO_OUT=<dir> cargo test --lib audio::scenes::render_scenes -- --ignored --nocapture`
//! writes one WAV (16-bit stereo, 48 kHz) and one CSV (time, rpm, gear, throttle, speed,
//! slide) per scene, and prints peak, RMS and the audio-thread cost per block.

use std::time::Instant;

use bevy::math::{Vec2, Vec3};

use super::stream::{BLOCK, Frame, RallySynth};
use super::{Tracker, car_params};
use crate::stage::Surface;
use crate::vehicle::{self, Car, Damage, Ground, Input, Obstacle, Spec};

/// Test ground: one surface, optional washboard bumps, a crest, a post to hit.
pub struct Course {
    pub surface: Surface,
    /// Bump height (m) and wavelength (m).
    pub bumps: (f32, f32),
    /// Crest: (z of the top, height m, half-length m); the far side drops away.
    pub crest: Option<(f32, f32, f32)>,
    /// A post (tree) to hit: centre xz, radius.
    pub post: Option<(Vec2, f32)>,
    /// Switch to another surface past this z.
    pub surface_after: Option<(f32, Surface)>,
}

impl Course {
    pub fn flat(surface: Surface) -> Self {
        Course { surface, bumps: (0.0, 1.0), crest: None, post: None, surface_after: None }
    }
}

impl Ground for Course {
    fn height(&self, x: f32, z: f32) -> f32 {
        let (a, l) = self.bumps;
        let mut h = 0.0;
        if a > 0.0 {
            let k = std::f32::consts::TAU / l;
            h += a * ((z * k).sin() * 0.7 + (z * k * 2.3 + x * 1.7).sin() * 0.3);
        }
        if let Some((zc, ch, half)) = self.crest {
            // Up a smooth ramp to the top, then a steeper drop back to the plain.
            let d = z - zc;
            if d < 0.0 && d > -half {
                let t = 1.0 + d / half;
                h += ch * t * t * (3.0 - 2.0 * t);
            } else if d >= 0.0 && d < half * 0.6 {
                let t = 1.0 - d / (half * 0.6);
                h += ch * t * t * (3.0 - 2.0 * t);
            }
        }
        h
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        let e = 0.05;
        let dx = self.height(x + e, z) - self.height(x - e, z);
        let dz = self.height(x, z + e) - self.height(x, z - e);
        Vec3::new(-dx, 2.0 * e, -dz).normalize()
    }
    fn surface(&self, _: f32, z: f32) -> Surface {
        match self.surface_after {
            Some((z0, s)) if z > z0 => s,
            _ => self.surface,
        }
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        out.clear();
        if let Some((c, rad)) = self.post
            && (c - p).length() < r + rad
        {
            out.push(Obstacle::solid(c, rad, 3.0));
        }
    }
}

/// One scripted scene.
pub struct Scene {
    pub name: &'static str,
    pub secs: f32,
    pub course: Course,
    pub damage: Damage,
    /// Bonnet camera (inside).
    pub interior: bool,
    pub script: fn(f32) -> Input,
}

/// What a render produced.
pub struct Rendered {
    pub audio: Vec<f32>,
    pub csv: String,
    /// Mean and worst time to render one 256-frame block (ns).
    pub block_ns: (f64, f64),
    pub counts: [u32; 3],
}

pub fn render(scene: &Scene, seed: u32) -> Rendered {
    let spec = Spec::default();
    let mut car = Car::new(Vec3::new(0.0, 0.75, 0.0), 0.0);
    car.damage = scene.damage;
    let mut synth = Box::new(RallySynth::new(seed));
    let mut tr = Tracker::default();
    let mut frame = Frame::default();
    let frames = (scene.secs * 60.0) as usize;
    let mut audio: Vec<f32> = Vec::with_capacity((scene.secs * 48_000.0) as usize * 2 + BLOCK * 4);
    let mut csv = String::from("t,rpm,gear,throttle,speed,slide_sum,impacts,engine_dmg,x,z,up_y,body\n");
    let dt = 1.0 / 60.0;
    let mut frac = 0.0f64;
    let (mut ns_sum, mut ns_max, mut blocks_n) = (0f64, 0f64, 0usize);
    let mut counts = [0u32; 3];
    // Exhaust pops: the main-thread scheduler, as in the game (chase: a little right).
    let mut sched = super::pops::PopScheduler::new(seed, crate::career::garage::PopProfile::Rally);
    let mut pops = Vec::new();
    for i in 0..frames {
        let t = i as f32 * dt;
        let input = (scene.script)(t);
        vehicle::step(&mut car, &spec, input, &scene.course, dt);
        frame.n_events = 0;
        frame.car = car_params(&car, &spec, input.throttle, if scene.interior { 1.0 } else { 0.0 });
        let c = tr.detect(&car, &spec, dt, &mut frame);
        for k in 0..3 {
            counts[k] += c[k];
        }
        pops.clear();
        let pin = super::pops::PopInput { active: true, throttle: input.throttle, rpm: car.rpm, redline: spec.redline, gear: car.gear, engine_damage: car.damage.engine };
        sched.update(dt, &pin, &mut pops);
        for p in &pops {
            frame.push(super::car::CarEvent::Pop { intensity: p.intensity, kind: p.kind, seed: p.seed, pan: if scene.interior { 0.15 } else { 0.25 } });
        }
        synth.apply(&frame);
        frac += dt as f64 * 48_000.0;
        let blocks = (frac / BLOCK as f64) as usize;
        frac -= (blocks * BLOCK) as f64;
        for _ in 0..blocks {
            let t0 = Instant::now();
            let b = synth.render_block();
            let ns = t0.elapsed().as_nanos() as f64;
            audio.extend_from_slice(b);
            ns_sum += ns;
            ns_max = ns_max.max(ns);
            blocks_n += 1;
        }
        let slide: f32 = frame.car.surfaces.iter().map(|s| s.slide).sum();
        csv.push_str(&format!(
            "{t:.3},{:.0},{},{:.2},{:.2},{slide:.3},{:.1},{:.2},{:.1},{:.1},{:.2},{}\n",
            car.rpm,
            car.gear,
            input.throttle,
            car.vel.length(),
            car.impacts,
            car.damage.engine,
            car.pos.x,
            car.pos.z,
            car.up().y,
            car.body_contact as u8
        ));
    }
    Rendered { audio, csv, block_ns: (ns_sum / blocks_n.max(1) as f64, ns_max), counts }
}

fn i(throttle: f32, brake: f32, steer: f32, handbrake: bool) -> Input {
    Input { throttle, brake, steer, handbrake }
}

/// The scenes the owner listens to.
pub fn scenes() -> Vec<Scene> {
    let none = Damage::default();
    vec![
        Scene { name: "01_idle", secs: 5.0, course: Course::flat(Surface::Gravel), damage: none, interior: false, script: |_| i(0.0, 0.0, 0.0, false) },
        Scene {
            name: "02_full_throttle_gears_tarmac",
            secs: 16.0,
            course: Course::flat(Surface::Tarmac),
            damage: none,
            interior: false,
            script: |t| if t < 0.7 { i(0.0, 0.0, 0.0, false) } else if t < 12.5 { i(1.0, 0.0, 0.0, false) } else { i(0.0, 0.0, 0.0, false) },
        },
        Scene {
            name: "03_lift_off_pops_gravel",
            secs: 12.0,
            course: Course::flat(Surface::Gravel),
            damage: none,
            interior: false,
            // Pull, lift, pull, lift: blow-off and overrun crackle each time.
            script: |t| match t {
                t if t < 0.5 => i(0.0, 0.0, 0.0, false),
                t if t < 5.0 => i(1.0, 0.0, 0.0, false),
                t if t < 7.0 => i(0.0, 0.0, 0.0, false),
                t if t < 9.5 => i(1.0, 0.0, 0.0, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "04_handbrake_slide_gravel",
            secs: 10.0,
            course: Course::flat(Surface::Gravel),
            damage: none,
            interior: false,
            script: |t| match t {
                t if t < 5.5 => i(1.0, 0.0, 0.0, false),
                t if t < 6.6 => i(0.3, 0.0, 1.0, true),
                t if t < 8.5 => i(0.9, 0.0, -0.4, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "05_tarmac_skid",
            secs: 10.0,
            course: Course::flat(Surface::Tarmac),
            damage: none,
            interior: false,
            // Up to speed, then a hard brake with steering (locked/ sliding tyres), then a
            // power slide.
            script: |t| match t {
                t if t < 6.0 => i(1.0, 0.0, 0.0, false),
                t if t < 7.6 => i(0.0, 1.0, 0.6, false),
                t if t < 9.0 => i(1.0, 0.0, -1.0, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "06_big_impact_post",
            secs: 8.0,
            course: Course { post: Some((Vec2::new(0.0, 70.0), 0.45)), ..Course::flat(Surface::Gravel) },
            damage: none,
            interior: false,
            script: |t| if t < 6.0 { i(1.0, 0.0, 0.0, false) } else { i(0.0, 0.0, 0.0, false) },
        },
        Scene {
            name: "07_damaged_engine_rough",
            secs: 10.0,
            course: Course { bumps: (0.05, 2.2), ..Course::flat(Surface::Gravel) },
            damage: Damage { engine: 0.8, suspension: 0.7, steering: 0.2, ..Damage::default() },
            interior: false,
            script: |t| match t {
                t if t < 0.5 => i(0.0, 0.0, 0.0, false),
                t if t < 7.0 => i(1.0, 0.0, 0.0, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "08_crest_jump_landing",
            secs: 9.0,
            course: Course { crest: Some((110.0, 1.6, 9.0)), ..Course::flat(Surface::Gravel) },
            damage: none,
            interior: false,
            script: |t| if t < 8.0 { i(1.0, 0.0, 0.0, false) } else { i(0.0, 0.0, 0.0, false) },
        },
        Scene {
            name: "09_cockpit_gravel_run",
            secs: 12.0,
            course: Course { bumps: (0.02, 3.0), ..Course::flat(Surface::Gravel) },
            damage: none,
            interior: true,
            script: |t| match t {
                t if t < 0.5 => i(0.0, 0.0, 0.0, false),
                t if t < 9.0 => i(1.0, 0.0, 0.0, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "10_chase_gravel_run",
            secs: 12.0,
            course: Course { bumps: (0.02, 3.0), ..Course::flat(Surface::Gravel) },
            damage: none,
            interior: false,
            script: |t| match t {
                t if t < 0.5 => i(0.0, 0.0, 0.0, false),
                t if t < 9.0 => i(1.0, 0.0, 0.0, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "11_surfaces_snow_then_mud",
            secs: 12.0,
            course: Course { surface_after: Some((70.0, Surface::Mud)), ..Course::flat(Surface::Snow) },
            damage: none,
            interior: false,
            script: |t| match t {
                t if t < 9.0 => i(0.75, 0.0, (t * 0.9).sin() * 0.25, false),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
        Scene {
            name: "12_roll_scrape",
            secs: 9.0,
            course: Course { crest: Some((80.0, 1.2, 6.0)), ..Course::flat(Surface::Gravel) },
            damage: none,
            interior: false,
            // Flat out with full lock over a crest: the car lands crossed up and rolls/slides on
            // its body.
            script: |t| match t {
                t if t < 5.2 => i(1.0, 0.0, 0.0, false),
                t if t < 7.0 => i(1.0, 0.0, 1.0, true),
                _ => i(0.0, 0.0, 0.0, false),
            },
        },
    ]
}

pub fn write_wav(path: &str, audio: &[f32]) {
    let clip = engine::sound::SoundClip::from_interleaved(audio.to_vec(), 2, 48_000);
    std::fs::write(path, clip.to_wav()).unwrap();
}

#[test]
#[ignore]
fn render_scenes() {
    let out = std::env::var("PACENOTE_AUDIO_OUT").ok();
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).unwrap();
    }
    let (mut all_ns, mut all_n, mut worst) = (0.0, 0, 0.0f64);
    for s in scenes() {
        let r = render(&s, 7);
        let peak = r.audio.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (r.audio.iter().map(|v| v * v).sum::<f32>() / r.audio.len() as f32).sqrt();
        let budget = BLOCK as f64 / 48_000.0 * 1e9;
        println!(
            "{:32} peak {peak:.3} rms {rms:.3} block mean {:6.1} µs max {:6.1} µs ({:.2} % of real time) events {:?}",
            s.name,
            r.block_ns.0 / 1e3,
            r.block_ns.1 / 1e3,
            r.block_ns.0 / budget * 100.0,
            r.counts
        );
        all_ns += r.block_ns.0;
        all_n += 1;
        worst = worst.max(r.block_ns.1);
        if let Some(dir) = &out {
            write_wav(&format!("{dir}/{}.wav", s.name), &r.audio);
            std::fs::write(format!("{dir}/{}.csv", s.name), &r.csv).unwrap();
        }
    }
    println!("mean block {:.1} µs, worst {:.1} µs (budget {:.0} µs)", all_ns / all_n as f64 / 1e3, worst / 1e3, BLOCK as f64 / 0.048);
}

/// `PACENOTE_AUDIO_OUT=<dir> cargo test --lib audio::scenes::render_previews -- --ignored
/// --nocapture`: the engine alone on slow rpm sweeps (full load, no load, chase and cockpit),
/// and co-driver lines over a driving engine. Writes WAVs (+ a CSV of rpm per 0.1 s for the
/// sweeps) for listening and for loudness checks.
#[test]
#[ignore]
fn render_previews() {
    use super::car::CarParams;
    use super::voice;
    let dir = std::env::var("PACENOTE_AUDIO_OUT").unwrap_or_else(|_| "/tmp".into());
    std::fs::create_dir_all(&dir).unwrap();
    let params = |rpm: f32, throttle: f32, interior: f32| CarParams { active: true, rpm, idle_rpm: 950.0, redline: 7600.0, throttle, gear: 3, interior, ..CarParams::default() };
    // Sweeps: 1000 → 7500 → 1000 rpm over 16 s (log rpm), parameters every 60 Hz frame.
    for (name, throttle, interior) in [("sweep_on_chase", 1.0, 0.0), ("sweep_off_chase", 0.0, 0.0), ("sweep_on_cockpit", 1.0, 1.0)] {
        let mut s = RallySynth::new(7);
        let (mut audio, mut csv) = (Vec::new(), String::from("t,rpm\n"));
        let mut frac = 0.0f64;
        for i in 0..16 * 60 {
            let t = i as f32 / 60.0;
            let u = 1.0 - (t / 8.0 - 1.0).abs();
            let rpm = 1000.0 * 7.5f32.powf(u);
            s.apply(&Frame { car: params(rpm, throttle, interior), ..Frame::default() });
            frac += 800.0;
            let n = (frac / BLOCK as f64) as usize;
            frac -= (n * BLOCK) as f64;
            s.render_into(n * BLOCK, &mut audio);
            if i % 6 == 0 {
                csv.push_str(&format!("{t:.2},{rpm:.0}\n"));
            }
        }
        write_wav(&format!("{dir}/{name}.wav"), &audio);
        std::fs::write(format!("{dir}/{name}.csv"), csv).unwrap();
        println!("{name}: {:.1} s", audio.len() as f32 / 96_000.0);
    }
    // Co-driver over the engine: the start, a run of notes, damage calls, a split, the finish.
    let bank = voice::bank().unwrap();
    let lines = [
        "Ready. 150 left four long, into right three.",
        "90 over crest, left two, careful.",
        "60 right hairpin, narrows.",
        "Ouch. The suspension took a knock.",
        "200 six left, into five right over crest.",
        "Split 1, 42 point 1 seconds.",
        "Big hit! The engine is badly damaged, nurse it home.",
        "100 to flying finish, flat out.",
        "Stop! Stage time 1 minute 23 point 4.",
    ];
    let mut s = RallySynth::new(9);
    let mut audio = Vec::new();
    let (mut serial, mut next_at, mut idx) = (0u32, 0.8f32, 0usize);
    let mut cur = voice::Utterance::default();
    let mut frac = 0.0f64;
    for i in 0..36 * 60 {
        let t = i as f32 / 60.0;
        let rpm = 4300.0 + 1500.0 * (t * 0.45).sin() + 600.0 * (t * 1.7).sin();
        let throttle = if (t * 0.45).cos() > -0.2 { 1.0 } else { 0.1 };
        if t >= next_at && idx < lines.len() {
            let (mut u, unknown) = bank.plan(lines[idx], 1.0);
            assert!(unknown.is_empty(), "{unknown:?}");
            serial += 1;
            u.serial = serial;
            u.gain = 1.0;
            next_at = t + bank.duration(&u) + 0.9;
            println!("{t:5.2} s: {:?} ({:.2} s)", lines[idx], bank.duration(&u));
            cur = u;
            idx += 1;
        }
        s.apply(&Frame { car: params(rpm, throttle, 0.0), voice: cur, ..Frame::default() });
        frac += 800.0;
        let n = (frac / BLOCK as f64) as usize;
        frac -= (n * BLOCK) as f64;
        s.render_into(n * BLOCK, &mut audio);
    }
    write_wav(&format!("{dir}/codriver_over_engine.wav"), &audio);
    // The voice alone (the same lines, no car) for checking the joins.
    let mut s = RallySynth::new(9);
    let mut audio = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        let (mut u, _) = bank.plan(l, 1.0);
        u.serial = k as u32 + 1;
        s.apply(&Frame { voice: u, ..Frame::default() });
        s.render_into(((bank.duration(&u) + 0.5) * 48_000.0) as usize, &mut audio);
    }
    write_wav(&format!("{dir}/codriver_voice_only.wav"), &audio);
}

/// `PACENOTE_AUDIO_OUT=<dir> cargo test --lib audio::scenes::render_places -- --ignored
/// --nocapture`: the player's car idling at a roadside (chase) while traffic passes both ways,
/// then the same in the cockpit; and the ambience by a river and in a village.
#[test]
#[ignore]
fn render_places() {
    use super::ambience::AmbParams;
    use super::car::CarParams;
    use super::traffic::{OtherCar, TrafficParams};
    let dir = std::env::var("PACENOTE_AUDIO_OUT").unwrap_or_else(|_| "/tmp".into());
    std::fs::create_dir_all(&dir).unwrap();
    let idle = |interior: f32| CarParams { active: true, rpm: 950.0, idle_rpm: 950.0, redline: 7600.0, gear: 1, interior, ..CarParams::default() };
    for (name, interior) in [("traffic_chase", 0.0), ("traffic_cockpit", 1.0)] {
        let mut s = RallySynth::new(5);
        let mut audio = Vec::new();
        // 16 s: one car passes left to right at 22 m/s 3 m away, then one the other way at
        // 14 m/s 6 m away, overlapping.
        for i in 0..16 * 60 {
            let t = i as f32 / 60.0;
            let mut cars = [OtherCar::default(); super::traffic::VOICES];
            for (k, (t0, v, side, id)) in [(0.0f32, 22.0f32, 3.0f32, 1u32), (5.0, -14.0, 6.0, 2)].into_iter().enumerate() {
                let x = (t - t0 - 5.0) * v; // passes at t0 + 5 s
                let d = (x * x + side * side).sqrt();
                if d < super::traffic::RANGE {
                    let closing = -x / d.max(0.1) * v;
                    cars[k] = OtherCar { active: true, id, dist: d, pan: (x / d.max(0.1)).clamp(-1.0, 1.0), speed: v.abs(), closing };
                }
            }
            s.apply(&Frame { car: idle(interior), traffic: TrafficParams { cars, interior }, ..Frame::default() });
            s.render_into(800, &mut audio);
        }
        write_wav(&format!("{dir}/{name}.wav"), &audio);
        println!("{name}: {:.1} s", audio.len() as f32 / 96_000.0);
    }
    for (name, water, town) in [("amb_river", 1.0, 0.0), ("amb_village", 0.0, 1.0), ("amb_forest", 0.0, 0.0)] {
        let mut s = RallySynth::new(9);
        let mut audio = Vec::new();
        for _ in 0..20 * 60 {
            s.apply(&Frame { amb: AmbParams { active: true, water, town, ..AmbParams::default() }, ..Frame::default() });
            s.render_into(800, &mut audio);
        }
        write_wav(&format!("{dir}/{name}.wav"), &audio);
        println!("{name}: {:.1} s", audio.len() as f32 / 96_000.0);
    }
}
