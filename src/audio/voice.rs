//! The co-driver's voice: a phrase bank (`assets/sounds/voice/*.ogg`, listed in `bank.txt`),
//! rendered once at build time by `tools/audio/cook_voice.py` (Piper, a public-domain voice). A
//! line is played by joining its clips: each word's faded tail overlaps the next word's head
//! (a short crossfade), commas and full stops are short pauses. No text-to-speech runs in the
//! game, so it sounds the same on every OS.
//!
//! Main thread: [`VoiceBank::plan`] turns a line's text into an [`Utterance`] (clip ids and
//! pauses; unknown words are skipped). It travels to the audio thread in the rally stream's
//! `Frame`; a new `serial` starts it (cutting off what was playing with a 12 ms fade).
//! Audio thread: [`VoicePlayer`] plays it and reports how much it is talking (an envelope the
//! stream ducks the car and the ambience with).

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use super::dsp::{RATE, Smooth};

/// Most clips and pauses in one line (a long damage line is ~16).
pub const MAX_ITEMS: usize = 48;

/// One step of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// A clip of the bank.
    Clip(u16),
    /// Silence, in milliseconds.
    Pause(u16),
}

/// A line to say, as the audio thread gets it. `serial` 0 or a line with no items is silence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Utterance {
    /// A new value starts this line (and cuts off the last one).
    pub serial: u32,
    pub items: [Item; MAX_ITEMS],
    pub n: u8,
    /// Samples (at 48 kHz) each word's tail overlaps the next word.
    pub overlap: u16,
    /// Linear gain (the co-driver volume times the engine mixer's `voice` bus).
    pub gain: f32,
}

impl Default for Utterance {
    fn default() -> Self {
        Utterance { serial: 0, items: [Item::Pause(0); MAX_ITEMS], n: 0, overlap: 0, gain: 1.0 }
    }
}

impl Utterance {
    pub fn items(&self) -> &[Item] {
        &self.items[..(self.n as usize).min(MAX_ITEMS)]
    }
}

/// A word, or punctuation that makes a pause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Comma,
    Stop,
}

/// Lower case words (letters, digits, apostrophes); `,;:` → [`Token::Comma`], `.!?` →
/// [`Token::Stop`]; anything else (hyphens, quotes) separates words.
pub fn normalise(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |w: &mut String, out: &mut Vec<Token>| {
        if !w.is_empty() {
            out.push(Token::Word(std::mem::take(w)));
        }
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '\'' || c == '’' {
            word.extend(if c == '’' { '\'' } else { c }.to_lowercase());
            continue;
        }
        // A decimal point inside a number ("23.4") is still a pause; times are spelled out.
        flush(&mut word, &mut out);
        match c {
            ',' | ';' | ':' => out.push(Token::Comma),
            '.' | '!' | '?' => out.push(Token::Stop),
            _ => {}
        }
    }
    flush(&mut word, &mut out);
    out
}

/// The decoded phrase bank.
pub struct VoiceBank {
    clips: Vec<Box<[f32]>>,
    /// Sample rate of the clips (the cook writes 24 kHz).
    rate: u32,
    keys: HashMap<String, u16>,
    /// Most words in any key.
    max_words: usize,
}

impl std::fmt::Debug for VoiceBank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VoiceBank({} clips, {} keys, {:.1} s at {} Hz)", self.clips.len(), self.keys.len(), self.seconds(), self.rate)
    }
}

/// Milliseconds of pause for a comma and a full stop, at the normal pace.
const COMMA_MS: f32 = 110.0;
const STOP_MS: f32 = 230.0;
/// Seconds each word's tail overlaps the next word at the normal pace (the cook leaves ~40 ms
/// of faded tail on each clip).
const OVERLAP_S: f32 = 0.035;

impl VoiceBank {
    /// Reads `assets/sounds/voice/bank.txt` and decodes its clips (missing ones are empty).
    pub fn load(assets: &Path) -> VoiceBank {
        let dir = assets.join("sounds/voice");
        let list = std::fs::read_to_string(dir.join("bank.txt")).unwrap_or_else(|e| {
            bevy::log::warn!("co-driver voice: can't read {}: {e}", dir.join("bank.txt").display());
            String::new()
        });
        let mut rate = 24_000;
        let entries = parse(&list);
        let clips = entries
            .iter()
            .map(|(stem, _)| {
                let (clip, r) = super::samples::load_mono(&dir.join(format!("{stem}.ogg")));
                if !clip.is_empty() {
                    rate = r;
                }
                clip
            })
            .collect();
        VoiceBank::new(clips, rate, entries.into_iter().map(|(_, k)| k).collect())
    }

    /// A bank from decoded clips (`rate` Hz) and each clip's keys.
    pub fn new(clips: Vec<Box<[f32]>>, rate: u32, keys: Vec<Vec<String>>) -> VoiceBank {
        let mut map = HashMap::new();
        let mut max_words = 1;
        for (i, ks) in keys.into_iter().enumerate() {
            for k in ks {
                max_words = max_words.max(k.split(' ').count());
                map.insert(k, i as u16);
            }
        }
        VoiceBank { clips, rate: rate.max(1), keys: map, max_words }
    }

    pub fn clip(&self, i: u16) -> &[f32] {
        self.clips.get(i as usize).map_or(&[], |c| &c[..])
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Seconds of audio held (at the bank's rate; ×4 bytes each sample).
    pub fn seconds(&self) -> f32 {
        self.clips.iter().map(|c| c.len()).sum::<usize>() as f32 / self.rate as f32
    }

    /// The clip for `key` (words joined by single spaces).
    pub fn find(&self, key: &str) -> Option<u16> {
        self.keys.get(key).copied()
    }

    /// The steps for `text` at `pace` (1 normal; 1.15 = 15 % quicker: shorter pauses, more
    /// overlap), and the words the bank doesn't have (skipped). Longest phrase first ("over
    /// crest" before "over"); a number the bank lacks is read digit by digit.
    pub fn plan(&self, text: &str, pace: f32) -> (Utterance, Vec<String>) {
        let pace = pace.clamp(0.6, 1.6);
        let toks = normalise(text);
        let mut items: Vec<Item> = Vec::new();
        let mut unknown = Vec::new();
        let pause = |items: &mut Vec<Item>, ms: f32| {
            let ms = (ms / pace) as u16;
            match items.last_mut() {
                // Collapse runs of punctuation into the longest pause; none at the start.
                Some(Item::Pause(p)) => *p = (*p).max(ms),
                Some(_) => items.push(Item::Pause(ms)),
                None => {}
            }
        };
        let mut i = 0;
        while i < toks.len() {
            match &toks[i] {
                Token::Comma => pause(&mut items, COMMA_MS),
                Token::Stop => pause(&mut items, STOP_MS),
                Token::Word(w) => {
                    // Longest run of words (no punctuation inside) that is a key.
                    let mut found = None;
                    let mut words = Vec::new();
                    for t in toks[i..].iter().take(self.max_words) {
                        let Token::Word(w) = t else { break };
                        words.push(w.as_str());
                        if let Some(c) = self.find(&words.join(" ")) {
                            found = Some((c, words.len()));
                        }
                    }
                    if let Some((c, n)) = found {
                        items.push(Item::Clip(c));
                        i += n;
                        continue;
                    }
                    if w.chars().all(|c| c.is_ascii_digit()) && w.len() > 1 {
                        let digits: Option<Vec<u16>> = w.chars().map(|d| self.find(&d.to_string())).collect();
                        if let Some(ds) = digits {
                            items.extend(ds.into_iter().map(Item::Clip));
                            i += 1;
                            continue;
                        }
                    }
                    unknown.push(w.clone());
                }
            }
            i += 1;
        }
        while matches!(items.last(), Some(Item::Pause(_))) {
            items.pop();
        }
        items.truncate(MAX_ITEMS);
        let mut u = Utterance { n: items.len() as u8, overlap: (OVERLAP_S * pace * RATE) as u16, ..Utterance::default() };
        u.items[..items.len()].copy_from_slice(&items);
        (u, unknown)
    }

    /// How long `u` takes to say (seconds), as [`VoicePlayer`] plays it.
    pub fn duration(&self, u: &Utterance) -> f32 {
        let mut s = 0.0;
        let overlap = u.overlap as f32 / RATE;
        let items = u.items();
        for (k, it) in items.iter().enumerate() {
            match *it {
                Item::Clip(c) => {
                    let len = self.clip(c).len() as f32 / self.rate as f32;
                    let next_is_clip = matches!(items.get(k + 1), Some(Item::Clip(_)));
                    s += if next_is_clip { (len - overlap).max(len * 0.5) } else { len };
                }
                Item::Pause(ms) => s += ms as f32 / 1000.0,
            }
        }
        s
    }
}

/// `bank.txt`: `<stem>\t<key>|<key>...` per line, `#` comments.
fn parse(text: &str) -> Vec<(String, Vec<String>)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (stem, keys) = l.split_once('\t')?;
            Some((stem.trim().to_string(), keys.split('|').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()))
        })
        .collect()
}

static VOICE: OnceLock<VoiceBank> = OnceLock::new();

/// Decodes the bank (blocking; called from the car audio loader thread).
pub fn load(assets: &Path) {
    if VOICE.get().is_some() {
        return;
    }
    let t0 = std::time::Instant::now();
    let bank = VoiceBank::load(assets);
    bevy::log::info!(
        "co-driver voice: {bank:?} decoded in {:.0} ms ({:.1} MB)",
        t0.elapsed().as_secs_f32() * 1000.0,
        bank.seconds() * bank.rate as f32 * 4.0 / 1e6
    );
    let _ = VOICE.set(bank);
}

/// The bank once loaded. In unit tests, loads this crate's `assets/` on first use.
pub fn bank() -> Option<&'static VoiceBank> {
    #[cfg(test)]
    {
        Some(VOICE.get_or_init(|| VoiceBank::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")))))
    }
    #[cfg(not(test))]
    {
        VOICE.get()
    }
}

#[derive(Clone, Copy, Debug)]
struct Playing {
    clip: u16,
    pos: f64,
    /// Gain, and how much it drops per sample (a cut-off fades out fast).
    fade: f32,
    fade_step: f32,
}

/// Plays utterances on the audio thread (mono). Allocation-free.
#[derive(Clone, Debug)]
pub struct VoicePlayer {
    line: Utterance,
    next: usize,
    cur: Option<Playing>,
    /// The word before, finishing its tail under the current one (or being cut off).
    prev: Option<Playing>,
    silence: u32,
    gain: Smooth,
    /// 1 while talking (fast up, slow down): the duck.
    env: Smooth,
}

impl Default for VoicePlayer {
    fn default() -> Self {
        VoicePlayer {
            line: Utterance::default(),
            next: 0,
            cur: None,
            prev: None,
            silence: 0,
            gain: Smooth::new(1.0, 0.02, 0.02, RATE),
            env: Smooth::new(0.0, 0.03, 0.25, RATE),
        }
    }
}

impl VoicePlayer {
    /// The latest line from the main thread: a new serial starts it, cutting off the last.
    pub fn set(&mut self, u: &Utterance) {
        self.line.gain = if u.gain.is_finite() { u.gain.clamp(0.0, 4.0) } else { 0.0 };
        if u.serial == self.line.serial {
            return;
        }
        // Cut off what was playing (12 ms fade).
        if let Some(mut c) = self.cur.take() {
            c.fade_step = c.fade / (0.012 * RATE);
            self.prev = Some(c);
        }
        let gain = self.line.gain;
        self.line = *u;
        self.line.gain = gain;
        self.next = 0;
        self.silence = 0;
    }

    /// Something is being said (or is about to be).
    pub fn talking(&self) -> bool {
        self.cur.is_some() || self.prev.is_some() || self.silence > 0 || self.next < self.line.items().len()
    }

    /// The duck envelope (0 quiet .. 1 talking), as of the last sample.
    pub fn duck(&self) -> f32 {
        self.env.v
    }

    /// Next sample (mono) from `bank` (`None`: not loaded yet, silence; the line is dropped).
    #[inline]
    pub fn next(&mut self, bank: Option<&VoiceBank>) -> f32 {
        let Some(bank) = bank else {
            self.next = self.line.items().len();
            self.cur = None;
            self.prev = None;
            self.env.next(0.0);
            return 0.0;
        };
        let step = bank.rate as f64 / RATE as f64;
        // The next step: a word starts when nothing plays or the last word is into its
        // overlap (its faded tail plays on under the new word); a pause once the word ended.
        if self.silence > 0 {
            self.silence -= 1;
        } else if let Some(&item) = self.line.items().get(self.next) {
            match (item, &self.cur) {
                (Item::Clip(id), cur) => {
                    let start = cur.is_none_or(|c| (bank.clip(c.clip).len() as f64 - c.pos) / step <= self.line.overlap as f64);
                    if start {
                        self.next += 1;
                        if let Some(c) = self.cur.take() {
                            self.prev = Some(c);
                        }
                        self.cur = Some(Playing { clip: id, pos: 0.0, fade: 1.0, fade_step: 0.0 });
                    }
                }
                (Item::Pause(ms), None) => {
                    self.next += 1;
                    self.silence = (ms as f32 * RATE / 1000.0) as u32;
                }
                (Item::Pause(_), Some(_)) => {}
            }
        }
        let v = play(&mut self.cur, bank, step) + play(&mut self.prev, bank, step);
        let talking = self.talking();
        self.env.next(if talking { 1.0 } else { 0.0 });
        let g = self.gain.next(self.line.gain);
        super::dsp::finite(v * g)
    }
}

/// The next sample of a playing clip (it empties the slot when done).
#[inline]
fn play(slot: &mut Option<Playing>, bank: &VoiceBank, step: f64) -> f32 {
    let Some(p) = slot else { return 0.0 };
    let x = bank.clip(p.clip);
    if p.pos >= x.len() as f64 || p.fade <= 0.0 {
        *slot = None;
        return 0.0;
    }
    let v = super::samples::read_clamped(x, p.pos) * p.fade;
    p.pos += step;
    p.fade -= p.fade_step;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny()-> VoiceBank {
        // Three "words" at 24 kHz: 0.2 s tones, and a "90".
        let tone = |hz: f32| -> Box<[f32]> { (0..4800).map(|i| 0.3 * (i as f32 * hz / 24_000.0 * std::f32::consts::TAU).sin()).collect() };
        VoiceBank::new(
            vec![tone(300.0), tone(400.0), tone(500.0), tone(600.0)],
            24_000,
            vec![vec!["left".into()], vec!["over crest".into()], vec!["90".into(), "ninety".into()], vec!["9".into()]],
        )
    }

    #[test]
    fn plans_longest_phrases_pauses_and_skips_unknown_words() {
        let b = tiny();
        let (u, unknown) = b.plan("90 left, over crest. Zorp left!", 1.0);
        assert_eq!(unknown, vec!["zorp".to_string()]);
        let it = u.items();
        assert_eq!(it[0], Item::Clip(2));
        assert_eq!(it[1], Item::Clip(0));
        assert!(matches!(it[2], Item::Pause(110)));
        assert_eq!(it[3], Item::Clip(1));
        assert!(matches!(it[4], Item::Pause(230)));
        assert_eq!(it[5], Item::Clip(0));
        assert_eq!(it.len(), 6, "no trailing pause: {it:?}");
        // An unknown number is read digit by digit.
        let (u, unknown) = b.plan("99", 1.0);
        assert!(unknown.is_empty());
        assert_eq!(u.items(), &[Item::Clip(3), Item::Clip(3)]);
        // Nothing known: nothing to say.
        assert_eq!(b.plan("hello there", 1.0).0.n, 0);
        assert_eq!(normalise("Co-driver on."), vec![Token::Word("co".into()), Token::Word("driver".into()), Token::Word("on".into()), Token::Stop]);
    }

    #[test]
    fn player_joins_clips_and_takes_as_long_as_planned() {
        let b = tiny();
        let (mut u, _) = b.plan("left, over crest 90", 1.0);
        u.serial = 1;
        let mut p = VoicePlayer::default();
        p.set(&u);
        let mut out = Vec::new();
        while p.talking() && out.len() < 48_000 * 5 {
            out.push(p.next(Some(&b)));
        }
        let secs = out.len() as f32 / RATE;
        let want = b.duration(&u);
        assert!((secs - want).abs() < 0.02, "played {secs} s, planned {want} s");
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 0.61));
        // The comma is heard as silence (~110 ms) after the first word (0.2 s).
        let gap = &out[(0.215 * RATE) as usize..(0.29 * RATE) as usize];
        assert!(gap.iter().all(|v| v.abs() < 1e-6));
        // Ducking follows: up while talking, back down after.
        assert!(p.duck() > 0.9);
        for _ in 0..48_000 {
            p.next(Some(&b));
        }
        assert!(p.duck() < 0.02);
    }

    #[test]
    fn a_new_line_cuts_off_the_last_quickly() {
        let b = tiny();
        let mut p = VoicePlayer::default();
        let (mut u, _) = b.plan("left left left left", 1.0);
        u.serial = 1;
        p.set(&u);
        for _ in 0..2000 {
            p.next(Some(&b));
        }
        let mut stop = Utterance { serial: 2, ..Utterance::default() };
        stop.gain = 1.0;
        p.set(&stop);
        let mut n = 0;
        while p.talking() {
            p.next(Some(&b));
            n += 1;
            assert!(n < 1000, "still talking");
        }
        assert!(n as f32 / RATE <= 0.013);
        // No bank: silent, never stuck.
        p.set(&Utterance { serial: 3, ..u });
        assert_eq!(p.next(None), 0.0);
        assert!(!p.talking());
    }

    #[test]
    fn the_real_bank_loads() {
        let b = bank().unwrap();
        assert!(b.len() >= 120, "{b:?}");
        assert_eq!(b.rate(), 24_000);
        assert!(b.clips.iter().all(|c| c.len() > 2400 && c.iter().all(|v| v.is_finite() && v.abs() <= 1.0)), "{b:?}");
        // Memory: ~9 MB decoded.
        assert!(b.seconds() * b.rate() as f32 * 4.0 < 12e6, "{b:?}");
    }
}
