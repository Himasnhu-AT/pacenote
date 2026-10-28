//! The guide's top-down map: a 2D orthographic camera over the stage (1 world unit = 1 m,
//! map x = world x, map y = −world z, north up), the road as retained gizmo lines built once
//! per stage, the next [`AHEAD_M`] highlighted, corner labels (grade + direction, coloured by
//! grade), start / split / finish marks and the car arrow.
//!
//! Controls: mouse wheel or `+`/`-` zoom, left-drag pans (stops following), `F` follows the
//! car again, `M` shows the whole stage, `H` toggles heading-up. Line widths and labels stay
//! the same size on screen at every zoom.

use bevy::asset::RenderAssetUsages;
use bevy::camera::ScalingMode;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore, GizmoLineJoint};
use bevy::gizmos::retained::Gizmo;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::ui::IsDefaultUiCamera;
use engine::actions::{ActionDef, ActionState, AddAction, Binding};

use super::{GuideFeed, GuideStage, GuideView, Note, upcoming, view_wanted};
use crate::stage::{STEP, Surface, Turn};

/// Metres of road ahead of the car that are highlighted.
pub const AHEAD_M: f32 = 600.0;
/// Width (px) of the info panel on the right; the map centres in the space left of it.
pub const PANEL_PX: f32 = 440.0;
/// Notes shown in the list and labelled brightly on the map.
pub const NOTES: usize = 8;

pub const BG: Color = Color::srgb(0.067, 0.078, 0.09);

/// Grade colour: 1 (hairpin, red) … 6 (flat, green).
pub fn grade_color(grade: u8) -> Color {
    match grade {
        1 => Color::srgb(0.95, 0.27, 0.25),
        2 => Color::srgb(0.98, 0.5, 0.2),
        3 => Color::srgb(0.98, 0.72, 0.2),
        4 => Color::srgb(0.93, 0.88, 0.3),
        5 => Color::srgb(0.62, 0.85, 0.35),
        _ => Color::srgb(0.38, 0.8, 0.5),
    }
}

pub const CREST: Color = Color::srgb(0.45, 0.75, 1.0);

fn surface_color(s: Surface) -> Color {
    match s {
        Surface::Tarmac => Color::srgb(0.6, 0.62, 0.66),
        Surface::Mud => Color::srgb(0.55, 0.42, 0.3),
        Surface::Snow => Color::srgb(0.85, 0.9, 0.97),
        _ => Color::srgb(0.72, 0.64, 0.5),
    }
}

/// World XZ → map.
pub fn m(p: Vec3) -> Vec2 {
    Vec2::new(p.x, -p.z)
}

/// Where the map looks.
#[derive(Resource, Clone, Debug)]
pub struct MapView {
    /// Map point at the centre of the visible map area.
    pub center: Vec2,
    /// Metres per screen pixel.
    pub scale: f32,
    pub follow: bool,
    pub heading_up: bool,
    /// Current camera rotation (rad), eased towards the target in heading-up.
    pub angle: f32,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { center: Vec2::ZERO, scale: 0.85, follow: true, heading_up: false, angle: 0.0 }
    }
}

#[derive(Component)]
pub struct MapCamera;

/// The map's per-frame systems (the panel runs after them).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapSystems;

/// Rebuilt when the stage changes.
#[derive(Component)]
struct MapStatic;

/// A label that keeps its screen size: map position, outward direction, pixel offset.
#[derive(Component)]
struct MapLabel {
    at: Vec2,
    out: Vec2,
    px: f32,
}

#[derive(Component)]
struct CornerLabel(usize);

#[derive(Component)]
struct CarMarker;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct HighlightGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct MarkGizmos;

pub(super) fn build(app: &mut App) {
    app.init_resource::<MapView>()
        .init_gizmo_group::<HighlightGizmos>()
        .init_gizmo_group::<MarkGizmos>()
        .add_action(ActionDef::button("map_zoom_in").label("Map zoom in").group("Guide").bind(Binding::key(KeyCode::Equal)).bind(Binding::key(KeyCode::NumpadAdd)))
        .add_action(ActionDef::button("map_zoom_out").label("Map zoom out").group("Guide").bind(Binding::key(KeyCode::Minus)).bind(Binding::key(KeyCode::NumpadSubtract)))
        .add_action(ActionDef::button("map_follow").label("Map: follow the car").group("Guide").bind(Binding::key(KeyCode::KeyF)))
        .add_action(ActionDef::button("map_whole").label("Map: whole stage").group("Guide").bind(Binding::key(KeyCode::KeyM)))
        .add_action(ActionDef::button("map_heading").label("Map: heading up").group("Guide").bind(Binding::key(KeyCode::KeyH)))
        .add_systems(Startup, configure_gizmos)
        .add_systems(
            Update,
            (spawn_camera.run_if(no_camera), keep_ui_camera, rebuild.run_if(needs_rebuild), controls, place_camera, draw_dynamic, place_labels)
                .chain()
                .in_set(MapSystems)
                .run_if(view_wanted),
        );
}

/// The map camera draws the UI. The engine's offscreen retarget can strip the tag when it
/// sees this camera and the menu's fallback camera in the same frame; put it back.
fn keep_ui_camera(mut commands: Commands, q: Query<Entity, (With<MapCamera>, Without<IsDefaultUiCamera>)>) {
    for e in &q {
        commands.entity(e).insert(IsDefaultUiCamera);
    }
}

fn no_camera(q: Query<(), With<MapCamera>>) -> bool {
    q.is_empty()
}

/// The stage changed, or the view was (re)spawned without its static map.
fn needs_rebuild(stage: Option<Res<GuideStage>>, built: Query<(), With<MapStatic>>) -> bool {
    stage.is_some_and(|s| s.is_changed() || built.is_empty())
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<HighlightGizmos>();
    c.line.width = 8.0;
    c.line.joints = GizmoLineJoint::Round(4);
    let (c, _) = store.config_mut::<MarkGizmos>();
    c.line.width = 3.0;
    c.line.joints = GizmoLineJoint::Round(3);
}

fn spawn_camera(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<ColorMaterial>>) {
    commands.spawn((
        Name::new("guide map camera"),
        GuideView,
        MapCamera,
        Camera2d,
        Camera { clear_color: ClearColorConfig::Custom(BG), ..default() },
        Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::WindowSize, scale: MapView::default().scale, ..OrthographicProjection::default_2d() }),
        IsDefaultUiCamera,
    ));
    // The car: an arrow, pointing +Y in its own space, ~26 px long at scale 1.
    let mut arrow = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    arrow.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 15.0, 0.0], [-10.0, -11.0, 0.0], [0.0, -5.0, 0.0], [10.0, -11.0, 0.0]]);
    arrow.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    let mut ring = Circle::new(15.0).mesh().resolution(28).build();
    ring.translate_by(Vec3::new(0.0, 0.0, -0.1));
    let arrow = meshes.add(arrow);
    let ring = meshes.add(ring);
    commands
        .spawn((Name::new("car marker"), GuideView, CarMarker, Transform::from_xyz(0.0, 0.0, 10.0), Visibility::Hidden))
        .with_children(|p| {
            p.spawn((Mesh2d(ring), MeshMaterial2d(mats.add(ColorMaterial::from_color(Color::srgba(0.0, 0.0, 0.0, 0.55)))), Transform::default()));
            p.spawn((Mesh2d(arrow), MeshMaterial2d(mats.add(ColorMaterial::from_color(Color::WHITE))), Transform::from_xyz(0.0, 0.0, 0.1)));
        });
}

/// The stage's static map: road lines, grid, marks, labels.
fn rebuild(mut commands: Commands, g: Res<GuideStage>, old: Query<Entity, With<MapStatic>>, mut gizmos: ResMut<Assets<GizmoAsset>>, mut view: ResMut<MapView>) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let road = &g.road;
    let pts: Vec<Vec2> = road.pts.iter().map(|p| m(*p)).collect();
    let (lo, hi) = pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    view.center = pts[0];

    // Grid every 250 m (faint), for a sense of distance.
    let mut grid = GizmoAsset::new();
    let step = 250.0;
    let (g0, g1) = ((lo / step).floor() * step - step, (hi / step).ceil() * step + step);
    let mut x = g0.x;
    while x <= g1.x {
        grid.line_2d(Vec2::new(x, g0.y), Vec2::new(x, g1.y), Color::srgba(1.0, 1.0, 1.0, 0.035));
        x += step;
    }
    let mut y = g0.y;
    while y <= g1.y {
        grid.line_2d(Vec2::new(g0.x, y), Vec2::new(g1.x, y), Color::srgba(1.0, 1.0, 1.0, 0.035));
        y += step;
    }
    let line = |w: f32| GizmoLineConfig { width: w, joints: GizmoLineJoint::Round(4), ..default() };
    commands.spawn((GuideView, MapStatic, Gizmo { handle: gizmos.add(grid), line_config: GizmoLineConfig { width: 1.0, ..default() }, ..default() }, Transform::from_xyz(0.0, 0.0, -3.0)));

    // Road: a dark casing, then the surface colour.
    let mut casing = GizmoAsset::new();
    casing.linestrip_2d(pts.iter().copied(), Color::srgb(0.02, 0.025, 0.03));
    commands.spawn((GuideView, MapStatic, Gizmo { handle: gizmos.add(casing), line_config: line(11.0), ..default() }, Transform::from_xyz(0.0, 0.0, -2.0)));
    let mut surf = GizmoAsset::new();
    surf.linestrip_gradient_2d(pts.iter().zip(&road.surface).map(|(p, s)| (*p, surface_color(*s).with_alpha(0.55))));
    commands.spawn((GuideView, MapStatic, Gizmo { handle: gizmos.add(surf), line_config: line(5.0), ..default() }, Transform::from_xyz(0.0, 0.0, -1.0)));

    // Start, splits (quarters, where the clock takes them), finish: bars across the road.
    let mut marks = GizmoAsset::new();
    let len = road.length();
    let mut label = |cmd: &mut Commands, text: &str, s: f32, color: Color| {
        let i = road.station(s);
        let d = road.dir[i];
        let across = Vec2::new(-d.y, -d.x); // right of travel, on the map
        let p = pts[i];
        marks.line_2d(p - across * 9.0, p + across * 9.0, color);
        cmd.spawn((
            GuideView,
            MapStatic,
            MapLabel { at: p, out: across, px: 26.0 },
            Text2d::new(text),
            engine::ui::theme::label(15.0),
            TextColor(color),
            Anchor::CENTER,
            Transform::from_xyz(p.x, p.y, 6.0),
        ));
    };
    label(&mut commands, "START", 0.0, Color::srgb(0.4, 0.85, 0.5));
    for k in 1..=3 {
        label(&mut commands, &format!("S{k}"), len * k as f32 / 4.0, Color::srgb(0.75, 0.78, 0.82));
    }
    label(&mut commands, "FINISH", len, Color::srgb(0.95, 0.35, 0.35));
    commands.spawn((GuideView, MapStatic, Gizmo { handle: gizmos.add(marks), line_config: line(3.0), ..default() }, Transform::from_xyz(0.0, 0.0, 1.0)));

    // Corner labels at the apex, on the outside of the bend.
    for (k, c) in g.corners.iter().enumerate() {
        let i = road.station((c.start + c.end) * 0.5);
        let d = road.dir[i];
        let right = Vec2::new(-d.y, -d.x);
        let out = if c.turn == Turn::Left { right } else { -right };
        let txt = format!("{}{}", if c.turn == Turn::Left { "L" } else { "R" }, c.grade);
        commands.spawn((
            GuideView,
            MapStatic,
            CornerLabel(k),
            MapLabel { at: pts[i], out, px: 20.0 },
            Text2d::new(txt),
            engine::ui::theme::display(17.0),
            TextColor(grade_color(c.grade)),
            Anchor::CENTER,
            Transform::from_xyz(pts[i].x, pts[i].y, 5.0),
        ));
    }
    let _ = STEP;
}

fn fit_scale(g: &GuideStage, size: Vec2) -> (Vec2, f32) {
    let (lo, hi) = g.road.pts.iter().map(|p| m(*p)).fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(a, b), p| (a.min(p), b.max(p)));
    let avail = Vec2::new((size.x - PANEL_PX - 80.0).max(200.0), (size.y - 120.0).max(200.0));
    let s = ((hi - lo) / avail).max_element();
    ((lo + hi) * 0.5, s)
}

#[allow(clippy::too_many_arguments)]
fn controls(
    actions: Res<ActionState>,
    scroll: Res<AccumulatedMouseScroll>,
    motion: Res<AccumulatedMouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    stage: Option<Res<GuideStage>>,
    cams: Query<&Camera, With<MapCamera>>,
    windows: Query<&Window>,
    mut view: ResMut<MapView>,
) {
    let size = cams.iter().next().and_then(|c| c.logical_viewport_size()).unwrap_or(Vec2::new(1920.0, 1080.0));
    let mut zoom = 0.0;
    if scroll.delta.y != 0.0 {
        zoom += match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
    }
    if actions.just_pressed("map_zoom_in") {
        zoom += 1.0;
    }
    if actions.just_pressed("map_zoom_out") {
        zoom -= 1.0;
    }
    let max_scale = stage.as_ref().map_or(4.0, |g| fit_scale(g, size).1 * 1.5);
    if zoom != 0.0 {
        view.scale = (view.scale * 1.2f32.powf(-zoom)).clamp(0.12, max_scale.max(0.5));
    }
    // Drag to pan (not when the press started over the panel).
    let over_panel = windows.iter().next().and_then(|w| w.cursor_position()).is_some_and(|c| c.x > size.x - PANEL_PX);
    if buttons.pressed(MouseButton::Left) && motion.delta != Vec2::ZERO && !over_panel {
        let d = Vec2::new(-motion.delta.x, motion.delta.y) * view.scale;
        let r = Vec2::from_angle(view.angle);
        view.center += r.rotate(d);
        view.follow = false;
    }
    if actions.just_pressed("map_follow") {
        view.follow = true;
    }
    if actions.just_pressed("map_heading") {
        view.heading_up = !view.heading_up;
    }
    if actions.just_pressed("map_whole")
        && let Some(g) = &stage
    {
        let (c, s) = fit_scale(g, size);
        view.center = c;
        view.scale = s;
        view.follow = false;
        view.heading_up = false;
    }
}

fn place_camera(time: Res<Time>, feed: Res<GuideFeed>, mut view: ResMut<MapView>, mut cams: Query<(&mut Transform, &mut Projection), With<MapCamera>>) {
    let Ok((mut tf, mut proj)) = cams.single_mut() else { return };
    let dt = time.delta_secs();
    if let Some(car) = feed.car {
        let h = car.heading();
        let hm = Vec2::new(h.x, -h.y);
        if view.follow {
            // Look a little ahead so more of the coming road is on screen.
            let want = m(car.pos) + hm * (140.0 * view.scale);
            let k = 1.0 - (-dt * 5.0).exp();
            view.center = view.center.lerp(want, k);
        }
        let target = if view.heading_up { hm.to_angle() - std::f32::consts::FRAC_PI_2 } else { 0.0 };
        let mut diff = (target - view.angle).rem_euclid(std::f32::consts::TAU);
        if diff > std::f32::consts::PI {
            diff -= std::f32::consts::TAU;
        }
        view.angle += diff * (1.0 - (-dt * 3.0).exp());
    }
    let r = Vec2::from_angle(view.angle);
    let c = view.center + r.rotate(Vec2::new(PANEL_PX * 0.5 * view.scale, 0.0));
    tf.translation = c.extend(0.0);
    tf.rotation = Quat::from_rotation_z(view.angle);
    if let Projection::Orthographic(o) = &mut *proj
        && o.scale != view.scale
    {
        o.scale = view.scale;
    }
}

fn draw_dynamic(
    feed: Res<GuideFeed>,
    stage: Option<Res<GuideStage>>,
    view: Res<MapView>,
    mut hi: Gizmos<HighlightGizmos>,
    mut mk: Gizmos<MarkGizmos>,
    mut car_q: Query<(&mut Transform, &mut Visibility), With<CarMarker>>,
    mut notes: Local<Vec<Note>>,
) {
    let Some(g) = stage else { return };
    let road = &g.road;
    let s = feed.s;
    let i0 = road.station(s);
    let i1 = road.station(s + AHEAD_M);
    // Driven part, dimmed; the next 600 m bright, fading towards the end.
    if i0 > 0 {
        mk.linestrip_2d(road.pts[..=i0].iter().map(|p| m(*p)), Color::srgba(0.35, 0.38, 0.42, 0.9));
    }
    if i1 > i0 {
        let n = (i1 - i0) as f32;
        hi.linestrip_gradient_2d(road.pts[i0..=i1].iter().enumerate().map(|(k, p)| {
            let t = k as f32 / n;
            (m(*p), Color::srgba(1.0, 0.78 - 0.25 * t, 0.25, 1.0 - 0.55 * t))
        }));
    }
    // Apex dots of the coming corners, crest chevrons.
    if notes.capacity() < NOTES {
        notes.reserve(NOTES);
    }
    upcoming(&g, s, NOTES, &mut notes);
    let px = view.scale;
    for n in notes.iter().filter(|n| n.at < s + AHEAD_M + 200.0) {
        match n.corner {
            Some(k) => {
                let c = &g.corners[k];
                let p = m(road.pts[road.station((c.start + c.end) * 0.5)]);
                mk.circle_2d(Isometry2d::from_translation(p), 7.0 * px, grade_color(c.grade));
                if c.crest {
                    let q = m(road.pts[road.station(c.start)]);
                    mk.circle_2d(Isometry2d::from_translation(q), 4.0 * px, CREST);
                }
            }
            None => {
                let i = road.station(n.at);
                let d = road.dir[i];
                let (f, r) = (Vec2::new(d.x, -d.y), Vec2::new(-d.y, -d.x));
                let p = m(road.pts[i]);
                mk.linestrip_2d([p - r * 8.0 * px - f * 4.0 * px, p + f * 4.0 * px, p + r * 8.0 * px - f * 4.0 * px], CREST);
            }
        }
    }
    if let Ok((mut tf, mut vis)) = car_q.single_mut() {
        match feed.car {
            Some(c) => {
                let h = c.heading();
                let hm = Vec2::new(h.x, -h.y);
                tf.translation = m(c.pos).extend(10.0);
                tf.rotation = Quat::from_rotation_z(hm.to_angle() - std::f32::consts::FRAC_PI_2);
                tf.scale = Vec3::splat(view.scale);
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// Labels: constant screen size, upright, pushed off the road; corner labels bright when
/// coming up, dim otherwise, hidden when zoomed far out.
fn place_labels(
    feed: Res<GuideFeed>,
    stage: Option<Res<GuideStage>>,
    view: Res<MapView>,
    mut labels: Query<(&MapLabel, Option<&CornerLabel>, &mut Transform, &mut TextColor, &mut Visibility)>,
    mut notes: Local<Vec<Note>>,
) {
    let Some(g) = stage else { return };
    upcoming(&g, feed.s, NOTES, &mut notes);
    let (lo, hi) = notes.iter().filter_map(|n| n.corner).fold((usize::MAX, 0), |(a, b), k| (a.min(k), b.max(k)));
    let rot = Quat::from_rotation_z(view.angle);
    for (l, corner, mut tf, mut color, mut vis) in &mut labels {
        let p = l.at + l.out * l.px * view.scale;
        tf.translation = p.extend(tf.translation.z);
        tf.rotation = rot;
        tf.scale = Vec3::splat(view.scale);
        if let Some(&CornerLabel(k)) = corner {
            let c = &g.corners[k];
            let soon = k >= lo && k <= hi;
            let passed = c.end < feed.s;
            let show = soon || view.scale < 1.6;
            vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
            let a = if soon {
                1.0
            } else if passed {
                0.25
            } else {
                0.55
            };
            let want = grade_color(c.grade).with_alpha(a);
            if color.0 != want {
                color.0 = want;
            }
        }
    }
}
