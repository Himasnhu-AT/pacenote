# Voice chat (`src/voice/`, `VoicePlugin`)

Driver and guide talk over the game's own net session. No new heavy crates: `cpal` (already in
the tree through rodio, same 0.17.3) for the microphone, the codec and jitter buffer are ours.

```text
mic (cpal thread, any rate) ─▶ downmix + 7 kHz low-pass + resample ─▶ 16 kHz mono ring
 ─▶ 20 ms frames (320 samples) ─▶ level meter, gate (push-to-talk / open mic)
 ─▶ IMA ADPCM 4:1 (164 B/frame) ─▶ VoiceFrame {talker, seq, ts, data}, Channel::Unreliable
 ─▶ net ─▶ jitter buffer per talker (60 ms, adaptive, loss concealment) ─▶ mix
 ─▶ rodio source on Bevy audio, pulled on the device clock ─▶ speakers
 while the other player talks: mix snapshot "voice_chat" (sfx/car −4 dB, ambience −6 dB)
```

| File | What |
|---|---|
| `adpcm.rs` | IMA ADPCM encoder/decoder. Frame = 4-byte header (predictor, step index) + 160 bytes of nibbles; every frame decodes on its own, so a loss never corrupts the next frame. 65.6 kbit/s payload. |
| `dsp.rs` | `Resampler` (any rate / channels → 16 kHz mono), `level_db`, open-mic `Gate` (threshold + 10 dB over a tracked noise floor, 300 ms hang), `SignalGen` test sources |
| `capture.rs` | `Capture`: a `voice-capture` thread owns the cpal input stream; `MicStatus` (`off/starting/live/no input device/failed/silent`) |
| `jitter.rs` | `JitterBuffer`: 64 slots keyed by capture frame index, playout delay max(60 ms, 3 × RFC 3550 jitter) ≤ 300 ms chosen per talk spurt, late frames dropped, losses concealed by repeating the last frame fading 1 → 0.6 → 0.2 → 0, spurt ends after 8 missing frames, one frame dropped when > delay + 80 ms is buffered (sender clock drift), sender restarts detected |
| `playback.rs` | `Player` (one jitter buffer per talker, ≤ 8), `PlayoutEngine` (inbox → render → status, no allocation), `VoiceStream`/`VoiceSource` (rodio source, 16 kHz mono) |
| `net.rs` | `VoiceFrame`, `VoiceRole`, `local_role`, send/receive/relay systems — the only file that touches the net API |
| `ui.rs` | Corner indicator, top right: red dot while you transmit, mic meter, hint ("Hold V / LB to talk", "Open mic", "No microphone found", "Mic silent: check permission"), green dot + "GUIDE talking" / "DRIVER talking" |
| `mod.rs` | `VoicePlugin`, `VoiceSettings`, `VoiceState`, systems, control commands |

## Behaviour

- **When it runs.** Capture starts only in a net session (`NetRole` ≠ `Offline`) with voice
  enabled, so solo play never opens the microphone (no permission prompt). Leaving the session
  stops the stream.
- **Roles.** `--host` (listen server) = driver, `--connect` = guide (`net::local_role`). The
  host sends with `broadcast_except(HOST)` (it never hears itself), clients `to_server`; the
  server stamps client frames with their `ClientId` (talker id; host = 0) and relays them to the
  other clients.
- **Talk modes** (`VoiceSettings::mode`): `Auto` (default) = open mic for the driver, whose hands
  are on the wheel, push-to-talk for the guide; or `PushToTalk` / `OpenMic` for both. The
  `voice.talk` action (V, gamepad LB) is in the `global` context with passthrough, so it works in
  menus and the pause screen and doesn't steal LB from tab switching. In open mic the talk button
  also forces transmit. When the gate opens, the two frames before it are sent first (40 ms
  pre-roll), so first syllables are not clipped.
- **Playout clock.** With an audio device, the jitter buffers live in the rodio source and are
  pulled by the audio thread, so playout follows the device clock and a slow game frame
  doesn't glitch voice. Packets go in through a preallocated inbox (`try_lock` on the audio
  side); talker status comes back the same way. Without a device (headless, `--mute`,
  test-driven runs without `--sound`) the same `PlayoutEngine` runs on the main thread against
  real time, so tests and `voice.status` see everything except the speaker.
- **Volume.** Device output = `VoiceSettings::volume` × the engine's voice bus gain
  (`Mixer::effective_gain(BusId::VOICE)`: master × Dialogue, mute, solo).
- **Ducking.** The engine's `DuckRule` only listens to engine voices, and voice chat is a stream,
  so the plugin turns a mix snapshot `voice_chat` on while a remote talker is audible (250 ms
  hold; fades 0.12 s in, 0.7 s out). `Mixer::effective_gain` includes snapshots, so the audio
  stream's `car` bus (child of `sfx`) and `ambience` duck with no code on their side.
- **Failure.** No input device, unsupported format, stream error: logged once, `MicStatus`
  says why, the indicator shows it, the game carries on and you still hear your teammate.

### macOS microphone permission

The first time a session starts, macOS asks "“Pacenote” would like to access the microphone."
For an unbundled binary started from a terminal, the prompt names the terminal app (Terminal,
iTerm, VS Code) and the grant applies to it. Until it is granted, or if it was denied, CoreAudio
delivers silence instead of an error: after 3 s of exact zeros the status becomes `silent`, the
indicator says "Mic silent: check permission", and the log says to allow it in System Settings ›
Privacy & Security › Microphone and restart. A packaged `.app` must carry
`NSMicrophoneUsageDescription` in its Info.plist, or macOS refuses access without asking (see
hand-back: `scripts/package.sh`).

## Settings

Section `"voice"` of settings.json, rows on the Audio tab: `voice_chat` (toggle), `voice_chat_volume`
(0–150 %), `voice_mode` (Auto / Push to talk / Open mic), `voice_threshold` (open-mic threshold,
−70…−20 dBFS, default −45). In the file only: `input_device` (device name, empty = system
default), `duck_db` (4).

## Debugging without a microphone

- `--voice-test tone | tone:<hz> | talk | <file.wav|ogg>` (dev builds): feeds a generated sine
  (−12 dBFS), a speech-like buzz in bursts (1.6 s on, 0.9 s off: exercises the gate), or a file
  (resampled to 16 kHz) instead of the microphone.
- Control commands: `voice.status` (role, talker, mode, mic status/device/level, transmitting,
  frames, sent/relayed/received, `talkers: [{talker, talking, level_db, delay_ms, jitter_ms,
  depth, stats: {received, played, late, duplicate, concealed, skipped, corrupt, spurts}}]`,
  `remote_talking`, `device_playout`), `voice.talk {on: true|false|null}` (hold push-to-talk),
  `voice.source {source}` (switch test source; `null` = microphone).
- Two processes on loopback UDP:
  `pacenote --host 7777 --voice-test talk --control 0` and
  `pacenote --connect 127.0.0.1:7777 --voice-test tone:300 --control 0`, then
  `voice.status` on each (add `--sound` to hear it).

## Tests

- Unit (`cargo test --lib voice`, 17): ADPCM round trip SNR on sines (> 20 dB at 220/440/1000 Hz,
  > 15 dB at 3 kHz) and white noise (> 9 dB), determinism, each frame decodes alone, clipping and
  bad input; resampler at 48 k stereo / 44.1 k / 16 k / 8 k (length and level), 12 kHz alias
  rejection at 48 kHz; gate opens on talk bursts, closes after the hang, ignores a steady hum;
  jitter buffer: clean link in order, reordering within the delay fixed, single loss concealed
  with a fade, late frames dropped and the delay grows on a jittery link, 20 % random loss never
  reorders, idles after a spurt, duplicates, far jumps, sender restart, 5 % clock drift caught
  up by skipping; two talkers mixed and reported.
- Integration (`cargo test --test voice`, 3): two headless apps on `LoopbackNet`: the driver's
  open-mic bursts reach the guide (talker 0, decoded audio loud in bursts and silent between),
  nothing flows the other way until the guide holds push-to-talk, then the guide's tone reaches
  the driver at its level and the driver sees "talking"; on a 40 ms / 15 ms jitter / 10 % loss /
  2 % duplicate / 2 % reorder link frames stay in order, losses are concealed, the duck
  snapshot is on while the driver talks and released after; offline does nothing.

## Measurements

Apple M5 Pro, dev profile, `cargo test --test voice -- --nocapture`:

| Case | Result |
|---|---|
| Clean loopback, driver open mic on `talk`, 5 s | 195 frames sent (2 bursts + pre-roll + hang), 195 received, 192 played, 0 late, playout delay 60 ms; the 16 concealed frames are the fade tails after each spurt |
| 40 ms + 15 ms jitter, 10 % loss, 2 % dup, 2 % reorder, 4 s tone | 217 sent, 182 received, 174 played, 5 late, 24 concealed, estimated jitter 16.7 ms, delay 60 ms |
| Bandwidth per talker | 164 B payload + ~10 B message/packet framing per 20 ms ≈ 70 kbit/s up while talking, 0 when silent |
| Cost | per 20 ms frame: encode 2.0 µs, decode 0.45 µs (dev profile, `cargo test --lib voice::adpcm -- --ignored --nocapture`); the playout path does no allocation after start-up; capture appends to a 0.5 s ring under a mutex once per device callback |

## Known limits

- No echo cancellation or noise suppression: use headphones (open mic on speakers will feed
  the other player's voice back).
- Input device: the system default unless `input_device` is set in settings.json (no picker
  yet); a device unplugged mid-session sets `failed` until the next session (no auto-reopen).
- Concealment repeats whole frames (audible as a short buzz on long losses); no packet-loss
  redundancy or FEC.
- Voice plays through its own rodio source rather than an engine voice, so it bypasses the
  engine renderer's bus low-pass/reverb (it follows the voice bus gain only). An engine
  "stream voice" (a voice fed by a ring buffer) would let it join the mix properly.
- Role comes from `NetRole` (host = driver); the guide-net stream's `net_session::Role` can
  replace `net::local_role` in one place.
