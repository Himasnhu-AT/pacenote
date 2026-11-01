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
use crate::vehicle::{self, Car, Ground, Input, Obstacle, Spec};

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
    let mut synth = Box::new(RallySynth::new(seed));
    let mut tr = Tracker::default();
    let mut frame = Frame::default();
    let frames = (scene.secs * 60.0) as usize;
    let mut audio: Vec<f32> = Vec::with_capacity((scene.secs * 48_000.0) as usize * 2 + BLOCK * 4);
    let mut csv = String::from("t,rpm,gear,throttle,speed,slide_sum,impacts,x,z,up_y,body\n");
    let dt = 1.0 / 60.0;
    let mut frac = 0.0f64;
    let (mut ns_sum, mut ns_max, mut blocks_n) = (0f64, 0f64, 0usize);
    let mut counts = [0u32; 3];
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
            "{t:.3},{:.0},{},{:.2},{:.2},{slide:.3},{:.1},{:.1},{:.1},{:.2},{}\n",
            car.rpm,
            car.gear,
            input.throttle,
            car.vel.length(),
            car.impacts,
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
    vec![
        Scene { name: "01_idle", secs: 5.0, course: Course::flat(Surface::Gravel), interior: false, script: |_| i(0.0, 0.0, 0.0, false) },
        Scene {
            name: "02_full_throttle_gears_tarmac",
            secs: 16.0,
            course: Course::flat(Surface::Tarmac),
           
            interior: false,
            script: |t| if t < 0.7 { i(0.0, 0.0, 0.0, false) } else if t < 12.5 { i(1.0, 0.0, 0.0, false) } else { i(0.0, 0.0, 0.0, false) },
        },
        Scene {
            name: "03_lift_off_pops_gravel",
            secs: 12.0,
            course: Course::flat(Surface::Gravel),
           
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
           
            interior: false,
            script: |t| if t < 6.0 { i(1.0, 0.0, 0.0, false) } else { i(0.0, 0.0, 0.0, false) },
        },
        Scene {
            name: "07_rough_gravel",
            secs: 10.0,
            course: Course { bumps: (0.05, 2.2), ..Course::flat(Surface::Gravel) },
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
           
            interior: false,
            script: |t| if t < 8.0 { i(1.0, 0.0, 0.0, false) } else { i(0.0, 0.0, 0.0, false) },
        },
        Scene {
            name: "09_cockpit_gravel_run",
            secs: 12.0,
            course: Course { bumps: (0.02, 3.0), ..Course::flat(Surface::Gravel) },
           
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
