//! Anonymous gameplay stats (see `docs/TELEMETRY.md`): run starts and ends, crashes with where
//! on the stage they happened, recovers, demo wishlist clicks. Sent through the engine's tracer
//! (`engine::trace`, feature `telemetry`); without the feature, or with "Share anonymous stats"
//! off, nothing is recorded.

use bevy::prelude::*;
use engine::flow::AppState;

use crate::drive::{DrivenCar, StageRun};
use crate::world::StageWorld;

#[cfg(feature = "telemetry")]
pub use engine::trace::Prop;

/// Stand-in for the tracer's property type when telemetry isn't compiled in.
#[cfg(not(feature = "telemetry"))]
#[derive(Clone, Copy, Debug)]
pub enum Prop {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(&'static str),
}

/// Every game event (their props are only sent for declared names).
pub const EVENTS: [&str; 5] = ["run.start", "run.end", "run.crash", "run.recover", "demo.wishlist"];

/// Hits below this severity aren't reported (scrapes).
const CRASH_MIN: f32 = 0.04;

/// Records a game event (a no-op without the `telemetry` feature or an endpoint).
#[allow(unused_variables)]
pub fn event(world: &World, name: &'static str, props: &[(&'static str, Prop)]) {
    #[cfg(feature = "telemetry")]
    engine::trace::trace_event(world, name, props);
}

/// What the current run has reported so far.
#[derive(Resource, Default, Debug)]
struct Watch {
    stage: u32,
    running: bool,
    hits: u32,
    penalty: f32,
    recovers: u32,
    /// The run and the car's damage as of the last frame of the run (a restart or quitting
    /// resets them before the end is reported).
    last: Option<(StageRun, crate::vehicle::Damage)>,
}

pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Watch>()
            .add_systems(Update, watch.run_if(resource_exists::<StageWorld>))
            .add_systems(OnExit(AppState::InGame), quit.before(crate::meta::teardown))
            .add_systems(OnEnter(AppState::MainMenu), notice);
        #[cfg(feature = "telemetry")]
        {
            use engine::trace::TraceAppExt;
            app.declare_trace_events(&EVENTS).add_systems(Update, consent);
        }
    }
}

/// The corner at `s` metres (a little before its start to a little after its end), if any.
pub fn corner_at(stage: &StageWorld, s: f32) -> Option<usize> {
    stage.corners.iter().position(|c| s >= c.start - 25.0 && s <= c.end + 15.0)
}

fn kind(world: &World) -> (Prop, Prop, Prop) {
    let mode = match world.get_resource::<crate::meta::RunMode>() {
        Some(crate::meta::RunMode::Team) => "team",
        _ => "solo",
    };
    let codriver = world.get_resource::<crate::codriver::CoDriver>().is_some_and(|c| c.enabled);
    (Prop::Str(mode), Prop::Bool(codriver), Prop::Bool(crate::demo::ACTIVE))
}

fn end(world: &mut World, result: &'static str) {
    let w = world.resource::<Watch>();
    let (stage, recovers) = (w.stage, w.recovers);
    let Some((run, d)) = w.last.clone() else {
        world.resource_mut::<Watch>().running = false;
        return;
    };
    let (mode, codriver, demo) = kind(world);
    let pct = |v: f32| Prop::Int((v * 100.0).round() as i64);
    let time = run.finished.unwrap_or(run.time + run.penalty);
    event(
        world,
        "run.end",
        &[
            ("stage", Prop::Int(stage as i64)),
            ("result", Prop::Str(result)),
            ("time_ms", Prop::Int((time * 1000.0).round() as i64)),
            ("penalty_s", Prop::Int(run.penalty.round() as i64)),
            ("progress_m", Prop::Int(run.progress.round() as i64)),
            ("hits", Prop::Int(d.hits as i64)),
            ("engine_pct", pct(d.engine)),
            ("steering_pct", pct(d.steering)),
            ("suspension_pct", pct(d.suspension)),
            ("recovers", Prop::Int(recovers as i64)),
            ("codriver", codriver),
            ("mode", mode),
            ("demo", demo),
        ],
    );
    world.resource_mut::<Watch>().running = false;
}

/// Follows the run: start, crashes, recovers, end.
fn watch(world: &mut World) {
    if world.get_resource::<crate::net_session::Role>() == Some(&crate::net_session::Role::Guide) || crate::loading::is_loading(world) {
        return;
    }
    let run = world.resource::<StageRun>().clone();
    let stage = world.resource::<StageWorld>().clone();
    let mut q = world.query::<&DrivenCar>();
    let Some(car) = q.iter(world).next().map(|c| c.car.clone()) else { return };
    let w = world.resource::<Watch>();
    let (running, hits, penalty) = (w.running, w.hits, w.penalty);
    // Back on the line while a run was going: restarted.
    if running && run.countdown > 0.0 {
        end(world, "restart");
        return;
    }
    if !running {
        if run.countdown <= 0.0 && run.finished.is_none() && !run.retired && run.time < 1.0 {
            let (mode, codriver, demo) = kind(world);
            let view = match world.get_resource::<crate::drive::CameraView>() {
                Some(crate::drive::CameraView::Bonnet) => "bonnet",
                Some(crate::drive::CameraView::Chase) => "chase",
                _ => "cockpit",
            };
            event(world, "run.start", &[("stage", Prop::Int(stage.def.id as i64)), ("mode", mode), ("codriver", codriver), ("camera", Prop::Str(view)), ("demo", demo)]);
            *world.resource_mut::<Watch>() = Watch { stage: stage.def.id, running: true, hits: car.damage.hits, penalty: run.penalty, recovers: 0, last: Some((run.clone(), car.damage)) };
        }
        return;
    }
    world.resource_mut::<Watch>().last = Some((run.clone(), car.damage));
    let s = run.progress;
    if car.damage.hits > hits {
        world.resource_mut::<Watch>().hits = car.damage.hits;
        let d = car.damage;
        if d.last >= CRASH_MIN {
            let corner = corner_at(&stage, s);
            let grade = corner.map_or(0, |k| stage.corners[k].grade as i64);
            // Speed before the hit is gone by now; the speed this frame is what's left of it,
            // so report the severity's equivalent closing speed instead.
            let v = crate::vehicle::DAMAGE_FROM + d.last.sqrt() * (crate::vehicle::DAMAGE_FATAL - crate::vehicle::DAMAGE_FROM);
            event(
                world,
                "run.crash",
                &[
                    ("stage", Prop::Int(stage.def.id as i64)),
                    ("s_m", Prop::Int(s.round() as i64)),
                    ("corner", Prop::Int(corner.map_or(-1, |k| k as i64))),
                    ("grade", Prop::Int(grade)),
                    ("severity", Prop::Float((d.last as f64 * 1000.0).round() / 1000.0)),
                    ("speed_kmh", Prop::Int((v * 3.6).round() as i64)),
                    ("retired", Prop::Bool(d.wrecked())),
                ],
            );
        }
    }
    if run.penalty >= penalty + 4.9 {
        let mut w = world.resource_mut::<Watch>();
        w.penalty = run.penalty;
        w.recovers += 1;
        event(world, "run.recover", &[("stage", Prop::Int(stage.def.id as i64)), ("s_m", Prop::Int(s.round() as i64))]);
    }
    if run.finished.is_some() {
        end(world, "finish");
    } else if run.retired {
        end(world, "retired");
    }
}

/// Left the stage mid-run.
fn quit(world: &mut World) {
    if world.resource::<Watch>().running {
        end(world, "quit");
    }
}

/// Settings → Game → "Share anonymous stats" applied to the tracer (it saves the choice too).
#[cfg(feature = "telemetry")]
fn consent(profile: Option<Res<crate::meta::Profile>>, tracer: Option<Res<engine::trace::GameTracer>>, mut applied: Local<Option<bool>>) {
    // Headless and test apps have no tracer.
    let (Some(profile), Some(tracer)) = (profile, tracer) else { return };
    let want = profile.share_stats;
    if *applied == Some(want) {
        return;
    }
    *applied = Some(want);
    for c in [ashlar_trace::ConsentCategory::Usage, ashlar_trace::ConsentCategory::Performance, ashlar_trace::ConsentCategory::Crash] {
        tracer.0.set_consent(c, want);
    }
}

/// First launch: say what's shared and where to turn it off (once).
fn notice(world: &mut World) {
    #[cfg(feature = "telemetry")]
    {
        let on = world.get_resource::<engine::trace::GameTracer>().is_some_and(|t| t.0.is_enabled());
        let seen = world.get_resource::<crate::meta::Profile>().is_none_or(|p| p.stats_notice_seen);
        if on && !seen {
            world.resource_mut::<crate::meta::Profile>().stats_notice_seen = true;
            world.write_message(
                engine::ui::toast::Toast::new("Anonymous stats are on")
                    .body("Pacenote shares anonymous run stats (times, crashes, performance) to tune the stages. Turn it off in Settings > Game.")
                    .secs(12.0),
            );
        }
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = world;
}
