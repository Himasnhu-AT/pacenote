//! Speaking lines out loud through the system's text-to-speech: `say` on macOS, `espeak-ng` /
//! `espeak` / `spd-say` on Linux, System.Speech through PowerShell on Windows. One line at a
//! time (a child process); no TTS available = the lines only show as subtitles.

use std::collections::VecDeque;
use std::process::{Child, Command, Stdio};

use super::calls::{Line, Priority};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Say,
    Espeak(&'static str),
    SpdSay,
    PowerShell,
    None,
}

fn detect() -> Backend {
    let has = |cmd: &str, arg: &str| Command::new(cmd).arg(arg).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
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

/// The co-driver's voice: a queue of lines and the one being spoken.
pub struct Speech {
    backend: Option<Backend>,
    queue: VecDeque<Line>,
    current: Option<(Child, Priority)>,
    /// Words per minute (co-drivers read fast).
    pub rate: u32,
    /// Speak at all (false: subtitles only, e.g. muted runs).
    pub audible: bool,
}

impl Default for Speech {
    fn default() -> Self {
        Speech { backend: None, queue: VecDeque::new(), current: None, rate: 230, audible: true }
    }
}

impl Speech {
    /// Queues a line by priority. An urgent line cuts off what's being said.
    pub fn say(&mut self, line: Line) {
        if line.priority == Priority::Urgent {
            self.queue.retain(|l| l.priority == Priority::Urgent);
            if let Some((c, p)) = &mut self.current
                && *p < Priority::Urgent
            {
                let _ = c.kill();
            }
        }
        let at = self.queue.iter().position(|l| l.priority < line.priority).unwrap_or(self.queue.len());
        self.queue.insert(at, line);
    }

    /// Drops everything (pause, restart, leaving the stage).
    pub fn hush(&mut self) {
        self.queue.clear();
        if let Some((mut c, _)) = self.current.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn busy(&mut self) -> bool {
        if let Some((c, _)) = &mut self.current {
            match c.try_wait() {
                Ok(None) => return true,
                _ => self.current = None,
            }
        }
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
        if self.audible {
            let backend = *self.backend.get_or_insert_with(detect);
            if let Some(child) = spawn(backend, &line.text, self.rate) {
                self.current = Some((child, line.priority));
            }
        }
        Some(line)
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        self.hush();
    }
}

fn spawn(b: Backend, text: &str, rate: u32) -> Option<Child> {
    let mut cmd = match b {
        Backend::Say => {
            let mut c = Command::new("say");
            c.arg("-r").arg(rate.to_string()).arg("--").arg(text);
            c
        }
        Backend::Espeak(e) => {
            let mut c = Command::new(e);
            c.arg("-s").arg(rate.to_string()).arg("--").arg(text);
            c
        }
        Backend::SpdSay => {
            let mut c = Command::new("spd-say");
            c.arg("-w").arg("-r").arg("40").arg("--").arg(text);
            c
        }
        Backend::PowerShell => {
            let safe = text.replace('\'', "''");
            let script = format!(
                "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Rate = 3; $s.Speak('{safe}')"
            );
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            c
        }
        Backend::None => return None,
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_orders_by_priority_and_drops_stale() {
        let mut sp = Speech::default();
        sp.audible = false;
        sp.say(Line { text: "a".into(), priority: Priority::Note, stale_after: Some(100.0) });
        sp.say(Line::new("b", Priority::Chat));
        sp.say(Line::new("c", Priority::Warn));
        assert_eq!(sp.pump(0.0).unwrap().text, "c");
        // "a" is stale past 100 m.
        assert_eq!(sp.pump(150.0).unwrap().text, "b");
        assert!(sp.pump(150.0).is_none());
        sp.say(Line::new("x", Priority::Note));
        sp.say(Line::new("boom", Priority::Urgent));
        assert_eq!(sp.pump(0.0).unwrap().text, "boom");
        assert!(sp.pump(0.0).is_none(), "urgent clears the queue");
    }
}
