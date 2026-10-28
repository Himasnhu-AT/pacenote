//! Voice end to end, headless and in-process: a driver app (listen server) and a guide app
//! (client) on the engine's in-memory `LoopbackNet`, each feeding a generated signal instead
//! of the microphone. Frames are encoded, sent unreliably, jitter-buffered and decoded on the
//! other side (main-thread playout, since there is no audio device).

use std::path::PathBuf;
use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use engine::net::{self, LinkConditioner, LoopbackNet, NetClient};
use engine::prelude::*;
use pacenote::voice::net::VoiceRole;
use pacenote::voice::{TalkMode, VoicePlugin, VoiceState};

const STEP: Duration = Duration::from_nanos(16_666_667);

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pacenote-voice-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn build(tag: &str, source: &str, link: LinkConditioner) -> App {
    let mut app = engine::app_with(GameInfo::new("pacenote", "Pacenote"), EngineOptions::headless_test(temp_dir(tag)));
    app.insert_resource(NetConfig { conditioner: link, ..default() }).add_plugins(VoicePlugin).insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
    app.finish();
    app.cleanup();
    let mut st = app.world_mut().resource_mut::<VoiceState>();
    st.set_test_source(Some(source)).unwrap();
    st.keep_output = true;
    app
}

struct Pair {
    driver: App,
    guide: App,
    _lo: LoopbackNet,
}

impl Pair {
    fn new(driver_src: &str, guide_src: &str, link: LinkConditioner) -> Self {
        let lo = LoopbackNet::new();
        let mut driver = build("driver", driver_src, link);
        let mut guide = build("guide", guide_src, LinkConditioner { seed: link.seed + 1, ..link });
        net::host(driver.world_mut(), lo.bind(7777), true);
        net::connect(guide.world_mut(), lo.bind(0), "127.0.0.1:7777".parse().unwrap());
        let mut p = Pair { driver, guide, _lo: lo };
        for _ in 0..300 {
            if p.guide.world().get_resource::<NetClient>().is_some_and(|c| c.id().is_some()) {
                break;
            }
            p.run(1);
        }
        assert!(p.guide.world().get_resource::<NetClient>().is_some_and(|c| c.id().is_some()), "guide never connected");
        p
    }

    fn run(&mut self, frames: usize) {
        for _ in 0..frames {
            self.driver.update();
            self.guide.update();
        }
    }

    fn st(app: &App) -> &VoiceState {
        app.world().resource::<VoiceState>()
    }
    fn st_mut(app: &mut App) -> Mut<'_, VoiceState> {
        app.world_mut().resource_mut::<VoiceState>()
    }
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn driver_open_mic_reaches_guide_and_guide_ptt_reaches_driver() {
    let mut p = Pair::new("talk", "tone:300", LinkConditioner::default());
    assert_eq!(Pair::st(&p.driver).role, VoiceRole::Driver);
    assert_eq!(Pair::st(&p.guide).role, VoiceRole::Guide);
    assert_eq!(Pair::st(&p.driver).mode, TalkMode::OpenMic);
    assert_eq!(Pair::st(&p.guide).mode, TalkMode::PushToTalk);

    // 5 s: the driver's talk bursts (1.6 s on / 0.9 s off) go through the open-mic gate.
    Pair::st_mut(&mut p.guide).output.clear();
    p.run(300);
    let d = Pair::st(&p.driver);
    let g = Pair::st(&p.guide);
    let sent = d.sent_frames();
    // Two bursts ≈ 2 × (80 frames + 15 hang + 2 preroll); nothing sent in the silences.
    assert!((150..=230).contains(&sent), "driver sent {sent} frames");
    let t = g.talkers.iter().find(|t| t.talker == 0).expect("guide hears the driver (talker 0)");
    eprintln!("clean link: driver sent {sent}, guide {:?}, delay {} ms", t.stats, t.delay_ms);
    assert!(t.stats.played as i64 >= sent as i64 - 12, "played {} of {sent}: {:?}", t.stats.played, t.stats);
    assert_eq!((t.stats.late, t.stats.concealed > 30), (0, false), "{:?}", t.stats);
    // The decoded audio is there: loud during bursts, silent in the gaps.
    let out = &g.output;
    assert!(out.len() >= 16_000 * 4);
    let loud = (0..out.len() / 1600).filter(|i| rms(&out[i * 1600..(i + 1) * 1600]) > 0.05).count();
    let quiet = (0..out.len() / 1600).filter(|i| rms(&out[i * 1600..(i + 1) * 1600]) < 0.001).count();
    assert!(loud > 15 && quiet > 8, "loud {loud} / quiet {quiet} 100 ms blocks");
    // The guide hasn't pressed talk: nothing went the other way.
    assert_eq!(g.sent_frames(), 0);
    assert!(d.talkers.is_empty());

    // Guide holds push-to-talk for 1 s.
    let guide_id = p.guide.world().resource::<NetClient>().id().unwrap().0;
    Pair::st_mut(&mut p.guide).force_talk = Some(true);
    p.run(60);
    assert!(Pair::st(&p.guide).transmitting);
    assert!(Pair::st(&p.driver).remote_talking, "driver sees the guide talking");
    Pair::st_mut(&mut p.guide).force_talk = Some(false);
    p.run(30);
    let sent_g = Pair::st(&p.guide).sent_frames();
    assert!((45..=55).contains(&sent_g), "guide sent {sent_g}");
    let d = Pair::st(&p.driver);
    let t = d.talkers.iter().find(|t| t.talker == guide_id).expect("driver hears the guide");
    assert!(t.stats.played as i64 >= sent_g as i64 - 3, "{:?}", t.stats);
    // A 300 Hz tone at −12 dBFS comes out at about that level.
    let tail = &d.output[d.output.len() - 16_000..d.output.len() - 8_000];
    let peak = tail.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.2 && peak < 0.3 || rms(tail) < 0.001, "peak {peak}");
    p.run(20);
    assert!(!Pair::st(&p.driver).remote_talking);
}

#[test]
fn lossy_jittery_link_keeps_order_and_conceals() {
    let link = LinkConditioner { latency_ms: 40.0, jitter_ms: 15.0, loss: 0.1, duplicate: 0.02, reorder: 0.02, seed: 9 };
    let mut p = Pair::new("tone:440", "tone:440", link);
    // Driver open mic on a continuous tone: 4 s of frames.
    p.run(240);
    let g = Pair::st(&p.guide);
    let t = g.talkers.iter().find(|t| t.talker == 0).expect("talker 0");
    let s = t.stats;
    let sent = Pair::st(&p.driver).sent_frames();
    eprintln!("lossy link: sent {sent}, {s:?}, delay {} ms, jitter {:.1} ms", t.delay_ms, t.jitter_ms);
    assert!(s.received as f32 > sent as f32 * 0.8, "{s:?} of {sent}");
    assert!(s.concealed > 5, "losses concealed: {s:?}");
    assert!(s.played as f32 > sent as f32 * 0.75, "{s:?}");
    assert!(t.delay_ms >= 60.0 && t.delay_ms <= 300.0, "delay {}", t.delay_ms);
    // The duck snapshot follows the remote talker: on (and fading in) while the driver talks.
    assert!(g.remote_talking);
    let mixer = p.guide.world().resource::<engine::sound::Mixer>();
    assert!(mixer.snapshot_weight(pacenote::voice::DUCK_SNAPSHOT) > 0.9, "duck weight {}", mixer.snapshot_weight(pacenote::voice::DUCK_SNAPSHOT));
    // Driver goes quiet: the duck releases.
    Pair::st_mut(&mut p.driver).set_test_source(Some("tone:0")).unwrap();
    p.run(120);
    let mixer = p.guide.world().resource::<engine::sound::Mixer>();
    assert!(mixer.snapshot_weight(pacenote::voice::DUCK_SNAPSHOT) < 0.1);
}

#[test]
fn offline_does_nothing() {
    let mut app = build("offline", "talk", LinkConditioner::default());
    for _ in 0..120 {
        app.update();
    }
    let st = app.world().resource::<VoiceState>();
    assert_eq!(st.role, VoiceRole::Offline);
    assert_eq!(st.sent_frames(), 0);
    assert_eq!(st.frames_captured, 0);
}
