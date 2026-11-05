//! The rally stream: [`RallySynth`] (car + ambience → bus gains/low-pass → compressor → exhaust
//! pops → limiter → soft clip), fed by a
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
use super::dsp::{Biquad, Dynamics, OnePole, RATE, Smooth, finite, soft_clip};
use super::traffic::{TrafficParams, TrafficSynth};
use super::voice::{Utterance, VoicePlayer};

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
    /// Someone is talking outside this stream (the co-driver's OS voice, a dev fallback): the
    /// car and ambience duck as for the co-driver's own voice.
    pub talk: bool,
}

impl Default for Mix {
    fn default() -> Self {
        Mix { car: 1.0, amb: 1.0, car_lp: 20_000.0, amb_lp: 20_000.0, talk: false }
    }
}

/// Car gain while the co-driver talks (−5 dB: engine, tyres, wind, impacts).
pub const TALK_DUCK: f32 = 0.562;
/// Ambience gain while the co-driver talks (−3 dB).
pub const TALK_DUCK_AMB: f32 = 0.708;

/// Everything the synth needs for the next stretch of audio.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The co-driver's current line (a new serial starts it).
    pub voice: Utterance,
    pub car: CarParams,
    pub amb: AmbParams,
    /// The other cars heard (open world traffic).
    pub traffic: TrafficParams,
    pub mix: Mix,
    pub events: [CarEvent; MAX_EVENTS],
    pub n_events: usize,
}

impl Default for Frame {
    fn default() -> Self {
        Frame { voice: Utterance::default(), car: CarParams::default(), amb: AmbParams::default(), traffic: TrafficParams::default(), mix: Mix::default(), events: [CarEvent::Shift(true); MAX_EVENTS], n_events: 0 }
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

/// Car + ambience + bus processing (gains, low-pass, DC blocker, compressor and look-ahead
/// limiter, a soft clip as the last guard). Renders whole blocks of interleaved stereo.
#[derive(Clone, Debug)]
pub struct RallySynth {
    pub car: CarSynth,
    pub amb: AmbienceSynth,
    pub traffic: TrafficSynth,
    mix: Mix,
    car_gain: Smooth,
    amb_gain: Smooth,
    car_lp: [OnePole; 2],
    amb_lp: [OnePole; 2],
    /// The pops: the car bus gain (never ducked) and low-pass.
    pop_gain: Smooth,
    pop_lp: [OnePole; 2],
    dc: [Biquad; 2],
    pub dynamics: Dynamics,
    /// The co-driver (mono, centre), mixed after the car's dynamics.
    pub voice: VoicePlayer,
    /// The OS voice (dev fallback) talking: 0..1, smoothed like the voice's own duck.
    talk: Smooth,
    car_buf: [f32; BLOCK * 2],
    amb_buf: [f32; BLOCK * 2],
    pop_buf: [f32; BLOCK * 2],
    out: [f32; BLOCK * 2],
}

impl RallySynth {
    pub fn new(seed: u32) -> Self {
        RallySynth {
            car: CarSynth::new(seed),
            amb: AmbienceSynth::new(seed),
            traffic: TrafficSynth::new(seed),
            mix: Mix::default(),
            car_gain: Smooth::new(1.0, 0.01, 0.01, RATE),
            amb_gain: Smooth::new(1.0, 0.01, 0.01, RATE),
            car_lp: [OnePole::new(20_000.0); 2],
            amb_lp: [OnePole::new(20_000.0); 2],
            pop_gain: Smooth::new(1.0, 0.01, 0.01, RATE),
            pop_lp: [OnePole::new(20_000.0); 2],
            dc: [Biquad::highpass(22.0, 0.7); 2],
            dynamics: Dynamics::default(),
            voice: VoicePlayer::default(),
            talk: Smooth::new(0.0, 0.03, 0.25, RATE),
            car_buf: [0.0; BLOCK * 2],
            amb_buf: [0.0; BLOCK * 2],
            pop_buf: [0.0; BLOCK * 2],
            out: [0.0; BLOCK * 2],
        }
    }

    pub fn apply(&mut self, f: &Frame) {
        self.car.set(f.car);
        self.amb.set(f.amb);
        self.traffic.set(f.traffic);
        for e in &f.events[..f.n_events.min(MAX_EVENTS)] {
            self.car.event(*e);
        }
        let g = |v: f32| if v.is_finite() { v.clamp(0.0, 4.0) } else { 0.0 };
        self.mix = Mix { car: g(f.mix.car), amb: g(f.mix.amb), car_lp: f.mix.car_lp, amb_lp: f.mix.amb_lp, talk: f.mix.talk };
        self.voice.set(&f.voice);
        for lp in self.car_lp.iter_mut().chain(&mut self.pop_lp) {
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
        self.pop_buf.fill(0.0);
        self.car.render_split(&mut self.car_buf, &mut self.pop_buf);
        self.traffic.render(&mut self.car_buf);
        self.amb.render(&mut self.amb_buf);
        let car_filtered = self.mix.car_lp < 19_000.0;
        let amb_filtered = self.mix.amb_lp < 19_000.0;
        let bank = super::voice::bank();
        for i in 0..BLOCK {
            let v = self.voice.next(bank);
            // Duck under the co-driver: car −5 dB, ambience −3 dB (30 ms down, 250 ms back).
            let duck = self.voice.duck().max(self.talk.next(if self.mix.talk { 1.0 } else { 0.0 }));
            let gc = self.car_gain.next(self.mix.car * (1.0 - (1.0 - TALK_DUCK) * duck));
            let ga = self.amb_gain.next(self.mix.amb * (1.0 - (1.0 - TALK_DUCK_AMB) * duck));
            // Pops are never ducked (AUDIO_PASS §5) and skip the compressor (§2.4).
            let gp = self.pop_gain.next(self.mix.car);
            let mut pops = [0.0; 2];
            for (ch, out) in pops.iter_mut().enumerate() {
                let mut p = self.pop_buf[i * 2 + ch];
                if car_filtered {
                    p = self.pop_lp[ch].lp(p);
                }
                *out = finite(p * gp);
            }
            let mut lr = [0.0; 2];
            for (ch, out) in lr.iter_mut().enumerate() {
                // DC blocker on the car before its gain, so a mute is silent at once.
                let mut c = self.dc[ch].run(self.car_buf[i * 2 + ch]);
                let mut a = self.amb_buf[i * 2 + ch];
                if car_filtered {
                    c = self.car_lp[ch].lp(c);
                }
                if amb_filtered {
                    a = self.amb_lp[ch].lp(a);
                }
                *out = finite(c * gc + a * ga);
            }
            let (l, r) = self.dynamics.run_with(lr[0], lr[1], pops[0], pops[1]);
            self.out[i * 2] = soft_clip(l + v);
            self.out[i * 2 + 1] = soft_clip(r + v);
        }
        for f in &mut self.dc {
            f.settle();
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
    /// Exhaust pops the synth has started (`pops::PopStats::played`).
    pub pops_played: AtomicU64,
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
        self.shared.pops_played.store(s.car.pops.played as u64, Ordering::Relaxed);
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
        rms(&mut src, 9600); // the 10 ms gain ramp settles
        assert!(rms(&mut src, 4800) < 1e-4, "muted bus");
        assert!(shared.rendered.load(Ordering::Relaxed) > 50_000);
    }

    #[test]
    fn co_driver_talk_ducks_the_car_only() {
        let render = |talk: bool| {
            let mut s = RallySynth::new(2);
            let mut f = Frame { car: CarParams { active: true, rpm: 4000.0, throttle: 0.5, ..CarParams::default() }, ..Frame::default() };
            f.mix.talk = talk;
            s.apply(&f);
            let mut out = Vec::new();
            s.render_into(48_000, &mut out);
            (out[48_000..].iter().map(|v| v * v).sum::<f32>() / 48_000.0).sqrt()
        };
        let ratio = render(true) / render(false);
        assert!((ratio - TALK_DUCK).abs() < 0.08, "duck ratio {ratio}");
    }

    #[test]
    fn co_driver_voice_plays_and_ducks_the_car_5_db() {
        let bank = crate::audio::voice::bank().unwrap();
        let render = |say: bool, gain: f32| {
            let mut s = RallySynth::new(2);
            let mut f = Frame { car: CarParams { active: true, rpm: 4000.0, throttle: 0.5, ..CarParams::default() }, ..Frame::default() };
            if say {
                let (mut u, unknown) = bank.plan("Over crest, left four long, into right two.", 1.0);
                assert!(unknown.is_empty(), "{unknown:?}");
                u.serial = 1;
                u.gain = gain;
                f.voice = u;
            }
            s.apply(&f);
            let mut out = Vec::new();
            s.render_into(48_000, &mut out);
            // 0.25..0.75 s: words, after the duck's attack.
            (out[24_000..72_000].iter().map(|v| v * v).sum::<f32>() / 48_000.0).sqrt()
        };
        let quiet = render(false, 0.0);
        let ducked = render(true, 0.0) / quiet;
        assert!((ducked - TALK_DUCK).abs() < 0.06, "car under the voice ×{ducked}");
        let with_voice = render(true, 1.0);
        assert!(with_voice > quiet * 1.3, "voice heard: {with_voice} vs car {quiet}");
    }

    /// Pops skip the compressor (AUDIO_PASS §2.4, §8.3): a bang over a loud engine moves the
    /// compressor's gain by ≤ 1 dB, the output never clips, and the pop stands out.
    #[test]
    fn pops_skip_the_compressor_and_never_clip() {
        use crate::audio::pops::PopKind;
        let run = |pop: bool| {
            let mut s = RallySynth::new(4);
            let mut f = Frame { car: CarParams { active: true, rpm: 6500.0, throttle: 1.0, speed: 30.0, ..CarParams::default() }, ..Frame::default() };
            s.apply(&f);
            let mut out = Vec::new();
            s.render_into(48_000, &mut out);
            let before = s.dynamics.reduction_db;
            f.car.throttle = 0.0;
            if pop {
                f.push(CarEvent::Pop { intensity: 1.0, kind: PopKind::Bang, seed: 9, pan: 0.3 });
            }
            s.apply(&f);
            let (mut worst, mut peak) = (0.0f32, 0.0f32);
            for _ in 0..40 {
                let b = s.render_block();
                peak = b.iter().fold(peak, |m, v| m.max(v.abs()));
                worst = worst.max(s.dynamics.reduction_db);
            }
            (before, worst, peak, s.car.pops.played)
        };
        let (b0, w0, p0, n0) = run(false);
        let (b1, w1, p1, n1) = run(true);
        assert_eq!((n0, n1), (0, 1));
        assert!(p1 <= 0.9 && p1 > p0 * 1.5, "peak {p0} → {p1}");
        // The reduction (compressor + limiter) the pop adds over the same lift without it.
        assert!(w1 - w0 <= 1.0 + (b1 - b0).abs(), "gain reduction {w0:.2} → {w1:.2} dB");
    }

    #[test]
    fn event_queue_keeps_untaken_events() {
        let shared = Shared::default();
        let mut f = Frame::default();
        f.push(CarEvent::Shift(true));
        shared.publish(&f);
        shared.publish(&f);
        assert_eq!(shared.frame.lock().unwrap().n_events, 2);
        for _ in 0..40 {
            f.push(CarEvent::Shift(true));
        }
        shared.publish(&f);
        assert_eq!(shared.frame.lock().unwrap().n_events, MAX_EVENTS);
    }
}
