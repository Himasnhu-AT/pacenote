//! Pacenote's own options and the Settings screen's layout.
//!
//! - [`Prefs`]: co-driver and HUD options (section `"prefs"` of settings.json). Camera view and
//!   ghosts live in `meta::Profile`, voice chat in `voice::VoiceSettings`, the rest in the
//!   engine's `CoreSettings` / accessibility settings.
//! - [`layout`] (Startup): every row on a rally game's tabs, in order: Driving, Co-driver,
//!   HUD, Audio, Voice chat, Video, Interface, Controls. Engine rows are moved, renamed and
//!   given rally wording by re-registering them (same key, same get/set); rows that do
//!   nothing in Pacenote (mouse look, music, sprint toggle, generic subtitles) are hidden.
//! - Side panels: a live HUD preview (units, which readouts) and a co-driver call preview
//!   (call order, timing).

use bevy::prelude::*;
use engine::settings::{FieldDef, FieldKind, RegisterSettings, SettingsRegistry, add_settings_section, format_percent};
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use crate::codriver::CoDriver;

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
    /// Keyboard steering speed (0.6–1.6, 1 = default).
    pub steer_speed: f32,
    /// Gamepad stick linearity (1 linear … 2.5 soft around centre).
    pub stick_linearity: f32,
    /// Gamepad stick deadzone (0–0.3).
    pub stick_deadzone: f32,
    /// Driving assists (applied to the car before a run starts).
    pub traction_control: bool,
    pub abs: bool,
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
            steer_speed: 1.0,
            stick_linearity: 1.0,
            stick_deadzone: 0.05,
            traction_control: true,
            abs: true,
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
    /// Speed for display: (value, unit).
    pub fn speed(&self, mps: f32) -> (f32, &'static str) {
        if self.mph { (mps.abs() * 2.236_936, "MPH") } else { (mps.abs() * 3.6, "KM/H") }
    }
}

/// The tabs, in order.
pub const TABS: [&str; 9] = ["Driving", "Co-driver", "HUD", "Audio", "Voice chat", "Video", "Interface", "Privacy", "Controls"];

pub struct PrefsPlugin;

impl Plugin for PrefsPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<Prefs>(app, "prefs");
        app.settings_defaults::<Prefs>()
            .settings_panel("HUD", hud_preview)
            .settings_panel("Co-driver", codriver_preview)
            .settings_panel("Voice chat", voice_panel)
            .add_systems(Startup, layout)
            .add_systems(Update, (sync_codriver, sync_steering, apply_assists, refresh_previews));
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
        });
}

/// Where each existing row goes: (key, tab, new label, new hint). `""` tab = hidden. Rows not
/// listed keep their place; the order here is the order on screen.
const LAYOUT: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    // Driving.
    ("steer_speed", "Driving", None, None),
    ("traction_control", "Driving", None, None),
    ("abs", "Driving", None, None),
    ("camera_view", "Driving", None, None),
    ("fov_deg", "Driving", Some("Field of view"), Some("Wider shows more of the road; narrower makes the cockpit feel faster.")),
    ("camera_shake", "Driving", Some("Camera shake"), Some("How much bumps, jumps and landings move the cockpit and bonnet views.")),
    ("ghost", "Driving", None, None),
    ("ghost_opacity", "Driving", None, None),
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
    // Audio.
    ("master_volume", "Audio", Some("Master"), Some("Everything at once.")),
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
    ("music_volume", "", None, None),
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

/// Prefs → the steering input shaping (input only: ghosts and boards don't change).
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
    world.commands().entity(panel).with_children(|p| {
        p.spawn((Node { height: px(150), flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::FlexEnd, row_gap: px(6), padding: UiRect::all(px(14)), ..default() }, BackgroundColor(Color::srgba(0.18, 0.22, 0.2, 0.55)), Pickable::IGNORE)).with_children(|s| {
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

/// Keeps the previews in step with the options while the settings screen is open.
fn refresh_previews(world: &mut World, mut last: Local<Option<Prefs>>) {
    let now = world.resource::<Prefs>().clone();
    if last.as_ref() == Some(&now) {
        return;
    }
    *last = Some(now);
    rebuild_hud_preview(world);
    rebuild_call_preview(world);
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
        assert_eq!(reg.tabs, vec!["Driving", "Co-driver", "HUD", "Audio", "Video", "Extras", "Controls"]);
        assert_eq!(reg.tab_fields("Driving").map(|f| f.key).collect::<Vec<_>>(), vec!["camera_view", "fov_deg"]);
        assert_eq!(reg.field("sensitivity").unwrap().tab, "Co-driver", "mouse look: the guide's head in the car");
        assert_eq!(reg.field("master_volume").unwrap().label, "Master");
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
