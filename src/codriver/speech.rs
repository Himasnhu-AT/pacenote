//! Saying lines out loud: by default from the phrase bank (`audio::voice`, clips rendered at
//! build time and joined in the game's own audio stream, the same on every OS). One line at a
//! time, by priority; the queue knows how long each line takes from the clip lengths.
//!
//! Dev fallback (`--os-tts`): the system's text-to-speech instead (`say` on macOS, `espeak-ng` /
//! `espeak` / `spd-say` on Linux, System.Speech through PowerShell on Windows), one child
//! process per line. Off by default; a missing TTS means subtitles only.

use std::collections::VecDeque;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::calls::{Line, Priority};
use crate::audio::voice::{self, Utterance};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Say,
    Espeak(&'static str),
    SpdSay,
    PowerShell,
    None,
}

fn detect() -> Backend {
    let has = |cmd: &str, arg: &str| no_window(Command::new(cmd).arg(arg)).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
    if cfg!(target_os = "macos") {
        return if has("say", "-v?") { Backend::Say } else { Backend::None };
    }
    if cfg!(target_os = "windows") {
        return if has("powershell", "-?") { Backend::PowerShell } else { Backend::None };
    }
    for e in ["espeak-ng", "espeak"] {
        if has(e, "--version") {
            return Backend::Espeak(e);
        }
    }
    if has("spd-say", "--version") { Backend::SpdSay } else { Backend::None }
}

/// What is being said now.
enum Current {
    /// A phrase-bank line, done at `until` (the audio thread plays it from [`Speech::utterance`]).
    Bank { until: Instant, priority: Priority },
    /// The OS voice (dev fallback).
    Os(Child, Priority),
}

impl Current {
    fn priority(&self) -> Priority {
        match self {
            Current::Bank { priority, .. } | Current::Os(_, priority) => *priority,
        }
    }
}

/// The co-driver's voice: a queue of lines and the one being spoken.
pub struct Speech {
    /// Use the system's text-to-speech (`--os-tts`, dev only) instead of the phrase bank.
    pub os_tts: bool,
    backend: Option<Backend>,
    queue: VecDeque<Line>,
    current: Option<Current>,
    /// The line for the audio thread (a new serial starts it; an empty line is silence).
    out: Utterance,
    /// Words per minute (co-drivers read fast): 230 normal; the bank's pace is rate / 230.
    pub rate: u32,
    /// Speak at all (false: subtitles only, e.g. muted runs).
    pub audible: bool,
    /// 0..1 (Settings > Co-driver > Voice volume).
    pub volume: f32,
    /// Words the bank didn't have, logged once each.
    unknown: Vec<String>,
}

impl Default for Speech {
    fn default() -> Self {
        Speech {
            os_tts: false,
            backend: None,
            queue: VecDeque::new(),
            current: None,
            out: Utterance::default(),
            rate: 230,
            audible: true,
            volume: 1.0,
            unknown: Vec::new(),
        }
    }
}

/// Seconds a line takes when there is no bank to time it by (subtitles only).
fn estimate(text: &str, rate: u32) -> f32 {
    let words = text.split_whitespace().count() as f32;
    0.2 + words * 60.0 / rate.max(60) as f32
}

impl Speech {
    /// Queues a line by priority. An urgent line cuts off what's being said.
    pub fn say(&mut self, line: Line) {
        if line.priority == Priority::Urgent {
            self.queue.retain(|l| l.priority == Priority::Urgent);
            if self.current.as_ref().is_some_and(|c| c.priority() < Priority::Urgent) {
                self.stop_current();
            }
        }
        let at = self.queue.iter().position(|l| l.priority < line.priority).unwrap_or(self.queue.len());
        self.queue.insert(at, line);
    }

    fn stop_current(&mut self) {
        match self.current.take() {
            Some(Current::Os(mut c, _)) => {
                let _ = c.kill();
                let _ = c.wait();
            }
            Some(Current::Bank { .. }) => self.silence(),
            None => {}
        }
    }

    /// The audio thread stops talking (a new, empty line).
    fn silence(&mut self) {
        let serial = self.out.serial.wrapping_add(1).max(1);
        self.out = Utterance { serial, gain: self.out.gain, ..Utterance::default() };
    }

    /// Drops everything (pause, restart, leaving the stage).
    pub fn hush(&mut self) {
        self.queue.clear();
        self.stop_current();
    }

    /// A line is being spoken (as of the last `busy`/`pump` check).
    pub fn speaking(&self) -> bool {
        self.current.is_some()
    }

    /// The OS voice (dev fallback) is speaking: the car audio ducks for it.
    pub fn os_speaking(&self) -> bool {
        matches!(self.current, Some(Current::Os(..)))
    }

    /// The line for the audio thread, with the volume applied (times `bus_gain`, the engine
    /// mixer's `voice` bus).
    pub fn utterance(&self, bus_gain: f32) -> Utterance {
        Utterance { gain: self.volume.clamp(0.0, 1.0) * bus_gain, ..self.out }
    }

    pub fn busy(&mut self) -> bool {
        match &mut self.current {
            Some(Current::Os(c, _)) => {
                if let Ok(None) = c.try_wait() {
                    return true;
                }
            }
            Some(Current::Bank { until, .. }) => {
                if Instant::now() < *until {
                    return true;
                }
            }
            None => return false,
        }
        self.current = None;
        false
    }

    /// Starts the next line when the last one is done. `s` drops lines gone stale. Returns the
    /// line started (for the subtitle).
    pub fn pump(&mut self, s: f32) -> Option<Line> {
        if self.busy() {
            return None;
        }
        let line = loop {
            let l = self.queue.pop_front()?;
            if l.stale_after.is_none_or(|at| s < at) {
                break l;
            }
        };
        if self.os_tts {
            if self.audible && self.volume > 0.01 {
                let backend = *self.backend.get_or_insert_with(detect);
                if let Some(child) = spawn(backend, &line.text, self.rate, self.volume) {
                    self.current = Some(Current::Os(child, line.priority));
                }
            }
            return Some(line);
        }
        let pace = self.rate as f32 / 230.0;
        let secs = match voice::bank().filter(|b| !b.is_empty()) {
            Some(bank) => {
                let (u, unknown) = bank.plan(&line.text, pace);
                for w in unknown {
                    if !self.unknown.contains(&w) {
                        bevy::log::debug!("co-driver: no clip for {w:?} (in {:?}); skipped", line.text);
                        self.unknown.push(w);
                    }
                }
                let secs = bank.duration(&u);
                if self.audible {
                    self.out = Utterance { serial: self.out.serial.wrapping_add(1).max(1), gain: self.out.gain, ..u };
                }
                secs
            }
            // Not loaded (no audio output, or still loading): keep the timing for subtitles.
            None => estimate(&line.text, self.rate),
        };
        self.current = Some(Current::Bank { until: Instant::now() + Duration::from_secs_f32(secs + 0.06), priority: line.priority });
        Some(line)
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        self.hush();
    }
}

/// No console window for a child of a windowed (no-console) game on Windows; a no-op elsewhere.
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn spawn(b: Backend, text: &str, rate: u32, volume: f32) -> Option<Child> {
    let vol = volume.clamp(0.0, 1.0);
    let mut cmd = match b {
        Backend::Say => {
            // `say` has no volume flag; its embedded `[[volm x]]` command sets it.
            let mut c = Command::new("say");
            c.arg("-r").arg(rate.to_string()).arg("--").arg(format!("[[volm {vol:.2}]] {text}"));
            c
        }
        Backend::Espeak(e) => {
            let mut c = Command::new(e);
            c.arg("-s").arg(rate.to_string()).arg("-a").arg(((vol * 100.0).round() as u32).to_string()).arg("--").arg(text);
            c
        }
        Backend::SpdSay => {
            let mut c = Command::new("spd-say");
            let r = ((rate as i32 - 150) / 2).clamp(-100, 100);
            c.arg("-w").arg("-r").arg(r.to_string()).arg("-i").arg(((vol * 200.0) as i32 - 100).to_string()).arg("--").arg(text);
            c
        }
        Backend::PowerShell => {
            let safe = text.replace('\'', "''");
            let script = format!(
                "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Rate = {}; $s.Volume = {}; $s.Speak('{safe}')",
                ((rate as i32 - 170) / 20).clamp(-10, 10),
                (vol * 100.0).round() as u32
            );
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            c
        }
        Backend::None => return None,
    };
    no_window(&mut cmd).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lets the current line finish (bank lines are timed by the clock).
    fn finish(sp: &mut Speech) {
        if let Some(Current::Bank { until, .. }) = &mut sp.current {
            *until = Instant::now();
        }
    }

    #[test]
    fn queue_orders_by_priority_and_drops_stale() {
        let mut sp = Speech::default();
        sp.audible = false;
        sp.say(Line { text: "a".into(), priority: Priority::Note, stale_after: Some(100.0) });
        sp.say(Line::new("b", Priority::Chat));
        sp.say(Line::new("c", Priority::Warn));
        assert_eq!(sp.pump(0.0).unwrap().text, "c");
        assert!(sp.pump(0.0).is_none(), "still saying c");
        finish(&mut sp);
        // "a" is stale past 100 m.
        assert_eq!(sp.pump(150.0).unwrap().text, "b");
        finish(&mut sp);
        assert!(sp.pump(150.0).is_none());
        sp.say(Line::new("x", Priority::Note));
        sp.say(Line::new("boom", Priority::Urgent));
        assert_eq!(sp.pump(0.0).unwrap().text, "boom");
        finish(&mut sp);
        assert!(sp.pump(0.0).is_none(), "urgent clears the queue");
    }

    #[test]
    fn bank_lines_go_to_the_audio_thread_and_urgent_cuts_off() {
        let mut sp = Speech::default();
        sp.say(Line::new("90 left four, into right two, careful", Priority::Note));
        sp.pump(0.0).unwrap();
        let u = sp.utterance(1.0);
        assert!(u.serial > 0 && u.n >= 7, "{u:?}");
        assert!(sp.speaking());
        // An urgent line stops it at once (an empty line), then plays next.
        sp.say(Line::new("Big hit! The engine is damaged.", Priority::Urgent));
        let cut = sp.utterance(1.0);
        assert!(cut.serial != u.serial && cut.n == 0);
        assert_eq!(sp.pump(0.0).unwrap().priority, Priority::Urgent);
        assert!(sp.utterance(1.0).n >= 4);
        // Volume and the voice bus scale it.
        sp.volume = 0.5;
        assert!((sp.utterance(0.5).gain - 0.25).abs() < 1e-6);
        sp.hush();
        assert_eq!(sp.utterance(1.0).n, 0);
        assert!(!sp.speaking());
    }
}
