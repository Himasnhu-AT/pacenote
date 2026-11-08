//! HUD: the Rhythm panel (bottom right, above the speedo), skill lines, Banked / Broken.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §2.4, `docs/festival/UX_PASS.md` §3.1, `docs/festival/DECISIONS.md` S4.
//! A read-only view of F's state; E owns `hud.rs` and the shared HUD look.
//! Despawn on leaving `InGame`; allocate nothing per frame while values are unchanged.
//!
//! ```text
//!   RHYTHM              ×2.4     header: the multiplier bumps when it grows
//!   2,480                        pending Style (P, running skills included)
//!   SLIDE             +1,120     up to 3 skill lines, newest on top; a repeat of the
//!   BIG AIR             +640     same skill (or a running one) counts up in place;
//!   CLOSE SHAVE         +250     each line stays 2 s
//!   ▬▬▬▬▬▬▬▬▬▬░░░░░░             time left before it banks (3.5 s)
//! ```
//!
//! Banked: `✓ BANKED`, the Style and "+N NOTES" in `good`; Broken: `✕ BROKEN`, the lost points
//! struck through and the reason in `bad`, a 4 px shake (none with reduce motion). Setting
//! HUD › Style HUD: Full / Compact (header and bank/break only; also in timed events) / Off.
//! Scoring runs whatever the setting.
//!
//! Also registers the scorer itself ([`crate::career::style::register`]) and the Style HUD
//! row of [`CareerSettings`].

use std::fmt::Write;

use bevy::prelude::*;
use engine::settings::{FieldDef, FieldKind, RegisterSettings};
use engine::ui::theme::*;

use crate::career::CareerSettings;
use crate::career::style::{LIVE_KINDS, RHYTHM_TIMEOUT, Rhythm, StyleEvent, StyleKind, StyleTag};
use crate::hud_challenge::icons::{HudIcons, Icon};
use crate::world::StageWorld;

/// Skill lines shown at most.
pub const LINES: usize = 3;
/// Seconds a finished skill line stays.
const LINE_SECS: f32 = 2.0;
/// Banked / Broken: shown this long, then fade out over [`FADE`].
const BANNER_SECS: f32 = 1.4;
const FADE: f32 = 0.35;
/// Panel bottom: above the speedo plate (`hud.rs`: bottom 34 + ~150 px tall).
const BOTTOM: f32 = 200.0;
const WIDTH: f32 = 330.0;

pub struct StyleHudPlugin;

impl Plugin for StyleHudPlugin {
    fn build(&self, app: &mut App) {
        crate::career::style::register(app);
        app.register_setting(FieldDef {
            key: "style_hud",
            tab: "HUD",
            label: "Style HUD",
            hint: "The Rhythm panel above the speedo: every skill (Full), only banks and breaks (Compact), or nothing (Off). Style still scores and pays.",
            kind: FieldKind::Choice { options: vec!["Full", "Compact", "Off"] },
            get: |w| w.get_resource::<CareerSettings>().map_or(0.0, |s| s.style_hud as f32),
            set: |w, v| {
                if let Some(mut s) = w.get_resource_mut::<CareerSettings>() {
                    s.style_hud = (v.round().max(0.0) as u32).min(2);
                }
            },
        })
        .init_resource::<Panel>()
        .add_systems(Update, (spawn, collect, draw).chain().run_if(resource_exists::<StageWorld>));
    }
}

#[derive(Component)]
struct Root;
#[derive(Component)]
struct Body;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Txt {
    Title,
    Mult,
    Total,
    Sub,
    LineLabel(usize),
    LinePoints(usize),
}
#[derive(Component)]
struct LineRow(usize);
#[derive(Component)]
struct HeadIcon;
#[derive(Component)]
struct BarFill;
#[derive(Component)]
struct BarTrack;

/// One skill line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Line {
    kind: StyleKind,
    tag: StyleTag,
    /// Completed points on this line.
    base: u32,
    /// Seconds since it last changed.
    age: f32,
    /// Seconds since it appeared (the slide-in).
    born: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Banner {
    Banked { style: u32, notes: u32 },
    Broken { lost: u32, why: &'static str },
}

/// What the panel shows (built from the messages and [`Rhythm`]; drawn by `draw`).
#[derive(Resource, Default)]
struct Panel {
    /// Newest first.
    lines: Vec<Line>,
    banner: Option<(Banner, f32)>,
    /// Seconds since the multiplier last grew (the bump).
    bump: f32,
    mult: f32,
}

fn spawn(mut commands: Commands, theme: Res<Theme>, icons: Option<Res<HudIcons>>, q: Query<(), With<Root>>) {
    if !q.is_empty() {
        return;
    }
    let t = &*theme;
    let icon = icons.as_ref().map(|i| i.get(Icon::Check)).unwrap_or_default();
    commands
        .spawn((
            Root,
            Name::new("HUD style"),
            crate::hud::SafeArea::BOTTOM_RIGHT,
            DespawnOnExit(engine::flow::AppState::InGame),
            Node { position_type: PositionType::Absolute, right: px(40), bottom: px(BOTTOM), width: px(WIDTH), display: Display::None, ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                Body,
                Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(2), padding: UiRect::new(px(18), px(18), px(10), px(12)), border: UiRect::left(px(3)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.62)),
                BorderColor::all(t.accent),
                UiTransform::default(),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                // Header: icon (banked / broken), title, multiplier.
                c.spawn((Node { align_items: AlignItems::Center, column_gap: px(8), ..default() }, Pickable::IGNORE)).with_children(|h| {
                    h.spawn((HeadIcon, ImageNode { image: icon, color: t.good, ..default() }, Node { width: px(24), height: px(24), display: Display::None, ..default() }, Pickable::IGNORE));
                    h.spawn((Txt::Title, text("RHYTHM", label(24.0), t.accent), Node { flex_grow: 1.0, ..default() }));
                    h.spawn((Txt::Mult, text("\u{d7}1.0", display(40.0), t.accent), UiTransform::default()));
                });
                c.spawn((Txt::Total, text("0", display(52.0), t.text), TextShadow::default()));
                c.spawn((Txt::Sub, text("", label(22.0), t.text_muted), Node { display: Display::None, ..default() }));
                for k in 0..LINES {
                    c.spawn((LineRow(k), Node { column_gap: px(12), justify_content: JustifyContent::SpaceBetween, display: Display::None, ..default() }, UiTransform::default(), Pickable::IGNORE)).with_children(|r| {
                        r.spawn((Txt::LineLabel(k), text("", label(22.0), t.text)));
                        r.spawn((Txt::LinePoints(k), text("", label(22.0), t.accent)));
                    });
                }
                c.spawn((BarTrack, Node { width: percent(100), height: px(5), margin: UiRect::top(px(6)), ..default() }, BackgroundColor(Color::srgba(0.95, 0.93, 0.88, 0.13)), Pickable::IGNORE)).with_children(|b| {
                    b.spawn((BarFill, Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                });
            });
        });
}

/// Folds the scorer's messages into the panel's lines and banner.
fn collect(time: Res<Time>, rhythm: Res<Rhythm>, mut events: MessageReader<StyleEvent>, mut panel: ResMut<Panel>) {
    let dt = time.delta_secs();
    let p = &mut *panel;
    for l in &mut p.lines {
        l.age += dt;
        l.born += dt;
    }
    p.bump += dt;
    if let Some((_, t)) = p.banner.as_mut() {
        *t += dt;
    }
    for e in events.read() {
        match *e {
            StyleEvent::Tick { kind, points, tag, .. } => {
                touch(&mut p.lines, kind, tag).base += points;
                p.banner = None;
            }
            StyleEvent::Bank { points, notes, .. } => {
                p.lines.clear();
                p.banner = Some((Banner::Banked { style: points, notes }, 0.0));
            }
            StyleEvent::Lost { points, why } => {
                p.lines.clear();
                p.banner = Some((Banner::Broken { lost: points, why: why.label() }, 0.0));
            }
        }
    }
    // Running skills keep (or open) their line.
    for (k, &live) in rhythm.live.iter().enumerate() {
        if live > 0 {
            touch(&mut p.lines, LIVE_KINDS[k], StyleTag::None);
            p.banner = None;
        }
    }
    p.lines.retain(|l| l.age < LINE_SECS);
    p.lines.truncate(LINES);
    if p.banner.is_some_and(|(_, t)| t > BANNER_SECS + FADE) {
        p.banner = None;
    }
    if (rhythm.mult - p.mult).abs() > 1e-3 {
        if rhythm.mult > p.mult {
            p.bump = 0.0;
        }
        p.mult = rhythm.mult;
    }
}

/// The line for `kind`, moved to the top (made if new), its age reset.
fn touch(lines: &mut Vec<Line>, kind: StyleKind, tag: StyleTag) -> &mut Line {
    match lines.iter().position(|l| l.kind == kind) {
        Some(0) => {}
        Some(i) => {
            let l = lines.remove(i);
            lines.insert(0, l);
        }
        None => lines.insert(0, Line { kind, tag, base: 0, age: 0.0, born: 0.0 }),
    }
    let l = &mut lines[0];
    l.age = 0.0;
    if tag != StyleTag::None {
        l.tag = tag;
    }
    l
}

/// "12,480".
pub fn commas(s: &mut String, n: u32) {
    let digits = n.to_string();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            s.push(',');
        }
        s.push(ch);
    }
}

type Line3 = [Option<(StyleKind, StyleTag, u32)>; LINES];

/// What was last written (only changes are written).
#[derive(Default)]
struct Shown {
    mode: Option<u8>,
    mult: Option<u32>,
    total: Option<u32>,
    sub: Option<(u8, u32)>,
    lines: Line3,
    bar: Option<u32>,
    alpha: Option<u8>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    theme: Res<Theme>,
    panel: Res<Panel>,
    rhythm: Res<Rhythm>,
    stage: Res<StageWorld>,
    (settings, access, screens, icons): (Option<Res<CareerSettings>>, Option<Res<engine::actions::Accessibility>>, Option<Res<engine::ui::prelude::Screens>>, Option<Res<HudIcons>>),
    mut roots: Query<&mut Node, (With<Root>, Without<LineRow>, Without<BarFill>, Without<HeadIcon>, Without<BarTrack>, Without<Txt>)>,
    mut body: Query<(&mut UiTransform, &mut BackgroundColor, &mut BorderColor), (With<Body>, Without<LineRow>, Without<Txt>, Without<BarFill>)>,
    mut texts: Query<(Entity, &Txt, &mut Text, &mut TextColor, &mut Node, Option<&mut UiTransform>), (Without<Root>, Without<LineRow>, Without<BarFill>, Without<HeadIcon>, Without<BarTrack>, Without<Body>)>,
    mut rows: Query<(&LineRow, &mut Node, &mut UiTransform), (Without<Root>, Without<Txt>, Without<BarFill>, Without<HeadIcon>, Without<BarTrack>, Without<Body>)>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), (With<BarFill>, Without<Root>, Without<Txt>, Without<LineRow>, Without<HeadIcon>, Without<Body>, Without<BarTrack>)>,
    mut track: Query<&mut Node, (With<BarTrack>, Without<Root>, Without<Txt>, Without<LineRow>, Without<HeadIcon>, Without<BarFill>, Without<Body>)>,
    mut icon: Query<(&mut Node, &mut ImageNode), (With<HeadIcon>, Without<Root>, Without<Txt>, Without<LineRow>, Without<BarFill>, Without<BarTrack>, Without<Body>)>,
    mut shown: Local<Shown>,
) {
    let t = &*theme;
    let setting = settings.map_or(0, |s| s.style_hud);
    let calm = access.as_ref().is_some_and(|a| a.reduce_motion);
    // Compact in timed events (UX §3.1) or by the setting.
    let compact = setting == 1 || !stage.free_roam();
    let under_screen = screens.is_some_and(|s| s.top().is_some());
    let banner = panel.banner;
    // 0 hidden, 1 rhythm, 2 banked, 3 broken.
    let mode: u8 = if setting >= 2 || under_screen {
        0
    } else {
        match banner {
            Some((Banner::Banked { .. }, _)) => 2,
            Some((Banner::Broken { .. }, _)) => 3,
            None if rhythm.open && rhythm.at_stake() > 0 => 1,
            None => 0,
        }
    };
    let mode_changed = shown.mode != Some(mode);
    if mode_changed {
        shown.mode = Some(mode);
        shown.alpha = None;
        for mut n in &mut roots {
            n.display = if mode == 0 { Display::None } else { Display::Flex };
        }
    }
    if mode == 0 {
        return;
    }
    // Fade (banners) and shake (broken).
    let age = banner.map_or(0.0, |(_, a)| a);
    let alpha = if mode >= 2 { (1.0 - (age - BANNER_SECS) / FADE).clamp(0.0, 1.0) } else { 1.0 };
    let alpha_q = (alpha * 50.0).round() as u8;
    let fading = shown.alpha != Some(alpha_q);
    shown.alpha = Some(alpha_q);
    let edge = match mode {
        2 => t.good,
        3 => t.bad,
        _ => t.accent,
    };
    for (mut tf, mut bg, mut border) in &mut body {
        let shake = if mode == 3 && !calm && age < 0.2 { 4.0 * (age * 90.0).sin() * (1.0 - age / 0.2) } else { 0.0 };
        let want = Val2::px(shake, 0.0);
        if tf.translation != want {
            tf.translation = want;
        }
        if fading || mode_changed {
            bg.0 = Color::srgba(0.02, 0.03, 0.025, 0.62 * alpha);
            *border = BorderColor::all(edge.with_alpha(alpha));
        }
    }
    if mode_changed || fading {
        for (mut n, mut img) in &mut icon {
            n.display = if mode >= 2 { Display::Flex } else { Display::None };
            if let Some(i) = icons.as_ref() {
                img.image = i.get(if mode == 3 { Icon::Cross } else { Icon::Check });
            }
            img.color = edge.with_alpha(alpha);
        }
        for mut n in &mut track {
            n.display = if mode == 1 { Display::Flex } else { Display::None };
        }
    }

    // Values.
    let mult = (rhythm.mult * 10.0).round() as u32;
    let (total, sub) = match banner {
        // (A bank too small to pay shows no Notes line.)
        Some((Banner::Banked { style, notes }, _)) if mode == 2 => (style, (if notes > 0 { 2u8 } else { 0 }, notes)),
        Some((Banner::Broken { lost, .. }, _)) if mode == 3 => (lost, (3u8, 0)),
        _ => (rhythm.at_stake(), (0u8, 0)),
    };
    let why = match banner {
        Some((Banner::Broken { why, .. }, _)) => why,
        _ => "",
    };
    let bump = if calm { 1.0 } else { 1.0 + 0.15 * (1.0 - panel.bump / 0.15).clamp(0.0, 1.0) };
    let lines: Line3 = std::array::from_fn(|k| {
        if mode != 1 || compact {
            return None;
        }
        panel.lines.get(k).map(|l| {
            let live = LIVE_KINDS.iter().position(|&lk| lk == l.kind).map_or(0, |i| rhythm.live[i]);
            (l.kind, l.tag, l.base + live)
        })
    });
    let recolour = mode_changed || fading;
    for (e, which, mut txt, mut col, mut node, tf) in &mut texts {
        match *which {
            Txt::Title => {
                if mode_changed {
                    txt.0.clear();
                    txt.0.push_str(match mode {
                        2 => "BANKED",
                        3 => "BROKEN",
                        _ => "RHYTHM",
                    });
                }
                if recolour {
                    col.0 = edge.with_alpha(alpha);
                }
            }
            Txt::Mult => {
                if shown.mult != Some(mult) || mode_changed {
                    txt.0.clear();
                    let _ = write!(txt.0, "\u{d7}{:.1}", mult as f32 / 10.0);
                    node.display = if mode == 3 { Display::None } else { Display::Flex };
                }
                if recolour {
                    col.0 = edge.with_alpha(alpha);
                }
                if let Some(mut tf) = tf {
                    let s = Vec2::splat(bump);
                    if tf.scale != s {
                        tf.scale = s;
                    }
                }
            }
            Txt::Total => {
                if shown.total != Some(total) || mode_changed {
                    txt.0.clear();
                    commas(&mut txt.0, total);
                    if mode == 3 {
                        commands.entity(e).insert((Strikethrough, StrikethroughColor(t.bad)));
                    } else {
                        commands.entity(e).remove::<(Strikethrough, StrikethroughColor)>();
                    }
                }
                if recolour {
                    col.0 = if mode == 3 { t.bad } else { t.text }.with_alpha(alpha);
                }
            }
            Txt::Sub => {
                if shown.sub != Some(sub) || mode_changed {
                    txt.0.clear();
                    match sub.0 {
                        2 => {
                            txt.0.push('+');
                            commas(&mut txt.0, sub.1);
                            txt.0.push_str(" NOTES");
                        }
                        3 => txt.0.push_str(&why.to_uppercase()),
                        _ => {}
                    }
                    node.display = if sub.0 == 0 { Display::None } else { Display::Flex };
                }
                if recolour {
                    col.0 = if mode == 3 { t.bad } else { t.good }.with_alpha(alpha);
                }
            }
            Txt::LineLabel(k) if shown.lines[k] != lines[k] => {
                txt.0.clear();
                if let Some((kind, tag, _)) = lines[k] {
                    txt.0.push_str(&tag.label().unwrap_or(kind.label()).to_uppercase());
                }
            }
            Txt::LinePoints(k) if shown.lines[k] != lines[k] => {
                txt.0.clear();
                if let Some((_, _, p)) = lines[k] {
                    txt.0.push('+');
                    commas(&mut txt.0, p);
                }
            }
            _ => {}
        }
    }
    shown.mult = Some(mult);
    shown.total = Some(total);
    shown.sub = Some(sub);
    for (row, mut node, mut tf) in &mut rows {
        let on = lines[row.0].is_some();
        let d = if on { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
        // A new line slides in from the right (12 px over 120 ms).
        let born = panel.lines.get(row.0).map_or(1.0, |l| l.born);
        let x = if calm || !on { 0.0 } else { 12.0 * (1.0 - born / 0.12).clamp(0.0, 1.0) };
        let want = Val2::px(x, 0.0);
        if tf.translation != want {
            tf.translation = want;
        }
    }
    shown.lines = lines;
    // The timeout bar.
    let bar = ((rhythm.timer / RHYTHM_TIMEOUT).clamp(0.0, 1.0) * 200.0).round() as u32;
    if shown.bar != Some(bar) {
        shown.bar = Some(bar);
        for (mut n, mut bg) in &mut fill {
            n.width = percent(bar as f32 / 2.0);
            // The last second: the bar turns paper white (about to bank).
            bg.0 = if rhythm.timer < 1.0 && rhythm.live.iter().all(|&p| p == 0) { t.text } else { t.accent };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_get_commas() {
        let f = |n| {
            let mut s = String::new();
            commas(&mut s, n);
            s
        };
        assert_eq!(f(0), "0");
        assert_eq!(f(999), "999");
        assert_eq!(f(1000), "1,000");
        assert_eq!(f(48250), "48,250");
        assert_eq!(f(1234567), "1,234,567");
    }

    #[test]
    fn lines_repeat_in_place_newest_on_top() {
        let mut lines = Vec::new();
        touch(&mut lines, StyleKind::Slide, StyleTag::None).base += 100;
        touch(&mut lines, StyleKind::Air, StyleTag::BigAir).base += 300;
        touch(&mut lines, StyleKind::Slide, StyleTag::None).base += 50;
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, StyleKind::Slide);
        assert_eq!(lines[0].base, 150);
        assert_eq!(lines[1].tag, StyleTag::BigAir);
    }
}
