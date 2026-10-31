//! What the driver sends the guide, and how small it is.
//!
//! Three replicated components (engine `net`, delta-compressed snapshots at 30 Hz):
//!
//! | Component | Entity | Bytes (before delta) | Rate |
//! |---|---|---|---|
//! | [`NetStage`] `{id, road_hash, weather, time}` | session entity | ~12 | on change |
//! | [`NetRun`] clock, penalty, progress, splits, finished | session entity | 8–20 | 10 Hz + on events |
//! | [`NetCar`] pose, velocity, steer, rpm, gear, wheel spin | the car | ~30 | 30 Hz, interpolated |
//!
//! Every value is quantised *before* it goes into the component ([`NetCar::from_car`],
//! [`NetRun::from_run`]), so a car at rest compares equal and isn't sent, and the guide sees
//! exactly what the driver encoded. The guide regenerates the stage from `NetStage.id` (same
//! seed → same road) and checks `road_hash` against its own.

use bevy::prelude::*;
use engine::net::wire::{Put, Reader, WireResult, hash64, pack_quat, put_vec3_q, read_vec3_q, unpack_quat};
use engine::net::{AlwaysRelevant, Codec, NetAppExt, NetServer, Replicated};

use crate::drive::{DrivenCar, StageRun};
use crate::stage::Road;
use crate::vehicle::Car;
use crate::world::StageWorld;

/// Position step (m).
pub const POS_STEP: f32 = 0.01;
/// Velocity step (m/s).
pub const VEL_STEP: f32 = 0.05;
/// Steering step (rad): i8 covers ±1.0 rad (the car's lock is 0.56).
pub const STEER_STEP: f32 = 1.0 / 127.0;
/// Rpm step: u8 covers 0..10 200 rpm.
pub const RPM_STEP: f32 = 40.0;
/// Wheel spin step (rad/s).
pub const SPIN_STEP: f32 = 0.1;
/// Clock step (s).
pub const TIME_STEP: f32 = 0.001;
/// Progress step (m).
pub const PROGRESS_STEP: f32 = 0.1;
/// Run updates between events: every this many fixed ticks (the guide extrapolates the clock).
pub const RUN_EVERY: u32 = 6;

/// The stage the driver is on (0 = none yet, e.g. in a menu).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NetStage {
    pub id: u32,
    pub road_hash: u64,
    /// Weather and time of day the driver picked (indices into `meta::stages::WEATHERS` /
    /// `TIMES`): the guide's panel and ambience follow them.
    pub weather: u8,
    pub time: u8,
}

/// The car as the guide sees it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct NetCar {
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    /// Steering angle (rad, + = left).
    pub steer: f32,
    pub rpm: f32,
    /// 1..=5, −1 reverse.
    pub gear: i8,
    /// Wheel spin (rad/s): FL, FR, RL, RR.
    pub spin: [f32; 4],
    /// The driver's throttle (0..1, 1/255 steps), for the car sound on the guide's side.
    pub throttle: f32,
}

fn q(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

impl NetCar {
    /// Quantised copy of the simulation state.
    pub fn from_car(c: &Car, throttle: f32) -> NetCar {
        NetCar {
            pos: (c.pos / POS_STEP).round() * POS_STEP,
            rot: unpack_quat(pack_quat(c.rot.normalize())),
            vel: (c.vel / VEL_STEP).round() * VEL_STEP,
            steer: q(c.steer.clamp(-1.0, 1.0), STEER_STEP),
            rpm: q(c.rpm.clamp(0.0, 255.0 * RPM_STEP), RPM_STEP),
            gear: c.gear,
            spin: c.wheels.map(|w| q(w.spin, SPIN_STEP)),
            throttle: q(throttle.clamp(0.0, 1.0), 1.0 / 255.0),
        }
    }

    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::NEG_Z
    }

    /// Forward speed (m/s, negative reversing).
    pub fn speed(&self) -> f32 {
        self.vel.dot(self.forward())
    }

    /// Heading on the map (XZ unit vector).
    pub fn heading(&self) -> Vec2 {
        let f = self.forward();
        Vec2::new(f.x, f.z).normalize_or(Vec2::Y)
    }

    pub fn lerp(a: &NetCar, b: &NetCar, t: f32) -> NetCar {
        NetCar {
            pos: a.pos.lerp(b.pos, t),
            rot: a.rot.slerp(b.rot, t),
            vel: a.vel.lerp(b.vel, t),
            steer: a.steer + (b.steer - a.steer) * t,
            rpm: a.rpm + (b.rpm - a.rpm) * t,
            gear: b.gear,
            spin: std::array::from_fn(|i| a.spin[i] + (b.spin[i] - a.spin[i]) * t),
            throttle: a.throttle + (b.throttle - a.throttle) * t,
        }
    }
}

/// [`NetCar`] on the wire: pos 3 zigzag varints (cm), rot 4 bytes, vel 3 varints, steer,
/// rpm, gear 1 byte each, spin 4 varints, throttle 1 byte. ~31 bytes moving, before delta compression.
pub struct NetCarCodec;

impl Codec<NetCar> for NetCarCodec {
    fn encode(c: &NetCar, out: &mut Vec<u8>) {
        put_vec3_q(out, c.pos, POS_STEP);
        out.put_u32(pack_quat(c.rot));
        put_vec3_q(out, c.vel, VEL_STEP);
        out.put_u8((c.steer / STEER_STEP).round().clamp(-127.0, 127.0) as i8 as u8);
        out.put_u8((c.rpm / RPM_STEP).round().clamp(0.0, 255.0) as u8);
        out.put_u8(c.gear as u8);
        for s in c.spin {
            out.put_ivar((s / SPIN_STEP).round() as i64);
        }
        out.put_u8((c.throttle * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    fn decode(r: &mut Reader) -> WireResult<NetCar> {
        let pos = read_vec3_q(r, POS_STEP)?;
        let rot = unpack_quat(r.u32()?);
        let vel = read_vec3_q(r, VEL_STEP)?;
        let steer = f32::from(r.u8()? as i8) * STEER_STEP;
        let rpm = f32::from(r.u8()?) * RPM_STEP;
        let gear = r.u8()? as i8;
        let mut spin = [0.0; 4];
        for s in &mut spin {
            *s = r.ivar()? as f32 * SPIN_STEP;
        }
        let throttle = f32::from(r.u8()?) / 255.0;
        Ok(NetCar { pos, rot, vel, steer, rpm, gear, spin, throttle })
    }
}

/// The run as the guide sees it (see [`StageRun`]).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct NetRun {
    pub countdown: f32,
    pub time: f32,
    pub penalty: f32,
    pub progress: f32,
    /// The first `n_splits` are set.
    pub splits: [f32; 3],
    pub n_splits: u8,
    pub finished: Option<f32>,
}

impl NetRun {
    pub fn from_run(r: &StageRun) -> NetRun {
        let mut splits = [0.0; 3];
        for (d, s) in splits.iter_mut().zip(&r.splits) {
            *d = q(*s, TIME_STEP);
        }
        NetRun {
            countdown: q(r.countdown.max(0.0), 0.01),
            time: q(r.time, TIME_STEP),
            penalty: q(r.penalty, 0.1),
            progress: q(r.progress, PROGRESS_STEP),
            splits,
            n_splits: r.splits.len().min(3) as u8,
            finished: r.finished.map(|f| q(f, TIME_STEP)),
        }
    }

    pub fn splits(&self) -> &[f32] {
        &self.splits[..usize::from(self.n_splits.min(3))]
    }

    pub fn running(&self) -> bool {
        self.countdown <= 0.0 && self.finished.is_none()
    }

    /// Something the guide must see now (not just the clock ticking or the car rolling on).
    fn event_differs(&self, o: &NetRun) -> bool {
        self.n_splits != o.n_splits || self.finished != o.finished || self.penalty != o.penalty || (self.countdown > 0.0) != (o.countdown > 0.0) || (self.countdown.ceil() != o.countdown.ceil())
    }
}

/// [`NetRun`] on the wire: flags, then varints (cs, ms, ds, dm, ms...). 8–20 bytes.
pub struct NetRunCodec;

impl Codec<NetRun> for NetRunCodec {
    fn encode(r: &NetRun, out: &mut Vec<u8>) {
        let n = r.n_splits.min(3);
        out.put_u8(n | (u8::from(r.finished.is_some()) << 2));
        out.put_var((r.countdown / 0.01).round() as u64);
        out.put_var((r.time / TIME_STEP).round() as u64);
        out.put_var((r.penalty / 0.1).round() as u64);
        out.put_var((r.progress / PROGRESS_STEP).round() as u64);
        for s in &r.splits[..usize::from(n)] {
            out.put_var((s / TIME_STEP).round() as u64);
        }
        if let Some(f) = r.finished {
            out.put_var((f / TIME_STEP).round() as u64);
        }
    }
    fn decode(r: &mut Reader) -> WireResult<NetRun> {
        let flags = r.u8()?;
        let n = (flags & 3).min(3);
        let countdown = r.var()? as f32 * 0.01;
        let time = r.var()? as f32 * TIME_STEP;
        let penalty = r.var()? as f32 * 0.1;
        let progress = r.var()? as f32 * PROGRESS_STEP;
        let mut splits = [0.0; 3];
        for s in &mut splits[..usize::from(n)] {
            *s = r.var()? as f32 * TIME_STEP;
        }
        let finished = if flags & 4 != 0 { Some(r.var()? as f32 * TIME_STEP) } else { None };
        Ok(NetRun { countdown, time, penalty, progress, splits, n_splits: n, finished })
    }
}

/// A hash of the generated road (centreline in cm, widths, surfaces): equal on driver and
/// guide when both generated the same stage.
pub fn road_hash(road: &Road) -> u64 {
    let mut b = Vec::with_capacity(road.len() * 14);
    for i in 0..road.len() {
        let p = road.pts[i];
        for c in [p.x, p.y, p.z] {
            b.extend_from_slice(&((c * 100.0).round() as i32).to_le_bytes());
        }
        b.push((road.width[i] * 10.0).round() as u8);
        b.push(road.surface[i] as u8);
    }
    hash64(&b)
}

/// The driver-side session entity (stage + run).
#[derive(Component)]
pub struct SessionEntity;

/// Registers the replicated types (every role, same order: they're hashed into the protocol
/// id) and the driver's publishing systems.
pub fn register(app: &mut App) {
    app.replicate_with::<NetStage, engine::net::Postcard>()
        .replicate_with::<NetRun, NetRunCodec>()
        .replicate_with::<NetCar, NetCarCodec>()
        .interpolate::<NetCar>(NetCar::lerp)
        .add_systems(FixedPostUpdate, (publish_stage, publish_run, publish_car).run_if(resource_exists::<NetServer>));
}

fn publish_stage(mut commands: Commands, stage: Option<Res<StageWorld>>, mut q: Query<&mut NetStage, With<SessionEntity>>) {
    let Ok(mut ns) = q.single_mut() else {
        commands.spawn((Name::new("net session"), SessionEntity, Replicated, AlwaysRelevant, NetStage::default(), NetRun::default()));
        return;
    };
    match stage {
        Some(s) if s.is_changed() || ns.id != s.def.id => {
            let want = NetStage {
                id: s.def.id,
                road_hash: road_hash(&s.road),
                weather: crate::meta::stages::weather_index(s.def.weather),
                time: crate::meta::stages::time_index(s.def.time),
            };
            if *ns != want {
                info!("driver: sharing stage {} (road hash {:016x})", want.id, want.road_hash);
                *ns = want;
            }
        }
        None if ns.id != 0 => *ns = NetStage::default(),
        _ => {}
    }
}

fn publish_run(run: Option<Res<StageRun>>, mut q: Query<&mut NetRun, With<SessionEntity>>, mut tick: Local<u32>) {
    let (Some(run), Ok(mut nr)) = (run, q.single_mut()) else { return };
    *tick = tick.wrapping_add(1);
    let want = NetRun::from_run(&run);
    if *nr != want && (tick.is_multiple_of(RUN_EVERY) || want.event_differs(&nr)) {
        *nr = want;
    }
}

fn publish_car(mut commands: Commands, actions: Option<Res<engine::actions::ActionState>>, run: Option<Res<StageRun>>, mut cars: Query<(Entity, &DrivenCar, Option<&mut NetCar>)>) {
    // What the simulation got: nothing while held at the start or after the finish.
    let held = run.is_none_or(|r| r.countdown > 0.0 || r.finished.is_some());
    let throttle = if held { 0.0 } else { actions.map_or(0.0, |a| a.value("throttle")) };
    for (e, c, nc) in &mut cars {
        let want = NetCar::from_car(&c.car, throttle);
        match nc {
            Some(mut nc) => {
                nc.set_if_neq(want);
            }
            None => {
                commands.entity(e).insert((Replicated, AlwaysRelevant, want));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage;

    fn round_trip<T, K: Codec<T>>(v: &T) -> (T, usize) {
        let mut b = Vec::new();
        K::encode(v, &mut b);
        let mut r = Reader::new(&b);
        let out = K::decode(&mut r).unwrap();
        assert!(r.is_empty(), "codec left bytes");
        (out, b.len())
    }

    fn moving_car() -> Car {
        let mut c = Car::new(Vec3::new(-1234.567, 87.654, 1999.999), 2.3);
        c.vel = Vec3::new(21.37, -0.42, -14.88);
        c.steer = -0.4321;
        c.rpm = 6543.0;
        c.gear = 4;
        for (i, w) in c.wheels.iter_mut().enumerate() {
            w.spin = 80.0 + i as f32 * 1.234;
        }
        c
    }

    #[test]
    fn car_quantisation_is_within_steps_and_exact_on_the_wire() {
        let c = moving_car();
        let n = NetCar::from_car(&c, 0.6);
        assert!((n.pos - c.pos).abs().max_element() <= POS_STEP * 0.5 + 1e-3);
        assert!((n.vel - c.vel).abs().max_element() <= VEL_STEP * 0.5 + 1e-4);
        assert!(n.rot.angle_between(c.rot) < 0.006, "rotation error {}", n.rot.angle_between(c.rot));
        assert!((n.steer - c.steer).abs() <= STEER_STEP * 0.5 + 1e-6);
        assert!((n.rpm - c.rpm).abs() <= RPM_STEP * 0.5);
        assert_eq!(n.gear, 4);
        for i in 0..4 {
            assert!((n.spin[i] - c.wheels[i].spin).abs() <= SPIN_STEP * 0.5 + 1e-4);
        }
        // Quantised values survive the codec unchanged (to float rounding).
        let (d, len) = round_trip::<NetCar, NetCarCodec>(&n);
        assert!((d.pos - n.pos).abs().max_element() < 1e-3);
        assert!(d.rot.angle_between(n.rot) < 1e-3);
        assert!((d.vel - n.vel).abs().max_element() < 1e-4);
        assert_eq!((d.steer, d.rpm, d.gear), (n.steer, n.rpm, n.gear));
        assert!((d.throttle - 0.6).abs() < 0.5 / 255.0 + 1e-6);
        assert!(len <= 33, "NetCar is {len} bytes");
        // Re-quantising is a no-op: a parked car compares equal and isn't resent.
        assert_eq!(NetCar::from_car(&c, 0.6), n);
    }

    #[test]
    fn reverse_gear_and_negative_values_round_trip() {
        let mut c = moving_car();
        c.gear = -1;
        c.steer = 0.56;
        c.wheels[2].spin = -12.3;
        let (d, _) = round_trip::<NetCar, NetCarCodec>(&NetCar::from_car(&c, 0.0));
        assert_eq!(d.gear, -1);
        assert!((d.steer - 0.56).abs() < STEER_STEP);
        assert!((d.spin[2] + 12.3).abs() < SPIN_STEP);
        assert!(d.speed() < 0.0 || d.vel.length() > 0.0);
    }

    #[test]
    fn run_round_trips_with_splits_and_finish() {
        let r = StageRun { countdown: 0.0, time: 187.4567, penalty: 5.0, finished: Some(192.4567), progress: 3199.87, splits: vec![45.1234, 96.5, 140.0001], retired: false };
        let n = NetRun::from_run(&r);
        let (d, len) = round_trip::<NetRun, NetRunCodec>(&n);
        assert_eq!(d.n_splits, 3);
        assert!((d.time - r.time).abs() <= TIME_STEP);
        assert!((d.finished.unwrap() - 192.4567).abs() <= TIME_STEP);
        assert!((d.progress - r.progress).abs() <= PROGRESS_STEP);
        assert!((d.splits()[0] - 45.1234).abs() <= TIME_STEP);
        assert_eq!(d.penalty, 5.0);
        assert!(len <= 24, "NetRun is {len} bytes");
        // Countdown, nothing else.
        let (c, len) = round_trip::<NetRun, NetRunCodec>(&NetRun::from_run(&StageRun { countdown: 2.5, ..default() }));
        assert!((c.countdown - 2.5).abs() < 0.006 && c.n_splits == 0 && c.finished.is_none() && !c.running());
        assert!(len <= 7);
    }

    #[test]
    fn run_events_are_detected() {
        let a = NetRun::from_run(&StageRun { time: 10.0, ..default() });
        let mut b = a;
        b.time = 10.05;
        assert!(!b.event_differs(&a), "clock ticking is not an event");
        b.n_splits = 1;
        assert!(b.event_differs(&a));
    }

    #[test]
    fn road_hash_is_deterministic_and_per_stage() {
        let defs = stage::stages();
        let a = road_hash(&stage::generate_road(&defs[0]));
        assert_eq!(a, road_hash(&stage::generate_road(&defs[0])));
        assert_ne!(a, road_hash(&stage::generate_road(&defs[1])));
    }
}
