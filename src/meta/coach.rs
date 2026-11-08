//! What the driver should press, and what to do next, on the driving screen:
//!
//! - [`Notice`]: one short pill bottom centre ("C · Chase camera"). A new notice replaces the
//!   one showing, so quick changes never stack up (the engine's toasts do).
//! - The controls card, top left: the driving keys (or the gamepad's buttons, whichever was
//!   used last) and how events start. It shows by itself for the first few free-roam drives
//!   ([`super::Profile::controls_hint_runs`]) and H shows / hides it any time.
//! - The event prompt: inside an event's start marker, its name, kind and length and
//!   "Stop here to start", with a bar that fills while the car stands still.
//! - The roof hint: the car on its roof or side and not moving says how to recover.
//!
//! None of this owns game state: it reads the run, the car and the events.

use bevy::prelude::*;
use engine::actions::{ActionState, Device};
use engine::flow::AppState;
use engine::ui::prelude::*;

use crate::drive::{DrivenCar, StageRun};
use crate::open::events::{Approach, EventKind, Events};
use crate::world::StageWorld;

/// Free-roam drives that open with the controls card.
pub const CONTROLS_AUTO_RUNS: u32 = 3;
/// How long the card stays when it opens by itself (s).
const CONTROLS_SECS: f32 = 16.0;
/// Stopped in a marker this long starts the event (as `open::events` does).
const START_HOLD_S: f32 = 0.6;
/// The car counts as on its roof or side below this `up.y`, slower than [`ROOF_SPEED`] m/s,
/// for [`ROOF_SECS`].
const ROOF_UP: f32 = 0.35;
const ROOF_SPEED: f32 = 2.0;
const ROOF_SECS: f32 = 1.0;

/// A short message bottom centre. The newest replaces the one showing.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct Notice {
    /// A key cap in front of the text ("C"), if any.
    pub key: Option<String>,
    pub text: String,
    pub secs: f32,
}

impl Notice {
    pub fn new(text: impl Into<String>) -> Self {
        Notice { key: None, text: text.into(), secs: 2.0 }
    }
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }
    pub fn secs(mut self, s: f32) -> Self {
        self.secs = s.max(0.5);
        self
    }
}

#[derive(Component)]
struct NoticePill {
    age: f32,
    secs: f32,
}

/// The controls card: how long it has shown, how long it may (None = until H), and the device
/// it was drawn for.
#[derive(Component)]
struct ControlsCard {
    age: f32,
    limit: Option<f32>,
    device: Device,
    free_roam: bool,
}

#[derive(Component)]
struct EventPrompt(u32);

#[derive(Component)]
struct PromptFill;

#[derive(Component)]
struct PromptLine;

#[derive(Component)]
struct RoofHint;

pub struct CoachPlugin;

impl Plugin for CoachPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Notice>().add_systems(
            Update,
            (notices, controls_card, event_prompt, roof_hint).run_if(in_state(AppState::InGame)).after(engine::ui::UiSet::Paint),
        );
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

#[cfg(feature = "dev")]
mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    pub fn register(app: &mut App) {
        // For screenshots of the prompts: put the car somewhere (an event marker), or on its roof.
        app.add_control_command(
            "coach.place",
            "Put the car at x, z or 6 m short of an event's start (stopped), optionally on its roof",
            params(&[("x", "number", "world x"), ("z", "number", "world z"), ("event", "integer", "event id (events.list)"), ("roof", "boolean", "upside down")]),
            |w, p: Value| {
                let at = match p.get("event").and_then(Value::as_u64) {
                    Some(id) => {
                        let ev = w.get_resource::<crate::open::events::Events>().ok_or("no events")?;
                        let e = ev.0.iter().find(|e| e.id == id as u32).ok_or("no such event")?;
                        let d = e.route.dir[0];
                        Vec2::new(e.route.pts[0].x, e.route.pts[0].z) - Vec2::new(d.x, d.y) * 6.0
                    }
                    None => Vec2::new(
                        p.get("x").and_then(Value::as_f64).ok_or("`x` or `event` is required")? as f32,
                        p.get("z").and_then(Value::as_f64).ok_or("`z` is required")? as f32,
                    ),
                };
                let (x, z) = (at.x, at.y);
                let roof = p.get("roof").and_then(Value::as_bool).unwrap_or(false);
                let y = crate::vehicle::Ground::height(w.get_resource::<crate::world::StageWorld>().ok_or("no stage")?, x, z);
                let mut q = w.query::<&mut crate::drive::DrivenCar>();
                let mut c = q.single_mut(w).map_err(|e| e.to_string())?;
                c.car.pos = Vec3::new(x, y + if roof { 1.6 } else { 0.6 }, z);
                c.car.rot = if roof { Quat::from_rotation_z(std::f32::consts::PI) } else { Quat::IDENTITY };
                c.car.vel = Vec3::ZERO;
                c.car.ang = Vec3::ZERO;
                c.prev = (c.car.pos, c.car.rot);
                Ok(json!({"at": [x, y, z]}))
            },
        );
    }
}

/// A screen (pause, results, settings) is open over the game.
fn screen_open(screens: &Option<Res<Screens>>) -> bool {
    screens.as_ref().is_some_and(|s| s.top().is_some())
}

/// Not for the guide (no car to drive) or while loading.
fn driver_view(world_ready: bool, role: &Option<Res<super::Role>>, loading: &Option<Res<crate::loading::Loading>>) -> bool {
    world_ready && loading.is_none() && role.as_ref().is_none_or(|r| **r != super::Role::Guide)
}

/// A centred absolute row at `bottom` px, for a pill or card.
fn centre_row(bottom: f32) -> impl Bundle {
    (
        Node { position_type: PositionType::Absolute, bottom: px(bottom), left: px(0), right: px(0), justify_content: JustifyContent::Center, ..default() },
        crate::hud::SafeArea::BOTTOM,
        Pickable::IGNORE,
    )
}

fn notices(mut commands: Commands, theme: Res<Theme>, time: Res<Time<Real>>, mut msgs: MessageReader<Notice>, mut pills: Query<(Entity, &mut NoticePill)>) {
    if let Some(n) = msgs.read().last().cloned() {
        for (e, _) in &pills {
            commands.entity(e).despawn();
        }
        let t = &*theme;
        commands
            .spawn((NoticePill { age: 0.0, secs: n.secs }, Name::new("Notice"), DespawnOnExit(AppState::InGame), centre_row(176.0)))
            .with_children(|row| {
                row.spawn((
                    Node { column_gap: px(GAP_S + 2.0), align_items: AlignItems::Center, padding: UiRect::axes(px(16), px(9)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                    BackgroundColor(t.panel.with_alpha(0.86)),
                    Pickable::IGNORE,
                ))
                .with_children(|p| {
                    if let Some(k) = &n.key {
                        p.spawn(key_cap(t, k));
                    }
                    p.spawn(text(n.text.to_uppercase(), label(SIZE_LABEL + 2.0), t.text));
                });
            });
        return;
    }
    let dt = time.delta_secs();
    for (e, mut p) in &mut pills {
        p.age += dt;
        if p.age >= p.secs {
            commands.entity(e).despawn();
        }
    }
}

/// The controls for `device`: (keys, what they do).
pub(crate) fn controls(device: Device) -> &'static [(&'static str, &'static str)] {
    match device {
        Device::Gamepad => &[("RT / LT", "Throttle / brake, reverse"), ("L stick", "Steer"), ("A", "Handbrake"), ("Y", "Recover onto the road"), ("Select", "Camera view"), ("Start", "Pause")],
        _ => &[("W / S", "Throttle / brake, reverse"), ("A / D", "Steer"), ("Space", "Handbrake"), ("R", "Recover onto the road"), ("C", "Camera view"), ("Esc", "Pause")],
    }
}

#[allow(clippy::too_many_arguments)]
fn controls_card(
    mut commands: Commands,
    theme: Res<Theme>,
    time: Res<Time<Real>>,
    stage: Option<Res<StageWorld>>,
    role: Option<Res<super::Role>>,
    loading: Option<Res<crate::loading::Loading>>,
    screens: Option<Res<Screens>>,
    actions: Option<Res<ActionState>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut profile: ResMut<super::Profile>,
    mut save: MessageWriter<engine::settings::SaveSettings>,
    mut was_loading: Local<bool>,
    mut cards: Query<(Entity, &mut ControlsCard, &mut Visibility)>,
    race_field: Option<Res<crate::career::racers::RaceField>>,
) {
    let race = race_field.is_some();
    let loaded = std::mem::replace(&mut *was_loading, loading.is_some()) && loading.is_none();
    if !driver_view(stage.is_some(), &role, &loading) {
        return;
    }
    let Some(stage) = stage else { return };
    let free_roam = stage.free_roam();
    let device = actions.as_ref().map_or(Device::KeyboardMouse, |a| a.last_device);
    let toggle = !screen_open(&screens) && keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::KeyH));
    // What to do with the card: keep, drop, or (re)draw it with a time limit.
    let mut want: Option<Option<f32>> = None;
    let mut drop = false;
    // Never over the race position block (UX_PASS §3.3).
    if race {
        for (e, ..) in &cards {
            commands.entity(e).despawn();
        }
        return;
    }
    match cards.single_mut() {
        Ok((_, mut c, mut vis)) => {
            c.age += time.delta_secs();
            let hide = screen_open(&screens);
            let v = if hide { Visibility::Hidden } else { Visibility::Inherited };
            if *vis != v {
                *vis = v;
            }
            // An event starting takes the first-drive card away (the countdown needs the screen);
            // one opened with H stays, redrawn without the free-roam line.
            let auto_into_event = c.limit.is_some() && c.free_roam && !free_roam;
            if toggle || auto_into_event || c.limit.is_some_and(|l| c.age >= l) {
                drop = true;
            } else if c.device != device || c.free_roam != free_roam {
                drop = true;
                want = Some(c.limit.map(|l| (l - c.age).max(1.0)));
            }
        }
        Err(_) => {
            if toggle {
                want = Some(None);
            } else if loaded && free_roam && profile.controls_hint_runs < CONTROLS_AUTO_RUNS {
                profile.controls_hint_runs += 1;
                save.write(engine::settings::SaveSettings);
                want = Some(Some(CONTROLS_SECS));
            }
        }
    }
    if drop {
        for (e, ..) in &cards {
            commands.entity(e).despawn();
        }
    }
    let Some(limit) = want else { return };
    let t = &*theme;
    commands
        .spawn((
            ControlsCard { age: 0.0, limit, device, free_roam },
            Name::new("Controls card"),
            crate::hud::SafeArea::TOP_LEFT,
            DespawnOnExit(AppState::InGame),
            Node { position_type: PositionType::Absolute, left: px(40), top: px(40), ..default() },
            Visibility::Inherited,
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(accent_panel(t, t.accent, 18.0, GAP_S)).with_children(|c| {
                c.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: px(GAP_L), ..default() }, Pickable::IGNORE)).with_children(|h| {
                    h.spawn(eyebrow("Controls", t.accent));
                    if device == Device::KeyboardMouse {
                        key_hint(h, t, "H", "Hide");
                    }
                });
                for (k, what) in controls(device) {
                    c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                        r.spawn((Node { width: px(72), ..default() }, Pickable::IGNORE)).with_children(|w| {
                            w.spawn(key_cap(t, k));
                        });
                        r.spawn(text(*what, body(SIZE_BODY - 2.0), t.text));
                    });
                }
                if free_roam {
                    c.spawn(rule(t.line));
                    c.spawn((text("Events: drive into a glowing column and stop inside it to start.", body(SIZE_SMALL), t.text_muted), Node { max_width: px(330), ..default() }));
                }
            });
        });
}

fn kind_color(k: EventKind) -> Color {
    match k {
        EventKind::Rally => Color::srgb(1.0, 0.62, 0.15),
        EventKind::Sprint => Color::srgb(0.45, 0.78, 1.0),
        EventKind::Lap => Color::srgb(0.78, 0.55, 1.0),
    }
}

/// "Stop here to start", or "Starting…" while the car stands still.
fn prompt_line(still: f32) -> &'static str {
    if still > 0.05 { "Hold still to start\u{2026}" } else { "Stop here to start" }
}

#[allow(clippy::too_many_arguments)]
fn event_prompt(
    mut commands: Commands,
    theme: Res<Theme>,
    stage: Option<Res<StageWorld>>,
    role: Option<Res<super::Role>>,
    loading: Option<Res<crate::loading::Loading>>,
    screens: Option<Res<Screens>>,
    approach: Option<Res<Approach>>,
    events: Option<Res<Events>>,
    prompts: Query<(Entity, &EventPrompt)>,
    mut fill: Query<&mut Node, With<PromptFill>>,
    mut line: Query<&mut Text, With<PromptLine>>,
) {
    let inside = (driver_view(stage.is_some(), &role, &loading) && stage.as_ref().is_some_and(|s| s.free_roam()) && !screen_open(&screens))
        .then(|| approach.as_ref().and_then(|a| a.inside.map(|id| (id, a.still.max(0.0)))))
        .flatten();
    let shown = prompts.iter().next().map(|(_, p)| p.0);
    if shown.is_some() && shown != inside.map(|i| i.0) {
        for (e, _) in &prompts {
            commands.entity(e).despawn();
        }
    }
    let Some((id, still)) = inside else { return };
    if shown == Some(id) {
        let f = (still / START_HOLD_S).clamp(0.0, 1.0) * 100.0;
        for mut n in &mut fill {
            if n.width != percent(f) {
                n.width = percent(f);
            }
        }
        let want = prompt_line(still);
        for mut t in &mut line {
            if t.0 != want {
                t.0 = want.into();
            }
        }
        return;
    }
    let Some(e) = events.as_ref().and_then(|ev| ev.0.iter().find(|e| e.id == id)) else { return };
    let t = &*theme;
    let kc = kind_color(e.kind);
    let facts = format!("{} \u{b7} {:.1} km", e.kind.label(), e.route.length() / 1000.0);
    commands.spawn((EventPrompt(id), Name::new("Event prompt"), DespawnOnExit(AppState::InGame), centre_row(236.0))).with_children(|row| {
        row.spawn(accent_panel(t, kc, 18.0, GAP_XS + 2.0)).with_children(|c| {
            c.spawn(eyebrow(&facts, kc));
            c.spawn(text(e.name.to_uppercase(), display(44.0), t.text));
            c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((PromptLine, text(prompt_line(0.0), body_bold(SIZE_BODY), t.accent)));
            });
            c.spawn((Node { width: px(340), height: px(4), ..default() }, BackgroundColor(t.line_strong), Pickable::IGNORE)).with_children(|b| {
                b.spawn((PromptFill, Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
            });
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn roof_hint(
    mut commands: Commands,
    theme: Res<Theme>,
    time: Res<Time>,
    stage: Option<Res<StageWorld>>,
    run: Option<Res<StageRun>>,
    role: Option<Res<super::Role>>,
    loading: Option<Res<crate::loading::Loading>>,
    screens: Option<Res<Screens>>,
    actions: Option<Res<ActionState>>,
    cars: Query<&DrivenCar>,
    hints: Query<Entity, With<RoofHint>>,
    mut stuck: Local<f32>,
) {
    let car = cars.iter().next();
    let ok = driver_view(stage.is_some(), &role, &loading) && !screen_open(&screens);
    let run_on = run.as_ref().is_some_and(|r| r.countdown <= 0.0 && r.finished.is_none() && !r.retired && r.down.is_none());
    let upside = car.is_some_and(|c| c.car.up().y < ROOF_UP && c.car.vel.length() < ROOF_SPEED);
    *stuck = if ok && run_on && upside { *stuck + time.delta_secs() } else { 0.0 };
    let want = *stuck >= ROOF_SECS;
    let have = !hints.is_empty();
    if want == have {
        return;
    }
    if !want {
        for e in &hints {
            commands.entity(e).despawn();
        }
        return;
    }
    let t = &*theme;
    let route = stage.as_ref().is_some_and(|s| !s.free_roam());
    let key = if actions.as_ref().is_some_and(|a| a.last_device == Device::Gamepad) { "Y" } else { "R" };
    let what = if route { "Recover onto the road (+5 s)" } else { "Recover onto the road" };
    commands
        .spawn((
            RoofHint,
            Name::new("Roof hint"),
            DespawnOnExit(AppState::InGame),
            Node { position_type: PositionType::Absolute, top: percent(30), left: px(0), right: px(0), justify_content: JustifyContent::Center, ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            row.spawn(accent_panel(t, t.accent, 22.0, GAP_S)).with_children(|c| {
                c.spawn(eyebrow("Stuck", t.accent));
                c.spawn(title("Car on its roof", SIZE_H2, t.text));
                c.spawn((Node { column_gap: px(GAP_L), ..default() }, Pickable::IGNORE)).with_children(|k| {
                    key_hint(k, t, key, what);
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_list_both_devices_with_recover_and_camera() {
        for d in [Device::KeyboardMouse, Device::Gamepad] {
            let what: Vec<&str> = controls(d).iter().map(|(_, w)| *w).collect();
            assert!(what.contains(&"Recover onto the road"), "{d:?}");
            assert!(what.contains(&"Camera view"), "{d:?}");
            assert!(what.contains(&"Pause"), "{d:?}");
        }
        assert_eq!(controls(Device::KeyboardMouse)[3].0, "R");
        assert_eq!(controls(Device::Gamepad)[3].0, "Y");
    }

    #[test]
    fn prompt_says_stop_then_hold() {
        assert_eq!(prompt_line(0.0), "Stop here to start");
        assert!(prompt_line(0.3).starts_with("Hold still"));
    }
}
