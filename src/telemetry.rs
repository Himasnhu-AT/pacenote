//! Anonymous gameplay stats and crash reports (see `docs/TELEMETRY.md`), sent through the
//! engine's tracer (`engine::trace`, ashlar-trace; feature `telemetry`).
//!
//! - [`CATALOG`]: every event, its props and, for string props, the fixed values allowed. Events
//!   are checked against it on every send in debug builds and tests; nothing else can be sent.
//!   Never a name, an address, a lobby code, a file path or any free text.
//! - Consent: "Share anonymous stats" → usage + performance, "Send crash reports" → crash
//!   (Settings > Privacy). "Forget me" there deletes local data, asks the server to delete the
//!   install's data and starts a new random id.
//! - The first-launch notice ([`NOTICE`] screen) says what is shared; nothing is sent until the
//!   player closes it with OK or opens the privacy settings from it (`notice_shown`).
//!   `DO_NOT_TRACK` and `TRACER_DISABLED` switch it all off (no notice then).
//! - Without the feature (dev builds without it) or without an endpoint, every call is a no-op.

use bevy::prelude::*;
use engine::flow::AppState;
use engine::ui::prelude::*;

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

/// What a prop may hold. Strings only ever come from a fixed list.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Bool,
    Int,
    Float,
    Str(&'static [&'static str]),
}

/// One declared event.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub props: &'static [(&'static str, Kind)],
}

const MODE: Kind = Kind::Str(&["solo", "team"]);
const CAMERA: Kind = Kind::Str(&["cockpit", "bonnet", "chase"]);
const WEATHER: Kind = Kind::Str(&["clear", "overcast", "fog", "rain"]);
const TIME_OF_DAY: Kind = Kind::Str(&["morning", "noon", "afternoon", "dusk"]);
const RESULT: Kind = Kind::Str(&["finish", "retired", "quit", "restart"]);
const WHERE: Kind = Kind::Str(&["menu", "end"]);
const STORE: Kind = Kind::Str(&["itch", "steam"]);
const UNITS: Kind = Kind::Str(&["metric", "imperial"]);
const TIMING: Kind = Kind::Str(&["early", "normal", "late"]);
const PACE: Kind = Kind::Str(&["calm", "normal", "fast"]);
const ORDER: Kind = Kind::Str(&["direction_first", "grade_first"]);
const CHANNEL: Kind = Kind::Str(&["demo", "full"]);
const QUALITY: Kind = Kind::Str(&["low", "medium", "high", "ultra"]);
const CONNECT: Kind = Kind::Str(&["ok", "failed", "timeout", "refused"]);
const ROLE: Kind = Kind::Str(&["host", "join"]);
const END_REASON: Kind = Kind::Str(&["left", "partner_left", "timeout", "error"]);

/// Every game event (category `game`). Keep docs/TELEMETRY.md's table in step.
pub const CATALOG: &[Spec] = &[
    Spec {
        name: "run.start",
        props: &[
            ("stage", Kind::Int),
            ("mode", MODE),
            ("codriver", Kind::Bool),
            ("camera", CAMERA),
            ("weather", WEATHER),
            ("time_of_day", TIME_OF_DAY),
            ("traction_assist", Kind::Bool),
            ("abs", Kind::Bool),
            ("demo", Kind::Bool),
        ],
    },
    Spec {
        name: "run.end",
        props: &[
            ("stage", Kind::Int),
            ("result", RESULT),
            ("time_ms", Kind::Int),
            ("penalty_s", Kind::Int),
            ("progress_m", Kind::Int),
            ("hits", Kind::Int),
            ("broken", Kind::Int),
            ("recovers", Kind::Int),
            ("codriver", Kind::Bool),
            ("mode", MODE),
            ("demo", Kind::Bool),
        ],
    },
    Spec {
        name: "run.crash",
        props: &[
            ("stage", Kind::Int),
            ("s_m", Kind::Int),
            ("corner", Kind::Int),
            ("grade", Kind::Int),
            ("severity", Kind::Float),
            ("speed_kmh", Kind::Int),
        ],
    },
    Spec { name: "run.recover", props: &[("stage", Kind::Int), ("s_m", Kind::Int)] },
    Spec {
        name: "session.settings",
        props: &[
            ("camera", CAMERA),
            ("units", UNITS),
            ("steering_speed", Kind::Float),
            ("traction_assist", Kind::Bool),
            ("abs", Kind::Bool),
            ("codriver", Kind::Bool),
            ("codriver_timing", TIMING),
            ("codriver_pace", PACE),
            ("call_order", ORDER),
            ("subtitles", Kind::Bool),
            ("quality", QUALITY),
            ("channel", CHANNEL),
        ],
    },
    Spec { name: "play_together.connect", props: &[("result", CONNECT), ("role", ROLE), ("latency_ms", Kind::Int)] },
    Spec { name: "play_together.end", props: &[("duration_s", Kind::Int), ("reason", END_REASON), ("role", ROLE)] },
    Spec { name: "demo.store_click", props: &[("where", WHERE), ("store", STORE)] },
];

/// The catalog's event names (declared to the tracer).
pub fn names() -> Vec<&'static str> {
    CATALOG.iter().map(|s| s.name).collect()
}

/// Is `name` with `props` allowed by [`CATALOG`]?
pub fn validate(name: &str, props: &[(&str, Prop)]) -> Result<(), String> {
    let spec = CATALOG.iter().find(|s| s.name == name).ok_or_else(|| format!("event `{name}` is not in the catalog"))?;
    for (k, v) in props {
        let (_, kind) = spec.props.iter().find(|(p, _)| p == k).ok_or_else(|| format!("`{name}`: prop `{k}` is not declared"))?;
        let ok = match (kind, v) {
            (Kind::Bool, Prop::Bool(_)) | (Kind::Int, Prop::Int(_)) | (Kind::Float, Prop::Float(_)) => true,
            (Kind::Str(allowed), Prop::Str(s)) => allowed.contains(s),
            _ => false,
        };
        if !ok {
            return Err(format!("`{name}`: prop `{k}` = {v:?} is not allowed"));
        }
    }
    Ok(())
}

/// Hits below this severity aren't reported (scrapes).
const CRASH_MIN: f32 = 0.04;

/// Records a game event (a no-op without the `telemetry` feature or an endpoint). In debug
/// builds and tests an event or value outside [`CATALOG`] panics, so it can't ship.
#[allow(unused_variables)]
pub fn event(world: &World, name: &'static str, props: &[(&'static str, Prop)]) {
    if let Err(e) = validate(name, props) {
        debug_assert!(false, "telemetry: {e}");
        return;
    }
    #[cfg(feature = "telemetry")]
    engine::trace::trace_event(world, name, props);
}

/// Reports an error the game caught and survived (`context`: a fixed label).
#[allow(unused_variables)]
pub fn handled(world: &World, error: &dyn std::fmt::Display, context: &'static str) {
    #[cfg(feature = "telemetry")]
    engine::trace::trace_handled(world, error, context);
}

/// Lower-case `snake_case`, dots allowed, starts with a letter, at most 64 bytes (the SDK's and
/// the server's rule for event and prop names).
pub fn valid_name(n: &str) -> bool {
    n.len() <= 64
        && n.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        && !n.contains("..")
        && !n.ends_with('.')
}

/// What the current run has reported so far.
#[derive(Resource, Default, Debug)]
struct Watch {
    stage: u32,
    running: bool,
    hits: u32,
    penalty: f32,
    recovers: u32,
    /// The run, the car's hits and what it knocked down as of the last frame of the run (a restart or quitting
    /// resets them before the end is reported).
    last: Option<(StageRun, crate::vehicle::Knocks, usize)>,
}

/// The privacy notice screen id.
pub const NOTICE: &str = "privacy";

pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Watch>()
            .add_systems(Update, watch.run_if(resource_exists::<StageWorld>))
            .add_systems(Update, (watch_play_together, session_settings, menu_events))
            .add_systems(OnExit(AppState::InGame), quit.before(crate::meta::teardown))
            .add_systems(OnEnter(AppState::MainMenu), show_notice)
            .register_screen(NOTICE, build_notice)
            .settings_panel("Privacy", privacy_panel);
        #[cfg(feature = "telemetry")]
        {
            use engine::trace::TraceAppExt;
            // We show our own notice and say when it's been seen.
            app.insert_resource(engine::trace::GameShowsTraceNotice).declare_trace_events(&names()).add_systems(Update, consent);
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

fn camera_name(world: &World) -> &'static str {
    match world.get_resource::<crate::drive::CameraView>() {
        Some(crate::drive::CameraView::Bonnet) => "bonnet",
        Some(crate::drive::CameraView::Chase) => "chase",
        _ => "cockpit",
    }
}

fn weather_name(w: crate::stage::Weather) -> &'static str {
    use crate::stage::Weather::*;
    match w {
        Clear => "clear",
        Overcast => "overcast",
        Fog => "fog",
        Rain => "rain",
    }
}

fn time_name(t: crate::stage::TimeOfDay) -> &'static str {
    use crate::stage::TimeOfDay::*;
    match t {
        Morning => "morning",
        Noon => "noon",
        Afternoon => "afternoon",
        Dusk => "dusk",
    }
}

fn end(world: &mut World, result: &'static str) {
    let w = world.resource::<Watch>();
    let (stage, recovers) = (w.stage, w.recovers);
    let Some((run, d, broken)) = w.last.clone() else {
        world.resource_mut::<Watch>().running = false;
        return;
    };
    let (mode, codriver, demo) = kind(world);
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
            ("hits", Prop::Int(d.count as i64)),
            ("broken", Prop::Int(broken as i64)),
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
            let a = crate::meta::run_assists(world);
            event(
                world,
                "run.start",
                &[
                    ("stage", Prop::Int(stage.def.id as i64)),
                    ("mode", mode),
                    ("codriver", codriver),
                    ("camera", Prop::Str(camera_name(world))),
                    ("weather", Prop::Str(weather_name(stage.def.weather))),
                    ("time_of_day", Prop::Str(time_name(stage.def.time))),
                    ("traction_assist", Prop::Bool(a.traction_control)),
                    ("abs", Prop::Bool(a.abs)),
                    ("demo", demo),
                ],
            );
            *world.resource_mut::<Watch>() = Watch { stage: stage.def.id, running: true, hits: car.knocks.count, penalty: run.penalty, recovers: 0, last: Some((run.clone(), car.knocks, car.broken.len())) };
        }
        return;
    }
    world.resource_mut::<Watch>().last = Some((run.clone(), car.knocks, car.broken.len()));
    let s = run.progress;
    if car.knocks.count > hits {
        world.resource_mut::<Watch>().hits = car.knocks.count;
        let d = car.knocks;
        if d.last >= CRASH_MIN {
            let corner = corner_at(&stage, s);
            let grade = corner.map_or(0, |k| stage.corners[k].grade as i64);
            // Speed before the hit is gone by now; the speed this frame is what's left of it,
            // so report the severity's equivalent closing speed instead.
            let v = crate::vehicle::Knocks::speed_of(d.last);
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

/// The settings in use, once per session (after the first frame).
fn session_settings(world: &mut World, mut sent: Local<bool>) {
    if *sent {
        return;
    }
    *sent = true;
    let Some(p) = world.get_resource::<crate::prefs::Prefs>().cloned() else { return };
    let quality = match world.get_resource::<engine::settings::CoreSettings>().map(|c| c.quality) {
        Some(engine::settings::GraphicsQuality::Low) => "low",
        Some(engine::settings::GraphicsQuality::Medium) => "medium",
        Some(engine::settings::GraphicsQuality::Ultra) => "ultra",
        _ => "high",
    };
    let camera = crate::meta::CAMERA_VIEWS[(world.get_resource::<crate::meta::Profile>().map_or(0, |p| p.camera) as usize).min(2)];
    let camera = match camera {
        crate::drive::CameraView::Cockpit => "cockpit",
        crate::drive::CameraView::Bonnet => "bonnet",
        crate::drive::CameraView::Chase => "chase",
    };
    event(
        world,
        "session.settings",
        &[
            ("camera", Prop::Str(camera)),
            ("units", Prop::Str(if p.mph { "imperial" } else { "metric" })),
            ("steering_speed", Prop::Float((p.steer_speed as f64 * 100.0).round() / 100.0)),
            ("traction_assist", Prop::Bool(p.traction_control)),
            ("abs", Prop::Bool(p.abs)),
            ("codriver", Prop::Bool(p.codriver)),
            ("codriver_timing", Prop::Str(["early", "normal", "late"][p.call_timing.min(2) as usize])),
            ("codriver_pace", Prop::Str(["calm", "normal", "fast"][p.codriver_pace.min(2) as usize])),
            ("call_order", Prop::Str(if p.call_order == 1 { "grade_first" } else { "direction_first" })),
            ("subtitles", Prop::Bool(p.subtitles)),
            ("quality", Prop::Str(quality)),
            ("channel", Prop::Str(if crate::demo::ACTIVE { "demo" } else { "full" })),
        ],
    );
}

/// Where a Play together session stands, for `play_together.*`.
#[derive(Default)]
struct Together {
    /// Role of the current attempt or session (`host` / `join`), None when solo.
    role: Option<&'static str>,
    /// The connect result was reported for this attempt.
    reported: bool,
    /// Real seconds when the peer connected (Some while connected).
    since: Option<f64>,
}

/// `play_together.connect` once per attempt (ok with the round trip, or why it failed) and
/// `play_together.end` with how long it lasted and why. No addresses, ids or names.
fn watch_play_together(world: &mut World, mut st: Local<Together>) {
    use crate::net_session::{Role, Session};
    use engine::net::DisconnectReason as R;
    let role = world.get_resource::<Role>().copied().unwrap_or_default();
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let Some(s) = world.get_resource::<Session>() else { return };
    let (connected, rtt, drop) = (s.peer_connected(), s.rtt_ms, s.last_drop);
    let want = match role {
        Role::Driver => Some("host"),
        Role::Guide => Some("join"),
        Role::Solo => None,
    };
    if want != st.role {
        // A new attempt (or back to solo): close what was open.
        if let (Some(r), Some(t)) = (st.role, st.since) {
            event(world, "play_together.end", &[("duration_s", Prop::Int((now - t).round() as i64)), ("reason", Prop::Str("left")), ("role", Prop::Str(r))]);
        }
        *st = Together { role: want, reported: false, since: None };
    }
    let Some(r) = st.role else { return };
    match (connected, st.since) {
        (true, None) => {
            st.since = Some(now);
        }
        (false, Some(t)) => {
            let reason = match drop {
                Some(R::Requested) => "left",
                Some(R::RemoteClosed | R::ServerShutdown) => "partner_left",
                Some(R::Timeout) => "timeout",
                _ => "error",
            };
            event(world, "play_together.end", &[("duration_s", Prop::Int((now - t).round() as i64)), ("reason", Prop::Str(reason)), ("role", Prop::Str(r))]);
            st.since = None;
        }
        _ => {}
    }
    if st.reported {
        return;
    }
    // Connected: report once the round trip is known (or after 3 s without one).
    if let Some(t) = st.since
        && (rtt > 0.0 || now - t > 3.0)
    {
        st.reported = true;
        let mut props = vec![("result", Prop::Str("ok")), ("role", Prop::Str(r))];
        if rtt > 0.0 {
            props.push(("latency_ms", Prop::Int(rtt.round() as i64)));
        }
        event(world, "play_together.connect", &props);
        return;
    }
    // The guide's first try failed (it keeps retrying; one report per attempt).
    if r == "join"
        && st.since.is_none()
        && let Some(d) = drop
        && !matches!(d, R::Requested)
    {
        st.reported = true;
        let result = match d {
            R::Timeout | R::ConnectTimeout => "timeout",
            R::ServerFull | R::ProtocolMismatch | R::Kicked => "refused",
            _ => "failed",
        };
        event(world, "play_together.connect", &[("result", Prop::Str(result)), ("role", Prop::Str(r))]);
    }
}

/// Hosting couldn't start (the port is taken, …).
pub fn host_failed(world: &World) {
    event(world, "play_together.connect", &[("result", Prop::Str("failed")), ("role", Prop::Str("host"))]);
}

/// Settings > Privacy: "Share anonymous stats" (usage + performance) and "Send crash reports"
/// (crash) applied to the tracer, which saves them.
#[cfg(feature = "telemetry")]
fn consent(profile: Option<Res<crate::meta::Profile>>, tracer: Option<Res<engine::trace::GameTracer>>, mut applied: Local<Option<(bool, bool)>>) {
    use ashlar_trace::ConsentCategory as C;
    // Headless and test apps have no tracer.
    let (Some(profile), Some(tracer)) = (profile, tracer) else { return };
    let want = (profile.share_stats, profile.send_crash_reports);
    if *applied == Some(want) {
        return;
    }
    *applied = Some(want);
    tracer.0.set_consent(C::Usage, want.0);
    tracer.0.set_consent(C::Performance, want.0);
    tracer.0.set_consent(C::Crash, want.1);
}

/// The tracer is on and waiting for the player to see the notice.
fn notice_pending(world: &World) -> bool {
    #[cfg(feature = "telemetry")]
    {
        world.get_resource::<engine::trace::GameTracer>().is_some_and(|t| t.0.is_enabled() && t.0.notice_pending())
    }
    #[cfg(not(feature = "telemetry"))]
    {
        let _ = world;
        false
    }
}

/// The player saw the notice: sending may start.
fn notice_seen(world: &mut World) {
    #[cfg(feature = "telemetry")]
    if let Some(t) = world.get_resource::<engine::trace::GameTracer>() {
        t.0.notice_shown();
    }
    if let Some(mut p) = world.get_resource_mut::<crate::meta::Profile>() {
        p.stats_notice_seen = true;
    }
}

/// "Forget me": local data deleted, the server asked to delete the install's data, new id.
fn forget(world: &mut World) {
    #[cfg(feature = "telemetry")]
    if let Some(t) = world.get_resource::<engine::trace::GameTracer>() {
        t.0.forget();
    }
    world.write_message(Toast::new("Done").body("Stats on this computer are deleted, the server is asked to delete what it has, and this install gets a new random id.").secs(7.0));
}

/// First launch (until the notice is closed): the privacy notice over the main menu.
fn show_notice(world: &mut World) {
    if notice_pending(world) {
        world.write_message(ScreenRequest::Push(NOTICE));
    }
}

fn menu_events(mut events: MessageReader<MenuEvent>, mut commands: Commands) {
    for e in events.read() {
        match e.0 {
            "privacy_ok" => commands.queue(|world: &mut World| {
                notice_seen(world);
                world.write_message(ScreenRequest::Pop);
            }),
            "privacy_settings" => commands.queue(|world: &mut World| {
                notice_seen(world);
                world.write_message(ScreenRequest::Replace(Some("main")));
                world.write_message(ScreenRequest::Push("settings"));
                let tab = crate::prefs::TABS.iter().position(|t| *t == "Privacy").unwrap_or(0);
                world.write_message(ScreenRequest::SetTab(tab));
            }),
            "privacy_forget" => commands.queue(forget),
            _ => {}
        }
    }
}

const NOTICE_LINES: [&str; 4] = [
    "To tune the stages, Pacenote shares anonymous stats: run times and results, where on each stage cars crash, the settings in use, and performance (frame rate, loading time).",
    "If the game crashes or freezes, a crash report says where in the code, with the device (OS, CPU, graphics card).",
    "Never your names, chat, voice, IP-based location or anything you type. Ids are random and can be reset with \"Forget me\".",
    "Nothing has been sent yet. Turn either part off in Settings > Privacy. DO_NOT_TRACK=1 or TRACER_DISABLED=1 in the environment turns it off completely.",
];

fn build_notice(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(root).with_children(|r| {
        menu_backdrop(r, &t, 0.65, 1.0);
        r.spawn(menu_column(GAP_M)).with_children(|c| {
            eyebrow_bar(c, "Privacy", t.accent);
            c.spawn(title("Anonymous stats", SIZE_H1, t.text));
            c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(GAP_S), max_width: px(760), ..default() }, Pickable::IGNORE)).with_children(|b| {
                for l in NOTICE_LINES {
                    b.spawn(text(l, body(SIZE_BODY), t.text_muted));
                }
            });
            c.spawn(spacer(GAP_S));
            c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|b| {
                action_button(b, &t, "OK", MenuAction::Custom("privacy_ok"), Some(1), true, 220.0);
                action_button(b, &t, "Privacy settings", MenuAction::Custom("privacy_settings"), Some(2), false, 300.0);
            });
        });
        spawn_footer(r, &t, &[("Enter", "Select")]);
    });
}

/// Settings > Privacy side panel: what's shared, the opt-outs, and "Forget me".
fn privacy_panel(world: &mut World, col: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(col).with_children(|c| {
        c.spawn(panel(&t, 20.0, GAP_S)).with_children(|p| {
            p.spawn(eyebrow("Your data", t.accent));
            for l in [
                "Never sent: names, chat, voice, addresses, anything you type.",
                "DO_NOT_TRACK=1 or TRACER_DISABLED=1 in the environment turns all of it off.",
                "Forget me deletes what's on this computer, asks the server to delete what it has, and starts a new random id.",
            ] {
                p.spawn(text(l, body(SIZE_SMALL + 1.0), t.text_muted));
            }
            p.spawn(spacer(GAP_XS));
            action_button(p, &t, "Forget me", MenuAction::Custom("privacy_forget"), Some(90), false, 240.0);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_names_and_props_are_valid_and_fixed() {
        let mut seen = std::collections::HashSet::new();
        for s in CATALOG {
            assert!(valid_name(s.name), "event name {}", s.name);
            assert!(seen.insert(s.name), "{} declared twice", s.name);
            assert!(s.props.len() <= 16, "{}: at most 16 props", s.name);
            let mut ps = std::collections::HashSet::new();
            for (p, k) in s.props {
                assert!(valid_name(p), "{}: prop {p}", s.name);
                assert!(ps.insert(*p), "{}: prop {p} twice", s.name);
                if let Kind::Str(values) = k {
                    assert!(!values.is_empty());
                    for v in *values {
                        assert!(v.len() <= 64 && valid_name(v), "{}: {p} value {v:?} must be a short fixed id", s.name);
                    }
                }
            }
        }
    }

    #[test]
    fn only_declared_events_and_fixed_values_pass() {
        assert!(validate("run.recover", &[("stage", Prop::Int(3)), ("s_m", Prop::Int(120))]).is_ok());
        assert!(validate("run.teleport", &[]).is_err(), "undeclared event");
        assert!(validate("run.recover", &[("driver_name", Prop::Int(1))]).is_err(), "undeclared prop");
        assert!(validate("run.start", &[("weather", Prop::Str("sleet"))]).is_err(), "value outside the fixed list");
        assert!(validate("run.start", &[("weather", Prop::Int(1))]).is_err(), "wrong type");
        assert!(validate("demo.store_click", &[("where", Prop::Str("menu")), ("store", Prop::Str("itch"))]).is_ok());
    }

    #[test]
    fn name_rule() {
        assert!(valid_name("play_together.connect"));
        assert!(!valid_name("Run.start"));
        assert!(!valid_name("run start"));
        assert!(!valid_name("1run"));
        assert!(!valid_name(&"a".repeat(65)));
    }
}
