//! The car screens: Garage, Upgrades and Dealership.
//!
//! **Owner: stream E (UI).** Design: `docs/festival/UX_PASS.md` §4, `GAME_SYSTEMS.md` §4 and
//! §6, `DECISIONS.md` E1-E12 and U4.
//!
//! All three: a list left, the car on the plinth in the middle (the showroom, framed for the
//! screen), a stats panel right, key hints in the footer, the shared top bar. They only read
//! `career::{Wallet, Garage}` and send `career::CareerRequest`s (purchases after the confirm
//! modal); F applies them, saves, and the screens rebuild when the career changes.
//!
//! - **Garage**: owned cars (`car:0..15`); Enter drives the focused car; paint (7 swatches,
//!   solid or metallic), race number, repair and sell for the car you drive.
//! - **Upgrades**: 7 rows `part:0..6` (Engine, Turbo, Tyres, Brakes, Suspension, Weight,
//!   Gearbox) × Stock / Street / Gravel / Works; ←/→ preview a level (the stats panel shows the
//!   change as hollow segments and +n), Enter fits it (buying missing levels below it; owned
//!   levels refit free). Too few Notes: the price shows ✕ in red and the dialog says how many
//!   are missing.
//! - **Dealership**: every car of the roster (`dealer:0..15`) with class, PI, price; Buy asks
//!   first.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use engine::ui::prelude::*;
use engine::ui::widgets::{ActionButton, Focus, Focusable, KeepColor, LookBg, NavInput, NavQueue, ScrollList, nav_look, secondary_look};

use super::confirm::{self, Confirm, OnYes};
use super::hub::{self, Glyph, Glyphs, class_plate, glyph, notes, thousands};
use super::showroom::{PAINTS, Shot, ShowroomCar};
use crate::audio::ui::UiSound;
use crate::career::CareerRequest;
use crate::career::garage::{self, CarDef, Class, Garage, OwnedCar, Paint, Refusal, UpgradeKind, level_name};
use crate::career::wallet::Wallet;

pub const CAR_IDS: [&str; 16] = ["car:0", "car:1", "car:2", "car:3", "car:4", "car:5", "car:6", "car:7", "car:8", "car:9", "car:10", "car:11", "car:12", "car:13", "car:14", "car:15"];
pub const DEALER_IDS: [&str; 16] = [
    "dealer:0", "dealer:1", "dealer:2", "dealer:3", "dealer:4", "dealer:5", "dealer:6", "dealer:7", "dealer:8", "dealer:9", "dealer:10", "dealer:11", "dealer:12", "dealer:13", "dealer:14", "dealer:15",
];
pub const PART_IDS: [&str; 7] = ["part:0", "part:1", "part:2", "part:3", "part:4", "part:5", "part:6"];
/// Level cells, `lvl:<part>:<level>` (mouse: click previews, a second click fits).
const LEVEL_IDS: [[&str; 4]; 7] = [
    ["lvl:0:0", "lvl:0:1", "lvl:0:2", "lvl:0:3"],
    ["lvl:1:0", "lvl:1:1", "lvl:1:2", "lvl:1:3"],
    ["lvl:2:0", "lvl:2:1", "lvl:2:2", "lvl:2:3"],
    ["lvl:3:0", "lvl:3:1", "lvl:3:2", "lvl:3:3"],
    ["lvl:4:0", "lvl:4:1", "lvl:4:2", "lvl:4:3"],
    ["lvl:5:0", "lvl:5:1", "lvl:5:2", "lvl:5:3"],
    ["lvl:6:0", "lvl:6:1", "lvl:6:2", "lvl:6:3"],
];
const PAINT_IDS: [&str; 7] = ["paint:0", "paint:1", "paint:2", "paint:3", "paint:4", "paint:5", "paint:6"];

/// Stat bar names, in `garage::stat_bars` order.
const STATS: [&str; 5] = ["Power", "Grip", "Handling", "Braking", "Weight"];
/// Segments per stat bar.
const SEGS: usize = 10;

/// What the car screens have selected (focused): an owned car, a roster car, a part and the
/// level previewed on each part row.
#[derive(Resource, Clone, Debug, Default)]
pub struct CarSel {
    pub owned: usize,
    pub dealer: usize,
    pub part: usize,
    /// Previewed level per part (`None` = what's fitted).
    pub preview: [Option<u8>; 7],
}

#[derive(Component, Clone, Copy)]
struct OwnedRow(usize);
#[derive(Component, Clone, Copy)]
struct DealerRow(usize);
#[derive(Component, Clone, Copy)]
struct PartRow(usize);

pub struct GarageUiPlugin;

impl Plugin for GarageUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CarSel>()
            .register_screen("garage", build_garage)
            .register_screen("upgrades", build_upgrades)
            .register_screen("dealer", build_dealer)
            .add_systems(Update, part_keys.before(engine::ui::UiSet::Input).run_if(engine::ui::ui_visible))
            .add_systems(Update, track_focus.after(engine::ui::UiSet::Paint));
    }
}

// ---------------------------------------------------------------- events

/// Menu ids of the car screens. Returns true when `id` was one of them.
pub fn on_menu(world: &mut World, id: &'static str) -> bool {
    if let Some(i) = id.strip_prefix("car:").and_then(|n| n.parse::<usize>().ok()) {
        let g = world.resource::<Garage>();
        if i < g.owned.len() && i != g.active {
            world.write_message(CareerRequest::SetActiveCar(i));
        }
        return true;
    }
    if let Some(k) = id.strip_prefix("dealer:").and_then(|n| n.parse::<usize>().ok()) {
        buy_car(world, k);
        return true;
    }
    if let Some(p) = id.strip_prefix("part:").and_then(|n| n.parse::<usize>().ok()) {
        fit(world, p);
        return true;
    }
    if let Some(rest) = id.strip_prefix("lvl:") {
        let mut it = rest.split(':').filter_map(|n| n.parse::<usize>().ok());
        if let (Some(p), Some(l)) = (it.next(), it.next()) {
            let sel = world.resource::<CarSel>().clone();
            let cur = level_shown(world, &sel, p);
            if cur == l as u8 && sel.part == p {
                fit(world, p);
            } else {
                let mut s = world.resource_mut::<CarSel>();
                s.part = p;
                s.preview[p] = Some(l as u8);
                world.write_message(RebuildScreen);
            }
        }
        return true;
    }
    if let Some(k) = id.strip_prefix("paint:").and_then(|n| n.parse::<u8>().ok()) {
        let metallic = world.resource::<Garage>().active_car().paint.metallic;
        world.write_message(CareerRequest::Paint(Paint { colour: k, metallic }));
        return true;
    }
    match id {
        "metallic" => {
            let p = world.resource::<Garage>().active_car().paint;
            world.write_message(CareerRequest::Paint(Paint { metallic: !p.metallic, ..p }));
        }
        "num_up" | "num_down" => {
            let n = world.resource::<Garage>().active_car().number.clamp(1, 99);
            let next = if id == "num_up" { n % 99 + 1 } else { (n + 97) % 99 + 1 };
            world.write_message(CareerRequest::SetNumber(next));
        }
        "service" => {
            world.write_message(CareerRequest::Service);
        }
        "sell" => sell(world),
        "fit" => {
            let p = world.resource::<CarSel>().part;
            fit(world, p);
        }
        "buy" => {
            let k = world.resource::<CarSel>().dealer;
            buy_car(world, k);
        }
        _ => return false,
    }
    true
}

/// Why a purchase is refused, in the player's words.
pub fn refusal_text(r: Refusal) -> String {
    match r {
        Refusal::Notes { need } => format!("Need {} more", notes(need)),
        Refusal::Owned => "Already in your garage".into(),
        Refusal::MaxLevel => "Already at the top level".into(),
        Refusal::Unknown => "Not in the Dealership".into(),
    }
}

/// The level shown on part row `p`: the preview, else what's fitted.
fn level_shown(world: &World, sel: &CarSel, p: usize) -> u8 {
    let kind = UpgradeKind::ALL[p];
    sel.preview[p].unwrap_or_else(|| world.resource::<Garage>().active_car().fitted.get(kind)).min(kind.max_level())
}

fn fit(world: &mut World, p: usize) {
    let Some(&kind) = UpgradeKind::ALL.get(p) else { return };
    let sel = world.resource::<CarSel>().clone();
    let level = level_shown(world, &sel, p);
    let (g, w) = (world.resource::<Garage>().clone(), world.resource::<Wallet>().clone());
    let car = g.active_car();
    if car.fitted.get(kind) == level {
        world.write_message(UiSound::Error);
        world.write_message(Toast::new(format!("{} {} is fitted", level_name(level), kind.name().to_lowercase())).body("Pick another level with Left / Right."));
        return;
    }
    let what = if level == 0 { format!("stock {}", kind.name().to_lowercase()) } else { format!("{} {}", level_name(level), kind.name().to_lowercase()) };
    let name = car.def().map_or("car", |d| d.name);
    match g.can_fit(&w, kind, level) {
        Ok(0) => {
            world.write_message(CareerRequest::FitPart { kind, level });
            world.write_message(Toast::new(format!("Fitted {what}")).body(format!("Owned already: no charge. On the {name}.")));
            world.resource_mut::<CarSel>().preview[p] = None;
        }
        Ok(cost) => {
            let class = Class::of(garage::pi(&garage::spec_of(car)));
            let have = car.bought.get(kind);
            let mut lines = vec![format!("{} \u{b7} balance after {}", notes(cost), notes(w.balance - cost))];
            if level > have + 1 {
                let below: Vec<String> = (have + 1..level).filter_map(|l| garage::part_price(kind, l, class).map(|pr| format!("{} {}", level_name(l), notes(pr)))).collect();
                lines.push(format!("Includes the levels below it: {}.", below.join(", ")));
            }
            lines.push(format!("For the {name}. Owned levels refit for free."));
            confirm::ask(world, Confirm { kicker: "Upgrades".into(), title: format!("Buy {what}?"), lines, yes: "Buy".into(), no: "Cancel".into(), on_yes: OnYes::Request(CareerRequest::FitPart { kind, level }), refused: None, safe_default: false });
            world.resource_mut::<CarSel>().preview[p] = None;
        }
        Err(r @ Refusal::Notes { .. }) => {
            let price = (car.bought.get(kind) + 1..=level).filter_map(|l| garage::part_price(kind, l, Class::of(garage::pi(&garage::spec_of(car))))).sum::<u32>();
            confirm::ask(world, Confirm { kicker: "Upgrades".into(), title: format!("Buy {what}?"), lines: vec![format!("{} \u{b7} you have {}", notes(price), notes(w.balance))], yes: "Buy".into(), no: "Back".into(), on_yes: OnYes::Request(CareerRequest::FitPart { kind, level }), refused: Some(refusal_text(r)), safe_default: false });
            world.write_message(UiSound::Error);
        }
        Err(r) => {
            world.write_message(UiSound::Error);
            world.write_message(Toast::new(refusal_text(r)));
        }
    }
}

fn buy_car(world: &mut World, k: usize) {
    let Some(def) = garage::CARS.get(k) else { return };
    let (g, w) = (world.resource::<Garage>().clone(), world.resource::<Wallet>().clone());
    match g.can_buy(&w, def.id) {
        Ok(price) => confirm::ask(
            world,
            Confirm {
                kicker: "Dealership".into(),
                title: format!("Buy the {}?", def.name),
                lines: vec![format!("{} \u{b7} balance after {}", notes(price), notes(w.balance - price)), "It joins your garage; drive it from the Garage.".into()],
                yes: "Buy".into(),
                no: "Cancel".into(),
                on_yes: OnYes::Request(CareerRequest::BuyCar(def.id.to_string())),
                refused: None,
                safe_default: false,
            },
        ),
        Err(r @ Refusal::Notes { .. }) => {
            world.write_message(UiSound::Error);
            confirm::ask(
                world,
                Confirm { kicker: "Dealership".into(), title: format!("Buy the {}?", def.name), lines: vec![format!("{} \u{b7} you have {}", notes(def.price), notes(w.balance))], yes: "Buy".into(), no: "Back".into(), on_yes: OnYes::Request(CareerRequest::BuyCar(def.id.to_string())), refused: Some(refusal_text(r)), safe_default: false },
            );
        }
        Err(r) => {
            world.write_message(UiSound::Error);
            world.write_message(Toast::new(refusal_text(r)));
        }
    }
}

/// What selling a car pays: 60 % of its price and 50 % of the upgrades bought for it.
pub fn sale_value(car: &OwnedCar) -> u32 {
    let Some(def) = car.def() else { return 0 };
    let class = Class::of(garage::pi(&garage::build_spec(def, &Default::default())));
    let parts: u32 = UpgradeKind::ALL.iter().map(|k| (1..=car.bought.get(*k)).filter_map(|l| garage::part_price(*k, l, class)).sum::<u32>()).sum();
    def.price * 6 / 10 + parts / 2
}

fn sell(world: &mut World) {
    let i = world.resource::<CarSel>().owned;
    let g = world.resource::<Garage>().clone();
    let Some(car) = g.owned.get(i) else { return };
    let name = car.def().map_or_else(|| car.model.clone(), |d| d.name.to_string());
    let refused = if g.owned.len() <= 1 {
        Some("You can't sell your only car".to_string())
    } else if i == g.active {
        Some("Drive another car first".to_string())
    } else {
        None
    };
    if refused.is_some() {
        world.write_message(UiSound::Error);
    }
    confirm::ask(
        world,
        Confirm {
            kicker: "Garage".into(),
            title: format!("Sell the {name}?"),
            lines: vec![format!("You get {} \u{b7} 60% of its price, half of its upgrades.", notes(sale_value(car)))],
            yes: "Sell".into(),
            no: "Keep it".into(),
            on_yes: OnYes::Request(CareerRequest::SellCar(i)),
            refused,
            safe_default: true,
        },
    );
}

// ---------------------------------------------------------------- input and focus

/// ←/→ on an Upgrades row previews the next or previous level.
fn part_keys(screens: Option<Res<Screens>>, mut queue: ResMut<NavQueue>, focus: Res<Focus>, rows: Query<&PartRow>, garage: Option<Res<Garage>>, mut sel: ResMut<CarSel>, mut out: MessageWriter<RebuildScreen>, mut sound: MessageWriter<UiSound>) {
    if screens.and_then(|s| s.top()) != Some("upgrades") || queue.0.is_empty() {
        return;
    }
    let (Some(row), Some(g)) = (focus.0.and_then(|e| rows.get(e).ok()), garage) else { return };
    let kind = UpgradeKind::ALL[row.0];
    let mut changed = false;
    queue.0.retain(|i| {
        let dir: i32 = match i {
            NavInput::Left => -1,
            NavInput::Right => 1,
            _ => return true,
        };
        let cur = sel.preview[row.0].unwrap_or_else(|| g.active_car().fitted.get(kind)) as i32;
        let next = (cur + dir).clamp(0, kind.max_level() as i32) as u8;
        if next as i32 != cur {
            sel.preview[row.0] = Some(next);
            changed = true;
        }
        false
    });
    if changed {
        sound.write(UiSound::Hover);
        out.write(RebuildScreen);
    }
}

/// The focused row picks the car on the plinth, the stats shown, the part the camera faces.
fn track_focus(world: &mut World) {
    let top = world.get_resource::<Screens>().and_then(|s| s.top());
    if !matches!(top, Some("garage" | "upgrades" | "dealer")) {
        return;
    }
    let Some(f) = world.get_resource::<Focus>().and_then(|f| f.0) else { return };
    let (owned, dealer, part) = (world.get::<OwnedRow>(f).map(|r| r.0), world.get::<DealerRow>(f).map(|r| r.0), world.get::<PartRow>(f).map(|r| r.0));
    let sel = world.resource::<CarSel>().clone();
    let mut rebuild = false;
    if let Some(i) = owned.filter(|i| *i != sel.owned) {
        world.resource_mut::<CarSel>().owned = i;
        rebuild = true;
    }
    if let Some(k) = dealer.filter(|k| *k != sel.dealer) {
        world.resource_mut::<CarSel>().dealer = k;
        rebuild = true;
    }
    if let Some(p) = part.filter(|p| *p != sel.part) {
        world.resource_mut::<CarSel>().part = p;
        rebuild = true;
    }
    // The plinth and camera.
    let sel = world.resource::<CarSel>().clone();
    match top {
        Some("garage") => {
            if let Some(c) = world.resource::<Garage>().owned.get(sel.owned).cloned() {
                set_showroom(world, ShowroomCar { model: c.model, paint: c.paint });
            }
        }
        Some("dealer") => {
            if let Some(d) = garage::CARS.get(sel.dealer) {
                set_showroom(world, ShowroomCar { model: d.id.into(), paint: d.paint });
            }
        }
        Some("upgrades") => {
            let want = Shot::Part(part_angle(UpgradeKind::ALL[sel.part.min(6)]));
            if *world.resource::<Shot>() != want {
                *world.resource_mut::<Shot>() = want;
            }
        }
        _ => {}
    }
    if rebuild {
        world.write_message(RebuildScreen);
    }
}

fn set_showroom(world: &mut World, car: ShowroomCar) {
    if *world.resource::<ShowroomCar>() != car {
        *world.resource_mut::<ShowroomCar>() = car;
    }
}

/// Where the camera looks for a part (0 = from behind, 180 = from the front).
fn part_angle(k: UpgradeKind) -> f32 {
    match k {
        UpgradeKind::Engine | UpgradeKind::Turbo => 150.0,
        UpgradeKind::Tyres | UpgradeKind::Brakes => 118.0,
        UpgradeKind::Suspension => 100.0,
        UpgradeKind::Weight => 80.0,
        UpgradeKind::Gearbox => 55.0,
    }
}

// ---------------------------------------------------------------- shared pieces

/// Dark at both sides, clear in the middle where the car stands.
fn car_backdrop(p: &mut ChildSpawnerCommands, t: &Theme) {
    let side = |p: &mut ChildSpawnerCommands, left: bool, w: f32, a: f32| {
        let stops = vec![ColorStop::new(t.bg.with_alpha(a), percent(0)), ColorStop::new(t.bg.with_alpha(a * 0.75), percent(55)), ColorStop::new(t.bg.with_alpha(0.0), percent(100))];
        let node = if left {
            Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: percent(w), ..default() }
        } else {
            Node { position_type: PositionType::Absolute, right: px(0), top: px(0), bottom: px(0), width: percent(w), ..default() }
        };
        let angle = if left { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
        p.spawn((node, BackgroundGradient::from(LinearGradient::new(angle, stops)), Pickable::IGNORE));
    };
    side(p, true, 48.0, 0.95);
    side(p, false, 40.0, 0.93);
    p.spawn((
        Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: percent(26), ..default() },
        BackgroundGradient::from(LinearGradient::new(0.0, vec![ColorStop::new(t.bg.with_alpha(0.9), percent(0)), ColorStop::new(t.bg.with_alpha(0.0), percent(100))])),
        Pickable::IGNORE,
    ));
    p.spawn((
        Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), height: px(hub::TOP_BAR_H + 40.0), ..default() },
        BackgroundGradient::from(LinearGradient::new(std::f32::consts::PI, vec![ColorStop::new(t.bg.with_alpha(0.85), percent(0)), ColorStop::new(t.bg.with_alpha(0.0), percent(100))])),
        Pickable::IGNORE,
    ));
}

/// The left column (kicker, title, then the caller's rows).
fn left_column(p: &mut ChildSpawnerCommands, intro: Option<hub::Intro>, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    let mut c = p.spawn((
        Node { position_type: PositionType::Absolute, left: px(EDGE), top: px(hub::TOP_BAR_H + 44.0), bottom: px(FOOTER_BOTTOM + 52.0), width: px(640), flex_direction: FlexDirection::Column, row_gap: px(GAP_S), ..default() },
        UiTransform::default(),
        Pickable::IGNORE,
    ));
    if let Some(i) = intro {
        c.insert(i);
    }
    c.with_children(f);
}

/// The right panel.
fn right_panel(p: &mut ChildSpawnerCommands, t: &Theme, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((Node { position_type: PositionType::Absolute, right: px(EDGE), top: px(hub::TOP_BAR_H + 44.0), width: px(470), flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE)).with_children(|c| {
        c.spawn(panel(t, 24.0, GAP_S + 2.0)).with_children(f);
    });
}

/// A focusable list row with the accent focus bar.
fn list_row(p: &mut ChildSpawnerCommands, t: &Theme, id: &'static str, order: u32, marker: impl Bundle, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((
        Button,
        Hovered::default(),
        ActionButton(MenuAction::Custom(id).into()),
        Focusable { order },
        marker,
        nav_look(t),
        Node { width: px(640), min_height: px(54), align_items: AlignItems::Center, padding: UiRect::new(px(18), px(14), px(6), px(6)), column_gap: px(GAP_M), margin: UiRect::left(px(-18)), ..default() },
        BackgroundColor(Color::NONE),
    ))
    .with_children(|r| {
        r.spawn((LookBg([Color::NONE, t.accent, Color::NONE]), Node { position_type: PositionType::Absolute, left: px(0), top: px(9), bottom: px(9), width: px(3), ..default() }, BackgroundColor(Color::NONE), Pickable::IGNORE));
        f(r);
    });
}

/// A small tag ("ACTIVE", "OWNED").
fn tag(p: &mut ChildSpawnerCommands, s: &str, fg: Color, bg: Color) {
    p.spawn((Node { padding: UiRect::axes(px(7), px(1)), border_radius: BorderRadius::all(px(2)), ..default() }, BackgroundColor(bg), Pickable::IGNORE)).with_children(|c| {
        c.spawn((text(s.to_uppercase(), label(15.0), fg), KeepColor));
    });
}

/// Five stat bars: `now` filled, the preview's gain as hollow accent segments with (+n), a
/// loss as dim segments with (−n).
fn stat_bars(p: &mut ChildSpawnerCommands, t: &Theme, now: [f32; 5], preview: Option<[f32; 5]>) {
    for (k, name) in STATS.iter().enumerate() {
        let a = (now[k] * SEGS as f32).round() as i32;
        let b = preview.map_or(a, |p| (p[k] * SEGS as f32).round() as i32);
        p.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
            r.spawn((Node { width: px(104), ..default() }, Pickable::IGNORE)).with_children(|c| {
                c.spawn(text(name.to_uppercase(), label(17.0), t.text_muted));
            });
            r.spawn((Node { column_gap: px(3), ..default() }, Pickable::IGNORE)).with_children(|s| {
                for i in 0..SEGS as i32 {
                    let (bg, border) = if i < a.min(b) {
                        (t.text, t.text)
                    } else if i < b {
                        (Color::NONE, t.accent)
                    } else if i < a {
                        (t.text_dim.with_alpha(0.35), t.text_dim)
                    } else {
                        (Color::NONE, t.line_strong)
                    };
                    s.spawn((Node { width: px(19), height: px(10), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(1)), ..default() }, BackgroundColor(bg), BorderColor::all(border), Pickable::IGNORE));
                }
            });
            if b != a {
                let (s, c) = if b > a { (format!("+{}", b - a), t.good) } else { (format!("\u{2212}{}", a - b), t.bad) };
                r.spawn(text(s, body_bold(16.0), c));
            }
        });
    }
}

fn car_name(c: &OwnedCar) -> String {
    c.def().map_or_else(|| c.model.clone(), |d| d.name.to_string())
}

fn body_name(d: &CarDef) -> &'static str {
    match d.body {
        garage::Body::Rally => "Rally hatch",
        garage::Body::Hatch => "Hatchback",
        garage::Body::Saloon => "Saloon",
        garage::Body::Estate => "Estate",
        garage::Body::Suv => "SUV",
    }
}

/// "Engine Street · Tyres Gravel", or "Stock".
fn build_line(c: &OwnedCar) -> String {
    let parts: Vec<String> = UpgradeKind::ALL.iter().filter(|k| c.fitted.get(**k) > 0).map(|k| format!("{} {}", k.name(), level_name(c.fitted.get(*k)))).collect();
    if parts.is_empty() { "Stock".into() } else { parts.join(" \u{b7} ") }
}

fn small_button(p: &mut ChildSpawnerCommands, t: &Theme, label_text: &str, id: &'static str, order: u32, width: f32) {
    p.spawn((
        Button,
        Hovered::default(),
        ActionButton(MenuAction::Custom(id).into()),
        Focusable { order },
        secondary_look(t),
        Node { width: px(width), height: px(42), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(RADIUS)), flex_shrink: 0.0, ..default() },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|b| {
        b.spawn(text(label_text.to_uppercase(), label(20.0), t.text));
    });
}

// ---------------------------------------------------------------- Garage

fn build_garage(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let g = world.resource::<Garage>().clone();
    let mut sel = world.resource::<CarSel>().clone();
    if sel.owned >= g.owned.len() {
        sel.owned = g.active.min(g.owned.len() - 1);
        world.resource_mut::<CarSel>().owned = sel.owned;
    }
    let shown = g.owned[sel.owned].clone();
    let active = g.active_car().clone();
    let hints = engine::ui::device_hints(world, &[("W / S", "Move"), ("Enter", "Drive / select"), ("Esc", "Back")], &[("D-pad", "Move"), ("A", "Drive / select"), ("B", "Back")]);
    let gl = world.get_resource::<Glyphs>().cloned().unwrap_or_default();
    let intro = hub::intro(world, root);
    world.commands().entity(root).with_children(|p| {
        car_backdrop(p, &t);
        left_column(p, intro, |c| {
            eyebrow_bar(c, "Car \u{b7} Garage", t.accent);
            c.spawn(title("Garage", SIZE_H1, t.text));
            c.spawn(text(format!("{} car{} \u{b7} Enter drives the one you pick", g.owned.len(), if g.owned.len() == 1 { "" } else { "s" }), body(SIZE_BODY - 2.0), t.text_muted));
            c.spawn(spacer(GAP_XS));
            for (i, car) in g.owned.iter().enumerate().take(CAR_IDS.len()) {
                let spec = garage::spec_of(car);
                let pi = garage::pi(&spec);
                list_row(c, &t, CAR_IDS[i], 1 + i as u32, OwnedRow(i), |r| {
                    r.spawn((Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE)).with_children(|n| {
                        n.spawn((text(format!("{}  #{}", car_name(car).to_uppercase(), car.number), label(27.0), t.text_muted), TextLayout::no_wrap()));
                        n.spawn((text(build_line(car), body(15.0), t.text_dim), KeepColor));
                    });
                    class_plate(r, &t, Class::of(pi), pi);
                    if i == g.active {
                        tag(r, "Driving", t.on_accent, t.accent);
                    }
                });
            }
            // The car you drive: paint, number, repair, sell.
            c.spawn(spacer(GAP_M));
            c.spawn((Node { column_gap: px(GAP_S + 2.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                h.spawn(text(format!("THE {} YOU DRIVE", car_name(&active).to_uppercase()), label(17.0), t.text_muted));
                h.spawn((Node { flex_grow: 1.0, height: px(1), ..default() }, BackgroundColor(t.line), Pickable::IGNORE));
            });
            // Paint swatches + metallic.
            c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((Node { width: px(96), ..default() }, Pickable::IGNORE)).with_children(|l| {
                    l.spawn(text("PAINT", label(17.0), t.text_dim));
                });
                let rally = active.def().is_some_and(|d| d.body == garage::Body::Rally);
                for (k, (_, c)) in PAINTS.iter().enumerate() {
                    let name = super::showroom::paint_name(&active.model, Paint { colour: k as u8, metallic: false });
                    let on = active.paint.colour as usize == k;
                    // The rally hatch's paint 0 is its livery: teal with a white flash.
                    let livery = rally && k == 0;
                    let c = if livery { [0.09, 0.42, 0.42] } else { *c };
                    r.spawn((
                        Button,
                        Hovered::default(),
                        ActionButton(MenuAction::Custom(PAINT_IDS[k]).into()),
                        Focusable { order: 30 + k as u32 },
                        secondary_look(&t),
                        Node { width: px(40), height: px(40), padding: UiRect::all(px(4)), border: UiRect::all(px(if on { 2.0 } else { 1.0 })), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                        BackgroundColor(Color::NONE),
                        BorderColor::all(if on { t.accent } else { t.line_strong }),
                        Name::new(name),
                    ))
                    .with_children(|s| {
                        s.spawn((Node { width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(px(2)), ..default() }, BackgroundColor(Color::srgb(c[0], c[1], c[2])), Pickable::IGNORE)).with_children(|m| {
                            if livery {
                                m.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: percent(40), height: percent(22), ..default() }, BackgroundColor(Color::srgb(0.9, 0.9, 0.86)), Pickable::IGNORE));
                            }
                            if on {
                                let ink = if (k == 0 && !livery) || k == 1 || k == 6 { t.bg } else { t.text };
                                glyph(m, &gl, Glyph::Check, 20.0, ink);
                            }
                        });
                    });
                }
                small_button(r, &t, if active.paint.metallic { "Metallic" } else { "Solid" }, "metallic", 37, 136.0);
            });
            c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((Node { width: px(96), ..default() }, Pickable::IGNORE)).with_children(|l| {
                    l.spawn(text("NUMBER", label(17.0), t.text_dim));
                });
                small_button(r, &t, "\u{2212}", "num_down", 38, 42.0);
                r.spawn((Node { width: px(64), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE)).with_children(|n| {
                    n.spawn(text(format!("#{}", active.number), display(34.0), t.text));
                });
                small_button(r, &t, "+", "num_up", 39, 42.0);
            });
            c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, margin: UiRect::top(px(GAP_XS)), ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((Node { width: px(96), ..default() }, Pickable::IGNORE)).with_children(|l| {
                    l.spawn(text("SERVICE", label(17.0), t.text_dim));
                });
                if active.damage > 0.005 {
                    small_button(r, &t, "Repair", "service", 40, 136.0);
                    r.spawn(text(format!("Damage {:.0}%", active.damage * 100.0), body(16.0), t.bad));
                } else {
                    r.spawn(text("No damage", body(16.0), t.text_muted));
                }
            });
        });
        // Right: the picked car.
        let def = shown.def();
        let spec = garage::spec_of(&shown);
        let pi = garage::pi(&spec);
        right_panel(p, &t, |c| {
            c.spawn(eyebrow(def.map_or("Car", body_name), t.text_dim));
            c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                h.spawn(text(car_name(&shown).to_uppercase(), display(40.0), t.text));
                class_plate(h, &t, Class::of(pi), pi);
            });
            c.spawn(text(format!("#{} \u{b7} {} \u{b7} {} kg", shown.number, super::showroom::paint_name(&shown.model, shown.paint), thousands(spec.mass.round() as u32)), body(SIZE_BODY - 2.0), t.text_muted));
            c.spawn(rule(t.line));
            stat_bars(c, &t, garage::stat_bars(&spec), None);
            c.spawn(rule(t.line));
            c.spawn(text("FITTED", label(16.0), t.text_dim));
            c.spawn((text(build_line(&shown), body(SIZE_BODY - 2.0), t.text), Node { max_width: px(420), ..default() }));
            c.spawn(spacer(GAP_XS));
            let driving = sel.owned == g.active;
            c.spawn((Node { column_gap: px(GAP_S), ..default() }, Pickable::IGNORE)).with_children(|b| {
                if driving {
                    tag(b, "You drive this car", t.on_accent, t.accent);
                } else {
                    action_button(b, &t, "Drive this car", MenuAction::Custom(CAR_IDS[sel.owned.min(15)]), None, true, 260.0);
                    small_button(b, &t, "Sell", "sell", 41, 120.0);
                }
            });
        });
        spawn_footer(p, &t, &hints);
    });
    world.flush();
    hub::top_bar(world, root);
}

// ---------------------------------------------------------------- Upgrades

fn build_upgrades(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let g = world.resource::<Garage>().clone();
    let w = world.resource::<Wallet>().clone();
    let sel = world.resource::<CarSel>().clone();
    let car = g.active_car().clone();
    let Some(def) = car.def() else { return };
    let spec = garage::spec_of(&car);
    let pi = garage::pi(&spec);
    let class = Class::of(pi);
    // The build with every previewed level.
    let mut preview = car.fitted;
    for (p, k) in UpgradeKind::ALL.iter().enumerate() {
        if let Some(l) = sel.preview[p] {
            preview.set(*k, l);
        }
    }
    let pspec = garage::build_spec(def, &preview);
    let ppi = garage::pi(&pspec);
    let focus_part = sel.part.min(6);
    let fkind = UpgradeKind::ALL[focus_part];
    let flevel = sel.preview[focus_part].unwrap_or(car.fitted.get(fkind));
    let fit_state = if car.fitted.get(fkind) == flevel { None } else { Some(g.can_fit(&w, fkind, flevel)) };
    let hints = engine::ui::device_hints(world, &[("W / S", "Part"), ("A / D", "Level"), ("Enter", "Fit"), ("Esc", "Back")], &[("D-pad", "Part, level"), ("A", "Fit"), ("B", "Back")]);
    let gl = world.get_resource::<Glyphs>().cloned().unwrap_or_default();
    let intro = hub::intro(world, root);
    world.commands().entity(root).with_children(|p| {
        car_backdrop(p, &t);
        left_column(p, intro, |c| {
            eyebrow_bar(c, &format!("Car \u{b7} Upgrades \u{b7} {} #{}", def.name, car.number), t.accent);
            c.spawn(title("Upgrades", SIZE_H1, t.text));
            c.spawn(text("Street, Gravel and Works parts. A level buys the ones below it; owned levels refit free.", body(SIZE_BODY - 2.0), t.text_muted));
            c.spawn(spacer(GAP_XS));
            // Header.
            c.spawn((Node { column_gap: px(6), padding: UiRect::left(px(146)), ..default() }, Pickable::IGNORE)).with_children(|h| {
                for l in 0..4u8 {
                    h.spawn((Node { width: px(106), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE)).with_children(|x| {
                        x.spawn(text(level_name(l).to_uppercase(), label(15.0), t.text_dim));
                    });
                }
            });
            for (pi_, kind) in UpgradeKind::ALL.iter().enumerate() {
                let fitted = car.fitted.get(*kind);
                let bought = car.bought.get(*kind);
                let shown = sel.preview[pi_].unwrap_or(fitted);
                list_row(c, &t, PART_IDS[pi_], 1 + pi_ as u32, PartRow(pi_), |r| {
                    r.spawn((Node { width: px(130), flex_shrink: 0.0, ..default() }, Pickable::IGNORE)).with_children(|n| {
                        n.spawn((text(kind.name().to_uppercase(), label(24.0), t.text_muted), TextLayout::no_wrap()));
                    });
                    r.spawn((Node { column_gap: px(6), ..default() }, Pickable::IGNORE)).with_children(|cells| {
                        for l in 0..4u8 {
                            if l > kind.max_level() {
                                cells.spawn((Node { width: px(106), height: px(40), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|x| {
                                    x.spawn((text("\u{2014}", body(16.0), t.text_dim), KeepColor));
                                });
                                continue;
                            }
                            let is_fitted = l == fitted;
                            let owned = l <= bought;
                            let picked = l == shown && shown != fitted;
                            let cost = (bought + 1..=l).filter_map(|x| garage::part_price(*kind, x, class)).sum::<u32>();
                            let afford = owned || cost <= w.balance;
                            let (txt, col, mark) = if is_fitted {
                                ("FITTED".to_string(), t.text, None)
                            } else if owned {
                                ("OWNED".to_string(), t.text_muted, Some(Glyph::Check))
                            } else if afford {
                                (thousands(cost), t.text, None)
                            } else {
                                (thousands(cost), t.bad, Some(Glyph::Lock))
                            };
                            let border = if picked { t.accent } else if is_fitted { t.text_muted } else { t.line };
                            let bg = if is_fitted { t.text.with_alpha(0.10) } else { Color::NONE };
                            cells
                                .spawn((
                                    Button,
                                    Hovered::default(),
                                    ActionButton(MenuAction::Custom(LEVEL_IDS[pi_][l as usize]).into()),
                                    Node { width: px(106), height: px(40), justify_content: JustifyContent::Center, align_items: AlignItems::Center, column_gap: px(4), border: UiRect::all(px(if picked { 2.0 } else { 1.0 })), border_radius: BorderRadius::all(px(3)), ..default() },
                                    BackgroundColor(bg),
                                    BorderColor::all(border),
                                ))
                                .with_children(|x| {
                                    if let Some(m) = mark {
                                        glyph(x, &gl, m, 14.0, col);
                                    }
                                    x.spawn((text(txt, label(17.0), col), KeepColor, TextLayout::no_wrap()));
                                });
                        }
                    });
                });
            }
            c.spawn(spacer(GAP_XS));
            c.spawn((text("Your times record the car and its class: the board shows a class chip by each.", body(SIZE_SMALL), t.text_dim), Node { max_width: px(600), ..default() }));
        });
        right_panel(p, &t, |c| {
            c.spawn(eyebrow("Build", t.text_dim));
            c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                h.spawn(text(def.name.to_uppercase(), display(36.0), t.text));
            });
            c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                class_plate(h, &t, class, pi);
                if ppi != pi {
                    glyph(h, &gl, Glyph::Arrow, 18.0, t.text_muted);
                    class_plate(h, &t, Class::of(ppi), ppi);
                    let d = ppi as i32 - pi as i32;
                    h.spawn(text(if d > 0 { format!("+{d} PI") } else { format!("\u{2212}{} PI", -d) }, body_bold(16.0), if d > 0 { t.good } else { t.bad }));
                }
            });
            c.spawn(rule(t.line));
            stat_bars(c, &t, garage::stat_bars(&spec), (preview != car.fitted).then(|| garage::stat_bars(&pspec)));
            let dm = pspec.mass - spec.mass;
            let mass = if dm.abs() >= 1.0 { format!("{} kg ({}{:.0})", thousands(pspec.mass.round() as u32), if dm < 0.0 { "\u{2212}" } else { "+" }, dm.abs()) } else { format!("{} kg", thousands(spec.mass.round() as u32)) };
            c.spawn(text(mass, body(SIZE_BODY - 2.0), t.text_muted));
            c.spawn(rule(t.line));
            c.spawn(text(format!("{} \u{b7} {}", fkind.name().to_uppercase(), level_name(flevel).to_uppercase()), label(20.0), t.text));
            match fit_state {
                None => {
                    c.spawn(text("Fitted now. Left / Right picks another level.", body(SIZE_BODY - 2.0), t.text_muted));
                }
                Some(Ok(0)) => {
                    action_button(c, &t, "Refit (owned, free)", MenuAction::Custom("fit"), None, true, 420.0);
                }
                Some(Ok(cost)) => {
                    action_button(c, &t, &format!("Fit \u{b7} {}", notes(cost)), MenuAction::Custom("fit"), None, true, 420.0);
                    c.spawn(text(format!("Balance after {}", notes(w.balance - cost)), body(SIZE_SMALL), t.text_muted));
                }
                Some(Err(r)) => {
                    c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|row| {
                        glyph(row, &gl, Glyph::Lock, 20.0, t.bad);
                        row.spawn(text(refusal_text(r), body_bold(SIZE_BODY), t.bad));
                    });
                }
            }
        });
        spawn_footer(p, &t, &hints);
    });
    world.flush();
    hub::top_bar(world, root);
}

// ---------------------------------------------------------------- Dealership

fn build_dealer(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let g = world.resource::<Garage>().clone();
    let w = world.resource::<Wallet>().clone();
    let sel = world.resource::<CarSel>().clone();
    let k = sel.dealer.min(garage::CARS.len().saturating_sub(1));
    let hints = engine::ui::device_hints(world, &[("W / S", "Car"), ("Enter", "Buy"), ("Esc", "Back")], &[("D-pad", "Car"), ("A", "Buy"), ("B", "Back")]);
    let gl = world.get_resource::<Glyphs>().cloned().unwrap_or_default();
    let intro = hub::intro(world, root);
    world.commands().entity(root).with_children(|p| {
        car_backdrop(p, &t);
        left_column(p, intro, |c| {
            eyebrow_bar(c, "Car \u{b7} Dealership", t.accent);
            c.spawn(title("Dealership", SIZE_H1, t.text));
            let n = garage::CARS.len();
            c.spawn(text(format!("{n} car{}, class D to S \u{b7} you have {}", if n == 1 { "" } else { "s" }, notes(w.balance)), body(SIZE_BODY - 2.0), t.text_muted));
            c.spawn(spacer(GAP_XS));
            c.spawn((ScrollList, bevy::ui::RelativeCursorPosition::default(), Node { flex_direction: FlexDirection::Column, max_height: px(600), overflow: Overflow::scroll_y(), padding: UiRect::left(px(18)), margin: UiRect::left(px(-18)), ..default() })).with_children(|list| {
                for (i, d) in garage::CARS.iter().enumerate().take(DEALER_IDS.len()) {
                    let spec = garage::build_spec(d, &Default::default());
                    let pi = garage::pi(&spec);
                    let state = g.can_buy(&w, d.id);
                    list_row(list, &t, DEALER_IDS[i], 1 + i as u32, DealerRow(i), |r| {
                        r.spawn((Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE)).with_children(|n| {
                            n.spawn((text(d.name.to_uppercase(), label(27.0), t.text_muted), TextLayout::no_wrap()));
                            n.spawn((text(body_name(d), body(15.0), t.text_dim), KeepColor));
                        });
                        class_plate(r, &t, Class::of(pi), pi);
                        r.spawn((Node { width: px(150), justify_content: JustifyContent::FlexEnd, ..default() }, Pickable::IGNORE)).with_children(|x| match state {
                            Err(Refusal::Owned) => tag(x, "Owned", t.text, t.raised),
                            Err(Refusal::Notes { .. }) => {
                                x.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|l| {
                                    glyph(l, &gl, Glyph::Lock, 16.0, t.bad);
                                    l.spawn((text(notes(d.price), body_bold(18.0), t.bad), KeepColor));
                                });
                            }
                            _ => {
                                x.spawn((text(notes(d.price), body_bold(18.0), t.text), KeepColor));
                            }
                        });
                    });
                }
            });
            if garage::CARS.len() <= 1 {
                c.spawn((text("More cars arrive as the Valley Trials grows.", body(SIZE_BODY - 2.0), t.text_dim), Node { margin: UiRect::top(px(GAP_S)), ..default() }));
            }
        });
        if let Some(d) = garage::CARS.get(k) {
            let spec = garage::build_spec(d, &Default::default());
            let pi = garage::pi(&spec);
            let state = g.can_buy(&w, d.id);
            // Compared with the car you drive.
            let mine = garage::stat_bars(&garage::spec_of(g.active_car()));
            right_panel(p, &t, |c| {
                c.spawn(eyebrow(body_name(d), t.text_dim));
                c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                    h.spawn(text(d.name.to_uppercase(), display(40.0), t.text));
                    class_plate(h, &t, Class::of(pi), pi);
                });
                c.spawn(text(format!("{:.0} kg \u{b7} {}", spec.mass, if spec.front_split > 0.2 && spec.front_split < 0.8 { "four-wheel drive" } else if spec.front_split <= 0.2 { "rear-wheel drive" } else { "front-wheel drive" }), body(SIZE_BODY - 2.0), t.text_muted));
                c.spawn(rule(t.line));
                stat_bars(c, &t, mine, Some(garage::stat_bars(&spec)));
                c.spawn(text(format!("Bars: the {} you drive, and the change with this car.", car_name(g.active_car())), body(SIZE_SMALL), t.text_dim));
                c.spawn(rule(t.line));
                c.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn(text("PRICE", label(18.0), t.text_dim));
                    r.spawn(text(notes(d.price), display(34.0), t.text));
                });
                match state {
                    Ok(price) => {
                        action_button(c, &t, "Buy", MenuAction::Custom("buy"), None, true, 420.0);
                        c.spawn(text(format!("Balance after {}", notes(w.balance - price)), body(SIZE_SMALL), t.text_muted));
                    }
                    Err(Refusal::Owned) => {
                        c.spawn(text("In your garage.", body_bold(SIZE_BODY), t.text_muted));
                    }
                    Err(r) => {
                        c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|row| {
                            glyph(row, &gl, Glyph::Lock, 20.0, t.bad);
                            row.spawn(text(refusal_text(r), body_bold(SIZE_BODY), t.bad));
                        });
                    }
                }
            });
        }
        spawn_footer(p, &t, &hints);
    });
    world.flush();
    hub::top_bar(world, root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_say_how_many_notes_are_missing() {
        assert_eq!(refusal_text(Refusal::Notes { need: 3_200 }), "Need 3,200 N more");
        assert_eq!(refusal_text(Refusal::Owned), "Already in your garage");
    }

    #[test]
    fn ids_cover_the_roster_and_parts() {
        assert!(garage::CARS.len() <= DEALER_IDS.len());
        assert_eq!(PART_IDS.len(), UpgradeKind::ALL.len());
        for (k, id) in PART_IDS.iter().enumerate() {
            assert_eq!(*id, format!("part:{k}"));
        }
        for (p, row) in LEVEL_IDS.iter().enumerate() {
            for (l, id) in row.iter().enumerate() {
                assert_eq!(*id, format!("lvl:{p}:{l}"));
            }
        }
    }

    #[test]
    fn a_stock_car_sells_for_60_percent() {
        let car = OwnedCar::default();
        assert_eq!(sale_value(&car), garage::CARS[0].price * 6 / 10);
    }
}
