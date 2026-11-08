//! Pacenote's own options and the Settings screen's layout.
//!
//! - [`Prefs`]: co-driver and HUD options (section `"prefs"` of settings.json). Camera view and
//!   ghosts live in `meta::Profile`, voice chat in `voice::VoiceSettings`, the rest in the
//!   engine's `CoreSettings` / accessibility settings.
//! - [`layout`] (Startup): every row on a rally game's tabs, in order: Driving, Camera, Co-driver,
//!   HUD, Audio, Voice chat, Video, Interface, Controls. Engine rows are moved, renamed and
//!   given rally wording by re-registering them (same key, same get/set); rows that do
//!   nothing in Pacenote (mouse look, sprint toggle, generic subtitles) are hidden.
//! - Side panels: a live HUD preview (units, which readouts, safe area), a co-driver call
//!   preview (call order, timing), a sun-dial strip for World › Start time and a side-view
//!   schematic of the camera view and field of view.

use bevy::prelude::*;
use engine::settings::{FieldDef, FieldKind, RegisterSettings, SettingsRegistry, add_settings_section, format_percent};
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use crate::codriver::CoDriver;
use crate::daytime::{self, DaySettings};

/// Co-driver and HUD options.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// The AI co-driver reads the notes (N toggles it in game).
    pub codriver: bool,
    /// 0..1.
    pub codriver_volume: f32,
    /// 0 calm, 1 normal, 2 fast.
    pub codriver_pace: u32,
    /// 0 early, 1 normal, 2 late.
    pub call_timing: u32,
    /// 0 "left three", 1 "three left".
    pub call_order: u32,
    pub subtitles: bool,
    /// Speed in mph instead of km/h.
    pub mph: bool,
    pub hud_speed: bool,
    pub hud_revs: bool,
    pub hud_clock: bool,
    pub hud_progress: bool,
    pub hud_damage: bool,
    /// HUD safe area: share of the window (0..0.05) every anchored HUD block moves in by.
    pub hud_safe_area: f32,
    /// Keyboard steering speed (0.6–1.6, 1 = default).
    pub steer_speed: f32,
    /// Gamepad stick linearity (1 linear … 2.5 soft around centre).
    pub stick_linearity: f32,
    /// Gamepad stick deadzone (0–0.3).
    pub stick_deadzone: f32,
    /// Driving assists (applied to the car before a run starts).
    pub traction_control: bool,
    pub abs: bool,
    /// What hits do: 0 none, 1 cosmetic (parts come off, drives as new), 2 simulation
    /// (`vehicle::DamageMode`).
    pub damage: u32,
    /// The guide's view: 0 in the car (tablet), 1 the map screen.
    pub guide_view: u32,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            codriver: true,
            codriver_volume: 1.0,
            codriver_pace: 1,
            call_timing: 1,
            call_order: 0,
            subtitles: true,
            mph: false,
            hud_speed: true,
            hud_revs: true,
            hud_clock: true,
            hud_progress: true,
            hud_damage: true,
            hud_safe_area: 0.0,
            steer_speed: 1.0,
            stick_linearity: 1.0,
            stick_deadzone: 0.05,
            traction_control: true,
            abs: true,
            damage: 2,
            guide_view: 0,
        }
    }
}

impl Prefs {
    /// Words per minute for the voice speed choice.
    pub fn codriver_rate(&self) -> u32 {
        [195, 230, 265][self.codriver_pace.min(2) as usize]
    }
    /// Read-ahead multiplier for the call timing choice.
    pub fn lead_scale(&self) -> f32 {
        [1.3, 1.0, 0.75][self.call_timing.min(2) as usize]
    }
    /// The assists picked.
    pub fn assists(&self) -> crate::meta::stages::Assists {
        crate::meta::stages::Assists { abs: self.abs, traction_control: self.traction_control }
    }
    /// The damage mode picked.
    pub fn damage_mode(&self) -> crate::vehicle::DamageMode {
        crate::vehicle::DamageMode::ALL[self.damage.min(2) as usize]
    }
    /// Speed for display: (value, unit).
    pub fn speed(&self, mps: f32) -> (f32, &'static str) {
        if self.mph { (mps.abs() * 2.236_936, "MPH") } else { (mps.abs() * 3.6, "KM/H") }
    }
}

/// The tabs, in order.
/// "Camera" (stream D's `camera::CameraFeel` rows) and "World" (stream C's
/// `daytime::DaySettings` rows) show once a row uses them.
pub const TABS: [&str; 11] = ["Driving", "Camera", "Co-driver", "HUD", "Audio", "Voice chat", "Video", "World", "Interface", "Privacy", "Controls"];

pub struct PrefsPlugin;

impl Plugin for PrefsPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<Prefs>(app, "prefs");
        app.settings_defaults::<Prefs>()
            .settings_panel("HUD", hud_preview)
            .settings_panel("Co-driver", codriver_preview)
            .settings_panel("Voice chat", voice_panel)
            .settings_panel("World", world_panel)
            .settings_panel("Camera", camera_panel)
            .add_systems(Startup, layout)
            .add_systems(Update, (sync_codriver, sync_steering, sync_damage, apply_assists, refresh_previews));
        register(app);
    }
}

fn p(w: &World) -> &Prefs {
    w.resource::<Prefs>()
}
fn pm(w: &mut World) -> Mut<'_, Prefs> {
    w.resource_mut::<Prefs>()
}
fn b(v: bool) -> f32 {
    if v { 1.0 } else { 0.0 }
}
fn idx(v: f32, n: u32) -> u32 {
    (v.round().max(0.0) as u32).min(n - 1)
}

fn register(app: &mut App) {
    let toggle = |key, tab, label, hint, get: fn(&World) -> f32, set: fn(&mut World, f32)| FieldDef { key, tab, label, hint, kind: FieldKind::Toggle, get, set };
    app.register_setting(toggle("codriver_voice", "Co-driver", "Co-driver voice", "Your AI co-driver reads the pace notes aloud. N turns it on or off while driving. Quiet when a human guide is connected.", |w| b(p(w).codriver), |w, v| pm(w).codriver = v > 0.5))
        .register_setting(FieldDef {
            key: "codriver_volume",
            tab: "Co-driver",
            label: "Voice volume",
            hint: "How loud the co-driver reads.",
            kind: FieldKind::Slider { min: 0.0, max: 1.0, step: 0.05, format: format_percent },
            get: |w| p(w).codriver_volume,
            set: |w, v| pm(w).codriver_volume = v.clamp(0.0, 1.0),
        })
        .register_setting(FieldDef {
            key: "codriver_pace",
            tab: "Co-driver",
            label: "Reading speed",
            hint: "How fast the notes are read. Fast keeps up on twisty stages.",
            kind: FieldKind::Choice { options: vec!["Calm", "Normal", "Fast"] },
            get: |w| p(w).codriver_pace as f32,
            set: |w, v| pm(w).codriver_pace = idx(v, 3),
        })
        .register_setting(FieldDef {
            key: "call_timing",
            tab: "Co-driver",
            label: "Call timing",
            hint: "How far ahead each corner is called. Early gives more time to brake; late keeps the notes close to the corner.",
            kind: FieldKind::Choice { options: vec!["Early", "Normal", "Late"] },
            get: |w| p(w).call_timing as f32,
            set: |w, v| pm(w).call_timing = idx(v, 3),
        })
        .register_setting(FieldDef {
            key: "call_order",
            tab: "Co-driver",
            label: "Call order",
            hint: "Direction first (\"left three\") or grade first (\"three left\"). Grades run 1 (hairpin) to 6 (flat out).",
            kind: FieldKind::Choice { options: vec!["Left 3", "3 Left"] },
            get: |w| p(w).call_order as f32,
            set: |w, v| pm(w).call_order = idx(v, 2),
        })
        .register_setting(toggle("codriver_subtitles", "Co-driver", "Subtitles", "Show each call at the top of the screen.", |w| b(p(w).subtitles), |w, v| pm(w).subtitles = v > 0.5))
        .register_setting(FieldDef {
            key: "guide_view",
            tab: "Co-driver",
            label: "Co-driver view",
            hint: "When you're the guide: ride in the passenger seat with the notes on a tablet (hold Space or right mouse to raise it), or read a full-screen map.",
            kind: FieldKind::Choice { options: vec!["In car", "Map screen"] },
            get: |w| p(w).guide_view as f32,
            set: |w, v| pm(w).guide_view = idx(v, 2),
        })
        .register_setting(FieldDef {
            key: "units",
            tab: "HUD",
            label: "Speed units",
            hint: "Speedometer in kilometres or miles per hour.",
            kind: FieldKind::Choice { options: vec!["km/h", "mph"] },
            get: |w| b(p(w).mph),
            set: |w, v| pm(w).mph = v > 0.5,
        })
        .register_setting(toggle("hud_speed", "HUD", "Speed and gear", "Speedometer and gear, bottom right.", |w| b(p(w).hud_speed), |w, v| pm(w).hud_speed = v > 0.5))
        .register_setting(toggle("hud_revs", "HUD", "Rev bar", "Engine revs, lighting up to the red line.", |w| b(p(w).hud_revs), |w, v| pm(w).hud_revs = v > 0.5))
        .register_setting(toggle("hud_clock", "HUD", "Stage clock", "Stage time and penalties, bottom left.", |w| b(p(w).hud_clock), |w, v| pm(w).hud_clock = v > 0.5))
        .register_setting(toggle("hud_progress", "HUD", "Stage progress", "Distance done and to go, under the clock.", |w| b(p(w).hud_progress), |w, v| pm(w).hud_progress = v > 0.5))
        .register_setting(FieldDef {
            key: "damage",
            tab: "Driving",
            label: "Damage",
            hint: "None: nothing breaks. Cosmetic: parts come off but the car drives as new. Simulation: hits hurt the engine, steering and suspension; a broken car waits for a recover (repaired where it is), and one crash that wrecks most of the car (60 %) is fatal (back home, or the event restarts). In an event, from the next run.",
            kind: FieldKind::Choice { options: crate::vehicle::DamageMode::ALL.iter().map(|m| m.label()).collect() },
            get: |w| p(w).damage.min(2) as f32,
            set: |w, v| pm(w).damage = idx(v, 3),
        })
        .register_setting(FieldDef {
            key: "steer_speed",
            tab: "Driving",
            label: "Keyboard steering speed",
            hint: "How fast a held steering key turns the wheel. Slower is smoother at speed; faster helps in hairpins. Letting go always centres at full speed.",
            kind: FieldKind::Slider { min: 0.6, max: 1.6, step: 0.1, format: engine::settings::format_multiplier },
            get: |w| p(w).steer_speed,
            set: |w, v| pm(w).steer_speed = v.clamp(0.6, 1.6),
        })
        .register_setting(toggle(
            "traction_control",
            "Driving",
            "Traction control",
            "Cuts wheelspin under power. Off: more wheelspin on gravel and snow, and the car gets loose on the throttle. Times with an assist off go on their own leaderboard. From the next run.",
            |w| b(p(w).traction_control),
            |w, v| pm(w).traction_control = v > 0.5,
        ))
        .register_setting(toggle(
            "abs",
            "Driving",
            "ABS",
            "Stops the wheels locking under braking. Off: the wheels can lock and slide, and stops are a little longer. Times with an assist off go on their own leaderboard. From the next run.",
            |w| b(p(w).abs),
            |w, v| pm(w).abs = v > 0.5,
        ))
        .register_setting(FieldDef {
            key: "stick_linearity",
            tab: "Controls",
            label: "Steering stick linearity",
            hint: "1.0: the wheel follows the stick exactly. Higher: gentler around the centre for fine corrections at speed, full lock still at the end.",
            kind: FieldKind::Slider { min: 1.0, max: 2.5, step: 0.1, format: |v| format!("{v:.1}") },
            get: |w| p(w).stick_linearity,
            set: |w, v| pm(w).stick_linearity = v.clamp(1.0, 2.5),
        })
        .register_setting(FieldDef {
            key: "stick_deadzone",
            tab: "Controls",
            label: "Steering stick deadzone",
            hint: "Small stick movements around the centre are ignored. Raise it if the car drifts with the stick at rest.",
            kind: FieldKind::Slider { min: 0.0, max: 0.3, step: 0.01, format: format_percent },
            get: |w| p(w).stick_deadzone,
            set: |w, v| pm(w).stick_deadzone = v.clamp(0.0, 0.3),
        })
        .register_setting(toggle("hud_damage", "HUD", "Damage", "Engine, steering and suspension damage, top right, once the car is hurt.", |w| b(p(w).hud_damage), |w, v| pm(w).hud_damage = v > 0.5))
        .register_setting(FieldDef {
            key: "hud_safe_area",
            tab: "HUD",
            label: "HUD safe area",
            hint: "Moves the HUD in from the edges of the screen, for TVs that crop the picture or a handheld's bezel.",
            kind: FieldKind::Slider { min: 0.0, max: 0.05, step: 0.005, format: |v| format!("{:.1}%", v * 100.0) },
            get: |w| p(w).hud_safe_area,
            set: |w, v| pm(w).hud_safe_area = v.clamp(0.0, 0.05),
        });
}

/// Where each existing row goes: (key, tab, new label, new hint). `""` tab = hidden. Rows not
/// listed keep their place; the order here is the order on screen.
const LAYOUT: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    // Driving.
    ("steer_speed", "Driving", None, None),
    ("traction_control", "Driving", None, None),
    ("abs", "Driving", None, None),
    ("damage", "Driving", None, None),
    ("ghost", "Driving", None, None),
    ("ghost_opacity", "Driving", None, None),
    // Camera (festival: DECISIONS D3, UX_PASS §5): the view, then the chase camera's feel
    // (stream D's `camera::CameraFeel` rows), field of view and shake.
    ("camera_view", "Camera", None, None),
    ("chase_distance", "Camera", None, None),
    ("chase_stiffness", "Camera", None, None),
    ("fov_deg", "Camera", Some("Field of view"), Some("Wider shows more of the road; narrower makes the cockpit feel faster.")),
    ("camera_shake", "Camera", Some("Camera shake"), Some("How much bumps, jumps and landings move the cockpit and bonnet views.")),
    ("landing_kick", "Camera", None, None),
    ("motion_blur", "Camera", None, None),
    // Co-driver (Prefs rows keep their order).
    ("codriver_voice", "Co-driver", None, None),
    ("codriver_volume", "Co-driver", None, None),
    ("codriver_pace", "Co-driver", None, None),
    ("call_timing", "Co-driver", None, None),
    ("call_order", "Co-driver", None, None),
    ("codriver_subtitles", "Co-driver", None, None),
    ("guide_view", "Co-driver", None, None),
    ("sensitivity", "Co-driver", Some("Look sensitivity (guide in car)"), Some("How fast the mouse turns your head in the passenger seat.")),
    ("invert_y", "Co-driver", Some("Invert look (guide in car)"), None),
    // HUD.
    ("units", "HUD", None, None),
    ("hud_speed", "HUD", None, None),
    ("hud_revs", "HUD", None, None),
    ("hud_clock", "HUD", None, None),
    ("hud_progress", "HUD", None, None),
    ("hud_damage", "HUD", None, None),
    // Festival HUD rows (stream F's `career::CareerSettings`, DECISIONS S4 / U5).
    ("style_hud", "HUD", None, None),
    ("challenge_cards", "HUD", None, None),
    ("race_positions", "HUD", None, None),
    ("hud_safe_area", "HUD", None, None),
    // Audio.
    ("master_volume", "Audio", Some("Master"), Some("Everything at once.")),
    ("music_volume", "Audio", Some("Music"), Some("Menu and loading-screen music. None plays while you drive.")),
    ("sfx_volume", "Audio", Some("Car"), Some("Engine, turbo, gearbox, tyres, gravel and impacts.")),
    ("ambience_volume", "Audio", Some("Stage ambience"), Some("Wind, birds, insects and rain around the stage.")),
    ("ui_volume", "Audio", Some("Menus"), Some("Menu clicks and notifications.")),
    // Voice chat.
    ("voice_chat", "Voice chat", Some("Voice chat"), Some("Talk to your teammate over the microphone while playing together.")),
    ("voice_chat_volume", "Voice chat", Some("Teammate volume"), None),
    ("voice_mode", "Voice chat", None, None),
    ("voice_threshold", "Voice chat", Some("Open mic sensitivity"), None),
    // Video.
    ("quality", "Video", None, None),
    ("window_mode", "Video", None, None),
    ("vsync", "Video", None, None),
    ("show_fps", "Video", Some("Show frame rate"), None),
    // World (stream C's `daytime::DaySettings` rows): when free roam starts, whether the
    // clock runs.
    ("day_start", "World", None, None),
    ("day_time_passes", "World", None, None),
    // Interface.
    ("language", "Interface", None, None),
    ("ui_scale", "Interface", Some("Menu and HUD size"), Some("Scales menus and the HUD as a whole.")),
    ("text_scale", "Interface", Some("Text size"), Some("Text only, on top of the menu and HUD size.")),
    ("high_contrast", "Interface", None, None),
    ("colorblind", "Interface", None, None),
    ("colorblind_strength", "Interface", None, None),
    ("reduce_motion", "Interface", Some("Reduce motion"), Some("Steadier cameras: no camera shake in the cockpit or on the bonnet.")),
    // Privacy (the tab's side panel has "Forget me").
    ("share_stats", "Privacy", None, None),
    ("crash_reports", "Privacy", None, None),
    // Controls (gamepad steering; bindings and rebinding are the tab's side panel).
    ("stick_linearity", "Controls", None, None),
    ("stick_deadzone", "Controls", None, None),
    // Nothing in Pacenote uses these: hidden.
    ("toggle_actions", "", None, None),
    ("rumble", "", None, None),
    ("voice_volume", "", None, None),
    ("captions", "", None, None),
    ("caption_size", "", None, None),
    ("caption_directions", "", None, None),
];

/// Lays the Settings screen out for a rally game (see [`LAYOUT`]). Startup: every plugin has
/// registered its rows by then.
pub fn layout(mut reg: ResMut<SettingsRegistry>) {
    let mut placed: Vec<FieldDef> = Vec::new();
    for (key, tab, label, hint) in LAYOUT {
        if let Some(mut f) = reg.field(key).cloned() {
            f.tab = tab;
            if let Some(l) = label {
                f.label = l;
            }
            if let Some(h) = hint {
                f.hint = h;
            }
            placed.push(f);
        }
    }
    let rest: Vec<FieldDef> = reg.fields.iter().filter(|f| !LAYOUT.iter().any(|(k, ..)| *k == f.key)).cloned().collect();
    reg.fields = placed.into_iter().chain(rest).collect();
    // The tabs that have rows, in rally order (Controls always: bindings + rebinding).
    let used: Vec<&'static str> = reg.fields.iter().map(|f| f.tab).filter(|t| !t.is_empty()).collect();
    let mut tabs: Vec<&'static str> = TABS.iter().copied().filter(|t| *t == "Controls" || used.contains(t)).collect();
    for t in used {
        if !tabs.contains(&t) {
            tabs.insert(tabs.len() - 1, t);
        }
    }
    reg.tabs = tabs;
}

/// Prefs ↔ the co-driver: the settings drive it, and N in game is remembered.
fn sync_codriver(cd: Option<ResMut<CoDriver>>, mut prefs: ResMut<Prefs>, mut last: Local<Option<bool>>) {
    let Some(mut cd) = cd else { return };
    // N toggled it since last frame: keep that.
    if last.is_some_and(|l| l != cd.enabled) && !prefs.is_changed() {
        prefs.codriver = cd.enabled;
    }
    if prefs.is_changed() || last.is_none() {
        let p = prefs.clone();
        if cd.enabled != p.codriver {
            cd.enabled = p.codriver;
        }
        cd.lead_scale = p.lead_scale();
        cd.grade_first = p.call_order == 1;
        cd.subtitles = p.subtitles;
        cd.set_voice(p.codriver_rate(), p.codriver_volume);
    }
    *last = Some(cd.enabled);
}

/// Prefs → the car's damage mode. In free roam at once (nothing is timed), in an event only
/// on the line like the assists. Leaving Simulation mends the car (parts that came off stay
/// off in Cosmetic) and clears a wreck.
fn sync_damage(
    prefs: Res<Prefs>,
    spec: Option<ResMut<crate::drive::CarSpec>>,
    stage: Option<Res<crate::world::StageWorld>>,
    run: Option<ResMut<crate::drive::StageRun>>,
    mut cars: Query<&mut crate::drive::DrivenCar>,
) {
    use crate::vehicle::{Damage, DamageMode};
    let Some(mut spec) = spec else { return };
    let want = prefs.damage_mode();
    if spec.0.damage == want {
        return;
    }
    let mid_event = stage.is_some_and(|s| !s.free_roam()) && run.as_ref().is_some_and(|r| r.countdown <= 0.0 || r.time > 0.0);
    if mid_event {
        return;
    }
    spec.0.damage = want;
    if want != DamageMode::Simulation {
        for mut c in &mut cars {
            let d = c.car.damage;
            let detached = if want == DamageMode::Cosmetic { d.detached } else { 0 };
            c.car.damage = Damage { hits: d.hits, detached, ..Damage::default() };
        }
        if let Some(mut run) = run {
            run.down = None;
        }
    }
}

fn sync_steering(prefs: Res<Prefs>, steer: Option<ResMut<crate::drive::SteerSettings>>) {
    let Some(mut steer) = steer else { return };
    let want = crate::drive::SteerSettings { keyboard_speed: prefs.steer_speed, stick_linearity: prefs.stick_linearity, stick_deadzone: prefs.stick_deadzone };
    if *steer != want {
        *steer = want;
    }
}

/// Prefs → the car's assists, only when no run is under way (in the menus, while loading, or
/// on the start line before the clock runs), so a run never changes its car mid-stage.
fn apply_assists(
    prefs: Res<Prefs>,
    spec: Option<ResMut<crate::drive::CarSpec>>,
    state: Option<Res<State<engine::flow::AppState>>>,
    run: Option<Res<crate::drive::StageRun>>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    let Some(mut spec) = spec else { return };
    let in_game = state.is_some_and(|s| *s.get() == engine::flow::AppState::InGame);
    let on_the_line = run.is_some_and(|r| r.countdown > 0.0 && r.time == 0.0);
    if in_game && loading.is_none() && !on_the_line {
        return;
    }
    if spec.0.abs != prefs.abs || spec.0.traction_assist != prefs.traction_control {
        spec.0.abs = prefs.abs;
        spec.0.traction_assist = prefs.traction_control;
    }
}

// ---------------------------------------------------------------- side panels

#[derive(Component)]
struct HudPreview;

#[derive(Component)]
struct CallPreview;

/// HUD tab: a small live speedo + rev bar in the chosen units, and which readouts are on.
fn hud_preview(world: &mut World, col: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(col).with_children(|c| {
        c.spawn((panel(&t, 20.0, GAP_S), HudPreview)).with_children(|p| {
            p.spawn(eyebrow("Preview", t.accent));
        });
    });
    world.flush();
    rebuild_hud_preview(world);
}

fn rebuild_hud_preview(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<HudPreview>>();
    let Some(panel) = q.iter(world).next() else { return };
    let t = world.resource::<Theme>().clone();
    let prefs = world.resource::<Prefs>().clone();
    if let Some(kids) = world.get::<Children>(panel).map(|c| c.iter().skip(1).collect::<Vec<_>>()) {
        for k in kids {
            world.entity_mut(k).despawn();
        }
    }
    let (v, unit) = prefs.speed(31.0);
    // The safe area drawn as a frame that share in from the box's edges; the readouts move in too.
    let inset = prefs.hud_safe_area.clamp(0.0, 0.05) * 100.0;
    world.commands().entity(panel).with_children(|p| {
        p.spawn((Node { height: px(150), flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::FlexEnd, row_gap: px(6), padding: UiRect::all(px(14.0 + inset * 3.0)), ..default() }, BackgroundColor(Color::srgba(0.18, 0.22, 0.2, 0.55)), Pickable::IGNORE)).with_children(|s| {
            if inset > 0.0 {
                s.spawn((Node { position_type: PositionType::Absolute, left: percent(inset), right: percent(inset), top: percent(inset), bottom: percent(inset), border: UiRect::all(px(1)), ..default() }, BorderColor::all(t.text_dim), Pickable::IGNORE));
            }
            if prefs.hud_damage {
                s.spawn(text("DAMAGE \u{2022} STEERING 18%", label(SIZE_TINY), t.accent));
            }
            if prefs.hud_revs {
                crate::hud::rev_bar_preview(s, &t, 0.72);
            }
            if prefs.hud_speed {
                s.spawn((Node { column_gap: px(10), align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn(text(format!("{v:.0}"), display(48.0), t.text));
                    r.spawn(text(unit, label(SIZE_LABEL), t.text_muted));
                    r.spawn(text("3", display(48.0), t.accent));
                });
            }
            if prefs.hud_clock {
                s.spawn(text(if prefs.hud_progress { "1:12.480  \u{b7}  1.62 / 3.20 km" } else { "1:12.480" }, label(SIZE_LABEL), t.text));
            } else if prefs.hud_progress {
                s.spawn(text("1.62 / 3.20 km", label(SIZE_LABEL), t.text));
            }
        });
        p.spawn(text("The HUD while driving, at 112 km/h in third. Changes show here as you make them.", body(SIZE_SMALL), t.text_dim));
        if inset > 0.0 {
            p.spawn(text(format!("The frame: the HUD sits {inset:.1}% in from every edge of the screen."), body(SIZE_SMALL), t.text_dim));
        }
    });
}

/// Co-driver tab: a sample call as it would be read and shown.
fn codriver_preview(world: &mut World, col: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(col).with_children(|c| {
        c.spawn((panel(&t, 20.0, GAP_S), CallPreview)).with_children(|p| {
            p.spawn(eyebrow("Sample call", t.accent));
        });
    });
    world.flush();
    rebuild_call_preview(world);
}

fn rebuild_call_preview(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<CallPreview>>();
    let Some(panel) = q.iter(world).next() else { return };
    let t = world.resource::<Theme>().clone();
    let prefs = world.resource::<Prefs>().clone();
    if let Some(kids) = world.get::<Children>(panel).map(|c| c.iter().skip(1).collect::<Vec<_>>()) {
        for k in kids {
            world.entity_mut(k).despawn();
        }
    }
    let call = if prefs.call_order == 1 { "150 three left long, into five right over crest" } else { "150 left three long, into right five over crest" };
    let lead = crate::codriver::calls::LEAD_S * prefs.lead_scale();
    let pace = ["calm", "normal", "fast"][prefs.codriver_pace.min(2) as usize];
    world.commands().entity(panel).with_children(|p| {
        p.spawn((Node { padding: UiRect::axes(px(14), px(8)), border_radius: BorderRadius::all(px(RADIUS)), column_gap: px(GAP_S), flex_wrap: FlexWrap::Wrap, ..default() }, BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.66)), Pickable::IGNORE)).with_children(|r| {
            r.spawn(text("CO-DRIVER", label(SIZE_TINY), t.accent));
            r.spawn(text(call.to_uppercase(), label(SIZE_LABEL + 2.0), t.text));
        });
        let state = if !prefs.codriver {
            "Co-driver off: no calls (N turns it back on while driving).".to_string()
        } else {
            format!("Read at {pace} speed, about {lead:.1} s before each corner at speed{}.", if prefs.subtitles { ", with subtitles" } else { ", no subtitles" })
        };
        p.spawn(text(state, body(SIZE_SMALL), t.text_dim));
        p.spawn(text("Grades: 1 hairpin \u{b7} 2 \u{b7} 3 \u{b7} 4 \u{b7} 5 \u{b7} 6 flat out. \"Long\": the corner keeps going; \"into\": the next follows straight after.", body(SIZE_SMALL), t.text_dim));
    });
}

/// Voice chat tab: how talking works (push-to-talk key, when the mic opens).
fn voice_panel(world: &mut World, col: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(col).with_children(|c| {
        c.spawn(panel(&t, 20.0, GAP_S)).with_children(|p| {
            p.spawn(eyebrow("How it works", t.accent));
            for line in [
                "The microphone opens only while a teammate is connected (Play together).",
                "Push to talk: hold V on the keyboard or LB on a gamepad. Change the key on the Controls tab.",
                "Auto: the driver talks on an open mic (hands on the wheel), the guide pushes to talk.",
                "Use headphones with an open mic, so the car's sound doesn't reach your teammate.",
            ] {
                p.spawn(text(line, body(SIZE_SMALL + 1.0), t.text_muted));
            }
        });
    });
}

/// What the World and Camera panels show: (start hour, time passes, view, fov, chase distance).
type ViewKey = (f32, bool, u32, f32, u32);

fn view_key(world: &World) -> ViewKey {
    let day = world.get_resource::<DaySettings>().cloned().unwrap_or_default();
    let wall = if day.start == daytime::REAL_CLOCK { daytime::wall_hours() } else { 0.0 };
    let start = daytime::start_hours(&day, wall, daytime::night_available(world));
    let view = world.get_resource::<crate::meta::Profile>().map_or(0, |p| p.camera);
    let fov = world.get_resource::<engine::settings::CoreSettings>().map_or(80.0, |c| c.fov_deg);
    let dist = world.get_resource::<crate::camera::CameraFeel>().map_or(1, |c| c.chase_distance);
    // To the minute, so "Now" doesn't rebuild every frame.
    ((start * 60.0).floor() / 60.0, day.time_passes, view, fov, dist)
}

/// Keeps the previews in step with the options while the settings screen is open.
fn refresh_previews(world: &mut World, mut last: Local<Option<Prefs>>, mut last_view: Local<Option<ViewKey>>) {
    let now = world.resource::<Prefs>().clone();
    if last.as_ref() != Some(&now) {
        *last = Some(now);
        rebuild_hud_preview(world);
        rebuild_call_preview(world);
    }
    // Only while a World / Camera panel is up (the wall clock is a syscall).
    let mut q = world.query_filtered::<(), Or<(With<WorldPreview>, With<CameraPreview>)>>();
    if q.iter(world).next().is_none() {
        *last_view = None;
        return;
    }
    let key = view_key(world);
    if *last_view != Some(key) {
        *last_view = Some(key);
        rebuild_world_preview(world);
        rebuild_camera_preview(world);
    }
}

/// Despawns a side panel's children after its eyebrow.
fn clear_panel(world: &mut World, panel: Entity) {
    if let Some(kids) = world.get::<Children>(panel).map(|c| c.iter().skip(1).collect::<Vec<_>>()) {
        for k in kids {
            world.entity_mut(k).despawn();
        }
    }
}

// ---------------------------------------------------------------- World: sun-dial strip

#[derive(Component)]
struct WorldPreview;

/// Half-hour cells in the sun-dial strip.
const DIAL_CELLS: usize = 48;
const NIGHT_SKY: Color = Color::srgb(0.06, 0.08, 0.16);
const LOW_SUN: Color = Color::srgb(0.86, 0.47, 0.22);
const DAY_SKY: Color = Color::srgb(0.5, 0.68, 0.84);

/// Where an hour sits along the strip, 0..1.
pub fn dial_x(hours: f32) -> f32 {
    hours.rem_euclid(24.0) / 24.0
}

/// Sky colour for a sun elevation (degrees): night below −6°, warm through the horizon, day
/// sky from 10° up.
pub fn sky_tint(el: f32) -> Color {
    use engine::ui::widgets::mix;
    if el <= -6.0 {
        NIGHT_SKY
    } else if el < 0.0 {
        mix(NIGHT_SKY, LOW_SUN, (el + 6.0) / 6.0)
    } else if el < 10.0 {
        mix(LOW_SUN, DAY_SKY, el / 10.0)
    } else {
        DAY_SKY
    }
}

/// Sunrise and sunset (the sun's elevation crossing 0°), clock hours.
pub fn sun_times() -> (f32, f32) {
    let cross = |mut lo: f32, mut hi: f32| {
        let rising = daytime::solar(lo).1 < daytime::solar(hi).1;
        for _ in 0..20 {
            let mid = (lo + hi) * 0.5;
            if (daytime::solar(mid).1 < 0.0) == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    };
    (cross(0.0, 13.0), cross(13.0, 24.0))
}

/// `9.5` → "09:30".
pub fn hhmm(h: f32) -> String {
    let m = (h.rem_euclid(24.0) * 60.0).round() as u32 % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// World tab: the 24 h day (night shaded, warm dawn and sunset) with the start time marked.
fn world_panel(world: &mut World, col: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(col).with_children(|c| {
        c.spawn((panel(&t, 20.0, GAP_S), WorldPreview)).with_children(|p| {
            p.spawn(eyebrow("Start time", t.accent));
        });
    });
    world.flush();
    rebuild_world_preview(world);
}

fn rebuild_world_preview(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<WorldPreview>>();
    let Some(panel) = q.iter(world).next() else { return };
    let t = world.resource::<Theme>().clone();
    let day = world.get_resource::<DaySettings>().cloned().unwrap_or_default();
    let (start, passes, ..) = view_key(world);
    clear_panel(world, panel);
    let name = daytime::START_PRESETS.get(day.start as usize).map_or("", |p| p.0);
    let el = daytime::solar(start).1;
    let (rise, set) = sun_times();
    let x = dial_x(start) * 100.0;
    world.commands().entity(panel).with_children(|p| {
        // The start time over its marker (kept inside the strip's ends).
        p.spawn((Node { height: px(26), ..default() }, Pickable::IGNORE)).with_children(|r| {
            r.spawn((text(hhmm(start), label(SIZE_LABEL + 4.0), t.accent), Node { position_type: PositionType::Absolute, left: percent(x.clamp(8.0, 92.0)), margin: UiRect::left(px(-26)), ..default() }));
        });
        p.spawn((Node { height: px(36), border_radius: BorderRadius::all(px(RADIUS)), overflow: Overflow::clip(), ..default() }, Pickable::IGNORE)).with_children(|s| {
            for i in 0..DIAL_CELLS {
                let h = (i as f32 + 0.5) * 24.0 / DIAL_CELLS as f32;
                s.spawn((Node { flex_grow: 1.0, height: percent(100), ..default() }, BackgroundColor(sky_tint(daytime::solar(h).1)), Pickable::IGNORE));
            }
            s.spawn((Node { position_type: PositionType::Absolute, left: percent(x), top: px(0), bottom: px(0), width: px(4), margin: UiRect::left(px(-2)), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
        });
        p.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|r| {
            for h in ["00", "06", "12", "18", "24"] {
                r.spawn(text(h, label(SIZE_SMALL), t.text_dim));
            }
        });
        let sun = if el > 0.0 { format!("sun {el:.0}\u{b0} up") } else { "dark".to_string() };
        p.spawn(text(format!("{name}: {} ({sun}). Sunrise {}, sunset {}.", hhmm(start), hhmm(rise), hhmm(set)), body(SIZE_SMALL + 1.0), t.text_muted));
        let clock = if passes { "The clock runs at 5\u{d7}: a whole day takes 4 h 48 min." } else { "The clock stays at the start time." };
        p.spawn(text(format!("{clock} Free roam only; events keep their own time."), body(SIZE_SMALL), t.text_dim));
    });
}

// ---------------------------------------------------------------- Camera: side-view schematic

#[derive(Component)]
struct CameraPreview;

/// Schematic box (px), its scale (px per metre) and where the car's centre meets the ground
/// (chase view).
const CAM_W: f32 = 338.0;
const CAM_H: f32 = 150.0;
const CAM_PX_M: f32 = 20.0;
const CAR_X: f32 = 220.0;
const GROUND_Y: f32 = 122.0;

/// The eye (metres forward of the car's centre, metres up) and its pitch (radians, up +) at rest
/// for a view (0 cockpit, 1 bonnet, 2 chase) and chase distance.
pub fn view_eye(view: u32, distance: u32) -> (Vec2, f32) {
    match view {
        0 => (Vec2::new(-0.3, 1.15), -0.05),
        1 => (Vec2::new(0.9, 1.0), 0.0),
        _ => {
            let (back, up) = crate::camera::rest_boom(0.0, distance);
            // The arm aims 4 m ahead of the car, 0.9 m up.
            (Vec2::new(-back, up), (0.9 - up).atan2(4.0 + back))
        }
    }
}

/// Camera tab: the car from the side with the chosen view's eye and its field of view.
fn camera_panel(world: &mut World, col: Entity) {
    let t = world.resource::<Theme>().clone();
    world.commands().entity(col).with_children(|c| {
        c.spawn((panel(&t, 20.0, GAP_S), CameraPreview)).with_children(|p| {
            p.spawn(eyebrow("View", t.accent));
        });
    });
    world.flush();
    rebuild_camera_preview(world);
}

fn rebuild_camera_preview(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<CameraPreview>>();
    let Some(panel) = q.iter(world).next() else { return };
    let t = world.resource::<Theme>().clone();
    let (.., view, fov, dist) = view_key(world);
    clear_panel(world, panel);
    let view = view.min(2);
    let (eye, pitch) = view_eye(view, dist);
    // The chase view needs room behind the car; the in-car views centre it.
    let car_x = if view == 2 { CAR_X } else { CAM_W * 0.4 };
    let to_px = |m: Vec2| Vec2::new(car_x + m.x * CAM_PX_M, GROUND_Y - m.y * CAM_PX_M);
    let e = to_px(eye);
    let half = fov.to_radians() * 0.5;
    let at = |x: f32, y: f32, w: f32, h: f32| Node { position_type: PositionType::Absolute, left: px(x), top: px(y), width: px(w), height: px(h), ..default() };
    let paper = Color::srgba(0.95, 0.93, 0.88, 0.9);
    world.commands().entity(panel).with_children(|p| {
        p.spawn((Node { width: px(CAM_W), height: px(CAM_H), overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::srgba(0.18, 0.22, 0.2, 0.55)), Pickable::IGNORE)).with_children(|b| {
            b.spawn((at(0.0, GROUND_Y, CAM_W, 2.0), BackgroundColor(t.text_dim), Pickable::IGNORE));
            // The view's wedge: faint rays across it, the two edges in accent.
            let len = 420.0;
            for k in 0..=12 {
                let a = pitch - half + 2.0 * half * k as f32 / 12.0;
                let (w, c) = if k == 0 || k == 12 { (2.0, t.accent) } else { (1.0, t.accent.with_alpha(0.22)) };
                let mid = e + Vec2::new(a.cos(), -a.sin()) * len * 0.5;
                b.spawn((at(mid.x - len * 0.5, mid.y - w * 0.5, len, w), UiTransform::from_rotation(Rot2::radians(-a)), BackgroundColor(c), Pickable::IGNORE));
            }
            // The car, nose right: body, cabin, wheels.
            let body = to_px(Vec2::new(-2.1, 1.05));
            b.spawn((Node { border_radius: BorderRadius::all(px(6)), ..at(body.x, body.y, 4.2 * CAM_PX_M, 0.6 * CAM_PX_M) }, BackgroundColor(paper), Pickable::IGNORE));
            let cab = to_px(Vec2::new(-1.4, 1.45));
            b.spawn((Node { border_radius: BorderRadius::new(px(10), px(4), px(0), px(0)), ..at(cab.x, cab.y, 2.0 * CAM_PX_M, 0.4 * CAM_PX_M) }, BackgroundColor(paper), Pickable::IGNORE));
            for wx in [-1.25, 1.25] {
                let w = to_px(Vec2::new(wx - 0.32, 0.64));
                b.spawn((Node { border_radius: BorderRadius::MAX, ..at(w.x, w.y, 0.64 * CAM_PX_M, 0.64 * CAM_PX_M) }, BackgroundColor(Color::srgb(0.08, 0.09, 0.08)), Pickable::IGNORE));
            }
            b.spawn((Node { border_radius: BorderRadius::MAX, ..at(e.x - 6.0, e.y - 6.0, 12.0, 12.0) }, BackgroundColor(t.accent), Pickable::IGNORE));
        });
        let name = ["Cockpit", "Bonnet", "Chase"][view as usize];
        let what = match view {
            0 => "From the driver's seat, looking a little down the road.".to_string(),
            1 => "From the bonnet, level with the road.".to_string(),
            _ => format!("Behind the car, {} distance.", ["near", "normal", "far"][dist.min(2) as usize]),
        };
        p.spawn(text(format!("{name}, {fov:.0}\u{b0} field of view"), label(SIZE_LABEL + 2.0), t.text));
        p.spawn(text(format!("{what} C changes the view while driving (Select on a gamepad)."), body(SIZE_SMALL), t.text_dim));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_puts_rows_on_rally_tabs_and_hides_unused_ones() {
        let mut app = App::new();
        let field = |key: &'static str, tab: &'static str| FieldDef { key, tab, label: "x", hint: "", kind: FieldKind::Toggle, get: |_| 0.0, set: |_, _| {} };
        app.init_resource::<SettingsRegistry>()
            .register_setting(field("sensitivity", "Controls"))
            .register_setting(field("fov_deg", "Video"))
            .register_setting(field("quality", "Video"))
            .register_setting(field("master_volume", "Audio"))
            .register_setting(field("camera_view", "Game"))
            .register_setting(field("hud_speed", "HUD"))
            .register_setting(field("something_new", "Extras"))
            .add_systems(Update, layout);
        app.update();
        let reg = app.world().resource::<SettingsRegistry>();
        assert_eq!(reg.tabs, vec!["Camera", "Co-driver", "HUD", "Audio", "Video", "Extras", "Controls"]);
        assert_eq!(reg.tab_fields("Camera").map(|f| f.key).collect::<Vec<_>>(), vec!["camera_view", "fov_deg"], "the camera rows moved to Camera");
        assert_eq!(reg.field("sensitivity").unwrap().tab, "Co-driver", "mouse look: the guide's head in the car");
        assert_eq!(reg.field("master_volume").unwrap().label, "Master");
    }

    #[test]
    fn sun_dial_marks_the_start_hour() {
        assert_eq!(dial_x(0.0), 0.0);
        assert_eq!(dial_x(12.0), 0.5);
        assert_eq!(dial_x(25.5), dial_x(1.5), "wraps");
        assert_eq!(sky_tint(daytime::solar(1.0).1), NIGHT_SKY);
        assert_eq!(sky_tint(daytime::solar(13.0).1), DAY_SKY);
        let (rise, set) = sun_times();
        assert!((5.5..5.75).contains(&rise) && (20.25..20.5).contains(&set), "{rise} {set}");
        assert_eq!(hhmm(9.5), "09:30");
        assert_eq!(hhmm(23.999), "00:00");
    }

    #[test]
    fn camera_schematic_eyes() {
        let (cockpit, down) = view_eye(0, 1);
        let (bonnet, _) = view_eye(1, 1);
        assert!(bonnet.x > cockpit.x && down < 0.0, "bonnet ahead of the seat; the cockpit looks down");
        let (near, pitch) = view_eye(2, 0);
        let (far, _) = view_eye(2, 2);
        assert!(far.x < near.x && far.y > near.y && pitch < 0.0, "far hangs further back and higher, looking down at the car");
        assert!(CAR_X + far.x * CAM_PX_M > 0.0, "the far chase eye fits in the box");
    }

    #[test]
    fn units_and_codriver_choices() {
        let mut p = Prefs::default();
        assert_eq!(p.speed(10.0).0.round(), 36.0);
        p.mph = true;
        let (v, unit) = p.speed(10.0);
        assert!((v - 22.369).abs() < 0.01 && unit == "MPH", "{v} {unit}");
        assert_eq!(p.codriver_rate(), 230, "normal is the voice's old speed");
        assert_eq!(p.lead_scale(), 1.0);
        p.call_timing = 0;
        assert!(p.lead_scale() > 1.0, "early reads further ahead");
    }
}
