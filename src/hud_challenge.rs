//! HUD: challenge approach cards, live readouts and result cards (top centre).
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §3, `docs/festival/UX_PASS.md` §3.2, `docs/festival/DECISIONS.md` C1.
//! A read-only view of F's state; E owns `hud.rs` and the shared HUD look.
//! Despawn on leaving `InGame`; allocate nothing per frame while values are unchanged.
//!
//! One card, top centre, in one of four states (free roam only; HUD › Challenge cards):
//! - **approach** (within 200 m, ahead ±45°, nearest wins, 5 s): `RADAR POST · Mill Straight`,
//!   then `BEST 142 km/h ★★☆ · NEXT ★ 155`;
//! - **live** (in a section): the value so far large, a bar with a notch per star threshold
//!   (the threshold printed under it), the distance left;
//! - **result** (3.5 s): the value large, three stars filling left to right 150 ms apart (at
//!   once with reduce motion), `NEW BEST`, `+400 N`;
//! - **void** (2 s): `FAILED · Missed a gate`.

use std::fmt::Write;

use bevy::prelude::*;
use engine::settings::{FieldDef, FieldKind, RegisterSettings};
use engine::ui::theme::*;

use crate::career::CareerSettings;
use crate::drive::DrivenCar;
use crate::hud_style::commas;
use crate::open::challenges::{APPROACH_M, Bests, ChallengeKind, ChallengeRun, Challenges, Placed};
use crate::world::StageWorld;
use icons::{HudIcons, Icon};

const WIDTH: f32 = 600.0;
const APPROACH_SECS: f32 = 5.0;
const RESULT_SECS: f32 = 3.5;
const VOID_SECS: f32 = 2.0;
const STAR_STEP: f32 = 0.15;

pub struct ChallengeHudPlugin;

impl Plugin for ChallengeHudPlugin {
    fn build(&self, app: &mut App) {
        app.register_setting(FieldDef {
            key: "challenge_cards",
            tab: "HUD",
            label: "Challenge cards",
            hint: "Roadside challenges: the card as you approach one, the live readout and the result. Results still count when it's off.",
            kind: FieldKind::Toggle,
            get: |w| if w.get_resource::<CareerSettings>().is_none_or(|s| s.challenge_cards) { 1.0 } else { 0.0 },
            set: |w, v| {
                if let Some(mut s) = w.get_resource_mut::<CareerSettings>() {
                    s.challenge_cards = v > 0.5;
                }
            },
        })
        .add_systems(Startup, icons::make)
        .add_systems(Update, (spawn, draw).chain().run_if(resource_exists::<StageWorld>));
    }
}

#[derive(Component)]
struct Root;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Txt {
    Kind,
    Name,
    Big,
    Unit,
    Line,
    Tag,
    Tick(usize),
}
#[derive(Component)]
struct KindChip;
#[derive(Component)]
struct StarIcon(usize);
#[derive(Component)]
struct Bar;
#[derive(Component)]
struct BarFill;
#[derive(Component)]
struct TickMark(usize);
#[derive(Component)]
struct BigRow;

fn spawn(mut commands: Commands, theme: Res<Theme>, icons: Option<Res<HudIcons>>, q: Query<(), With<Root>>) {
    if !q.is_empty() {
        return;
    }
    let t = &*theme;
    let hollow = icons.as_ref().map(|i| i.get(Icon::StarHollow)).unwrap_or_default();
    commands
        .spawn((
            Root,
            Name::new("HUD challenge"),
            DespawnOnExit(engine::flow::AppState::InGame),
            Node {
                position_type: PositionType::Absolute,
                top: px(40),
                left: percent(50),
                width: px(WIDTH),
                margin: UiRect::left(px(-WIDTH / 2.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                padding: UiRect::new(px(20), px(20), px(12), px(14)),
                border: UiRect::top(px(3)),
                border_radius: BorderRadius::all(px(RADIUS)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.62)),
            BorderColor::all(t.accent),
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            // Kind and name.
            c.spawn((Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((KindChip, Node { width: px(12), height: px(12), border_radius: BorderRadius::all(px(2)), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                r.spawn((Txt::Kind, text("", label(22.0), t.accent)));
                r.spawn((Txt::Name, text("", label(22.0), t.text_muted)));
            });
            // The value, its unit, the stars and a tag.
            c.spawn((BigRow, Node { align_items: AlignItems::Center, column_gap: px(12), ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((Txt::Big, text("", display(56.0), t.text), TextShadow::default()));
                r.spawn((Txt::Unit, text("", label(22.0), t.text_muted), Node { margin: UiRect::top(px(14)), ..default() }));
                r.spawn((Node { column_gap: px(4), margin: UiRect::left(px(8)), ..default() }, Pickable::IGNORE)).with_children(|st| {
                    for k in 0..3 {
                        st.spawn((StarIcon(k), ImageNode { image: hollow.clone(), color: t.accent, ..default() }, Node { width: px(34), height: px(34), ..default() }, Pickable::IGNORE));
                    }
                });
                r.spawn((Txt::Tag, text("", label(22.0), t.accent), Node { margin: UiRect::left(px(8)), padding: UiRect::axes(px(8), px(2)), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(RADIUS)), display: Display::None, ..default() }, BorderColor::all(t.accent)));
            });
            // The live bar: value against the 3★ mark, a notch per star.
            c.spawn((Bar, Node { width: percent(100), height: px(30), margin: UiRect::top(px(2)), display: Display::None, ..default() }, Pickable::IGNORE)).with_children(|b| {
                b.spawn((Node { position_type: PositionType::Absolute, top: px(0), width: percent(100), height: px(6), ..default() }, BackgroundColor(Color::srgba(0.95, 0.93, 0.88, 0.13)), Pickable::IGNORE)).with_children(|tr| {
                    tr.spawn((BarFill, Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                });
                for k in 0..3 {
                    b.spawn((TickMark(k), Node { position_type: PositionType::Absolute, top: px(-3), left: percent(0), width: px(3), height: px(12), ..default() }, BackgroundColor(t.text), Pickable::IGNORE));
                    b.spawn((Txt::Tick(k), text("", label(18.0), t.text_muted), Node { position_type: PositionType::Absolute, top: px(10), left: percent(0), ..default() }));
                }
            });
            c.spawn((Txt::Line, text("", label(22.0), t.text_muted)));
        });
}

/// The card's state.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Card {
    Hidden,
    Approach { idx: usize },
    Live { idx: usize },
    Result { idx: usize, age: f32 },
    Void { idx: usize, why: &'static str },
}

/// The challenge to announce: within [`APPROACH_M`], ahead within ±45°; a section only when
/// driven its way.
fn approaching(placed: &[Placed], pos: Vec3, fwd: Vec3) -> Option<usize> {
    let f = Vec2::new(fwd.x, fwd.z).normalize_or_zero();
    placed
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let g = p.first();
            let d = Vec2::new(g.p.x - pos.x, g.p.z - pos.z);
            let dist = d.length();
            let ahead = d.dot(f) >= dist * std::f32::consts::FRAC_1_SQRT_2;
            let way = !p.def.kind.section() || g.dir.dot(f) > 0.5;
            (dist <= APPROACH_M && dist > 3.0 && ahead && way).then_some((i, dist))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A value as shown: speeds in the player's unit.
fn shown_value(kind: ChallengeKind, v: f32, mph: bool) -> f32 {
    match kind {
        ChallengeKind::RadarPost | ChallengeKind::PaceSection if mph => v / 1.609_344,
        _ => v,
    }
}

fn unit(kind: ChallengeKind, mph: bool) -> &'static str {
    match kind {
        ChallengeKind::RadarPost | ChallengeKind::PaceSection => {
            if mph {
                "MPH"
            } else {
                "KM/H"
            }
        }
        ChallengeKind::SlideSection => "PTS",
        ChallengeKind::YumpBoard => "M",
    }
}

fn number(s: &mut String, kind: ChallengeKind, v: f32) {
    if kind == ChallengeKind::YumpBoard {
        let _ = write!(s, "{v:.1}");
    } else {
        commas(s, v.round().max(0.0) as u32);
    }
}

/// What was last written.
#[derive(Default)]
struct Shown {
    card: Option<(u8, usize)>,
    big: Option<i64>,
    line: Option<(u8, i64, i64)>,
    stars: Option<u8>,
    fill: Option<u32>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    time: Res<Time>,
    theme: Res<Theme>,
    stage: Res<StageWorld>,
    run: Res<ChallengeRun>,
    (challenges, bests, settings, prefs): (Option<Res<Challenges>>, Option<Res<Bests>>, Option<Res<CareerSettings>>, Option<Res<crate::prefs::Prefs>>),
    (access, screens, icons): (Option<Res<engine::actions::Accessibility>>, Option<Res<engine::ui::prelude::Screens>>, Option<Res<HudIcons>>),
    cars: Query<&DrivenCar>,
    mut root: Query<(&mut Node, &mut BorderColor), (With<Root>, Without<Txt>, Without<Bar>, Without<BarFill>, Without<TickMark>)>,
    mut texts: Query<(&Txt, &mut Text, &mut TextColor, &mut Node), (Without<Root>, Without<Bar>, Without<BarFill>, Without<TickMark>)>,
    mut chip: Query<&mut BackgroundColor, (With<KindChip>, Without<BarFill>)>,
    mut stars: Query<(&StarIcon, &mut ImageNode, &mut Node), (Without<Root>, Without<Txt>, Without<Bar>, Without<BarFill>, Without<TickMark>)>,
    mut bar: Query<&mut Node, (With<Bar>, Without<Root>, Without<Txt>, Without<BarFill>, Without<TickMark>, Without<StarIcon>)>,
    mut fill: Query<&mut Node, (With<BarFill>, Without<Root>, Without<Txt>, Without<Bar>, Without<TickMark>, Without<StarIcon>)>,
    mut ticks: Query<(&TickMark, &mut Node), (Without<Root>, Without<Txt>, Without<Bar>, Without<BarFill>, Without<StarIcon>)>,
    mut approach: Local<(Option<usize>, f32)>,
    mut shown: Local<Shown>,
) {
    let t = &*theme;
    let Some(ch) = challenges else { return };
    let placed = &ch.0;
    let on = settings.as_ref().is_none_or(|s| s.challenge_cards);
    let under = screens.is_some_and(|s| s.top().is_some());
    let mph = prefs.as_ref().is_some_and(|p| p.mph);
    let calm = access.as_ref().is_some_and(|a| a.reduce_motion);
    let car = cars.single().ok().map(|c| (c.car.pos, c.car.forward()));
    // Which card.
    let near = car.and_then(|(p, f)| approaching(placed, p, f));
    if near != approach.0 {
        *approach = (near, 0.0);
    } else {
        approach.1 += time.delta_secs();
    }
    let card = if !on || under || !stage.free_roam() {
        Card::Hidden
    } else if let Some(l) = run.live {
        Card::Live { idx: l.idx }
    } else if let Some((idx, _, age)) = run.last.filter(|l| l.2 < RESULT_SECS) {
        Card::Result { idx, age }
    } else if let Some((idx, why, _)) = run.void.filter(|v| v.2 < VOID_SECS) {
        Card::Void { idx, why }
    } else if let (Some(idx), true) = (approach.0, approach.1 < APPROACH_SECS) {
        Card::Approach { idx }
    } else {
        Card::Hidden
    };
    let (tag, idx) = match card {
        Card::Hidden => (0u8, 0),
        Card::Approach { idx } => (1, idx),
        Card::Live { idx } => (2, idx),
        Card::Result { idx, .. } => (3, idx),
        Card::Void { idx, .. } => (4, idx),
    };
    let changed = shown.card != Some((tag, idx));
    if changed {
        *shown = Shown { card: Some((tag, idx)), ..default() };
        for (mut n, mut b) in &mut root {
            n.display = if tag == 0 { Display::None } else { Display::Flex };
            if let Some(p) = placed.get(idx) {
                *b = BorderColor::all(if tag == 4 { t.bad } else { p.def.kind.color() });
            }
        }
    }
    let Some(p) = placed.get(idx).filter(|_| tag != 0) else { return };
    let kind = p.def.kind;
    let best = bests.as_ref().and_then(|b| b.get(p.def.id).copied());
    let thr = p.def.thresholds;
    // The big value (×10 to compare), its stars.
    let (big, nstars): (Option<f32>, u8) = match card {
        Card::Live { .. } => (run.live.map(|l| l.value), 0),
        Card::Result { age, .. } => {
            let r = run.last.map(|l| l.1).unwrap();
            let filled = if calm { r.result.stars } else { ((age / STAR_STEP) as u8).min(r.result.stars) };
            (Some(r.result.value), filled)
        }
        Card::Approach { .. } => (None, best.map_or(0, |b| b.stars)),
        _ => (None, 0),
    };
    let big_key = big.map_or(i64::MIN, |v| (shown_value(kind, v, mph) * 10.0).round() as i64);
    let big_changed = shown.big != Some(big_key);
    shown.big = Some(big_key);
    let line_key = match card {
        Card::Approach { .. } => (1u8, best.map_or(-1, |b| (b.value * 10.0) as i64), 0),
        Card::Live { .. } => (2, run.live.map_or(0, |l| ((1.0 - l.done) * p.length / 10.0) as i64), 0),
        Card::Result { .. } => (3, run.last.map_or(0, |l| l.1.notes as i64), 0),
        Card::Void { .. } => (4, 0, 0),
        Card::Hidden => (0, 0, 0),
    };
    let line_changed = shown.line != Some(line_key);
    shown.line = Some(line_key);
    if changed {
        for mut c in &mut chip {
            c.0 = kind.color();
        }
        for mut n in &mut bar {
            n.display = if tag == 2 { Display::Flex } else { Display::None };
        }
        // Notches at each star's share of the bar (which runs to 110 % of 3★).
        let top = thr[2] * 1.1;
        for (m, mut n) in &mut ticks {
            n.left = percent(thr[m.0] / top * 100.0);
        }
    }
    if shown.stars != Some(nstars) || changed {
        shown.stars = Some(nstars);
        let show = matches!(card, Card::Result { .. } | Card::Approach { .. });
        for (s, mut img, mut n) in &mut stars {
            n.display = if show { Display::Flex } else { Display::None };
            if let Some(i) = icons.as_ref() {
                img.image = i.get(if (s.0 as u8) < nstars { Icon::Star } else { Icon::StarHollow });
            }
            img.color = t.accent;
        }
    }
    if tag == 2 {
        let v = run.live.map_or(0.0, |l| l.value);
        let f = ((v / (thr[2] * 1.1)).clamp(0.0, 1.0) * 400.0) as u32;
        if shown.fill != Some(f) {
            shown.fill = Some(f);
            for mut n in &mut fill {
                n.width = percent(f as f32 / 4.0);
            }
        }
    }
    let new_best = run.last.is_some_and(|l| l.1.result.new_best);
    for (which, mut txt, mut col, mut node) in &mut texts {
        match *which {
            Txt::Kind if changed => {
                txt.0.clear();
                txt.0.push_str(&kind.label().to_uppercase());
                col.0 = kind.color();
            }
            Txt::Name if changed => {
                txt.0.clear();
                let _ = write!(txt.0, "\u{b7}  {}", p.def.name.to_uppercase());
            }
            Txt::Big if big_changed || changed => {
                txt.0.clear();
                match (card, big) {
                    (Card::Void { .. }, _) => txt.0.push_str("FAILED"),
                    (_, Some(v)) => number(&mut txt.0, kind, shown_value(kind, v, mph)),
                    (Card::Approach { .. }, None) => {
                        // The first star to aim for.
                        let next = thr.iter().copied().find(|&x| best.is_none_or(|b| b.value < x)).unwrap_or(thr[2]);
                        txt.0.push_str("NEXT ");
                        number(&mut txt.0, kind, shown_value(kind, next, mph));
                    }
                    _ => {}
                }
                col.0 = if matches!(card, Card::Void { .. }) { t.bad } else { t.text };
            }
            Txt::Unit if changed => {
                txt.0.clear();
                if !matches!(card, Card::Void { .. }) {
                    txt.0.push_str(unit(kind, mph));
                }
            }
            Txt::Tag if changed || big_changed => {
                let on = matches!(card, Card::Result { .. }) && new_best;
                node.display = if on { Display::Flex } else { Display::None };
                if on {
                    txt.0.clear();
                    txt.0.push_str("NEW BEST");
                }
            }
            Txt::Tick(k) if changed => {
                // The threshold under its notch (the notch's place on the bar is the cue).
                txt.0.clear();
                number(&mut txt.0, kind, shown_value(kind, thr[k], mph));
                node.margin = UiRect::left(px(-8));
            }
            Txt::Line if line_changed || changed => {
                txt.0.clear();
                col.0 = t.text_muted;
                match card {
                    Card::Approach { .. } => match best {
                        Some(b) => {
                            txt.0.push_str("BEST ");
                            number(&mut txt.0, kind, shown_value(kind, b.value, mph));
                            let _ = write!(txt.0, " {}  \u{b7}  DRIVE THROUGH TO START", unit(kind, mph));
                        }
                        None => txt.0.push_str("NO RESULT YET  \u{b7}  DRIVE THROUGH TO START"),
                    },
                    Card::Live { .. } => {
                        let left = run.live.map_or(0.0, |l| (1.0 - l.done) * p.length);
                        let _ = write!(txt.0, "{:.1} KM TO THE FINISH", left / 1000.0);
                    }
                    Card::Result { .. } => {
                        let notes = run.last.map_or(0, |l| l.1.notes);
                        if notes > 0 {
                            txt.0.push('+');
                            commas(&mut txt.0, notes);
                            txt.0.push_str(" NOTES");
                            col.0 = t.good;
                        } else {
                            txt.0.push_str("NO NEW STAR");
                        }
                    }
                    Card::Void { why, .. } => {
                        txt.0.push_str(&why.to_uppercase());
                        col.0 = t.bad;
                    }
                    Card::Hidden => {}
                }
            }
            _ => {}
        }
    }
}

/// Small HUD glyphs the fonts don't have (★ ☆ ✓ ✕), drawn into images at startup. White with
/// alpha coverage: tint them with `ImageNode::color`.
pub mod icons {
    use bevy::asset::RenderAssetUsages;
    use bevy::prelude::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    const N: u32 = 64;
    /// Supersamples per axis.
    const SS: u32 = 4;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Icon {
        Star,
        StarHollow,
        Check,
        Cross,
    }

    #[derive(Resource, Clone)]
    pub struct HudIcons([Handle<Image>; 4]);

    impl HudIcons {
        pub fn get(&self, i: Icon) -> Handle<Image> {
            self.0[i as usize].clone()
        }
    }

    pub(super) fn make(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
        let Some(mut images) = images else { return };
        let mut img = |i: Icon| images.add(image(&raster(i)));
        commands.insert_resource(HudIcons([img(Icon::Star), img(Icon::StarHollow), img(Icon::Check), img(Icon::Cross)]));
    }

    fn image(cover: &[u8]) -> Image {
        let data: Vec<u8> = cover.iter().flat_map(|&a| [255, 255, 255, a]).collect();
        Image::new(Extent3d { width: N, height: N, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
    }

    /// The five-pointed star (unit square, y down), scaled about the centre.
    fn star(scale: f32) -> [Vec2; 10] {
        std::array::from_fn(|k| {
            let r = if k % 2 == 0 { 0.48 } else { 0.2 } * scale;
            let a = std::f32::consts::PI * (k as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
            Vec2::new(0.5, 0.53) + Vec2::new(a.cos(), a.sin()) * r
        })
    }

    fn inside(poly: &[Vec2], p: Vec2) -> bool {
        let mut c = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[j]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                c = !c;
            }
            j = i;
        }
        c
    }

    fn seg(p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        p.distance(a + ab * t)
    }

    /// Coverage (0..255) per pixel.
    pub fn raster(i: Icon) -> Vec<u8> {
        let (outer, inner) = (star(1.0), star(0.62));
        let hit = |p: Vec2| match i {
            Icon::Star => inside(&outer, p),
            Icon::StarHollow => inside(&outer, p) && !inside(&inner, p),
            Icon::Check => seg(p, Vec2::new(0.16, 0.52), Vec2::new(0.4, 0.76)).min(seg(p, Vec2::new(0.4, 0.76), Vec2::new(0.86, 0.24))) < 0.085,
            Icon::Cross => seg(p, Vec2::new(0.2, 0.2), Vec2::new(0.8, 0.8)).min(seg(p, Vec2::new(0.8, 0.2), Vec2::new(0.2, 0.8))) < 0.085,
        };
        let mut out = vec![0u8; (N * N) as usize];
        for y in 0..N {
            for x in 0..N {
                let mut n = 0;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let p = Vec2::new((x * SS + sx) as f32 + 0.5, (y * SS + sy) as f32 + 0.5) / (N * SS) as f32;
                        n += hit(p) as u32;
                    }
                }
                out[(y * N + x) as usize] = (n * 255 / (SS * SS)) as u8;
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn icons_have_ink_and_a_hollow_star_has_a_hole() {
            for i in [Icon::Star, Icon::StarHollow, Icon::Check, Icon::Cross] {
                let r = raster(i);
                let ink = r.iter().filter(|&&a| a > 128).count();
                assert!(ink > 200 && ink < (N * N) as usize / 2, "{i:?}: {ink}");
            }
            let full = raster(Icon::Star);
            let hollow = raster(Icon::StarHollow);
            let centre = (N / 2 * N + N / 2) as usize;
            assert_eq!(full[centre], 255);
            assert_eq!(hollow[centre], 0);
        }
    }
}
