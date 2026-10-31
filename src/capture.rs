//! Dev-only video capture (feature `dev`), for trailers and gameplay clips from offscreen runs.
//!
//! - `--capture-hz 60`: every frame advances game time by exactly 1/60 s
//!   (`TimeUpdateStrategy::ManualDuration`), however long it takes to render, so the 60 Hz sim
//!   runs one step per frame and a recording plays back at true speed with no dropped frames.
//!   Pair with `--car-audio-wav` to render the car synth in step with the same game time.
//! - `capture.start {path, crf?, ffmpeg?}`: from the next frame, every rendered frame (the
//!   `--offscreen` image, 1920×1080) is piped as raw RGB to ffmpeg, which writes an H.264 master
//!   at `path` (60 fps). A sidecar `<path>.csv` gets one line of car state per frame (for audio
//!   sync and editing). The reply holds the frame number of the first captured frame.
//! - `capture.stop`: stop capturing; ffmpeg finishes once every requested frame has arrived.
//! - `capture.status`: `{recording, requested, written, done, frame}`.
//! - Frames while paused are skipped, so two games can be stepped in lockstep with `time.step`.
//! - `--capture-gate <port>`: a frame gate for filming two games in lockstep (driver + guide).
//!   A plain TCP socket on 127.0.0.1:<port>; the client sends one byte per instruction: `o` gate
//!   on, `f` gate off, `g` grant one frame. While gated, every frame waits at `First` for a grant
//!   and answers `a` when it starts. With `--capture-hz` in both games, each one's game and real
//!   time then advance exactly 1/60 s per granted frame, so their network clocks stay in step.
//! - `capture.wait {frames}`: replies once that many frames have passed (scripts sync to game
//!   time with it, since a captured frame takes longer than 1/60 s of wall time).

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use engine::devtools::{AddControlCommand, PendingReplies, ReplyId, params};
use serde_json::{Value, json};

use crate::drive::{DrivenCar, DriverInput, StageRun};
use crate::guide::sync::{NetCar, NetRun};

pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if let Some(hz) = engine::cli::dev_value("capture-hz").and_then(|v| v.parse::<f64>().ok()) {
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / hz.clamp(1.0, 480.0))));
        }
        if let Some(port) = engine::cli::dev_value("capture-gate").and_then(|v| v.parse::<u16>().ok()) {
            app.insert_resource(Gate::open(port)).add_systems(First, wait_at_gate);
        }
        app.init_resource::<Recorder>()
            .init_resource::<Waits>()
            // Headless test apps have no control port or frame counter: run only where they exist.
            .add_systems(
                Last,
                (
                    (log_text, grab).chain().run_if(resource_exists::<FrameCount>),
                    answer_waits.run_if(resource_exists::<PendingReplies>.and_then(resource_exists::<FrameCount>)),
                ),
            )
            .add_control_command(
                "capture.start",
                "Record every frame of the offscreen image to an H.264 file through ffmpeg (use with --capture-hz 60)",
                params(&[("path", "string", "output .mp4"), ("crf", "integer", "x264 quality (default 12)"), ("ffmpeg", "string", "ffmpeg binary (default /opt/homebrew/bin/ffmpeg)")]),
                start,
            )
            .add_control_command("capture.stop", "Stop recording", params(&[]), stop)
            .add_control_command("capture.status", "Recording progress", params(&[]), status)
            .add_deferred_control_command("capture.wait", "Reply after N frames", params(&[("frames", "integer", "frames to wait")]), wait);
    }
}

struct Writer {
    tx: mpsc::Sender<(u64, Vec<u8>)>,
    written: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
    expected: Arc<AtomicU64>,
}

#[derive(Resource, Default)]
struct Recorder {
    on: bool,
    requested: u64,
    first_frame: u32,
    writer: Option<Writer>,
    csv: Option<std::io::BufWriter<std::fs::File>>,
    /// `<path>.text.csv`: on-screen text that changed (co-driver subtitles, cards), by frame.
    texts: Option<std::io::BufWriter<std::fs::File>>,
}

/// See `--capture-gate` in the module docs.
#[derive(Resource)]
struct Gate {
    gated: Arc<AtomicBool>,
    grants: Arc<AtomicU64>,
    ack: Arc<std::sync::Mutex<Option<std::net::TcpStream>>>,
}

impl Gate {
    fn open(port: u16) -> Gate {
        let gate = Gate { gated: Arc::new(AtomicBool::new(false)), grants: Arc::new(AtomicU64::new(0)), ack: Arc::new(std::sync::Mutex::new(None)) };
        let (gated, grants, ack) = (gate.gated.clone(), gate.grants.clone(), gate.ack.clone());
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                info!("capture gate on 127.0.0.1:{port}");
                std::thread::spawn(move || {
                    use std::io::Read;
                    for stream in listener.incoming().flatten() {
                        let _ = stream.set_nodelay(true);
                        *ack.lock().unwrap_or_else(|e| e.into_inner()) = stream.try_clone().ok();
                        let mut stream = stream;
                        let mut buf = [0u8; 256];
                        while let Ok(n) = stream.read(&mut buf) {
                            if n == 0 {
                                break;
                            }
                            for b in &buf[..n] {
                                match b {
                                    b'o' => gated.store(true, Ordering::Release),
                                    b'f' => gated.store(false, Ordering::Release),
                                    b'g' => {
                                        grants.fetch_add(1, Ordering::AcqRel);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        // Client gone: never leave the game stuck.
                        gated.store(false, Ordering::Release);
                    }
                });
            }
            Err(e) => error!("capture gate: can't listen on {port}: {e}"),
        }
        gate
    }
}

fn wait_at_gate(gate: Res<Gate>) {
    if !gate.gated.load(Ordering::Acquire) {
        return;
    }
    let t0 = std::time::Instant::now();
    loop {
        let g = gate.grants.load(Ordering::Acquire);
        if g > 0 && gate.grants.compare_exchange(g, g - 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            break;
        }
        // Ungated meanwhile, or nobody granting for a minute: carry on.
        if !gate.gated.load(Ordering::Acquire) || t0.elapsed().as_secs() > 60 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    if let Some(s) = gate.ack.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = s.write_all(b"a");
    }
}

#[derive(Resource, Default)]
struct Waits(Vec<(u32, ReplyId)>);

fn frame(world: &World) -> u32 {
    world.get_resource::<FrameCount>().map_or(0, |f| f.0)
}

fn start(world: &mut World, p: Value) -> Result<Value, String> {
    let path = p.get("path").and_then(Value::as_str).ok_or("`path` is required")?.to_string();
    let crf = p.get("crf").and_then(Value::as_u64).unwrap_or(12);
    let ffmpeg = p.get("ffmpeg").and_then(Value::as_str).unwrap_or("/opt/homebrew/bin/ffmpeg").to_string();
    if world.get_resource::<engine::devtools::offscreen::OffscreenTarget>().is_none() {
        return Err("capture needs --offscreen".into());
    }
    if world.resource::<Recorder>().on || world.resource::<Recorder>().writer.is_some() {
        return Err("already recording (capture.stop, then wait for done)".into());
    }
    let (w, h) = engine::devtools::offscreen::SIZE;
    let mut child = std::process::Command::new(&ffmpeg)
        .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{w}x{h}"), "-r", "60", "-i", "-"])
        .args(["-c:v", "libx264", "-preset", "fast", "-crf", &crf.to_string(), "-pix_fmt", "yuv420p", "-movflags", "+faststart", &path])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't start {ffmpeg}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no ffmpeg stdin")?;
    let (tx, rx) = mpsc::channel::<(u64, Vec<u8>)>();
    let (written, done, expected) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(u64::MAX)));
    let (wr, dn, ex) = (written.clone(), done.clone(), expected.clone());
    // Frames can come back out of order: write them by index.
    std::thread::spawn(move || {
        let mut next = 0u64;
        let mut held: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        loop {
            if next >= ex.load(Ordering::Acquire) {
                break;
            }
            match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok((i, px)) => {
                    held.insert(i, px);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            while let Some(px) = held.remove(&next) {
                if stdin.write_all(&px).is_err() {
                    break;
                }
                next += 1;
                wr.store(next, Ordering::Release);
            }
        }
        drop(stdin);
        let _ = child.wait();
        dn.store(true, Ordering::Release);
    });
    let csv = std::fs::File::create(format!("{path}.csv")).ok().map(std::io::BufWriter::new);
    let texts = std::fs::File::create(format!("{path}.text.csv")).ok().map(std::io::BufWriter::new);
    let f = frame(world) + 1;
    let mut r = world.resource_mut::<Recorder>();
    *r = Recorder { on: true, requested: 0, first_frame: f, writer: Some(Writer { tx, written, done, expected }), csv, texts };
    if let Some(c) = r.csv.as_mut() {
        let _ = writeln!(c, "i,frame,clock,progress,kmh,rpm,gear,throttle,brake,handbrake,steer,impacts,retired,x,z,wall,vt");
    }
    Ok(json!({"path": path, "first_frame": f}))
}

fn stop(world: &mut World, _: Value) -> Result<Value, String> {
    let mut r = world.resource_mut::<Recorder>();
    r.on = false;
    let n = r.requested;
    if let Some(w) = &r.writer {
        w.expected.store(n, Ordering::Release);
    }
    if let Some(c) = r.csv.as_mut() {
        let _ = c.flush();
    }
    r.csv = None;
    if let Some(c) = r.texts.as_mut() {
        let _ = c.flush();
    }
    r.texts = None;
    Ok(json!({"requested": n}))
}

fn status(world: &mut World, _: Value) -> Result<Value, String> {
    let f = frame(world);
    let mut r = world.resource_mut::<Recorder>();
    let (written, done) = r.writer.as_ref().map_or((0, true), |w| (w.written.load(Ordering::Acquire), w.done.load(Ordering::Acquire)));
    if done && !r.on {
        r.writer = None;
    }
    Ok(json!({"recording": r.on, "requested": r.requested, "written": written, "done": done, "frame": f, "first_frame": r.first_frame}))
}

fn wait(world: &mut World, p: Value, reply: ReplyId) -> Result<(), String> {
    let n = p.get("frames").and_then(Value::as_u64).unwrap_or(1).max(1) as u32;
    let at = frame(world) + n;
    world.resource_mut::<Waits>().0.push((at, reply));
    Ok(())
}

fn answer_waits(frames: Res<FrameCount>, mut waits: ResMut<Waits>, mut pending: ResMut<PendingReplies>) {
    let now = frames.0;
    waits.0.retain(|(at, reply)| {
        if now >= *at {
            pending.reply(*reply, Ok(json!({"frame": now})));
            false
        } else {
            true
        }
    });
}

/// Last, before `grab`: log on-screen text that changed this frame (words, not numbers), so
/// co-driver lines can be voiced in the edit at the frame they appeared.
fn log_text(mut rec: ResMut<Recorder>, time: Res<Time<Virtual>>, texts: Query<&Text, Changed<Text>>) {
    if !rec.on || time.delta_secs() <= 0.0 {
        return;
    }
    let i = rec.requested;
    let Some(out) = rec.texts.as_mut() else { return };
    for t in &texts {
        let words = t.0.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2).count();
        if words >= 2 {
            let _ = writeln!(out, "{i}\t{}", t.0.replace(['\n', '\t'], " "));
        }
    }
}

/// Last: ask for this frame's image and log the car.
#[allow(clippy::too_many_arguments)] // a Bevy system: one param per resource it reads
fn grab(
    mut commands: Commands,
    mut rec: ResMut<Recorder>,
    target: Option<Res<engine::devtools::offscreen::OffscreenTarget>>,
    frames: Res<FrameCount>,
    car: Query<(&DrivenCar, Option<&DriverInput>)>,
    run: Option<Res<StageRun>>,
    time: Res<Time<Virtual>>,
    net: Query<(Option<&NetCar>, Option<&NetRun>)>,
) {
    // Paused frames (between `time.step`s) are not part of the film.
    if !rec.on || time.delta_secs() <= 0.0 {
        return;
    }
    let (Some(target), Some(w)) = (target, rec.writer.as_ref()) else { return };
    let i = rec.requested;
    let tx = w.tx.clone();
    commands.spawn(Screenshot::image(target.0.clone())).observe(move |shot: On<ScreenshotCaptured>| {
        let px = match shot.image.clone().try_into_dynamic() {
            Ok(img) => img.to_rgb8().into_raw(),
            Err(e) => {
                error!("capture: frame {i}: {e:?}");
                let (w, h) = engine::devtools::offscreen::SIZE;
                vec![0; (w * h * 3) as usize]
            }
        };
        let _ = tx.send((i, px));
    });
    rec.requested += 1;
    let f = frames.0;
    let Some(c) = rec.csv.as_mut() else { return };
    let vt = time.elapsed_secs_f64();
    let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let (mut clock, mut progress, retired) = run.map_or((0.0, 0.0, false), |r| (r.time, r.progress, r.retired));
    // The guide has no car or run of its own: log the replicated ones.
    let mut netpos = None;
    for (nc, nr) in &net {
        if let Some(nr) = nr {
            (clock, progress) = (nr.time, nr.progress);
        }
        if let Some(nc) = nc {
            netpos = Some(nc.pos);
        }
    }
    match car.iter().next() {
        Some((d, inp)) => {
            let inp = inp.map(|x| x.0).unwrap_or_default();
            let c2 = &d.car;
            let _ = writeln!(
                c,
                "{i},{f},{clock:.3},{progress:.1},{:.1},{:.0},{},{:.2},{:.2},{},{:.3},{:.2},{},{:.2},{:.2},{wall:.4},{vt:.4}",
                c2.speed() * 3.6,
                c2.rpm,
                c2.gear,
                inp.throttle,
                inp.brake,
                inp.handbrake as u8,
                inp.steer,
                c2.impacts,
                retired as u8,
                c2.pos.x,
                c2.pos.z
            );
        }
        None => {
            let (x, z) = netpos.map_or((f32::NAN, f32::NAN), |p| (p.x, p.z));
            let _ = writeln!(c, "{i},{f},{clock:.3},{progress:.1},,,,,,,,,{},{x:.2},{z:.2},{wall:.4},{vt:.4}", retired as u8);
        }
    }
}
