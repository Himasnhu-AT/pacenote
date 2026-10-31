//! The AI co-driver: when no human guide is in the car, reads the pace notes out loud ahead of
//! the car, and reports damage, splits, the finish, going off the road and rolling over. Lines
//! are spoken with the system's text-to-speech ([`speech`]) and shown as a subtitle.
//!
//! On by default for solo runs; quiet while a human guide is connected (they read the notes),
//! and `N` toggles it.

pub mod calls;
pub mod speech;

use bevy::prelude::*;
use engine::actions::{ActionDef, ActionState, AddAction, Binding};
use engine::flow::AppState;
use engine::pause::Paused;

use crate::drive::{DrivenCar, StageRun};
use crate::net_session::{Role, Session};
use crate::vehicle::Damage;
use crate::world::StageWorld;
use calls::{Line, Planner, Priority};
use speech::Speech;

/// Seconds a subtitle stays after its line starts.
const SUBTITLE_S: f32 = 3.0;

/// The co-driver's state.
#[derive(Resource)]
pub struct CoDriver {
    /// The player wants it (toggled with `N`; Settings > Co-driver > Co-driver voice).
    pub enabled: bool,
    /// Call timing: read-ahead × this (Settings > Co-driver).
    pub lead_scale: f32,
    /// "Three left" rather than "left three".
    pub grade_first: bool,
    /// Show what's said at the top of the screen.
    pub subtitles: bool,
    speech: Speech,
    planner: Option<Planner>,
    /// Which stage the planner is for.
    stage: u32,
    last_damage: Damage,
    last_progress: f32,
    said_ready: bool,
    splits_said: usize,
    finish_said: bool,
    off_road: f32,
    on_roof: f32,
    /// Seconds until the next off-road / roof warning may be said.
    nag_cooldown: f32,
    subtitle: Option<(String, f32)>,
}

impl Default for CoDriver {
    fn default() -> Self {
        CoDriver {
            enabled: true,
            lead_scale: 1.0,
            grade_first: false,
            subtitles: true,
            speech: Speech::default(),
            planner: None,
            stage: 0,
            last_damage: Damage::default(),
            last_progress: 0.0,
            said_ready: false,
            splits_said: 0,
            finish_said: false,
            off_road: 0.0,
            on_roof: 0.0,
            nag_cooldown: 0.0,
            subtitle: None,
        }
    }
}

impl CoDriver {
    /// The co-driver's voice is speaking now (the car audio ducks under it).
    pub fn speaking(&self) -> bool {
        self.speech.speaking()
    }

    /// Forget the run (new stage, restart): notes from `s`, nothing pending.
    fn reset(&mut self, s: f32) {
        self.speech.hush();
        if let Some(p) = &mut self.planner {
            p.seek(s);
        }
        self.said_ready = false;
        self.splits_said = 0;
        self.finish_said = false;
        self.last_damage = Damage::default();
        self.off_road = 0.0;
        self.on_roof = 0.0;
        self.subtitle = None;
    }

    fn say(&mut self, line: Line) {
        self.speech.say(line);
    }

    /// Voice speed (words per minute) and volume (0..1).
    pub fn set_voice(&mut self, rate: u32, volume: f32) {
        self.speech.rate = rate;
        self.speech.volume = volume;
    }
}

#[derive(Component)]
struct Subtitle;

/// The subtitle's plate (shown while a line is up).
#[derive(Component)]
struct SubtitlePlate;

pub struct CoDriverPlugin;

impl Plugin for CoDriverPlugin {
    fn build(&self, app: &mut App) {
        let quiet = engine::cli::dev_flag("mute") || (engine::cli::test_driven() && !engine::cli::dev_flag("sound"));
        let mut cd = CoDriver::default();
        cd.speech.audible = !quiet;
        app.insert_resource(cd)
            .add_action(ActionDef::button("codriver").label("Co-driver on / off").group("Driving").bind(Binding::key(KeyCode::KeyN)))
            .add_systems(Update, (toggle, talk, subtitle).chain().run_if(resource_exists::<StageWorld>))
            .add_systems(OnExit(AppState::InGame), stop);
    }
}

/// Speaking at all: the player's choice, no human guide reading, not the guide's own machine.
fn active(cd: &CoDriver, role: Option<&Role>, session: Option<&Session>) -> bool {
    match role {
        Some(Role::Guide) => false,
        Some(Role::Driver) => cd.enabled && session.is_none_or(|s| s.guide.is_none()),
        _ => cd.enabled,
    }
}

fn toggle(actions: Res<ActionState>, mut cd: ResMut<CoDriver>) {
    if !actions.just_pressed("codriver") {
        return;
    }
    cd.enabled = !cd.enabled;
    cd.speech.hush();
    let text = if cd.enabled { "Co-driver on" } else { "Co-driver off" };
    if cd.enabled {
        cd.say(Line::new("Co-driver on.", Priority::Urgent));
    }
    cd.subtitle = Some((text.into(), SUBTITLE_S));
}

#[allow(clippy::too_many_arguments)]
fn talk(
    time: Res<Time>,
    stage: Res<StageWorld>,
    run: Res<StageRun>,
    cars: Query<&DrivenCar>,
    paused: Option<Res<Paused>>,
    loading: Option<Res<crate::loading::Loading>>,
    role: Option<Res<Role>>,
    session: Option<Res<Session>>,
    mut cd: ResMut<CoDriver>,
) {
    let cd = &mut *cd;
    let dt = time.delta_secs();
    if let Some(sub) = &mut cd.subtitle {
        sub.1 -= dt;
        if sub.1 <= 0.0 {
            cd.subtitle = None;
        }
    }
    if cd.planner.is_none() || cd.stage != stage.def.id {
        cd.planner = Some(Planner::new(&stage.corners, &stage.crests, stage.road.length()));
        cd.stage = stage.def.id;
        cd.reset(0.0);
    }
    if let Some(p) = &mut cd.planner {
        p.lead_scale = cd.lead_scale;
        p.grade_first = cd.grade_first;
    }
    if !active(cd, role.as_deref(), session.as_deref()) || paused.is_some_and(|p| p.0) || loading.is_some() {
        cd.speech.hush();
        return;
    }
    let Ok(c) = cars.single() else { return };
    let car = &c.car;
    let s = run.progress;
    // A restart (or the stage again): back to the start of the notes.
    if s + 50.0 < cd.last_progress || (run.countdown > 2.9 && cd.said_ready) {
        cd.reset(s);
    }
    cd.last_progress = s;
    cd.nag_cooldown = (cd.nag_cooldown - dt).max(0.0);

    if run.countdown > 0.0 {
        // Read the first note on the line, as a co-driver would.
        if !cd.said_ready && run.countdown < 2.6 {
            cd.said_ready = true;
            let mut out = Vec::new();
            if let Some(p) = &mut cd.planner {
                p.update(s, calls::LEAD_MAX / calls::LEAD_S, &mut out);
            }
            let first = out.first().map(|l| l.text.clone()).unwrap_or_else(|| "flat out".into());
            cd.say(Line::new(format!("Ready. {first}."), Priority::Note));
            for l in out.into_iter().skip(1) {
                cd.say(l);
            }
        }
    } else if run.finished.is_none() && !run.retired {
        let speed = car.speed();
        let mut out = Vec::new();
        if let Some(p) = &mut cd.planner {
            p.update(s, speed, &mut out);
        }
        for l in out {
            cd.say(l);
        }
        if run.splits.len() > cd.splits_said {
            cd.splits_said = run.splits.len();
            let t = run.splits[cd.splits_said - 1];
            cd.say(Line::new(format!("Split {}, {}.", cd.splits_said, spoken_time(t)), Priority::Chat));
        }
        // Off the road for a while, or on the roof: say how to get going again.
        let p = Vec2::new(car.pos.x, car.pos.z);
        cd.off_road = if stage.road.nearest(p, 14.0).is_none() { cd.off_road + dt } else { 0.0 };
        cd.on_roof = if car.up().y < 0.25 && car.vel.length() < 2.0 { cd.on_roof + dt } else { 0.0 };
        if cd.nag_cooldown <= 0.0 {
            if cd.on_roof > 1.5 {
                cd.say(Line::new("We're on the roof. Press R to get back on the road.", Priority::Urgent));
                cd.nag_cooldown = 10.0;
            } else if cd.off_road > 2.0 && car.up().y > 0.5 {
                cd.say(Line::new("We're off the road. Get back on, or press R.", Priority::Warn));
                cd.nag_cooldown = 10.0;
            }
        }
    }
    // Damage.
    let d = car.damage;
    if d.hits > cd.last_damage.hits {
        if let Some(l) = calls::damage_line(&cd.last_damage, &d) {
            cd.say(l);
        }
        cd.last_damage = d;
    }
    if let Some(t) = run.finished
        && !cd.finish_said
    {
        cd.finish_said = true;
        cd.say(Line::new(format!("Stop! Stage time {}.", spoken_time(t)), Priority::Urgent));
    }
    if let Some(l) = cd.speech.pump(s) {
        cd.subtitle = Some((l.text, SUBTITLE_S));
    }
}

/// `83.4` → "1 minute 23 point 4".
pub fn spoken_time(t: f32) -> String {
    let m = (t / 60.0).floor() as u32;
    let sec = t - m as f32 * 60.0;
    let tenths = ((sec * 10.0).round() as u32).min(599);
    let (whole, frac) = (tenths / 10, tenths % 10);
    if m > 0 { format!("{m} minute{} {whole} point {frac}", if m > 1 { "s" } else { "" }) } else { format!("{whole} point {frac} seconds") }
}

/// The line being said, top centre in a dark plate (theme fonts, accent "CO-DRIVER" tag).
fn subtitle(mut commands: Commands, cd: Res<CoDriver>, theme: Option<Res<engine::ui::theme::Theme>>, mut q: Query<&mut Text, With<Subtitle>>, mut plates: Query<&mut Visibility, With<SubtitlePlate>>) {
    use engine::ui::theme::*;
    let Ok(mut text) = q.single_mut() else {
        let Some(t) = theme else { return };
        commands
            .spawn((
                SubtitlePlate,
                DespawnOnExit(AppState::InGame),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(26),
                    left: percent(50),
                    width: px(1100),
                    margin: UiRect::left(px(-550)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .with_children(|p| {
                p.spawn((
                    Node { column_gap: px(GAP_M), align_items: AlignItems::Baseline, padding: UiRect::axes(px(18), px(8)), border_radius: BorderRadius::all(px(RADIUS)), max_width: px(1100), ..default() },
                    BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.66)),
                    Pickable::IGNORE,
                ))
                .with_children(|r| {
                    r.spawn((text("CO-DRIVER", label(SIZE_LABEL), t.accent), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }));
                    r.spawn((Subtitle, text("", label(30.0), t.text)));
                });
            });
        return;
    };
    let show = cd.subtitles && cd.subtitle.is_some();
    if let Some((s, _)) = &cd.subtitle
        && !text.0.eq_ignore_ascii_case(s)
    {
        text.0 = s.to_uppercase();
    }
    for mut v in &mut plates {
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

fn stop(mut cd: ResMut<CoDriver>) {
    cd.speech.hush();
    cd.planner = None;
    cd.stage = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_out() {
        assert_eq!(spoken_time(83.44), "1 minute 23 point 4");
        assert_eq!(spoken_time(42.06), "42 point 1 seconds");
        assert_eq!(spoken_time(185.0), "3 minutes 5 point 0");
    }
}
