//! Driver + guide in one process over the engine's in-memory `LoopbackNet`: the guide gets the
//! stage id, regenerates the same road (hash), follows the car and the run, never simulates;
//! the driver survives the guide leaving; the guide can come back.
//! Run: `cargo test --test guide_net`.

use std::path::PathBuf;
use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use engine::net::{self, LoopbackNet, NetClient, NetServer, Remote};
use engine::prelude::*;
use pacenote::drive::{DrivenCar, StageRun};
use pacenote::guide::sync::{NetCar, road_hash};
use pacenote::guide::{GuideFeed, GuidePlugin, GuideStage};
use pacenote::net_session::{self, Role, Session};
use pacenote::stage;
use pacenote::vehicle::Car;
use pacenote::world::{self as pworld, StageWorld};

const STEP: Duration = Duration::from_nanos(16_666_667);

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pacenote-net-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(tag: &str) -> App {
    let mut app = engine::app_with(GameInfo::new("pacenote", "Pacenote"), EngineOptions::headless_test(temp_dir(tag)));
    app.add_plugins(GuidePlugin).insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
    app.finish();
    app.cleanup();
    app
}

struct Pair {
    lo: LoopbackNet,
    driver: App,
    guide: App,
}

const ADDR: &str = "127.0.0.1:7777";

impl Pair {
    fn new(stage_id: u32) -> Pair {
        let lo = LoopbackNet::new();
        let mut driver = app("driver");
        driver.world_mut().resource_mut::<Session>().name = "Ana".into();
        net_session::host_with(driver.world_mut(), lo.bind(7777));
        // Driven in non-default weather and time: the guide has to follow them.
        let def = stage::stages().into_iter().find(|d| d.id == stage_id).unwrap();
        let def = pacenote::meta::stages::with_conditions(def, stage::Weather::Fog, stage::TimeOfDay::Dusk);
        let s = pworld::generate(&def);
        let (pos, heading) = s.road.start();
        driver.world_mut().insert_resource(s);
        driver.world_mut().insert_resource(StageRun { countdown: 3.0, ..default() });
        let car = Car::new(pos + Vec3::Y * 0.9, heading);
        driver.world_mut().spawn((Name::new("car"), DrivenCar { prev: (car.pos, car.rot), car }));
        let mut guide = app("guide");
        guide.world_mut().resource_mut::<Session>().name = "Ben".into();
        let l = lo.clone();
        net_session::join_with(guide.world_mut(), ADDR.parse().unwrap(), move || l.bind(0));
        Pair { lo, driver, guide }
    }

    fn run(&mut self, frames: usize) {
        for _ in 0..frames {
            self.driver.update();
            self.guide.update();
        }
    }

    fn run_until(&mut self, max: usize, what: &str, mut f: impl FnMut(&mut Pair) -> bool) {
        for _ in 0..max {
            if f(self) {
                return;
            }
            self.run(1);
        }
        panic!("timed out waiting for: {what}");
    }

    /// Moves the driver's car `metres` along the road and runs the clock (what `simulate`
    /// would do with the throttle down).
    fn drive_to(&mut self, metres: f32, time: f32) {
        let w = self.driver.world_mut();
        let road = w.resource::<StageWorld>().road.clone();
        let i = road.station(metres);
        let (p, d) = (road.pts[i], road.dir[i]);
        let mut q = w.query::<&mut DrivenCar>();
        for mut c in q.iter_mut(w) {
            c.car.pos = p + Vec3::Y * 0.6;
            c.car.rot = Car::new(p, d.x.atan2(d.y)).rot;
            c.car.vel = Vec3::new(d.x, 0.0, d.y) * 25.0;
            c.car.gear = 3;
            c.car.rpm = 5200.0;
        }
        let mut run = w.resource_mut::<StageRun>();
        run.countdown = 0.0;
        run.time = time;
        run.progress = metres;
        let len = road.length();
        while run.splits.len() < 3 && metres >= len / 4.0 * (run.splits.len() + 1) as f32 {
            run.splits.push(time);
        }
    }

    fn guide_car(&mut self) -> Option<NetCar> {
        let w = self.guide.world_mut();
        let mut q = w.query_filtered::<&NetCar, With<Remote>>();
        q.iter(w).next().copied()
    }
}

#[test]
fn guide_receives_stage_car_and_run_and_never_simulates() {
    let mut p = Pair::new(3);
    p.run_until(300, "guide connected", |p| p.guide.world().resource::<Session>().connected);
    assert_eq!(*p.driver.world().resource::<Role>(), Role::Driver);
    assert_eq!(*p.guide.world().resource::<Role>(), Role::Guide);
    p.run_until(120, "driver sees the guide", |p| p.driver.world().resource::<Session>().guide.is_some());

    // Names for the leaderboard.
    p.run_until(60, "names exchanged", |p| p.driver.world().resource::<Session>().peer_name.is_some() && p.guide.world().resource::<Session>().peer_name.is_some());
    assert_eq!(p.driver.world().resource::<Session>().peer_name.as_deref(), Some("Ben"));
    assert_eq!(p.guide.world().resource::<Session>().peer_name.as_deref(), Some("Ana"));

    // Same stage, same road.
    p.run_until(120, "guide generated the stage", |p| p.guide.world().contains_resource::<GuideStage>());
    let g = p.guide.world().resource::<GuideStage>().clone();
    let s = p.driver.world().resource::<StageWorld>().clone();
    assert_eq!(g.def.id, 3);
    assert_eq!((g.def.weather, g.def.time), (stage::Weather::Fog, stage::TimeOfDay::Dusk), "guide sees the driver's weather and time");
    assert_eq!(g.road_hash, road_hash(&s.road), "guide road differs from the driver's");
    assert!(g.hash_ok);
    assert_eq!(g.corners.len(), s.corners.len());
    assert_eq!(*g.corners, *s.corners);

    // The car and the run follow (interpolated ~0.1 s behind).
    p.drive_to(900.0, 61.25);
    p.run(30);
    let want = p.driver.world_mut().query::<&DrivenCar>().iter(p.driver.world()).next().unwrap().car.pos;
    let car = p.guide_car().expect("car replicated");
    assert!(car.pos.distance(want) < 0.05, "guide car at {:?}, driver at {want:?}", car.pos);
    assert_eq!(car.gear, 3);
    assert!((car.speed() - 25.0).abs() < 0.2, "speed {}", car.speed());
    assert!((car.rpm - 5200.0).abs() <= 20.0);
    let feed = p.guide.world().resource::<GuideFeed>().clone();
    assert!((feed.run.time - 61.25).abs() < 0.01, "run time {}", feed.run.time);
    assert_eq!(feed.run.n_splits, 1, "first split at a quarter of {:.0} m", s.road.length());
    assert!((feed.s - 900.0).abs() < 5.0, "guide places the car at {} m", feed.s);

    // Damage and body parts off reach the guide (its car drops the same parts).
    {
        let w = p.driver.world_mut();
        let mut q = w.query::<&mut DrivenCar>();
        for mut c in q.iter_mut(w) {
            c.car.damage.hit(0.3, Vec3::new(-0.2, 0.0, -2.0), pacenote::vehicle::HitZone::Front);
        }
    }
    let want = p.driver.world_mut().query::<&DrivenCar>().iter(p.driver.world()).next().unwrap().car.damage;
    assert!(want.off("FrontSplitter") && want.off("FrontBumper") && want.engine > 0.2, "{want:?}");
    p.run(30);
    let got = p.guide.world().resource::<GuideFeed>().damage;
    assert_eq!(got, pacenote::guide::sync::NetDamage::from_damage(&want));
    assert_eq!(got.detached, want.detached);
    assert!((got.parts()[0] - want.engine).abs() <= 0.026, "engine {} vs {}", got.parts()[0], want.engine);

    // The guide never simulates: no stage world, no driven car.
    assert!(!p.guide.world().contains_resource::<StageWorld>());
    assert_eq!(p.guide.world_mut().query::<&DrivenCar>().iter(p.guide.world()).count(), 0);

    // Bandwidth: car moving every tick for 2 s.
    for k in 0..120 {
        p.drive_to(900.0 + k as f32 * 0.42, 61.25 + k as f32 / 60.0);
        p.run(1);
    }
    let down = p.guide.world().resource::<NetClient>().endpoint.stats().unwrap().recv_bytes_per_sec;
    println!("guide downlink with the car moving: {:.0} B/s", down);
    assert!(down < 4000.0, "guide downlink {down} B/s");
}

#[test]
fn driver_keeps_driving_when_the_guide_leaves_and_the_guide_can_rejoin() {
    let mut p = Pair::new(1);
    p.run_until(300, "connected", |p| p.driver.world().resource::<Session>().guide.is_some() && p.guide.world().contains_resource::<GuideStage>());
    net_session::leave(p.guide.world_mut());
    p.run_until(120, "driver notices", |p| p.driver.world().resource::<Session>().guide.is_none());
    assert_eq!(*p.guide.world().resource::<Role>(), Role::Solo, "leave goes back to solo");
    assert!(p.driver.world().resource::<Session>().peer_name.is_none());
    assert!(p.driver.world().resource::<Session>().last_drop.is_some());
    assert!(p.driver.world().contains_resource::<NetServer>(), "driver still hosting");
    // The driver's world carries on.
    p.drive_to(500.0, 30.0);
    p.run(10);
    assert_eq!(p.driver.world().resource::<StageRun>().progress, 500.0);
    // No reconnect after a deliberate leave.
    p.run(200);
    assert!(!p.guide.world().contains_resource::<NetClient>());
    // Rejoin: the stage and car arrive again.
    let l = p.lo.clone();
    net_session::join_with(p.guide.world_mut(), ADDR.parse().unwrap(), move || l.bind(0));
    p.run_until(300, "reconnected", |p| p.guide.world().resource::<Session>().connected && p.driver.world().resource::<Session>().guide.is_some());
    p.run_until(60, "car again", |p| p.guide_car().is_some_and(|c| c.pos.distance(Vec3::ZERO) > 1.0));
    let feed = p.guide.world().resource::<GuideFeed>().clone();
    assert_eq!(feed.stage.id, 1);
    assert!((feed.run.progress - 500.0).abs() < 0.2);
}

#[test]
fn guide_retries_when_the_link_drops() {
    let mut p = Pair::new(1);
    p.run_until(300, "connected", |p| p.guide.world().resource::<Session>().connected);
    // The driver stops hosting (e.g. quits to the menu), then hosts again.
    net::disconnect(p.driver.world_mut());
    p.run_until(120, "guide sees the drop", |p| !p.guide.world().resource::<Session>().connected);
    assert!(p.guide.world().resource::<Session>().was_connected);
    net_session::host_with(p.driver.world_mut(), p.lo.bind(7777));
    // RETRY_SECS is real time: the manual time strategy steps 1/60 s per update.
    p.run_until(60 * 8, "guide reconnected by itself", |p| p.guide.world().resource::<Session>().connected);
}

#[test]
fn guide_call_timing_follows_the_replicated_run() {
    use pacenote::guide::coach::GuideCalls;
    let mut p = Pair::new(1);
    p.run_until(300, "guide has the stage", |p| p.guide.world().contains_resource::<GuideStage>());
    p.drive_to(5.0, 0.5);
    p.run(30);
    let first = p.guide.world().resource::<GuideCalls>().coach.notes().iter().find(|n| n.at > 60.0).copied().expect("notes");
    assert_eq!(p.guide.world().resource::<GuideCalls>().coach.tally.count(), 0, "nothing graded at the start");
    // The car goes past the first note with no call: missed.
    p.drive_to(first.at + 20.0, 12.0);
    p.run(30);
    let t = p.guide.world().resource::<GuideCalls>().coach.tally;
    assert!(t.missed >= 1, "{t:?}");
    assert_eq!(t.score(), Some(0));
    // A restart (the countdown again) starts the tally over.
    p.driver.world_mut().insert_resource(StageRun { countdown: 3.0, ..default() });
    p.run(30);
    assert_eq!(p.guide.world().resource::<GuideCalls>().coach.tally.count(), 0);
    // Still no simulation on the guide's side.
    assert!(!p.guide.world().contains_resource::<StageWorld>());
}
