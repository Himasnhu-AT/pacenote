//! The HUD minimap (free roam only): bottom left, heading up, the map image turned and slid
//! under a fixed arrow; event starts as dots in their kind's colour (the picked or nearest one
//! held at the rim when off the map), nearby traffic as grey dots, the route line to a picked
//! event, and a caption with that event and its distance. Zooms out with speed.

use bevy::prelude::*;
use engine::ui::screens::Screens;
use engine::ui::theme::*;

use super::{MapArt, ROUTE, Waypoint, car_pose, distance_label, guide_target, kind_color, paint, start_of};
use crate::drive::DrivenCar;
use crate::open::events::Events;
use crate::prefs::Prefs;
use crate::world::StageWorld;

/// Side of the minimap (UI px).
const SIDE: f32 = 270.0;
/// Metres per UI px: standing still, and at speed.
const ZOOM_SLOW: f32 = 1.5;
const ZOOM_FAST: f32 = 2.8;
/// Traffic dots shown at most.
const TRAFFIC: usize = 12;

#[derive(Component)]
struct MiniRoot;
#[derive(Component)]
struct MiniPivot;
/// The clipped square the map, dots and arrow sit in.
#[derive(Component)]
struct MiniFrame;
#[derive(Component)]
struct MiniImage;
/// Holds the route line's nodes (children of the image); rebuilt when the route changes.
#[derive(Component)]
struct MiniRoute(u32);
/// The route's first piece, from the car to the line (moved every frame).
#[derive(Component)]
struct MiniLead;
#[derive(Component)]
struct MiniMarker(u32);
#[derive(Component)]
struct MiniTraffic(usize);
#[derive(Component)]
struct MiniNorth;
/// A roadside challenge (index into `open::challenges::Challenges`): a star, shown within
/// [`CHALLENGE_M`] and tinted by the stars earned.
#[derive(Component)]
struct MiniChallenge(usize);
/// Challenges show on the minimap within this many metres.
const CHALLENGE_M: f32 = 400.0;
#[derive(Component)]
enum MiniText {
    Eyebrow,
    Name,
    Distance,
}

pub(super) fn build(app: &mut App) {
    app.add_systems(Update, (spawn, challenge_marks, markers, challenges, (place, caption)).chain().run_if(resource_exists::<StageWorld>).run_if(resource_exists::<MapArt>));
}

fn spawn(mut commands: Commands, theme: Res<Theme>, art: Res<MapArt>, events: Option<Res<Events>>, q: Query<(), With<MiniRoot>>) {
    let (Some(map), Some(events)) = (art.map.clone(), events) else { return };
    if !q.is_empty() {
        return;
    }
    let t = &*theme;
    commands
        .spawn((
            MiniRoot,
            Name::new("minimap"),
            DespawnOnExit(engine::flow::AppState::InGame),
            Node { position_type: PositionType::Absolute, left: px(40), bottom: px(104), flex_direction: FlexDirection::Column, row_gap: px(6), display: Display::None, ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                MiniFrame,
                Node { width: px(SIDE), height: px(SIDE), overflow: Overflow::clip(), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(RADIUS * 2.0)), ..default() },
                BackgroundColor(Color::srgb(0.1, 0.12, 0.1)),
                BorderColor::all(Color::srgba(0.95, 0.93, 0.88, 0.35)),
                Pickable::IGNORE,
            ))
            .with_children(|f| {
                f.spawn((MiniPivot, Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: px(0), height: px(0), ..default() }, UiTransform::default(), Pickable::IGNORE)).with_children(|p| {
                    p.spawn((MiniImage, Node { position_type: PositionType::Absolute, width: px(1000), height: px(1000), ..default() }, ImageNode::new(map), Pickable::IGNORE)).with_children(|img| {
                        img.spawn((MiniRoute(u32::MAX), Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }, Pickable::IGNORE));
                    });
                    // From the car to the route line (in the pivot's frame: the car is at 0, 0).
                    p.spawn((MiniLead, Node { position_type: PositionType::Absolute, height: px(4), display: Display::None, border_radius: BorderRadius::MAX, ..default() }, UiTransform::default(), BackgroundColor(ROUTE), Pickable::IGNORE));
                });
                for k in 0..TRAFFIC {
                    f.spawn((MiniTraffic(k), Node { position_type: PositionType::Absolute, width: px(7), height: px(7), border_radius: BorderRadius::MAX, display: Display::None, ..default() }, BackgroundColor(Color::srgba(0.85, 0.86, 0.88, 0.8)), Pickable::IGNORE));
                }
                for e in &events.0 {
                    f.spawn((MiniMarker(e.id), Node { position_type: PositionType::Absolute, width: px(16), height: px(16), display: Display::None, ..default() }, ImageNode::new(art.dot.clone()).with_color(kind_color(e.kind)), Pickable::IGNORE));
                }
                f.spawn((MiniNorth, Node { position_type: PositionType::Absolute, width: px(18), height: px(18), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.75)), Pickable::IGNORE)).with_children(|n| {
                    n.spawn(text("N", label(SIZE_TINY - 2.0), t.text));
                });
                // The car, fixed in the middle, pointing up.
                f.spawn((Node { position_type: PositionType::Absolute, left: px(SIDE * 0.5 - 13.0), top: px(SIDE * 0.5 - 13.0), width: px(22), height: px(22), ..default() }, ImageNode::new(art.arrow.clone()), Pickable::IGNORE));
            });
            // Caption: what the dot at the rim is.
            root.spawn((
                Node { width: px(SIDE), flex_direction: FlexDirection::Column, padding: UiRect::new(px(12), px(12), px(6), px(8)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.55)),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((MiniText::Eyebrow, text("NEAREST EVENT", label(SIZE_TINY), t.text_muted)));
                    r.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|k| {
                        k.spawn(key_cap(t, "M"));
                        k.spawn(text("MAP", label(SIZE_TINY), t.text_dim));
                    });
                });
                c.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Baseline, column_gap: px(10), ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((MiniText::Name, text("", label(SIZE_LABEL), t.text)));
                    r.spawn((MiniText::Distance, text("", display(26.0), t.text)));
                });
            });
        });
}

/// The minimap's state last frame (only changes are written).
#[derive(Default)]
struct Shown {
    on: Option<bool>,
    caption: Option<(u32, bool, String)>,
}

/// Shows it in free roam only, and slides / turns the map under the car.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place(
    time: Res<Time<Real>>,
    stage: Res<StageWorld>,
    screens: Option<Res<Screens>>,
    cars: Query<&DrivenCar>,
    wp: Res<Waypoint>,
    mut root: Query<&mut Node, (With<MiniRoot>, Without<MiniImage>, Without<MiniLead>)>,
    mut pivot: Query<&mut UiTransform, (With<MiniPivot>, Without<MiniLead>)>,
    mut image: Query<&mut Node, (With<MiniImage>, Without<MiniRoot>, Without<MiniLead>)>,
    mut lead: Query<(&mut Node, &mut UiTransform), (With<MiniLead>, Without<MiniPivot>, Without<MiniRoot>, Without<MiniImage>)>,
    mut zoom: Local<Option<f32>>,
    mut shown: Local<Shown>,
    loading: Option<Res<crate::loading::Loading>>,
) {
    let car = cars.single().ok();
    // Hidden under any menu screen, like the rest of the HUD.
    let menu = screens.is_some_and(|s| !s.stack.is_empty());
    let on = stage.free_roam() && car.is_some() && loading.is_none() && !menu;
    if shown.on != Some(on) {
        shown.on = Some(on);
        for mut n in &mut root {
            n.display = if on { Display::Flex } else { Display::None };
        }
    }
    let (true, Some(car)) = (on, car) else { return };
    let (pos, h) = car_pose(car);
    let speed = car.car.vel.length();
    let want = ZOOM_SLOW + (ZOOM_FAST - ZOOM_SLOW) * (speed / 30.0).clamp(0.0, 1.0);
    let z = zoom.get_or_insert(want);
    *z += (want - *z) * (1.0 - (-time.delta_secs() * 1.5).exp());
    let side = 2.0 * crate::open::roads::HALF / *z;
    let uv = paint::uv(pos);
    for mut n in &mut image {
        n.width = px(side);
        n.height = px(side);
        n.left = px(-uv.x * side);
        n.top = px(-uv.y * side);
    }
    let angle = heading_angle(h);
    for mut t in &mut pivot {
        t.rotation = Rot2::radians(angle);
    }
    // The lead: from the car to the route's second point.
    if let Ok((mut n, mut t)) = lead.single_mut() {
        match wp.path.get(1) {
            Some(&q) => {
                let d = (paint::uv(q) - uv) * side;
                let len = d.length();
                n.display = Display::Flex;
                n.width = px(len);
                n.left = px(-len * 0.5 + d.x * 0.5);
                n.top = px(d.y * 0.5 - 2.0);
                t.rotation = Rot2::radians(d.y.atan2(d.x));
            }
            None => n.display = Display::None,
        }
    }
}

/// The minimap's turn (UI rotation, clockwise) that puts the car's heading `h` (x, z) up.
fn heading_angle(h: Vec2) -> f32 {
    // On the image (x, z) is UI (x, y): the heading points along (x, z) there.
    -std::f32::consts::FRAC_PI_2 - h.y.atan2(h.x)
}

/// Where a map point lands in the frame (UI px from its top-left), for this car pose.
fn frame_pos(p: Vec2, car: Vec2, h: Vec2, mpp: f32) -> Vec2 {
    let d = (paint::uv(p) - paint::uv(car)) * (2.0 * crate::open::roads::HALF / mpp);
    Rot2::radians(heading_angle(h)) * d + Vec2::splat(SIDE * 0.5)
}

/// Star markers for the challenges, once both the minimap and the challenges exist.
fn challenge_marks(mut commands: Commands, challenges: Option<Res<crate::open::challenges::Challenges>>, icons: Option<Res<crate::hud_challenge::icons::HudIcons>>, frame: Query<Entity, With<MiniFrame>>, have: Query<(), With<MiniChallenge>>) {
    let (Some(ch), Some(icons), Ok(f)) = (challenges, icons, frame.single()) else { return };
    if !have.is_empty() || ch.0.is_empty() {
        return;
    }
    let star = icons.get(crate::hud_challenge::icons::Icon::Star);
    commands.entity(f).with_children(|f| {
        for k in 0..ch.0.len() {
            f.spawn((MiniChallenge(k), Node { position_type: PositionType::Absolute, width: px(14), height: px(14), display: Display::None, ..default() }, ImageNode::new(star.clone()), Pickable::IGNORE));
        }
    });
}

/// Places the challenge stars (within [`CHALLENGE_M`] of the car) and tints them by stars.
#[allow(clippy::type_complexity)]
fn challenges(
    stage: Res<StageWorld>,
    ch: Option<Res<crate::open::challenges::Challenges>>,
    bests: Option<Res<crate::open::challenges::Bests>>,
    cars: Query<&DrivenCar>,
    image: Query<&Node, (With<MiniImage>, Without<MiniChallenge>)>,
    mut marks: Query<(&MiniChallenge, &mut Node, &mut ImageNode), Without<MiniImage>>,
) {
    let (Some(ch), Ok(car)) = (ch, cars.single()) else { return };
    if !stage.free_roam() {
        return;
    }
    let (pos, h) = car_pose(car);
    let side = image.iter().next().and_then(|n| if let Val::Px(v) = n.width { Some(v) } else { None }).unwrap_or(1000.0);
    let mpp = 2.0 * crate::open::roads::HALF / side;
    let rim = SIDE * 0.5 - 8.0;
    for (m, mut n, mut img) in &mut marks {
        let Some(p) = ch.0.get(m.0) else { continue };
        let at = Vec2::new(p.first().p.x, p.first().p.z);
        let q = frame_pos(at, pos, h, mpp) - Vec2::splat(SIDE * 0.5);
        let show = at.distance(pos) <= CHALLENGE_M && q.x.abs() <= rim && q.y.abs() <= rim;
        let d = if show { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
        if show {
            n.left = px(SIDE * 0.5 + q.x - 9.0);
            n.top = px(SIDE * 0.5 + q.y - 9.0);
            let stars = bests.as_ref().and_then(|b| b.get(p.def.id)).map_or(0, |b| b.stars);
            let c = crate::open::challenges::star_tint(stars);
            if img.color != c {
                img.color = c;
            }
        }
    }
}

/// Event dots, the north mark, traffic, and the route line when it changes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn markers(
    mut commands: Commands,
    stage: Res<StageWorld>,
    events: Option<Res<Events>>,
    wp: Res<Waypoint>,
    cars: Query<&DrivenCar>,
    image: Query<&Node, (With<MiniImage>, Without<MiniMarker>, Without<MiniTraffic>, Without<MiniNorth>)>,
    mut marks: Query<(&MiniMarker, &mut Node), (Without<MiniTraffic>, Without<MiniNorth>)>,
    mut traffic_q: Query<(&MiniTraffic, &mut Node), (Without<MiniMarker>, Without<MiniNorth>)>,
    mut north: Query<&mut Node, (With<MiniNorth>, Without<MiniMarker>, Without<MiniTraffic>)>,
    mut route: Query<(Entity, &mut MiniRoute)>,
    traffic: Option<Res<crate::open::traffic::TrafficSolids>>,
) {
    if !stage.free_roam() {
        return;
    }
    let (Ok(car), Some(events)) = (cars.single(), events) else { return };
    let (pos, h) = car_pose(car);
    // The image's side tells the zoom `place` chose (it runs after: one frame behind, unseen).
    let side = image.iter().next().and_then(|n| if let Val::Px(v) = n.width { Some(v) } else { None }).unwrap_or(1000.0);
    let mpp = 2.0 * crate::open::roads::HALF / side;
    let target = guide_target(&events, &wp, pos).map(|t| t.0.id);
    let rim = SIDE * 0.5 - 10.0;
    for (m, mut n) in &mut marks {
        let Some(e) = events.0.iter().find(|e| e.id == m.0) else { continue };
        let mut q = frame_pos(start_of(e), pos, h, mpp) - Vec2::splat(SIDE * 0.5);
        let inside = q.x.abs() <= rim && q.y.abs() <= rim;
        let show = inside || target == Some(e.id);
        if !inside {
            // Held at the rim, pointing the way.
            q *= rim / q.x.abs().max(q.y.abs());
        }
        let sz = if target == Some(e.id) { 18.0 } else { 13.0 };
        n.display = if show { Display::Flex } else { Display::None };
        n.width = px(sz);
        n.height = px(sz);
        n.left = px(SIDE * 0.5 + q.x - sz * 0.5 - 2.0);
        n.top = px(SIDE * 0.5 + q.y - sz * 0.5 - 2.0);
    }
    // North: on the rim, the way north is.
    if let Ok(mut n) = north.single_mut() {
        let d = Rot2::radians(heading_angle(h)) * Vec2::new(0.0, -1.0);
        let q = d * ((SIDE * 0.5 - 13.0) / d.x.abs().max(d.y.abs()));
        n.left = px(SIDE * 0.5 + q.x - 11.0);
        n.top = px(SIDE * 0.5 + q.y - 11.0);
    }
    // Nearest traffic.
    let mut near: Vec<Vec2> = Vec::new();
    if let Some(tr) = traffic
        && let Ok(poses) = tr.0.read()
    {
        near.extend(poses.iter().map(|p| Vec2::new(p.0.x, p.0.z)).filter(|p| p.distance(pos) < 260.0 && p.distance(pos) > 3.0));
        near.sort_by(|a, b| a.distance_squared(pos).total_cmp(&b.distance_squared(pos)));
    }
    for (k, mut n) in &mut traffic_q {
        match near.get(k.0) {
            Some(&p) => {
                let q = frame_pos(p, pos, h, mpp);
                let ok = (q - Vec2::splat(SIDE * 0.5)).abs().max_element() < SIDE * 0.5 - 4.0;
                n.display = if ok { Display::Flex } else { Display::None };
                n.left = px(q.x - 5.5);
                n.top = px(q.y - 5.5);
            }
            None => n.display = Display::None,
        }
    }
    // The route line, rebuilt when the route changes (not per frame).
    if let Ok((e, mut r)) = route.single_mut()
        && r.0 != wp.version
    {
        r.0 = wp.version;
        commands.entity(e).despawn_children();
        if wp.path.len() > 2 {
            let pts = wp.path[1..].to_vec();
            commands.entity(e).with_children(|p| super::spawn_line(p, &pts, 4.0, ROUTE));
        }
    }
}

/// The caption: the picked event (ROUTE) or the nearest one, and how far.
#[allow(clippy::too_many_arguments)]
fn caption(theme: Res<Theme>, stage: Res<StageWorld>, events: Option<Res<Events>>, wp: Res<Waypoint>, prefs: Option<Res<Prefs>>, cars: Query<&DrivenCar>, mut texts: Query<(&MiniText, &mut Text, &mut TextColor)>, mut shown: Local<Shown>) {
    if !stage.free_roam() {
        return;
    }
    let (Ok(car), Some(events)) = (cars.single(), events) else { return };
    let (pos, _) = car_pose(car);
    let Some((e, d, route)) = guide_target(&events, &wp, pos) else { return };
    let mph = prefs.is_some_and(|p| p.mph);
    let dist = distance_label(d, mph);
    let key = (e.id, route, dist.clone());
    // (Nothing to write until the minimap's texts exist: it was spawned this frame.)
    if shown.caption.as_ref() == Some(&key) || texts.is_empty() {
        return;
    }
    shown.caption = Some(key);
    for (k, mut t, mut c) in &mut texts {
        match k {
            MiniText::Eyebrow => {
                t.0 = if route { "ROUTE TO".into() } else { "NEAREST EVENT".into() };
                c.0 = if route { ROUTE } else { theme.text_muted };
            }
            MiniText::Name => {
                t.0 = e.name.to_uppercase();
                c.0 = kind_color(e.kind);
            }
            MiniText::Distance => t.0.clone_from(&dist),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_up_puts_the_car_ahead_at_the_top() {
        for h in [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-0.6, -0.8)] {
            let car = Vec2::new(100.0, -300.0);
            let ahead = frame_pos(car + h * 50.0, car, h, 2.0);
            let c = Vec2::splat(SIDE * 0.5);
            assert!((ahead - c).x.abs() < 0.01 && ahead.y < c.y - 20.0, "{h:?}: {ahead:?}");
            // Left of travel (up × direction) is left on the screen.
            let left = Vec2::new(h.y, -h.x);
            let l = frame_pos(car + left * 50.0, car, h, 2.0);
            assert!(l.x < c.x - 20.0, "{h:?}: left {l:?}");
        }
    }
}
