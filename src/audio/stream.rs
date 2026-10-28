//! The rally stream: [`RallySynth`] (car + ambience → bus gains/low-pass → soft clip), fed by a
//! [`Frame`] of parameters the main thread writes once per frame, and played as one rodio
//! source next to the engine mixer's.
//!
//! Main → audio thread: a `Copy` [`Frame`] under a mutex, taken with `try_lock` once per block
//! (256 frames, 5.3 ms), so neither side allocates or waits. Audio → main: render time and
//! frame counters (atomics) for the CPU figure in `RallyAudioStats`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::asset::Asset;
use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::reflect::TypePath;

use super::ambience::{AmbParams, AmbienceSynth};
use super::car::{CarEvent, CarParams, CarSynth};
use super::dsp::{OnePole, RATE, Smooth, finite, soft_clip};

pub const BLOCK: usize = 256;
pub const MAX_EVENTS: usize = 16;

/// Bus gains as the engine mixer computes them (settings, mute/solo, snapshots, ducking).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub car: f32,
    pub amb: f32,
    /// Lowest low-pass on the bus chain (Hz; ≥ 19 kHz = off), e.g. the pause snapshot.
    pub car_lp: f32,
    pub amb_lp: f32,
}

impl Default for Mix {
    fn default() -> Self {
        Mix { car: 1.0, amb: 1.0, car_lp: 20_000.0, amb_lp: 20_000.0 }
    }
}

/// Everything the synth needs for the next stretch of audio.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub car: CarParams,
    pub amb: AmbParams,
    pub mix: Mix,
    pub events: [CarEvent; MAX_EVENTS],
    pub n_events: usize,
}

impl Default for Frame {
    fn default() -> Self {
        Frame { car: CarParams::default(), amb: AmbParams::default(), mix: Mix::default(), events: [CarEvent::Shift; MAX_EVENTS], n_events: 0 }
    }
}

impl Frame {
    /// Queues a one-shot (dropped if the queue is full this frame).
    pub fn push(&mut self, e: CarEvent) {
        if self.n_events < MAX_EVENTS {
            self.events[self.n_events] = e;
            self.n_events += 1;
        }
    }
}

/// Car + ambience + bus processing. Renders whole blocks of interleaved stereo.
#[derive(Clone, Debug)]
pub struct RallySynth {
    pub car: CarSynth,
    pub amb: AmbienceSynth,
    mix: Mix,
    car_gain: Smooth,
    amb_gain: Smooth,
    car_lp: [OnePole; 2],
    amb_lp: [OnePole; 2],
    car_buf: [f32; BLOCK * 2],
    amb_buf: [f32; BLOCK * 2],
    out: [f32; BLOCK * 2],
}

impl RallySynth {
    pub fn new(seed: u32) -> Self {
        RallySynth {
            car: CarSynth::new(seed),
            amb: AmbienceSynth::new(seed),
            mix: Mix::default(),
            car_gain: Smooth::new(1.0, 0.01, 0.01, RATE),
            amb_gain: Smooth::new(1.0, 0.01, 0.01, RATE),
            car_lp: [OnePole::new(20_000.0); 2],
            amb_lp: [OnePole::new(20_000.0); 2],
            car_buf: [0.0; BLOCK * 2],
            amb_buf: [0.0; BLOCK * 2],
            out: [0.0; BLOCK * 2],
        }
    }

    pub fn apply(&mut self, f: &Frame) {
        self.car.set(f.car);
        self.amb.set(f.amb);
        for e in &f.events[..f.n_events.min(MAX_EVENTS)] {
            self.car.event(*e);
        }
        let g = |v: f32| if v.is_finite() { v.clamp(0.0, 4.0) } else { 0.0 };
        self.mix = Mix { car: g(f.mix.car), amb: g(f.mix.amb), car_lp: f.mix.car_lp, amb_lp: f.mix.amb_lp };
        for lp in &mut self.car_lp {
            lp.set(self.mix.car_lp.min(20_000.0));
        }
        for lp in &mut self.amb_lp {
            lp.set(self.mix.amb_lp.min(20_000.0));
        }
    }

    /// Renders one block (BLOCK stereo frames).
    pub fn render_block(&mut self) -> &[f32] {
        self.car_buf.fill(0.0);
        self.amb_buf.fill(0.0);
        self.car.render(&mut self.car_buf);
        self.amb.render(&mut self.amb_buf);
        let car_filtered = self.mix.car_lp < 19_000.0;
        let amb_filtered = self.mix.amb_lp < 19_000.0;
        for i in 0..BLOCK {
            let gc = self.car_gain.next(self.mix.car);
            let ga = self.amb_gain.next(self.mix.amb);
            for ch in 0..2 {
                let mut c = self.car_buf[i * 2 + ch];
                let mut a = self.amb_buf[i * 2 + ch];
                if car_filtered {
                    c = self.car_lp[ch].lp(c);
                }
                if amb_filtered {
                    a = self.amb_lp[ch].lp(a);
                }
                self.out[i * 2 + ch] = finite(soft_clip(c * gc + a * ga));
            }
        }
        &self.out
    }

    /// Renders `frames` (rounded up to whole blocks) into `out`.
    pub fn render_into(&mut self, frames: usize, out: &mut Vec<f32>) {
        for _ in 0..frames.div_ceil(BLOCK) {
            let b = self.render_block();
            out.extend_from_slice(b);
        }
    }
}

/// State shared by the main thread and the audio thread.
#[derive(Debug, Default)]
pub struct Shared {
    pub frame: Mutex<Frame>,
    pub render_ns: AtomicU64,
    pub rendered: AtomicU64,
}

impl Shared {
    /// Main thread: write this frame's parameters, append events.
    pub fn publish(&self, f: &Frame) {
        if let Ok(mut s) = self.frame.lock() {
            let n = s.n_events;
            let events = s.events;
            *s = *f;
            // Events not yet taken by the audio thread are kept (ahead of the new ones).
            s.n_events = 0;
            for e in &events[..n] {
                s.push(*e);
            }
            for e in &f.events[..f.n_events] {
                s.push(*e);
            }
        }
    }
}

/// The rodio source asset (one per run).
#[derive(Asset, TypePath)]
pub struct RallyStream {
    synth: Mutex<Option<Box<RallySynth>>>,
    shared: Arc<Shared>,
    gain: f32,
}

impl RallyStream {
    pub fn new(synth: RallySynth, shared: Arc<Shared>, gain: f32) -> Self {
        RallyStream { synth: Mutex::new(Some(Box::new(synth))), shared, gain }
    }
}

pub struct RallySource {
    synth: Option<Box<RallySynth>>,
    shared: Arc<Shared>,
    gain: f32,
    buf: [f32; BLOCK * 2],
    idx: usize,
}

impl RallySource {
    fn refill(&mut self) {
        let Some(s) = self.synth.as_mut() else {
            self.buf.fill(0.0);
            return;
        };
        let t0 = Instant::now();
        if let Ok(mut f) = self.shared.frame.try_lock() {
            let frame = *f;
            f.n_events = 0;
            drop(f);
            s.apply(&frame);
        }
        let out = s.render_block();
        for (d, v) in self.buf.iter_mut().zip(out) {
            *d = v * self.gain;
        }
        self.shared.render_ns.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.shared.rendered.fetch_add(BLOCK as u64, Ordering::Relaxed);
    }
}

impl Iterator for RallySource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.idx >= BLOCK * 2 {
            self.refill();
            self.idx = 0;
        }
        let s = self.buf[self.idx];
        self.idx += 1;
        Some(s)
    }
}

impl Source for RallySource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(RATE as u32).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl Decodable for RallyStream {
    type Decoder = RallySource;
    fn decoder(&self) -> RallySource {
        let synth = self.synth.lock().ok().and_then(|mut s| s.take());
        RallySource { synth, shared: self.shared.clone(), gain: self.gain, buf: [0.0; BLOCK * 2], idx: BLOCK * 2 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::car::{Contact, GRAVEL};

    #[test]
    fn source_follows_published_frames_and_bus_gain() {
        let shared = Arc::new(Shared::default());
        let stream = RallyStream::new(RallySynth::new(1), shared.clone(), 1.0);
        let mut src = stream.decoder();
        assert!(stream.decoder().synth.is_none(), "decoder hands the synth over once");
        let rms = |src: &mut RallySource, n: usize| (src.by_ref().take(n).map(|v| v * v).sum::<f32>() / n as f32).sqrt();
        assert!(rms(&mut src, 9600) == 0.0, "silent before a car exists");
        let mut surfaces = [Contact::default(); 5];
        surfaces[GRAVEL] = Contact { roll: 1.0, slide: 0.5 };
        let mut f = Frame { car: CarParams { active: true, rpm: 5000.0, throttle: 1.0, speed: 25.0, surfaces, ..CarParams::default() }, ..Frame::default() };
        f.push(CarEvent::Impact(10.0));
        shared.publish(&f);
        rms(&mut src, 48_000);
        assert_eq!(shared.frame.lock().unwrap().n_events, 0, "events consumed");
        let loud = rms(&mut src, 48_000);
        assert!(loud > 0.02, "{loud}");
        f.n_events = 0;
        f.mix.car = 0.25;
        shared.publish(&f);
        rms(&mut src, 4800);
        let quiet = rms(&mut src, 48_000);
        assert!(quiet < loud * 0.4 && quiet > loud * 0.1, "{quiet} vs {loud}");
        f.mix.car = 0.0;
        shared.publish(&f);
        rms(&mut src, 4800);
        assert!(rms(&mut src, 4800) < 1e-4, "muted bus");
        assert!(shared.rendered.load(Ordering::Relaxed) > 50_000);
    }

    #[test]
    fn event_queue_keeps_untaken_events() {
        let shared = Shared::default();
        let mut f = Frame::default();
        f.push(CarEvent::Shift);
        shared.publish(&f);
        shared.publish(&f);
        assert_eq!(shared.frame.lock().unwrap().n_events, 2);
        for _ in 0..40 {
            f.push(CarEvent::Shift);
        }
        shared.publish(&f);
        assert_eq!(shared.frame.lock().unwrap().n_events, MAX_EVENTS);
    }
}
